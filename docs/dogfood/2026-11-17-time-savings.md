# Dogfood record: 2026-11-17, the first time-savings comparison

The first comparison, the one the top-level README's table is from. **Most fields
were not captured at the time**, and are marked `not recorded`. Nothing here is
estimated. The commands, commits and the target's exact revision were not kept.

## Run

| | |
|---|---|
| Date | 2026-11-17 (the times of day are not recorded for the ctxloom runs; the `rust-ops` raw files are timestamped 17:44 to 17:46 local, UTC-6) |
| Targets | (1) ctxloom, package `deps`; (2) ctxloom, `address.rs`; (3) the fixture `fixtures/rust-ops` of this repository |
| Target commits | ctxloom: not recorded. The fixture: this repository at the time; not recorded |
| Cherenkov commit and version | not recorded. The README's table was committed in `ced3f4f` (2026-11-17 21:19 -0600); the binary was not kept. Version 0.1.0 |
| Comparison tool | cargo-mutants 26.2.0 (the README's header; not re-checked) |
| Operator set | `cargo-mutants` for the comparison |
| Diff scope | whole target (a package, a file, a fixture); not recorded beyond that |
| Command lines | not recorded |
| Jobs | not recorded |
| Build | debug builds, per the README |

## Machine

x86-64 Linux, per the README. Cores, RAM, OS version and the load average:
**not recorded.**

## Timings and verdicts

From the README's table of the day; the `rust-ops` row's raw cargo-mutants output
is kept (`data/2026-11-17-time-savings/rust-ops-cargo-mutants.out`).

| Target | Mutants | cargo-mutants wall | Cherenkov wall | cargo-mutants verdicts | Cherenkov verdicts |
|---|---|---|---|---|---|
| ctxloom `deps` | 305 | 605 s | 38 s | 264 caught, 41 unviable, 0 missed | 237 killed, 68 not yet runnable; 0 disagreeing |
| ctxloom `address.rs` | 51 | 89.6 s | "about 13 s" | 51/51 | 51/51 |
| `rust-ops` fixture | 168 (from the raw output) | 34.8 s (the raw output: "168 mutants tested in 34s: 3 missed, 165 caught") | 8.0 s | 3 missed, 165 caught | "same verdicts" (the Cherenkov run's own output is not kept) |

Phase breakdowns (build, planning, baseline, recording, mutants) and the
cargo-mutants phase times: **not recorded** for all three. One figure from the
README's prose: the recording run and per-test selection took the `deps` run
from 109 s to 38 s (the 109 s configuration is not recorded).

Kill rate over the mutants both could run, on `deps`: Cherenkov ran 237 and killed
237 (100%), the README stating 0 disagreeing; cargo-mutants caught 264 of 264
viable. The per-mutant table was not kept, so the rate on the same set is not
recomputable.

## Disk

Peak extra disk of the runs: not recorded. A different, smaller measurement (the
README's "Disk space" section, `rust-ops` with `cargo mutants -j1 --leak-dirs`):
one cargo-mutants scratch copy was 19 MB extra; the project's own `target/` was
19 MB; Cherenkov added none. That is one scratch copy of a 16 KB fixture, not the
disk used by these runs.

## Agreement

`deps`: 0 disagreeing of the 237 Cherenkov ran; the mutant-by-mutant table was
not kept. `address.rs` and `rust-ops`: the same verdicts, by the README; no table kept.

## Inconsistency between sources

`docs/operator-sets.md` gives the `deps` execution as "Cherenkov kills 233, with 1
timeout and 71 it cannot run yet, in 176 s"; the README gives "237 killed, 68 not
yet runnable" in 38 s. The two describe different states of the code or
configuration; neither run's raw output was kept, so which state each describes
is not recorded.

## Caveats

The load average of the machine and whether other work ran during these
measurements were not recorded. The records of 2026-11-19 are the first with
timings per phase.
