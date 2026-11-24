# Cherenkov Mutation Testing: Make Weak Tests Glow, at the Speed of Light

Cherenkov is a mutation tester that applies mutants under a debugger instead
of recompiling them. It builds the project's test binaries once. For each
mutant it either patches machine code (tier P) or stops at a breakpoint and
changes state or control flow through GDB (tier B). The tests run in one
reused debugger session per test binary.

Cherenkov measures test quality: whether your tests detect faults in the
code they call. It does not measure coverage and runs no coverage tool.

Nothing in Cherenkov writes source files. Every mutant exists only in the
memory of a process under the debugger.

Design notes: [`docs/design.md`](docs/design.md).

## Time savings

Every dogfood run has a record with timings and the machine's load:
[`docs/dogfood/`](docs/dogfood/README.md) lists them. The table below is the
first run, 2026-11-17 ([record](docs/dogfood/2026-11-17-time-savings.md), which
marks what was not captured then). It was measured on x86-64 Linux, debug builds,
against cargo-mutants 26.2.0.

| Target | Mutants | cargo-mutants | Cherenkov | Verdicts |
|---|---|---|---|---|
| ctxloom `deps` package | 305 | 605 s (264 caught, 41 unviable, 0 missed) | 38 s (237 killed, 68 not yet runnable) | 0 disagreeing |
| ctxloom `address.rs` | 51 | 89.6 s | about 13 s | both 51/51 |
| fixture `rust-ops` | | 34.8 s | 8.0 s | same verdicts |

On the `deps` package Cherenkov is 16 times faster. The 68 mutants it cannot
run yet are being closed. The later records, from 2026-11-19 on three angzarr
packages ([bus](docs/dogfood/2026-11-19-angzarr-bus.md),
[embedded](docs/dogfood/2026-11-19-angzarr-embedded.md),
[ports](docs/dogfood/2026-11-19-angzarr-ports.md)), were taken on a loaded machine
and compare mutant by mutant; they also list the disagreements.

Why it is faster:

- **No per-mutant rebuild.** cargo-mutants edits the source, then recompiles
  and relinks for every mutant. Cherenkov compiles once.
- **One debugger session per test binary.** The session is reused across
  mutants instead of starting a process and debugger each time.
- **Each mutant runs only the tests that call its code.** One recording run
  notes, at each planned function's entry, which test is on the stack (a
  GDB-side breakpoint that never stops the run). A mutant then runs only
  those tests. A function called by too many tests, or from a thread with no
  test on its stack, runs them all. Mutants no selected test calls are
  counted as `not-called` and skipped. This alone took the `deps`
  package from 109 s to 38 s.

## Dogfooding

Cherenkov is run on real projects and compared mutant by mutant with
cargo-mutants: that is a dogfood run. The records are in
[`docs/dogfood/`](docs/dogfood/README.md), one per run, from a template. Each
has the commits and command lines, the wall time of each phase for both tools
(build, planning, baseline, recording, mutants, total), the peak disk, the
load average of the machine, the verdict counts, the agreement table and the
disagreements with who is right.

## Disk space

cargo-mutants copies the source tree into a scratch directory and builds it
there. That is a full second `target/` per parallel job, on top of the
project's own build. Cherenkov uses the project's own test build. It makes no
copy and no extra `target/`.

Measured on the `rust-ops` fixture with `cargo mutants -j1 --leak-dirs`:

| | Size |
|---|---|
| Fixture source (without `target/`) | 16 KB |
| Project's own `target/` | 19 MB |
| cargo-mutants scratch copy (one job) | 19 MB extra |
| Cherenkov | 0 extra |

With `-j N`, cargo-mutants keeps N scratch builds. We have not measured
larger projects; the scratch build should scale with the size of their
`target/`.

## Mutation rule sets, across languages

Mutations form one language-neutral, named superset (for example
`ror/lt->le`, `aor/rem->div`, `struct-field/delete`). Operators are written
once against a small concept model. No operator code branches on language.

Languages are data, in `profiles/<lang>/`:

- `concepts.scm`: tree-sitter queries that map syntax to concepts
- `tokens.toml`: the language's operator tokens
- `profile.toml`: file extensions, excludes, default values, body syntax

Named operator sets select from the superset. Each set names its source and
what it does not cover yet:

```console
$ cherenkov sets   # abridged: long lines and the pending lists are cut
breadth (default for the other languages): the six prototype operators: one per action kind (design section...
  source: docs/design.md section 4.9
  operators (6):
    delete-statement: call
    aor (Mothra AOR): add->sub (Dextool AORS) sub->add (Dextool AORS) mul->div (Dextool AORS) div->mul (Dex...
    ror (Mothra ROR): lt->le (PIT CONDITIONALS_BOUNDARY) le->lt (PIT CONDITIONALS_BOUNDARY) gt->ge (PIT CON...
    negate-condition: negate-condition
    return-bool-flip: return-bool-flip
    body-default: body-default
cargo-mutants (default for rust): what cargo-mutants generates, for a mutant-by-mutant comparison
  source: cargo-mutants 26.2.0
  operators (11):
    ror (Mothra ROR): eq->ne (PIT NEGATE_CONDITIONALS) ne->eq (PIT NEGATE_CONDITIONALS) lt->eq lt->gt lt->l...
    lcr: and->or (Mothra LCR) or->and (Mothra LCR)
    aor (Mothra AOR): add->sub (Dextool AORS) add->mul sub->add (Dextool AORS) sub->div mul->add mul->div (...
    shift: shl->shr shr->shl
    bitwise: and->or and->xor or->and or->xor xor->or xor->and
    compound-assign: add->sub (Dextool AORS, Mothra AOR) add->mul (Mothra AOR) sub->add (Dextool AORS, Moth...
    unary (cargo-mutants UnaryOperator): delete-not (Dextool UOI) delete-neg
    match-arm (cargo-mutants MatchArm): delete
    match-guard (cargo-mutants MatchArmGuard): true false
    struct-field (cargo-mutants StructField): delete
    fn-value (cargo-mutants FnValue): fn-value
sufficient: non-redundant sufficient sets plus operators real faults need
  source: docs/operator-sets.md (Kaminski 2013, Just & Schweiggert 2014, Just et al. 2014, mutest-rs 2026)
  operators (8):
    ror (Mothra ROR): lt->le (PIT CONDITIONALS_BOUNDARY) lt->ne gt->ge (PIT CONDITIONALS_BOUNDARY) gt->ne l...
    lcr: and->eq or->ne
    aor (Mothra AOR): add->sub (Dextool AORS) sub->add (Dextool AORS) mul->div (Dextool AORS) div->mul (Dex...
    shift: shl->shr shr->shl
    bitwise: and->or or->and xor->and
    compound-assign: add->sub (Dextool AORS, Mothra AOR) sub->add (Dextool AORS, Mothra AOR) mul->div (Dext...
    delete-statement: call
    fn-value (cargo-mutants FnValue): fn-value
```

Each mutation records where it came from and which other tools have the same
or a similar rule, so results can be compared across tools and languages:

```console
$ cherenkov mutations
ror/lt->le
  derived from cargo-mutants
  derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014
  derived from Cherenkov design
  same as PIT CONDITIONALS_BOUNDARY
  same as mutmut
  same as go-mutesting
  same as Stryker
  same as Gremlins
  same as Mull
ror/lt->eq
  derived from cargo-mutants
...
```

Background: [`docs/operator-sets.md`](docs/operator-sets.md),
[`docs/operator-research.md`](docs/operator-research.md),
[`docs/rust-operator-evaluation.md`](docs/rust-operator-evaluation.md).

Language status:

| Language | Debugger | Status |
|---|---|---|
| Rust | GDB (14 or later) | The mature path: `plan` and `run`, fidelity-checked |
| Go | Delve | `plan` and `run`; earlier prototype |
| Python | debugpy | Profile only, for `plan`; `run` is not implemented |

## Quick start

Requirements: Linux on x86-64, `gdb` 14 or later, `cargo`, and `protoc` on
`PATH` (or named by `$PROTOC`) to build: the run layer's messages are
generated from `proto/` at build time. macOS, ARM64 and LLDB are planned.

```sh
cargo build --release -p cherenkov

# List the mutants that would run.
target/release/cherenkov plan path/to/project --operators cargo-mutants

# Run them.
target/release/cherenkov run path/to/project --operators cargo-mutants

# Only what one test calls (libtest name; repeatable).
target/release/cherenkov run path/to/project --test calc::test_total
```

`--operators` defaults to `cargo-mutants` for Rust and `breadth` for Go and Python. `--exclude GLOB` skips more files.

### Which tests judge a mutant (Rust workspaces)

As in cargo-mutants (`docs/test-scope.md`): a mutant is judged by the tests
of its own package, unless `--test-workspace true` (whole workspace) or
`--test-package A,B` (those packages) says otherwise, on the command line or
as `test_workspace` / `test_package` in `.cargo/mutants.toml`. Only the test
programs of the packages that judge a planned mutant are built, prepared and
recorded, so `--in-diff` on a large workspace costs about what the diff
touches. Each phase's wall time is written to stderr (`cherenkov: phase
build: 0.3 s`, `viability check`, `preparation and recording`, `mutants`).

Output is a stream of events, each written as it happens
(`docs/output-events.md`): JSON Lines when stdout is not a terminal, text
on one (`--format json|text` chooses). `plan` is the dry run: the run
planned, nothing run. Each mutant's verdict is written the moment it is
decided, in completion order, with what it changed and who killed it:

```text
f2ba84283a346cc0 src/lib.rs:4:5 total aor killed p
  `+` becomes `-`
  killed by test_total at tests/calc.rs:5:5: got -1, expected 5
  took 492 ms
```

```sh
cherenkov run . | jq 'select(.event == "mutant.decided" and .verdict == "survived")'
```

An error is `{"code", "args", "message"}` on stderr under JSON (a line
`cherenkov: message` under text), with exit code 32 when something broke
and 48 when the request cannot be done (no project, no debugger, no such
operator set, a store another run holds). Every success exits 0, or 16 under the global
`--detailed-exitcode` flag (failures and refusals keep 32 and 48, a usage
error is 48); no success says more than "ok", so 17 ("would change") is
never used.

A run is kept in a store (`.cherenkov/store/runs.sqlite`, or `--store FILE`;
`--ephemeral` keeps nothing), one process at a time: a second `cherenkov run`
on a held store is refused at once, naming the holder. Every run is kept;
`cherenkov store prune --keep N [--dry-run]` keeps the last N finished runs,
and `cherenkov store unlock` clears what a killed run left of its hold (it
refuses while the holder is alive).

Development recipes (`justfile`):

- `just check`: format check, clippy, and the fast tests
- `just tierp-gate`: the tier P arithmetic gate alone (compiles and runs
  natively every arithmetic patch against rustc's own mutant; about 40 s, in
  `just check` too)
- `just tierp-harvest`: rewrites `isa/tierp-shapes.toml` (the arithmetic
  shapes tier P may patch in Rust code) with the toolchain in use and fails
  if it differs from the committed one; run it after a rustc bump
- `just test-slow`: tests against real debuggers and toolchains (gdb, dlv, go)
- `just test-determinism`: runs the Rust fixture twice under gdb and fails if
  any mutant's verdict or tier differs between the runs (slow; not in
  `just check`)
- `just plan <dir>`: list planned mutants for a directory

## Fidelity

A debugger mutant is only useful if it behaves like the source mutant.
`crates/test/fidelity/cherenkov-fidelity` checks each mutant against compiling the mutated
source and running the same tests.

Current state on the `rust-ops` fixture with the `cargo-mutants` set: every
runnable mutant is faithful. Mutants that do not compile are unviable, as
cargo-mutants also finds. The rest are refusals on a written list
(`ACCEPTED_REFUSALS` in `crates/test/fidelity/cherenkov-fidelity/tests/faithful.rs`):

- `struct-field/delete`: deleting a field of a struct literal has no patch
  or effect.
- `aor/rem->div` in one closure: GDB cannot evaluate `*x / 2` through
  `&&i64`.

Any other refusal fails the test. So does a list entry that is no longer
refused, so the list stays current.

## Limits

- Struct field deletion is not supported.
- Some values of project-defined types cannot be built in the debugger, so
  some mutants are not yet runnable (68 of 305 on the ctxloom `deps` package).
- Linux on x86-64 only.
- Go is less mature than Rust. Python cannot run mutants yet.

## Development container

`.devcontainer/` defines a dev container with everything Cherenkov needs: Rust
1.96, `protoc`, GDB 16, Delve and Go, debugpy, `just`, `lefthook`, `gitleaks` and
cargo-mutants 26.2.0, all pinned. Open the folder in a dev container, or run `just
devcontainer-build` and `just devcontainer-shell`. The container is started with
`--cap-add=SYS_PTRACE` and `--security-opt=seccomp=unconfined` so that GDB and
Delve can trace processes. CI calls `just` recipes, not tool commands, so the
recipes you run in the container are the ones CI runs.

## Licence

Apache-2.0 OR BSD-3-Clause, at your option: see [LICENSE-APACHE](LICENSE-APACHE)
and [LICENSE-BSD](LICENSE-BSD).

One source file is derived from cargo-mutants (MIT): see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
