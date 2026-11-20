# Dogfood record: 2026-11-19, angzarr-bus

Taken by hand from the run's output (the CI job and `cherenkov compare` do not
write records yet). Taken on a loaded machine: the timings are not a quiet-machine
comparison.

## Run

| | |
|---|---|
| Date and time (UTC) | Cherenkov 2026-11-19 22:03:34 to 22:05:24; cargo-mutants 2026-11-19 20:27:16 to 21:37:08 |
| Target repo and commit | angzarr `core`, `angzarr-bus` package (`crates/angzarr-bus`), commit `099ef5ae` as named in the run notes. The run worktrees are gone; the commit was not re-derived from them |
| Target scope | cargo-mutants: packages `angzarr-bus` (478 mutants) and `angzarr-ports` (14), 492 together. Cherenkov: `crates/angzarr-bus` (476 mutants); the `angzarr-ports` mutants are in [their own record](2026-11-19-angzarr-ports.md) |
| Cherenkov commit and version | version 0.1.0; commit `a6d6381` ("a mutant in awaiting code runs every test") or the working tree on it. The release binary is not preserved with the run and the working tree's state was not recorded, so this is not exact |
| Comparison tool | cargo-mutants 26.2.0, toolchain `stable` (the exact rustc version was not recorded) |
| Operator set | `cargo-mutants` (`cherenkov sets`) |
| Diff scope | Cherenkov `--in-diff core.diff` (per the run notes); cargo-mutants: the packages as above, its command line was not recorded |
| Command lines | Cherenkov: `cherenkov run <crate dir> --operators cargo-mutants --in-diff core.diff --deficiency-log F --cargo-arg=--all-features` (per the run notes; the `time` file's copy is truncated at the shell's column limit). cargo-mutants: not recorded. Its debug log shows the baseline as `cargo test --no-run --verbose --package=angzarr-bus@0.1.0 --package=angzarr-ports@0.1.0`, **without** `--all-features` |
| Jobs | Cherenkov: 20 workers (`run.started` `jobs`). cargo-mutants: `-j` not recorded; the sum of its phase durations (4189.3 s) equals its wall time (4191.8 s), so it ran **one job at a time**, with its default |

## Machine

| | |
|---|---|
| Cores, RAM, OS | 20 cores, 31 GiB, Debian 13 (Linux 6.12), x86-64 |
| Load average at the time | not sampled by the run. The run notes say 14 to 50 on 20 cores throughout, "other sessions were building"; that is an observation, not a sample. (A later, different run in the same session sampled 36.92, 38.98, 30.77: not this run.) |
| Other work | the angzarr-embedded cargo-mutants run started in the same second as this one and ran 49 minutes beside it; other sessions' builds |

## Timings

Wall seconds. Cherenkov's from the `time` output (`data/2026-11-19-angzarr-bus/` has the
run's `run.started`, `program.prepared` and `run.finished` events) and the
`run.finished` event; cargo-mutants's from `outcomes.json`.

| Phase | Cherenkov (run 6) | cargo-mutants |
|---|---|---|
| Build | not reported separately; the build and the planning together are the 6.5 s before `run.started` (wall 116.24 s from `time`, minus `took_ms` 109.74 s; includes the process's exit). Incremental build | baseline build 34.4 s; the mutants' build phases sum to 920.8 s (bus 857.4 s, ports 63.4 s) |
| Planning / listing | in the 6.5 s above | about 0.8 s: first build starts at 0.83 s of `debug.log` (listing and scratch copy) |
| Baseline run | 0.03 s and 0.14 s (the two test binaries; `baseline_ms`) | baseline test phase 0.6 s |
| Recording run | preparation of the two binaries, run in parallel, ended at +1.6 s (lib tests, 18 tests, `prepared_ms` 1570) and +33.0 s (contract tests, 26 tests, `prepared_ms` 32956). `prepared_ms` includes the baseline | not applicable |
| Mutants | 76.7 s from the end of preparation to `run.finished` (derived from event times); the 476 mutants' own times sum to 1466.9 s over 20 workers | build 920.8 s + test 3233.6 s = 4154.4 s (bus 4089.1 s, ports 65.2 s) |
| Total wall | **109.74 s** (`run.finished.took_ms`), 116.24 s by `time` | **4191.8 s = 69 min 52 s** (20:27:16.55 to 21:37:08.36 UTC), which includes the 0.8 s of listing and copying |

Cherenkov's earlier runs the same day (same target, earlier code and different load;
`took_ms` from each `run.finished`, wall by `time` not recorded except run 6):

| Run | UTC start | `took_ms` | Preparation ends | Note |
|---|---|---|---|---|
| 2 | 21:39:51 | 35.9 s | +3.9 s | 3 errors, 2 not-reached in the counts: timeouts were not yet exposed, so not comparable |
| 3 | 21:41:46 | 105.4 s | +21.6 s | |
| 4 | 21:45:00 | 74.4 s | +20.7 s | |
| 5 | 21:50:54 | 93.3 s | +13.1 s | the run the notes break down |
| 6 | 22:03:34 | 109.7 s | +33.0 s | this record's run; wall 116.24 s |

The spread, 74 to 110 s, with the preparation 13 to 33 s, is the machine's load
(and code changes between runs); no run was repeated under the same conditions.
The 35.9 s of run 2 is "36 s on a quiet machine" in the notes: the load at the
time was not sampled, and that run is not comparable (errors, no timeout handling).

### Where the mutants' time goes

From the 476 `mutant.decided` events of run 5 (the notes' breakdown): timeouts
121 mutants / 753 s (median 6.2 s with a 2 s budget: the hung session is killed
and restarted under load), unsupported 193 / 439 s, killed 47 / 104 s (median
1.6 s), survived 70 / 111 s (median 0.55 s). Not recomputed for run 6.

## Disk

| | Cherenkov | cargo-mutants |
|---|---|---|
| Peak extra disk | not recorded. It builds in the worktree's own `target/`; the growth was not measured | not recorded |
| Max RSS | not recorded | not recorded |
| Known from elsewhere | none | the scratch copy of the same package's source, **source only, before any build**: 9,045,304 bytes in 2,385 files per scratch directory (`debug.log` of a later cargo-mutants run on `angzarr-bus`, `m3`, which made 8 copies, so 8 jobs). This is a different run and a lower bound of a scratch directory: each also holds a `target/`, which the log does not size |

cargo-mutants put its scratch directories in `/tmp/cargo-mutants-*.tmp`, which on
this machine is a 16 GiB tmpfs, so they are RAM-backed. That is from the later
run's log, not this run's.

## Verdicts

Cherenkov (476 mutants, run 6, `exit 0`, status `complete`):

| killed | survived | timeout | not-called | unviable | unsupported |
|---|---|---|---|---|---|
| 47 | 70 | 124 | 10 | 31 | 194 |

Unsupported, by reason code (`data/2026-11-19-angzarr-bus/deficiencies.jsonl`, 194 rows,
trimmed of source text): value-name-unresolved 61, symbol-not-in-scope 35,
value-not-buildable 34, function-not-found 21, generic-copies 19,
no-patch-rule 14, evaluator-cannot-apply 7, evaluation-hung 2, other 1.

cargo-mutants (492 mutants, bus and ports together): caught 129, missed 95,
timeout 160, unviable 108. For `angzarr-bus` alone (478): caught 120, missed 91,
timeout 160, unviable 107.

Kill rate (detected = killed or timeout / caught or timeout; viable = not unviable):

| | Detected | Viable | Rate |
|---|---|---|---|
| cargo-mutants, all 478 bus mutants | 280 | 371 | 75.5% |
| Cherenkov, the 251 mutants it ran | 171 | 251 | 68.1% |
| cargo-mutants, on those same mutants | 171 | 247 | 69.2% |

The 69.2% against 68.1% is the like-for-like figure. (Four of Cherenkov's 251
are mutants cargo-mutants finds unviable, so cargo-mutants's set is 247.)

## Agreement, mutant by mutant

Matched by file, line, original and replacement; same-line twins by the mutated
text (Cherenkov's column is the expression's start, cargo-mutants's the operator's).
476 matched; 2 cargo-mutants mutants have no Cherenkov counterpart (`hub.rs:943`,
`+=` as `-=` and as `*=`, both caught). Rows: `data/2026-11-19-angzarr-bus/agreement.tsv`.

| | Count |
|---|---|
| Agree detected | 170 |
| Agree missed (cargo-mutants missed; Cherenkov survived 69 or not-called 6) | 75 |
| Agree unviable | 31 |
| Disagree: cargo-mutants detected, Cherenkov survived | 1 |
| Disagree: cargo-mutants missed, Cherenkov killed | 1 |
| cargo-mutants unviable, Cherenkov not-called | 4 |
| Cherenkov unsupported: cargo-mutants detected / missed / unviable | 107 / 15 / 72 |
| Cherenkov only | 0 |
| cargo-mutants only | 2 |

Of the 194 unsupported, 72 do not compile in source either (61 value-name-unresolved,
9 value-not-buildable, 2 generic-copies), so about 122 are real gaps: 107 would be
detected by cargo-mutants and 15 missed.

## Disagreements

1. `hub.rs:634` `delete !` (`Serve::place`, `let drives = !lane.driving` in a
   closure): cargo-mutants caught it, Cherenkov survived (tier B, assignment
   `drives = lane.driving`). The same mutant gave survived, timeout and error in
   different runs: the tier B assignment into a closure-local is unreliable.
   cargo-mutants is right; Cherenkov has a false negative. Not fixed.
2. `hub.rs:743` `delete match arm Body::Event(page)`: cargo-mutants missed it,
   Cherenkov killed it (tier P). Deleting an arm that has a wildcard fallthrough
   should behave like the source; Cherenkov's patch evidently does not. Probably
   Cherenkov is wrong (a false kill); not verified at instruction level. Not fixed.
3. Four mutants cargo-mutants finds unviable and Cherenkov ran to not-called
   (`hub.rs` 241, 247, 252 and `pump.rs:27`): both agree no test detects them;
   cargo-mutants is right that the mutant does not exist.

Resolved earlier the same day (commit "a prost Message derive gives a type a
Default"): `contract.rs:1116`, `contract.rs:1370` and `recovery.rs:147` were
called unviable by Cherenkov and compiled and ran in cargo-mutants (the types are
prost messages, whose derive gives them a `Default`). They agree now.

## Survivors

75 mutants both tools missed. Not judged one by one. By file: 34 in `contract.rs`
and 8 in `transport_contract.rs`, the conformance suite's own code (a function
replaced by `()` still passes because the suite is only run against correct
implementations: a gap in the suite, not an equivalent mutant); 18 in `hub.rs`, 12
in `store.rs`, 4 in `retry.rs`, 2 in `recovery.rs`, 1 in `credit.rs` (accessors,
counters, comparison boundaries, guards). `recovery.rs:116` (`||` and `&&`) and
`store.rs:192` (`>`) may be equivalent given invariants; not proven. The list is
the `missed` rows of the agreement file.

## Caveats

- **Load.** 14 to 50 on 20 cores, not sampled per run. Timeouts are
  budget-sensitive under load: with a 2 s floor, 5 mutants flipped between
  survived, killed and timeout across runs; tier B now gets three times the
  budget (6 s).
- **The comparison is not like for like in parallelism.** Cherenkov used 20
  workers; cargo-mutants ran one job. cargo-mutants at `-j20` would be faster than
  70 minutes by an amount that was not measured (and each job needs its own build).
- **Features.** The run notes say the cargo-mutants baseline used `--all-features`
  and Cherenkov was run with it; cargo-mutants's debug log for this run shows a
  baseline without it (a later run, `m3`, has it). Which features the 478 mutants
  were built under is therefore not settled.
- **The runs did not overlap.** The cargo-mutants run ended about two
  minutes before Cherenkov's first run of the day; they did not share the machine
  with each other, but both shared it with other sessions.
- A later cargo-mutants run, `m3`, in the worktree `core--bus-interface` (commit not recorded; 369 mutants, `--all-features`, 8
  jobs, 363 outcomes: 236 caught, 9 timeout, 14 missed, 104 unviable; no
  `end_time`, so it did not finish cleanly), is kept as timing only in
  `data/2026-11-19-angzarr-bus/cargo-mutants-partial-m3-timing.json`. Its phases
  sum to 7000 s (build 5719 s, test 1264 s) over its jobs; the wall time is not
  recorded (its last output file was written about 73 minutes after the start).
  It is not compared with Cherenkov.
