# Dogfood record: 2026-11-19, angzarr-embedded

Taken by hand from the run's output (the CI job and `cherenkov compare` do not
write records yet). Taken on a loaded machine: the timings are not a quiet-machine
comparison.

## Run

| | |
|---|---|
| Date and time (UTC) | Cherenkov 2026-11-19 22:01:36 to 22:03:13; cargo-mutants 2026-11-19 20:27:16 to 21:16:49 |
| Target repo and commit | angzarr-embedded, `angzarr-embedded` package (`crates/angzarr-embedded`), commit `232580d` as named in the run notes. The run worktrees are gone; the commit was not re-derived from them |
| Target scope | cargo-mutants: packages `angzarr-embedded` (90 mutants) and `angzarr-store-sqlite` (1), 91 together. Cherenkov: `crates/angzarr-embedded` (90 mutants) |
| Cherenkov commit and version | version 0.1.0; commit `a6d6381` ("a mutant in awaiting code runs every test") or the working tree on it; run 1 (below) predates that fix. The binary is not preserved and the working tree's state was not recorded, so this is not exact |
| Comparison tool | cargo-mutants 26.2.0, toolchain 1.96.1 |
| Operator set | `cargo-mutants` |
| Diff scope | Cherenkov `--in-diff emb.diff` (per the run notes); cargo-mutants: the packages as above, its command line was not recorded. Its baseline is `cargo test --no-run --verbose --package=angzarr-embedded@0.1.0 --package=angzarr-store-sqlite@0.1.0` |
| Command lines | Cherenkov: `cherenkov run <crate dir> --operators cargo-mutants --in-diff emb.diff --deficiency-log F` (per the run notes, without `--all-features`). cargo-mutants: not recorded |
| Jobs | Cherenkov: 20 workers. cargo-mutants: `-j` not recorded; the sum of its phase durations (2972.1 s) equals its wall time (2972.5 s), so it ran **one job at a time** |

## Machine

| | |
|---|---|
| Cores, RAM, OS | 20 cores, 31 GiB, Debian 13 (Linux 6.12), x86-64 |
| Load average at the time | not sampled by the run. The run notes say 14 to 50 on 20 cores throughout; that is an observation, not a sample |
| Other work | the angzarr-bus cargo-mutants run started in the same second as this one and ran 70 minutes beside it; other sessions' builds |

## Timings

Wall seconds. Cherenkov's from `time` and the run's events
(`data/2026-11-19-angzarr-embedded/cherenkov-run-events.jsonl`); cargo-mutants's from
`outcomes.json` (`cargo-mutants-timing.json`).

| Phase | Cherenkov (run 2) | cargo-mutants |
|---|---|---|
| Build | not reported separately; the build and the planning together are the 24.7 s before `run.started` (wall 121.98 s from `time`, minus `took_ms` 97.28 s). Incremental | baseline build 55.9 s; the mutants' build phases sum to 486.4 s (embedded 479.1 s, store-sqlite 7.2 s) |
| Planning / listing | in the 24.7 s above | about 0.6 s (first build starts at 0.58 s of `debug.log`) |
| Baseline run | 0.65 s (535 tests; `baseline_ms`) | baseline test phase 32.7 s |
| Recording run | preparation ended at +35.7 s (`prepared_ms` 35713, which includes the baseline) | not applicable |
| Mutants | 61.6 s from the end of preparation to `run.finished` (derived); the 90 mutants' own times sum to 950.0 s over 20 workers | build 486.4 s + test 2397.2 s = 2883.6 s |
| Total wall | **97.28 s** (`took_ms`), 121.98 s by `time` | **2972.5 s = 49 min 32 s** (20:27:16.48 to 21:16:48.99 UTC), including 0.6 s of listing and copying |

Cherenkov's run 1 the same day (before the awaiting-code fix; first build in the
worktree, which the run notes call cold): wall 125.97 s by `time`, `took_ms`
46.17 s, so 79.8 s before `run.started` (the build and the planning), preparation
13.99 s, 90 mutants, 11 timeouts where run 2 had 16. Run 2 is the record's run.

## Disk

| | Cherenkov | cargo-mutants |
|---|---|---|
| Peak extra disk | not recorded. It builds in the worktree's own `target/`; the growth was not measured | not recorded |
| Max RSS | not recorded (the run's log: 20 workers, about 1145 MiB per worker available) | not recorded |
| Known from elsewhere | none | the scratch copy of the same project's source, **source only, before any build**: 3,339,466 bytes in 220 files per scratch directory (`debug.log` of a later cargo-mutants run, `me1`, which made 6 copies, so 6 jobs). A different run and a lower bound: each directory also holds a `target/`, which the log does not size |

## Verdicts

Cherenkov (90 mutants, run 2, `exit 0`, `complete`):

| killed | survived | timeout | not-called | unviable | unsupported |
|---|---|---|---|---|---|
| 5 | 2 | 16 | 4 | 5 | 58 |

Unsupported, by reason code (`data/2026-11-19-angzarr-embedded/deficiencies.jsonl`, 58 rows,
trimmed of source text): generic-copies 36, no-patch-rule 9, value-not-buildable 9,
value-name-unresolved 4.

cargo-mutants (91 mutants): caught 42, missed 11, timeout 7, unviable 31. For
`angzarr-embedded` alone (90): caught 41, missed 11, timeout 7, unviable 31.

| | Detected | Viable | Rate |
|---|---|---|---|
| cargo-mutants, all 90 embedded mutants | 48 | 59 | 81.4% |
| cargo-mutants, the 81 matched | 43 | 50 | 86.0% |
| Cherenkov, the 27 mutants it ran | 21 | 27 | 77.8% |
| cargo-mutants, on those same mutants | 22 | 26 | 84.6% |

The 84.6% against 77.8% is the like-for-like figure.

## Agreement, mutant by mutant

81 matched. The 9 unmatched on each side are the nine `struct-field` `delete field`
mutants: cargo-mutants has them at one column, Cherenkov plans them as unsupported
(`no-patch-rule`) at a different line, so they did not match; they are the same
mutants, and Cherenkov could not run them. Rows: `data/2026-11-19-angzarr-embedded/agreement.tsv`.

| | Count |
|---|---|
| Agree detected | 21 |
| Agree missed | 4 |
| Agree unviable | 5 |
| Disagree: cargo-mutants detected, Cherenkov survived | 1 |
| Disagree: cargo-mutants missed, Cherenkov detected | 0 |
| cargo-mutants unviable, Cherenkov not-called | 1 |
| Cherenkov unsupported: cargo-mutants detected / missed / unviable | 21 / 3 / 25 |
| Unmatched, cargo-mutants side (5 detected, 4 missed) | 9 |
| Unmatched, Cherenkov side (all unsupported) | 9 |

Of the 58 unsupported, 49 are matched: 25 of them are mutants cargo-mutants finds
unviable (not real gaps), 21 it detects and 3 it misses. The other 9 are the
unmatched `delete field` mutants (5 detected, 4 missed by cargo-mutants). So about 33
are real gaps.

## Disagreements

1. `lib.rs:335` `kernel_samples -> vec![]`: cargo-mutants caught it, Cherenkov
   survived. Selection under the recording run is the likely cause (cargo-mutants
   caught it with a test the recording did not select); not investigated. Cherenkov
   has a false negative.
2. `error.rs:611` `from`: cargo-mutants unviable, Cherenkov not-called. Both agree
   no test detects it; cargo-mutants is right that the mutant does not exist.

Fixed since the first run (commit "a mutant in awaiting code runs every test"):
`served.rs:115` `relay_pending -> Ok(())` was caught by cargo-mutants and survived
or was not-called in Cherenkov because the recording did not see the killing test
call the async function; it now times out, so agrees. The run notes say
`served.rs:266` `missing_served -> Ok(())` agrees too; in this run's data it is
**unsupported** (`generic-copies`), so it is a gap, not an agreement. The notes
and the data differ here.

## Survivors

4 agreed-missed mutants. `step_relayed` (`served.rs:346` to `353`) comparisons are
not called by any test: a gap.

## Caveats

- **Load.** 14 to 50 on 20 cores, not sampled per run.
- **Parallelism.** Cherenkov 20 workers; cargo-mutants one job. Not like for like.
- **Timeouts.** 16 of Cherenkov's 90 are timeouts, many of them the hung-session
  restarts that the load makes common; the count moved from 11 (run 1) to 16
  (run 2) with the code and the load.
- **Which toolchain.** cargo-mutants used 1.96.1; Cherenkov built with the
  toolchain of the worktree (not recorded).
- A later partial cargo-mutants run, `me1` (89 mutants, 6 jobs, 38 outcomes, all
  unviable; no `end_time`), is kept as timing only in
  `data/2026-11-19-angzarr-embedded/cargo-mutants-partial-me1-timing.json`: its
  phases sum to 1427.7 s (baseline build 106.2 s and test 52.0 s; the mutants' build
  1269.5 s). It did not finish and is not compared.
