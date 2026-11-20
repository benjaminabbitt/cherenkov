# Run layer: domains, bus and concurrency

How a run decides its mutants with many workers at once on
angzarr-embedded, without workers contending on sequence numbers: the
design behind `runlayer-plan.md` phases 3 (the bus) and 4 (the domains on
it), as phase 5 reshaped it (the adoption: verdicts decided in the
aggregate, runners as projectors on the bus, fan out and fan in as
policies on the bus).

## Problem
A run decides mutants with n workers at once. angzarr-embedded orders each
stream by sequence number, which collides with that concurrency twice:

- **Writes.** An append carries the sequence it expects to land at
  (optimistic concurrency, per stream). A write that loses the race to
  another on the same stream is run again on the new head (the aggregate
  loaded again, the command decided again), up to a cap; past the cap the
  command fails. n workers recording verdicts on one run stream contend on
  every verdict.
- **Reads.** A projector's position on a stream is one sequence number. n
  instances of one projector finish out of order, so a single "where it
  is" either skips an unfinished event after a crash or redoes finished
  ones.

The rule that answers both: **every stream has one writer, and every
position knows which events above it are already handled.**

## Domains

| Domain | Stream per | Holds | Written by |
|---|---|---|---|
| `run` | run | planned (settings, the mutants in plan order); each program prepared or failed (as facts); ready (every program prepared); each mutant decided (as a fact); running tallies; finished | the planning call (the plan, then each program's preparation), then the fan-in policy alone |
| `mutant` | mutant, keyed by its content-hash id | planned (the executable plan, the run that asked); each program's run of it (as facts: outcome, tier, killers, tests selected); decided | the fan-out policy (planned), then the runner projector serving it, one per mutant at a time (its facts); the aggregate decides |
| (`program`: not a domain) | | a test program's preparation is facts on the run | the planning call, before fan out |

- **`run`** is an aggregate with real state: which mutants it planned,
  which are decided, the tallies, whether it is finished. It judges each
  program's baseline, records `RunReady` with the last program prepared,
  accepts a mutant's decision once, for its own mutants only, and records
  `finished` with the last one. Nothing is recorded after.
- **`mutant`** outlives a run: its verdict is what incremental runs reuse.
  It folds the outcomes its runner reports, decides the verdict once per
  asking run, and says when the runner can stop (a detection, or an
  operator with no rule).
- **`program`** stays facts inside `run`: preparation is written by the
  planning call before any mutant is fanned out, so it needs no writer of
  its own; a domain keyed by the binary hash earns its place when
  incremental runs (phase 6) invalidate verdicts by it.

## Flow
1. **Plan.** The CLI sends `PlanRun` to a run stream; the run records its
   mutants planned, in plan order. `plan` (the dry run) stops here.
2. **Prepare.** Each test program's baseline and recording are reported to
   the run as facts; the run records it prepared, or failed, which
   finishes the run.
3. **All prepared.** With its last `ProgramPrepared` the run records
   `RunReady`: every program exists, so a mutant can run from here on.
4. **Fan out: policy run -> mutant, on the bus.** A policy (source `run`,
   target `mutant`) owes, for `RunReady`, one `PlanMutant` per mutant, each
   on root = the mutant id, carrying its plan and the run. The bus serves
   it as one instance. A mutant appears on its stream only once its
   programs exist.
5. **Run: the runner projector.** A projector subscription on the bus with
   n instances, the worker count, served `MutantPlanned`. It runs the
   mutant program by program and records each run as a fact on that
   mutant's stream through the kernel (`Kernel::handle`, an external id
   derived from the cause), as ctxloom records its runners' outcomes
   (`App::serve` and `App::handle` with derived external ids). Each
   call's result carries the events the aggregate wrote; when
   `MutantDecided` comes back, the runner runs no further programs. A
   The bus hands a stream one event at a time, which keeps each mutant on
   one instance at a time.
6. **Fan in: policy mutant -> run, on the bus.** A policy (source
   `mutant`, target `run`) records, for each `MutantDecided`, the fact
   `DecisionReported { mutant, verdict }` on root = the run named in the
   event. The bus serves it as **one instance**, so the run's stream has
   one writer.
7. **Follow.** The follower in the application renders the run's events
   and its mutants' decisions as they are recorded (an output projector, in
   phase 5.6). The live-runs projector, ephemeral, says when the run has
   ended: the bus is asked whether to go on each time it is idle.

Both policies are served by the bus, none inline in the kernel, so the
whole flow runs the same in process and distributed. Fan out, with one
instance, is the mutant streams' one writer until their runners start; its
command ids derive from the policy's name and the trigger, so a
redelivery re-derives them and deduplicates. Fan in, inline, would run
inside the write of every runner's fact and give the run's stream n
writers. Its fact's id derives the same way, so a decision handed again is
a duplicate. A fact is never refused; a failing one is dead-lettered at
its target.

## The bus
angzarr core's `angzarr-bus` is the interface (`Bus`, `Subscription`, a
conformance suite; `docs/design/bus-interface.md` in angzarr-project), and
angzarr-embedded's `Bus` is its implementation over the event store and the
kernel; a distributed coordinator is the other implementation, later. The
run layer builds it in `cherenkov-run-application/src/host.rs`.

- **Subscribe** a handler with its scope (the streams of some domains) and
  its instances: a projector, which answers "handled" and may record facts
  through the kernel as a side effect, or a policy (`BusPolicy`), which
  answers outputs the bus routes. Either may fail.
- **One event per instance, and one event of a stream at a time.** The
  handler's answer is the ack. The bus reads each event from the store
  when an instance is free for it. A policy's outputs commit with its
  position, through the store's outbox, and are relayed by the kernel.
- **Positions: one mark for each stream** (every sequence below it is
  handled), stored under the subscription's name, or kept in memory for an
  ephemeral projector, which is handed every stream from its first event
  each serve and so rebuilds a read model. A policy cannot be ephemeral.
- **Delivery is at least once.** A crash between the answer and the mark
  hands the event again, so handlers are idempotent: the facts a runner
  records have ids derived from the event that planned the mutant and the
  program, and a policy's outputs have ids the kernel derives from its
  name and the trigger.
- **A held failure** is handed again after the retry policy's delay; a bug
  is dead-lettered and passed. `Served` reports every failure.
- **Wake**: the store's `CommitNotify` pushes each commit; the change
  token is polled as the fallback.
- **Several subscriptions** share one bus, each with its own instances:
  the runners (n, the worker count), fan out and fan in (one each) and the
  live-runs projector (one, ephemeral).

A runner is handed a mutant by the bus. After a crash the bus hands again
only what was not marked: to the runners, each `MutantPlanned` they had
not finished (a mutant that was running runs again, only in the programs
it has no fact of, and not at all if decided); to the policies, their
triggers. Anything recorded twice is a duplicate by its derived id.

## Why one writer per stream
- The expensive, concurrent work, running a mutant's tests, writes only to
  that mutant's stream, which nothing else writes.
- The run's stream is written by the fan-in policy alone once its programs
  are prepared. Recording a fact is microseconds against seconds of tests,
  so one writer keeps up with n runners.
- Sequence numbers keep their meaning: the order of one stream.

## Rejected
- **n workers recording on one run stream, racing and retrying.** Correct
  but costly: each lost race reloads the run, and under load a verdict can
  hit the retry cap and be parked, and the run never finishes. An inline
  fan-in policy is this shape; `runlayer-plan.md` decides for serving it
  on the bus with one instance.
- **One recorder thread for all verdicts (the spike's shape).** No races,
  but it funnels the run's work through one writer outside the kernel's
  delivery, so redelivery and resume are Cherenkov's to build.
- **Verdicts decided outside the aggregate** (phase 4's mutation-test
  projector, handed a verdict string to copy). The runner records what it
  observed; the Mutant aggregate decides.
- **Fan in as a projector** (phase 4's). Moving work across domains is a
  policy's.
- **The runner as a policy.** Considered and rejected: policies move work
  across domains, and running a mutant's tests is external work on one
  domain, a projector with a side effect that records facts through the
  kernel. As a projector it also sees each fact's events, so it learns the
  early stop with no widening of the trait.
- **Fan out inline in the planning call**, on `MutantsPlanned`. It fanned
  mutants out before their programs were prepared; served on the bus after
  `RunReady`, a mutant exists only once it can run, and the whole flow runs
  the same in process and distributed.
- **A work-list read model and a worker pool in the application.** It
  rebuilds what the bus already gives: n instances, one event of a stream in
  flight,
  stored positions and re-serving after a crash. ctxloom's launcher, once
  named as its model, is a held `CatchUp` stepped once per CLI call,
  launching serially with no wake, no concurrency limit and no resume.
- **A global sequence across streams.** The store has none; nothing here
  needs it.
- **An event-carrying channel or queue in front of handlers.** A second
  copy of what the store holds, with its own overflow and redelivery rules.

## Costs and risks
- **Streams:** about one per mutant (2,000 for angzarr) instead of one per
  run.
- **Aggregate loading:** the run and the mutant are snapshotted after
  every write and load from the snapshot and the events after it, so no
  fact folds the run's stream. Measured in phase 4, in memory: a fact on
  the run's stream cost about 0.6 ms rising to 1.9 ms over 2,000 mutants
  (folding: 2.6 to 8 ms), and a whole run of 2,000 instant mutants 3.0 s
  (folding: 10.8 s). About 85% of that was the in-memory store copying a
  stream's whole history on every read, lookup and append. With the store
  reading a stream's tail without copying it (phase 5.0's pins), a fact
  costs about 270 µs falling to 50 µs over the run, and the whole run
  0.95 s: what is left follows the run's snapshot, as large as its
  undecided mutants, so a fact gets cheaper as the run goes on and the
  fan-in is no longer quadratic.
- **The bus scans** all of a subscription's streams on each commit
  (`StoreFeed::scan` lists the domain's streams and reads each head) and
  picks the next delivery by walking them, so each commit costs O(streams)
  and a run O(mutants squared). Measured in phase 5.4 (release, 2,000
  instant mutants on 8 workers, `CHERENKOV_MEASURE`): the fan in as before
  (430 us falling to 73 us a fact), a whole run 1.11 s before and 9.6 s
  after, 0.58 s at 500 mutants and 2.5 s at 1,000. Real mutants take
  tens of milliseconds to seconds each, so the bus's share is small at
  2,000 and grows as the square; the change is angzarr-bus's (an
  incremental scan, and a set of the streams that owe).
- **More facts per mutant:** one per program run, where phase 4 recorded
  one verdict. Each is microseconds against the tests it reports.
- **Hops:** a mutant starts one policy step after `RunReady`, and a
  verdict reaches its run one policy step after it is recorded; output
  shows the mutant's decision first, the tally after.
- **Routing:** a mutant outlives runs, so its `MutantDecided` names the run
  that asked; the fan-in policy routes by it. A verdict reused from an
  earlier run is decided by the mutant when it is planned, and reaches the
  run through the same policy.
- **Stable ids:** mutant ids (content hashes today) must not change on
  re-planning, or resume and reuse find the wrong streams.

## Fit with later phases
- **Resume (phase 5.7):** automatic when the unfinished run's plan matches,
  else the old run is recorded abandoned. The bus's positions say what is
  owed: the runners are handed again each unhandled `MutantPlanned`, and
  the policies their unhandled triggers; what is recorded twice is dropped
  by its derived id.
- **Incremental runs (phase 6):** a mutant planned for a run whose inputs
  (the plan, the test program hash, the selected tests) match an earlier
  decision's decides as it is planned, and the runners run only the rest.
- **`--fast` (phase 7):** a setting of the run; the runner stops at the
  first failing test and its fact marks the kill list partial.

## Order
`runlayer-plan.md` phases 3 and 4, done; reshaped by phase 5.
