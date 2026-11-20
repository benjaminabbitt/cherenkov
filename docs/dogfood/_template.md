# Dogfood record: DATE, TARGET

Copy this file to `YYYY-MM-DD-target.md`, fill every field, and add a row to
the table in [README.md](README.md). The rules:

- A number that was not measured is written `not recorded`. A number is
  never filled in by estimate, and a derived number says what it was derived
  from.
- A figure copied from the tool's own output says which file. Raw artifacts
  the record cites go to `data/<record name>/` (small ones only; no source
  text of other projects).
- If the machine was loaded, say so here, with the load average.

## Run

| | |
|---|---|
| Date and time (UTC) | |
| Target repo and exact commit | |
| Target scope (packages, files, diff) | |
| Cherenkov commit and version | |
| Comparison tool and version | cargo-mutants 26.2.0 |
| Operator set | |
| Diff scope | whole target / `--in-diff FILE` (what the file covers) |
| Command lines | Cherenkov: <br> comparison tool: |
| Jobs | Cherenkov workers: <br> comparison tool `-j`: |

## Machine

| | |
|---|---|
| Cores, RAM, OS | |
| Load average at the time (1, 5, 15 minutes) | sampled before: <br> sampled after: <br> or `not recorded` and what is known |
| Other work on the machine | |

## Timings

Wall time, in seconds, from the tool's own output (name the source).

| Phase | Cherenkov | cargo-mutants |
|---|---|---|
| Build | | baseline build |
| Planning / listing | | listing and scratch copy |
| Baseline run | | baseline test phase |
| Recording run | | not applicable |
| Mutants | | sum of the mutants' build phases and of their test phases, separately |
| Total wall | | `end_time - start_time` of `outcomes.json` |

Say which of these are derived (for example "total wall minus the run's own
duration") and which phases the tool does not report separately.

## Disk

| | Cherenkov | cargo-mutants |
|---|---|---|
| Peak extra disk of the run | growth of the target dir, plus anything in the temp dir | the scratch copies, one `target` per job |
| Peak max RSS | | |
| How measured | `scripts/measure-run.sh` / not recorded | |

## Verdicts

Cherenkov (counts per verdict; `unsupported` with its reason codes):

| killed | survived | timeout | not-called | unviable | unsupported |
|---|---|---|---|---|---|

Reason codes of the unsupported mutants:

cargo-mutants (caught, missed, timeout, unviable):

Kill rate over the mutants both tools could run (detected = killed or
timeout; Cherenkov and cargo-mutants on the same set):

## Agreement, mutant by mutant

How the mutants were matched (file, line, column, original, replacement),
and what was left unmatched.

| | Count |
|---|---|
| Agree detected | |
| Agree missed | |
| Agree unviable | |
| Disagree: cargo-mutants detected, Cherenkov missed | |
| Disagree: cargo-mutants missed, Cherenkov detected | |
| Cherenkov could not run (unsupported), by cargo-mutants's verdict | |
| Matched on one side only | |

## Disagreements

One entry per disagreement: file and line, what each tool said, who is right
and why, and whether it was fixed.

## Caveats

Load, flakiness (a mutant whose verdict changed between runs), timeouts that
depend on the budget, differences in the build (features, toolchain), and
anything the numbers above do not support.
