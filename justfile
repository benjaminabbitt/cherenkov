default: check

# Format, lint and test the workspace. Needs protoc on PATH and access to
# github.com.
check: fmt-check lint lint-arch test

# The purge flavour (`--features purge`, the build that can delete: `store
# prune`); `check` is the default one, append-only. CI runs both. The arch and
# format gates do not depend on the flavour and are not repeated.
check-purge: lint-purge test-purge

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

lint-purge:
    cargo clippy --workspace --all-targets --features purge -- -D warnings

# arch-lint at rust-support's pinned commit (the one Cargo.toml pins for
# cli-support), installed once into the cache.
arch_lint_rev := "e92609d8de072c2947438a0cc395548fba51aedf"
arch_lint_root := env_var_or_default("XDG_CACHE_HOME", env_var("HOME") / ".cache") / "cherenkov-arch-lint" / arch_lint_rev
arch_lint := arch_lint_root / "bin/arch-lint"

# The workspace's dependency edges against arch-rules.toml: a crate's
# directory, crates/<ring>/<role>/<crate>, is its role.
lint-arch:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! grep -q 'rust-support", rev = "{{arch_lint_rev}}"' Cargo.toml; then
        echo "arch_lint_rev in the justfile is not the rust-support rev Cargo.toml pins" >&2
        exit 1
    fi
    [ -x '{{arch_lint}}' ] || cargo install --quiet --git https://github.com/benjaminabbitt/rust-support \
        --rev {{arch_lint_rev}} arch-lint --locked --root '{{arch_lint_root}}'
    '{{arch_lint}}' --manifest-path Cargo.toml --rules arch-rules.toml

test:
    cargo test --workspace

test-purge:
    cargo test --workspace --features purge

# The program that deletes: `cherenkov` built with the purge feature (the
# release names it cherenkov-purge). Prints its path.
build-purge:
    cargo build --release -p cherenkov --features purge

# Tests against real debuggers and toolchains (dlv, go). Slow. Skips the
# fixture regenerator, which only `just regen-replay-fixtures` runs.
test-slow:
    cargo test --workspace -- --ignored --skip regenerate_the_fixtures_of_the_current_rules_version

# The tier P gate (runs in `just test`): every arithmetic patch against the
# mutant rustc compiles from the same source, natively, over edge values and
# seeded random inputs (every value of an 8-bit type), and the committed
# isa/tierp-shapes.toml against what the gate finds.
tierp-gate:
    cargo test -p cherenkov-exec-dap --test tierp_gate

# Regenerate isa/tierp-shapes.toml with the toolchain in use and fail on any
# difference from the committed one (CI: a toolchain bump; commit the result).
tierp-harvest:
    TIERP_HARVEST=write cargo test -p cherenkov-exec-dap --test tierp_gate
    git diff --exit-code -- isa/tierp-shapes.toml

# The same corpus run twice must decide every mutant the same way: run it
# on a loaded machine to mean something. Outside `just check`.
test-determinism:
    cargo test -p cherenkov --test run -- --ignored test_run_twice_decides

# Record the replay fixtures of cherenkov-run-domain's current rules version
# (its major.minor), after a change to what the aggregates' state holds or how
# their events fold bumped it. Writes crates/core/domain/cherenkov-run-domain/
# tests/fixtures/<major.minor>/ and refuses a version whose directory exists:
# a recorded version is added to, never rewritten.
regen-replay-fixtures:
    cargo test -p cherenkov-run-domain --test replay -- --ignored regenerate_the_fixtures_of_the_current_rules_version

# List planned mutants for a directory, e.g. `just plan fixtures/go`.
plan dir *args:
    cargo run -q -p cherenkov -- plan {{dir}} {{args}}

mutants file:
    git worktree add --detach ../.mutants-worktree HEAD
    cargo mutants -d ../.mutants-worktree --in-place --timeout 120 -f {{file}} -- --lib
    git worktree remove ../.mutants-worktree --force

# Pinned gitleaks image for the secrets scans. Bumps are deliberate commits.
GITLEAKS_IMAGE := "ghcr.io/gitleaks/gitleaks:v8.28.0@sha256:cdbb7c955abce02001a9f6c9f602fb195b7fadc1e812065883f695d1eeaba854"

# The first container runtime whose daemon answers (docker's may be down
# where podman works).
_container-runtime:
    #!/usr/bin/env bash
    for r in docker podman; do
        if command -v "$r" >/dev/null 2>&1 && "$r" info >/dev/null 2>&1; then
            echo "$r"; exit 0
        fi
    done
    echo "no working container runtime (docker or podman, `info` must succeed)" >&2
    exit 1

# Refuse the commit if any staged file contains a secret (gitleaks). Scans the
# staged (index) content of added/copied/modified/renamed files, exported to a
# temp tree; honours a repo-local `.gitleaks.toml` and `.gitleaksignore`.
scan-secrets:
    #!/usr/bin/env bash
    set -euo pipefail
    repo_root="$(git rev-parse --show-toplevel)"
    runtime="$(just --justfile '{{justfile()}}' _container-runtime)"
    stage="$(mktemp -d)"
    trap 'rm -rf "${stage}"' EXIT
    git diff --cached --name-only -z --diff-filter=ACMR \
        | git checkout-index --prefix="${stage}/src/" -z --stdin
    [ -d "${stage}/src" ] || exit 0
    args=(dir . --no-banner --redact --verbose)
    if [ -f "${repo_root}/.gitleaks.toml" ]; then
        cp "${repo_root}/.gitleaks.toml" "${stage}/gitleaks.toml"
        args+=(--config /scan/gitleaks.toml)
    fi
    if [ -f "${repo_root}/.gitleaksignore" ]; then
        cp "${repo_root}/.gitleaksignore" "${stage}/gitleaksignore"
        args+=(--gitleaks-ignore-path /scan/gitleaksignore)
    fi
    if ! "${runtime}" run --rm --network none -v "${stage}:/scan:ro" -w /scan/src \
        "{{GITLEAKS_IMAGE}}" "${args[@]}"; then
        {
            echo
            echo "REJECTED: gitleaks found a secret in the staged changes (above)."
            echo "Remove it from the change and rotate it if it was ever real."
            echo "A confirmed false positive goes in .gitleaksignore (its"
            echo "fingerprint) or a .gitleaks.toml allowlist, never --no-verify."
        } >&2
        exit 1
    fi

# Scan the whole git history for secrets (CI, and on demand). Findings are
# redacted. Honours `.gitleaks.toml` and `.gitleaksignore`.
scan-secrets-all:
    #!/usr/bin/env bash
    set -euo pipefail
    repo_root="$(git rev-parse --show-toplevel)"
    runtime="$(just --justfile '{{justfile()}}' _container-runtime)"
    # gitleaks reads the repository's .git; a worktree's .git is a file
    # pointing outside the mount, so scan the common git dir's checkout.
    common="$(cd "$(git rev-parse --git-common-dir)" && pwd)"
    args=(git /repo --no-banner --redact --verbose)
    if [ -f "${repo_root}/.gitleaks.toml" ]; then
        args+=(--config /repo/.gitleaks.toml)
    fi
    if [ -f "${repo_root}/.gitleaksignore" ]; then
        args+=(--gitleaks-ignore-path /repo/.gitleaksignore)
    fi
    stage="$(mktemp -d)"
    trap 'rm -rf "${stage}"' EXIT
    cp -r "${common}" "${stage}/.git"
    git -C "${stage}" config --unset core.worktree 2>/dev/null || true
    git -C "${stage}" config core.bare false
    [ -f "${repo_root}/.gitleaks.toml" ] && cp "${repo_root}/.gitleaks.toml" "${stage}/"
    [ -f "${repo_root}/.gitleaksignore" ] && cp "${repo_root}/.gitleaksignore" "${stage}/"
    if ! "${runtime}" run --rm --network none -v "${stage}:/repo:ro" -w /repo \
        "{{GITLEAKS_IMAGE}}" "${args[@]}"; then
        echo "REJECTED: gitleaks found a secret in the history (above); see scan-secrets for the remedy." >&2
        exit 1
    fi
    echo "scan-secrets-all: no leaks found"

# lefthook's pre-push step: `check` on the working tree. Red tests are allowed
# off the default branch (warned: a TDD red commit) and refused on it. git's
# pushed refs arrive on stdin; a push to refs/heads/master is judged strictly.
# Simpler than angzarr's push-check: it tests the working tree, not each pushed
# commit's own tree. Check a push to master by hand:
#     echo "refs/heads/B $(git rev-parse HEAD) refs/heads/master 0" | just push-check
push-check:
    #!/usr/bin/env bash
    set -uo pipefail
    to_default=0
    while read -r _local_ref local_sha remote_ref _remote_sha; do
        [ "${local_sha//0/}" = "" ] && continue  # a deletion
        [ "${remote_ref}" = "refs/heads/master" ] && to_default=1
    done
    if just --justfile '{{justfile()}}' check; then
        exit 0
    fi
    if [ "${to_default}" = 1 ]; then
        echo "FAIL push-check: red tests (or format/lint) cannot be pushed to master." >&2
        exit 1
    fi
    echo "RED  push-check: check failed; allowed off master, but this branch cannot reach master until it passes" >&2

import 'devcontainer.just'

mod? dev '.dev'

# Compare `cherenkov plan --operators cargo-mutants` with `cargo mutants --list` on the
# corpus (scripts/cm-corpus.txt); fails on a difference scripts/cm-differences.txt does not explain.
cm-differential *args:
    cargo build -q -p cherenkov
    python3 -I scripts/cm-differential.py {{args}}

# Regenerate the operator tables of the docs (blocks between `<!-- generated: cherenkov ARGS -->`
# markers) from `cherenkov operators`; a test fails when a committed block differs from the dump.
docs:
    cargo build -q -p cherenkov
    python3 -I scripts/gen-operator-docs.py docs/mutation-operators.md docs/operator-sets.md docs/rust-operator-evaluation.md
