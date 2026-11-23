# Which tests judge a mutant

Cherenkov's Rust runs choose their test programs the way cargo-mutants
chooses its tests, so a diff-scoped run costs about what the diff touches.

## cargo-mutants' rules

Re-derived from cargo-mutants 26.2.0 (MIT, Copyright (c) 2022-2025 Martin
Pool; checked against the 27.1 binary's `--help` and key names): the book's
"Workspace and package support" page, `Options::from_args` in `options.rs`
(the precedence below) and the lab's per-mutant package choice in `lab.rs`.
Described here, not copied.

| Setting | Tests that judge a mutant |
|---|---|
| nothing set (default) | the package the mutant is in |
| `--test-workspace true`, or `test_workspace = true` | every package of the workspace |
| `--test-workspace false` | the default (and stops the configuration's `test_workspace = true`) |
| `--test-package A,B` (repeatable, comma separated), or `test_package = [...]` | packages A and B, for every mutant |

Precedence, first that applies, the command line over the file:

1. `--test-workspace true`
2. `--test-package`
3. `test_workspace = true` (only if `--test-workspace` was not given at all)
4. `test_package`
5. the mutated package

`--test-workspace false` on the command line therefore overrides the file's
`test_workspace = true` but leaves the file's `test_package` in force. The
rules are `TestPackages::resolve` in `crates/cherenkov-cli/src/test_scope.rs`.

## What Cherenkov does with them

- It builds only the test programs of the packages that judge a planned
  mutant (`cargo test --no-run --package ...`), after `--in-diff` has
  narrowed the plan. Under `test_workspace` it builds the whole workspace's,
  as before. A planned mutant whose file is in no workspace package falls
  back to the whole workspace.
- Every mutant is routed only to the programs of its judging packages
  (`cherenkov_exec_dap::routing::OnlyJudged`): a program records only the
  mutants it judges, and says `not-called` for the others without running
  them. A program no mutant is routed to is never built, prepared or
  recorded.
- Within a judging program a mutant runs only the tests the recording saw
  call its function (this was already so; `driven` in the run layer).
- The viability check (`cargo check --all-targets` on the scratch copy)
  compiles only the packages the planned mutants are in.
- `--test-package` is no longer a `--package` argument for every cargo
  call: it names the judging packages, as in cargo-mutants.

## Phase times

Written to stderr: `cherenkov: phase build`, `phase viability check`, and,
when the run finishes, `phase preparation and recording` (up to the last
test program prepared) and `phase mutants` (the rest).
