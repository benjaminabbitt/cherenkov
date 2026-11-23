default: check

# Format, lint and test the workspace. Needs protoc on PATH and access to
# github.com.
check: fmt-check lint test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Tests against real debuggers and toolchains (dlv, go). Slow.
test-slow:
    cargo test --workspace -- --ignored

# The same corpus run twice must decide every mutant the same way: run it
# on a loaded machine to mean something. Outside `just check`.
test-determinism:
    cargo test -p cherenkov-cli --test run -- --ignored test_run_twice_decides

# List planned mutants for a directory, e.g. `just plan fixtures/go`.
plan dir *args:
    cargo run -q -p cherenkov-cli -- plan {{dir}} {{args}}

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
    cargo build -q -p cherenkov-cli
    python3 -I scripts/cm-differential.py {{args}}

# Regenerate the operator tables of the docs (blocks between `<!-- generated: cherenkov ARGS -->`
# markers) from `cherenkov operators`; a test fails when a committed block differs from the dump.
docs:
    cargo build -q -p cherenkov-cli
    python3 -I scripts/gen-operator-docs.py docs/mutation-operators.md docs/operator-sets.md docs/rust-operator-evaluation.md
