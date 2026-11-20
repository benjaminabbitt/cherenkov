# Run output: a stream of actionable events

## Requirements
- **Complete kill lists by default.** Every selected test runs; every test
  that kills a mutant is reported. An agent rewriting code needs to know
  every test guarding a behaviour, or it finds them one rerun at a time.
- **`--fast`** for CI and for people working iteratively: a mutant's run
  stops at its first failing test (libtest's own record of a failed test,
  not a panic: `#[should_panic]` and `catch_unwind` panic without failing).
  The event says the kill list is partial.
- **Actionable as it happens.** Output is a stream: each mutant's result is
  written the moment it is decided, in completion order, each event usable
  on its own. An agent starts on the first finding while the run goes on.
  (Plan order held results back behind the slowest earlier mutant.)
- **ctxloom's conventions** (see `ctxloom --help`):
  - `--format json|text`, JSON when stdout is not a terminal.
  - JSON on stdout only; everything for people goes to stderr in JSON mode.
  - Plain documents, no envelope.
  - Errors as `{"code": "<kebab-case>", "args": {..}, "message": ".."}`
    with a non-zero exit code.

  ctxloom prints one document per command; a run is a stream, so it prints
  one compact document per line (JSON Lines). `jq` reads a stream of
  documents natively: `cherenkov run . | jq 'select(.event ==
  "mutant.decided" and .verdict == "survived")'`.
- **The same events as the run layer.** These are the facts the
  event-sourced run layer (plan aggregate, spool, mutation-test projector on
  angzarr-embedded) records. Today the CLI writes them to stdout; later the
  projector records them and the CLI follows the aggregate. Moving is a
  change of transport, not of schema.

## Events
Every event has `event` (its name, the document's first key), `run` (the
run's id), and `at` (RFC 3339 time). Field names are snake_case. The
documents are the messages of `proto/cherenkov/output/v1/output.proto`,
written by the JSON generated from its options; durations (`baseline_ms`,
`prepared_ms`, `took_ms`) are milliseconds in 32 bits.

### `run.started`
`root`, `language`, `operators` (set name), `jobs`, `fast` (bool),
`mutants` (planned count).

### `mutant.planned`
One per mutant, in plan order, right after `run.started`: the mutant's
fields as `mutant.decided` has them, without a verdict. A dry run
(`cherenkov plan`) is `run.started` and these alone.

### `program.prepared`
A test program ready to run mutants: `program` (binary path), `baseline_ms`,
`prepared_ms`, `tests` (count; `null` when the executor cannot list them),
`selection` (`"recorded"`, `"recorded-with-failures"` plus `failed_tests`,
or `"every-test"` with `reason`). Written as each program is ready, not
once all are.

### `program.failed`
A test program that could not be prepared (its tests fail unmutated, the
debugger cannot run it): `program` (when known) and `reason`. The run
stops; `run.finished` follows with `status` `"failed"`.

### `mutant.decided`
One per mutant, in completion order. Self-contained:
- `id`, `file`, `line`, `column`, `end_line`, `end_column`, `function`;
- `operator` (e.g. `ror`), `original` and `mutated` text (the site's
  source and what the mutant has there; `mutated` is `null` for a body
  replacement whose value only the debugger knows), `description` (the
  action, e.g. `eval-branch "n >= 10"`);
- `verdict`: `killed`, `survived`, `timeout`, `unviable`, `unsupported`,
  `not-called` (no selected test calls its function: decided without a
  run), `not-reached` (its function is called but no test reaches the
  site), `error`; `tier` (`p` or `b`) when it ran;
- `took_ms`;
- `tests_run`: the tests the mutant ran with;
- for `killed`: `killed_by`, one entry per killing test, each with `test`,
  `location` (`file:line:col`), `message`, and `got`/`expected` for
  equality assertions; and `complete` (false under `--fast`);
- for `survived`: `executed_by`, the tests that executed the mutated code
  and did not notice — where a new or stronger assertion belongs;
- for `unsupported`, `unviable`, `not-reached`, `error`: `reason`, and
  `reason_code`, its short machine-readable form. For `unsupported` (a
  mutant Cherenkov cannot run, which a run accepts and logs, never fails
  on): `inlined`, `generic-copies`, `value-not-buildable`,
  `evaluation-hung`, `function-not-found`, `symbol-not-in-scope`,
  `evaluator-cannot-apply`, `debugger-failed` (the debugger session failed
  on this mutant), `no-breakpoint-site`, `no-patch-rule`, `other`. For
  `unviable`: `no-default`, `does-not-compile`. A hung mutant is `timeout`,
  decided in a few seconds: a run's budget is five times the unmutated run,
  two seconds at least (three times that for a run that evaluates at every
  hit).

### `run.finished`
`counts` by verdict, `deficiencies` (the `unsupported` mutants' count for
each `reason_code`), `took_ms`, `exit` (the process's exit code), `status`
(`"complete"`, or `"failed"` when the mutants could not run).

## The deficiency log
Every mutant Cherenkov could not run is a `mutant.decided` with verdict
`unsupported` and a `reason_code`: `cherenkov run . | jq 'select(.event ==
"mutant.decided" and .verdict == "unsupported")'`. `--deficiency-log FILE`
writes just those documents to a file, one a line, as they are decided.
`run.finished` counts them by reason in `deficiencies`.

## Scoping to a diff
`--in-diff FILE` (on `plan` and `run`) takes a unified diff, as
cargo-mutants does, and keeps the mutants whose span touches a line the
diff adds. Paths in the diff are from the repository's root.

## Text mode
The same events rendered for a person on stdout, one line per mutant as
today (`<id> <file:line:col> <fn> <op> <verdict> [tier]`), with kill and
survival details indented beneath it, in completion order. Tests compare
text output order-independently.

## Exit codes
0 when the run completes; non-zero for a run that could not run (no
project, tests that fail unmutated, debugger missing), with the error
document. Survivors are findings, not failures; a `--fail-on-survivor`
option for CI is a later addition.

## Order of work
1. Events and `--format` in the CLI: completion order, JSON Lines, ctxloom's
   flags and error shape, a `jq` round-trip test. Share ctxloom's format
   code through a library where it fits (the owner suggested it) rather
   than reimplementing it.
2. `--fast`: stop at the first failure libtest records.
3. The run layer: the same events through the spool and plan aggregate.
