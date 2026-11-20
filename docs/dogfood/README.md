# Dogfood runs

A dogfood run is Cherenkov run on a real project and compared, mutant by
mutant, with cargo-mutants on the same code. It answers two questions: how much
faster is Cherenkov, and does it agree (does a mutant that cargo-mutants kills,
Cherenkov kill, and the same for the ones that survive). Each run has a record in
this directory, with timings, the load of the machine and the disagreements.

## The runs

Wall times are seconds, from each tool's own output. "Agreement" is over the
mutants matched between the tools; the rest of each record says what was left
over. Cherenkov's wall is its own `took_ms`, with the time of the `time` command
(which includes the build and planning before the run starts) in the record.
**Every 2026-11-19 run was taken at a load average of 14 to 50 on 20 cores**
(an observation in the run notes, not a sample), so the speedups below are of
loaded runs, and cargo-mutants ran one job where Cherenkov used 20 workers.

| Date | Target | Mutants | Cherenkov wall | cargo-mutants wall | Agreement | Load | Record |
|---|---|---|---|---|---|---|---|
| 2026-11-17 | ctxloom `deps` package | 305 | 38 s | 605 s | 0 disagreeing of the 237 run; 68 not runnable | not recorded | [record](2026-11-17-time-savings.md) |
| 2026-11-17 | ctxloom `address.rs` | 51 | about 13 s | 89.6 s | 51/51 both | not recorded | [record](2026-11-17-time-savings.md) |
| 2026-11-17 | fixture `rust-ops` | 168 | 8.0 s | 34.8 s | same verdicts | not recorded | [record](2026-11-17-time-savings.md) |
| 2026-11-19 | angzarr-bus (core `099ef5ae`) | 476 (cargo-mutants 478) | 109.7 s (116.2 s by `time`) | 4191.8 s (69 min 52 s), shared with ports | 276 agree of 476 matched; 2 disagree; 4 cargo-mutants-unviable; 194 not runnable | 14 to 50, not sampled | [record](2026-11-19-angzarr-bus.md) |
| 2026-11-19 | angzarr-embedded (`232580d`) | 90 | 97.3 s (122.0 s by `time`) | 2972.5 s (49 min 32 s) | 30 agree of 81 matched; 1 disagrees; 1 cargo-mutants-unviable; 49 not runnable | 14 to 50, not sampled | [record](2026-11-19-angzarr-embedded.md) |
| 2026-11-19 | angzarr-ports (core `099ef5ae`) | 14 | 16.7 s (47.8 s by `time`) | not separable (65.2 s of phase time inside the bus run) | 7 agree of 8 matched; 0 disagree | 14 to 50, not sampled | [record](2026-11-19-angzarr-ports.md) |

How these records were made: by hand, from the runs' output. The two earliest
records are thin because the 2026-11-17 runs kept little; they say which fields
are `not recorded`.

## How to read a record

[`_template.md`](_template.md) lists the fields every record has. In short:

- **Run** says what was run: the target and its commit, Cherenkov's commit and
  version, cargo-mutants's version, the operator set, whether a diff limited the
  mutants, and the command lines.
- **Machine** gives cores, RAM, OS and the **load average at the time**. A loaded
  machine inflates timeouts and every wall time; a record says whether it was.
- **Timings** has the same breakdown for both tools: build, planning or listing,
  the baseline run, Cherenkov's recording run, the mutants, and the total wall
  time. cargo-mutants's are taken from its `outcomes.json` (the baseline scenario
  first, then each mutant's `Build` and `Test` phase durations, summed
  separately; `start_time` and `end_time` give the wall total, which includes
  listing and copying the tree). Cherenkov's are from its `run.started`,
  `program.prepared` (`baseline_ms`, `prepared_ms`) and `run.finished`
  (`took_ms`) events, and the `time` output. Where a phase is not reported
  separately, the record says what it is derived from.
- **Disk** gives the peak extra disk and max RSS of each run. Cherenkov builds in
  the project's own `target/`, so its extra is the growth of that directory and
  anything in the temp directory; cargo-mutants copies the source tree into a
  scratch directory and builds there, one `target/` per job.
- **Verdicts** are counts per verdict for each tool, Cherenkov's `unsupported` with
  reason codes, and the kill rate over the mutants both could run.
- **Agreement** is the mutant-by-mutant table: agree detected, agree missed,
  agree unviable, and the disagreements, then **Disagreements**, with who is
  right.
- **Caveats** lists the load, the flaky mutants and what the numbers do not
  support.

A number that was not measured is written `not recorded`; nothing is estimated.
Detected means killed or timed out (Cherenkov), caught or timed out
(cargo-mutants). Missed means survived or not-called.

Raw artifacts that a record cites are in [`data/`](data/), in a directory named
after the record: the run's start, preparation and finish events, the per-mutant
agreement (`agreement.tsv`: locations, functions and verdicts, no source text), the
deficiency log (trimmed of the mutated source text) and cargo-mutants's
timing summary. The raw `mutants.out` and event streams are larger and are not
kept here.

## Recording a run

Until the tools below exist, **records are made by hand** from the run's output:
copy [`_template.md`](_template.md), fill it from the events and `outcomes.json`
as described above, and add a row to the table above.

Two helpers exist:

- `scripts/measure-run.sh WATCH COMMAND...` (or `just measure-run WATCH COMMAND...`)
  runs a command and prints one JSON block with wall time, max RSS, the **peak
  extra disk** of the watched directories (sampled every second; `SAMPLE_SECS`
  changes it), the load average before and after, and the core count. Watch
  Cherenkov's target directory and the temp directory; watch cargo-mutants's
  scratch directories as a glob:

  ```sh
  just measure-run 'target:/tmp/cherenkov-*' target/release/cherenkov run . --operators cargo-mutants
  just measure-run '/tmp/cargo-mutants-*' cargo mutants -j4
  ```

  The block goes in the record's Machine and Disk sections. Quote the globs so the
  script expands them at each sample, as the scratch directories appear mid-run.
  Max RSS needs GNU time (`/usr/bin/time`; the dev container has it); without it
  the field is `null`. Disk is sampled, so a peak between samples can be missed:
  the figure is a lower bound.
- cargo-mutants's phase sums, from its output directory:

  ```sh
  jq '[.outcomes[] | select(.scenario != "Baseline") | .phase_results[] | select(.phase == "Build") | .duration] | add' mutants.out/outcomes.json
  jq '[.outcomes[] | select(.scenario != "Baseline") | .phase_results[] | select(.phase == "Test") | .duration] | add' mutants.out/outcomes.json
  jq '.outcomes[0].phase_results, .start_time, .end_time' mutants.out/outcomes.json
  ```

  A run with `end_time: null` did not finish cleanly; its wall time is not in the file.

Run Cherenkov and cargo-mutants one after the other, not together, and write down
`uptime` before each: the 2026-11-19 records show how much a loaded machine moves
the numbers.

### TODO

These write records in this format when they exist. Neither does yet.

- [ ] **The CI dogfood job** (`.github/workflows/ci.yml` has a placeholder
  comment for it): run Cherenkov and cargo-mutants on a pinned target, under
  `measure-run.sh`, and commit or upload the filled template.
- [ ] **`cherenkov compare`** (not built): run both tools on one target and write
  the agreement table, the verdict counts and the timings, from the events and
  `outcomes.json`, as a record.
- [ ] Per-run load samples (`uptime` before, during and after) in every record.
