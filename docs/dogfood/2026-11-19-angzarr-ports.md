# Dogfood record: 2026-11-19, angzarr-ports

Taken by hand from the run's output (the CI job and `cherenkov compare` do not
write records yet). A small run: 14 mutants. Taken on a loaded machine.

## Run

| | |
|---|---|
| Date and time (UTC) | Cherenkov 2026-11-19 22:05:58 to 22:06:15; cargo-mutants: inside the 20:27:16 to 21:37:08 run of the [angzarr-bus record](2026-11-19-angzarr-bus.md) |
| Target repo and commit | angzarr `core`, `angzarr-ports` package (`crates/angzarr-ports`), commit `099ef5ae` as named in the run notes (not re-derived) |
| Target scope | 14 mutants (cargo-mutants's count for the package; Cherenkov planned the same 14) |
| Cherenkov commit and version | version 0.1.0; commit `a6d6381` or the working tree on it; not exact (see the bus record) |
| Comparison tool | cargo-mutants 26.2.0, toolchain `stable` |
| Operator set | `cargo-mutants` |
| Diff scope | the ports mutants in the bus run's `core.diff`; cargo-mutants ran them with the bus package in one invocation (`--package=angzarr-bus --package=angzarr-ports`) |
| Command lines | Cherenkov: `cherenkov run <crate dir> --operators cargo-mutants --in-diff core.diff` (per the run notes; the `time` file's copy is truncated). cargo-mutants: not recorded |
| Jobs | Cherenkov: 20 workers. cargo-mutants: one job (see the bus record) |

## Machine

| | |
|---|---|
| Cores, RAM, OS | 20 cores, 31 GiB, Debian 13 (Linux 6.12), x86-64 |
| Load average at the time | not sampled. The run notes say 14 to 50 on 20 cores throughout |
| Other work | other sessions' builds |

## Timings

| Phase | Cherenkov | cargo-mutants |
|---|---|---|
| Build | not reported separately; build and planning together are the 31.1 s before `run.started` (wall 47.83 s from `time`, minus `took_ms` 16.70 s). Includes ports' build script (the `wasm-*` program in the prepared event) | its share of the mutants' build phases: 63.4 s. The baseline build (34.4 s) is shared with the bus package |
| Planning / listing | in the 31.1 s above | shared with the bus run |
| Baseline run | 12 test programs prepared, baselines 0.04 to 0.76 s each (`baseline_ms`) | shared with the bus run (0.6 s) |
| Recording run | preparation of the 12 programs, in parallel, ended at +6.3 s | not applicable |
| Mutants | 10.4 s from the end of preparation to `run.finished` (derived); the 14 mutants' own times sum to 40.8 s | build 63.4 s + test 1.8 s = 65.2 s of phase time |
| Total wall | **16.70 s** (`took_ms`), 47.83 s by `time` | not separable: the 14 mutants ran inside a 4191.8 s one-job run; their phases account for 65.2 s of it |

## Disk

Not recorded for either tool; no figure known from elsewhere for this package.

## Verdicts

Cherenkov: killed 5, survived 1, unviable 1, unsupported 7 (no-patch-rule 6,
value-not-buildable 1), timeout 0, not-called 0.

cargo-mutants (the 14): caught 9, missed 4, unviable 1, timeout 0.

Kill rate: Cherenkov ran 6 mutants and detected 5 (83.3%); cargo-mutants on the same 6 detected 5 (83.3%).
cargo-mutants on all 14: 9 of 13 viable (69.2%).

## Agreement, mutant by mutant

8 matched; the 6 `struct-field` `delete field` mutants (`contract.rs:2331` to `2353`)
are unmatched on both sides (cargo-mutants has 3 caught and 3 missed there;
Cherenkov plans them as unsupported, `no-patch-rule`, at other columns).
Rows: `data/2026-11-19-angzarr-ports/agreement.tsv`.

| | Count |
|---|---|
| Agree detected | 5 |
| Agree missed | 1 |
| Agree unviable | 1 |
| Disagreements | 0 |
| Cherenkov unsupported (cargo-mutants detected), `value-not-buildable` | 1 |
| Unmatched, cargo-mutants side (3 detected, 3 missed) | 6 |
| Unmatched, Cherenkov side (all unsupported) | 6 |

## Disagreements

None among the 6 mutants both tools ran.

## Caveats

- Load: as in the other 2026-11-19 records.
- Too small a sample for a kill rate to mean much.
- cargo-mutants's time for this package cannot be separated from the bus run's.
