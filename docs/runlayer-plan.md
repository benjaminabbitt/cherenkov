# Plan: the run layer on angzarr-embedded

## Goal
Runs become event-sourced, and the events are the state. A run aggregate
plans a run's mutants; each mutant is an aggregate of its own, which
decides its verdict from the facts its runner reports; policies carry the
plans out to the mutants once the run's programs are prepared, and the
decisions back to the run; runners, projectors on the bus, run the mutants
and record what they observe; read models say what to print. How that
avoids contention on sequence numbers is `runlayer-concurrency.md`. That
gives streaming output (`output-events.md`), a dry run (the plan without
the runners), resume after a crash, incremental runs, and a committed
verdict file that diffs as one line per changed verdict.

## Where it stands
After 5.5 the root hosts the core (below); after 5.4 the aggregates decide (the Mutant its own verdict, from the facts
of its programs' runs), the streams can be executed (a runner needs the
streams alone), and the run is served by angzarr's bus: the runners, both
policies and the live-runs read model, built in one place
(`cherenkov-run-application/src/host.rs`). The system around them still
works its own way:

- **The read side is a loop**: the follower re-reads a mutant's whole
  stream per decision to print it (`layer.rs`, `Printer::decision`), and
  `Follow` polls. 5.6 makes the output a projector.
- **Each process mints a new run id**, and the store is in memory. There is
  nothing to resume from yet. 5.7.
- **The executor port returns raw outcomes**, and the CLI hands the
  workers a libtest parser (`Explain`). Typed observations from each
  adapter are the next step.
- **The bus scans all of a subscription's streams on each commit**, which
  makes a run of n mutants cost O(n squared) in the bus (see 5.4).
- **Layering**: flat, no arch-lint, presentation and infrastructure in the
  application, `Diagnostic` only at the top, the store in memory.

## Done
1. **Spike**: events, aggregate, a projector over the workers, follow, dry
   run, the store in memory.
2. **The CLI on the run layer**: `run` and `plan`, `--format json|text`,
   ctxloom's error shape.
3. **The bus**, in angzarr-embedded: n instances per subscription, one event
   each, `CommitNotify` wake-up with polling as the fallback; `CommitNotify`
   for store-jsonl. (Its positions were a low-water mark plus the handled
   set above it; the owner removed that: a stream has one event in flight
   and one mark, and work that should run side by side is a stream of its
   own, as a mutant is.)
4. **Domains on the bus**: the `mutant` domain, fan out inline, the
   mutation-test projector with one instance per worker, fan in as a
   one-instance subscription, tallies and finish, the split into crates,
   snapshots after every write (a 2,000-mutant run in memory from 10.8 s to
   3.0 s; most of the rest was the in-memory store copying a stream's
   history, which angzarr core has since fixed).

Since then the messages became `.proto` files built by angzarr-build's
`wire`, with component, command, fact, event and refusal status options,
and `encodings.txt` pins their bytes. 5.3 and 5.4 (below) made the streams
executable and put the run on the bus.

## Decided
- Cherenkov is Apache-2.0 OR BSD-3-Clause; Rust 1.96; `protoc` at
  build time.
- **Everything is proto**: `.proto` sources, generated Rust, generated
  serde, the JSON form declared by annotations in the proto. Conversion
  from the awkward prost types (`Option` on every message field, angzarr's
  `UUID` message) to clean domain types happens in one place,
  `markup_domain`, never by hand. What `markup_domain` cannot express is an
  angzarr-build gap to close.
- **The store** is a rework of angzarr-platform's store-jsonl (below).
- **Pins**: angzarr core moves now to the in-memory store fix.
- **Layout**: ctxloom's rings, held by arch-lint.
- **Policies only move work across domains**, and both are served on the
  bus with one instance, none inline in the kernel, so the whole flow runs
  the same in process and distributed:
  - **fan out** (run -> mutant) triggers on `RunReady`, which the run
    records with its last `ProgramPrepared`, so a mutant appears on its
    stream only once its programs exist;
  - **fan in** (mutant -> run) keeps one writer on the run's stream.
    Inline, it would run inside every runner fact's write and give the
    run's stream n writers; a fact that loses past the race cap is parked
    and the run never finishes.
- **Runners are projectors on the bus**: a projector subscription with n
  instances (the worker count), served `MutantPlanned`. Each runs the
  mutant's test programs and records one fact per program run through the
  kernel (`Kernel::handle`, a fact with an external id derived from the
  cause), as ctxloom records its runners' outcomes (`App::serve` and
  `App::handle` with derived external ids). A projector with a side effect
  may record facts this way; the defect in phase 4 was deciding verdicts
  outside the aggregate, and the Mutant aggregate still decides. Each
  `Kernel::handle` result carries the events the aggregate wrote, so when
  `MutantDecided` comes back the runner runs no further programs: the early
  stop needs no widening of the trait. The runner is not a policy: policies
  move work across domains.
- **No work list.** The bus already gives n instances, per-stream credit,
  stored positions (a mark plus the handled set) and re-serving after a
  crash, so resuming owed mutants follows from delivery positions plus the
  aggregates' dedup by derived id. There is no work-list read model and no
  pool in the application. ctxloom's launcher is not a model for this: it
  is a held `CatchUp` stepped once per CLI call, launching serially in a
  `for` loop, with no wake, no concurrency limit and no resume (it sweeps
  orphans by lock probes).
- **One bus interface for in process and distributed**: a transport-neutral
  subscription trait in angzarr core, which angzarr-embedded's `Bus` and the
  distributed coordinator each implement (below).
- **Resume is automatic** when the project has an unfinished run with the
  same plan (mutant ids and settings); otherwise the old run is recorded
  abandoned and a new one starts.
- **Locations**: the store in `.cherenkov/store/`, git-ignored; the verdict
  file `.cherenkov/verdicts.jsonl`, committed.

## What we build on
- **angzarr-embedded**: the kernel and markup; the bus (to implement
  angzarr core's subscription trait, 5.4); the host's `App` (resumes what a
  crash left owed at `start`, keeps dead letters); the runner's `Wait` and
  `Jitter`; `testing` (`Harness`, `Samples`, `FaultyStore`,
  `assert_replay_fixtures`).
- **angzarr-build** and angzarr-codegen: `wire`, `markup_domain`,
  `register`; once extended (5.1), serde from the proto's JSON annotations
  in place of pbjson.
- **angzarr-platform**: store-jsonl (reworked, 5.7); `os::spawn_tree` and
  `kill_tree`, `file-system`, `file-lock`.
- **angzarr core**: angzarr-ports (store ports, the in-memory store,
  `FaultyStore`, the contract suite), angzarr-proto, angzarr-diagnostic,
  angzarr-presentation.
- **ctxloom as the worked example** (`/home/babbitt/workspace/ctxloom/ctxloom/main`):
  `core_link` (`App` over one store at the composition root, which owns
  the clock and ids); the `Edge` port and its `embedded-edge` adapter;
  `Feeds`; the runs domain from `markup_domain` with one refusal message
  per rejection; `arch-rules.toml`. ctxloom registers components by hand
  and generates no descriptors, so neither is a gap against it. Its
  runners' outcomes, recorded through `App::serve` and `App::handle` with
  derived external ids, are the model for Cherenkov's runners; its
  launcher is not a model for owed work (decided above).
- **Pins**: revisions merged to main that build on one angzarr core, never
  an unmerged branch. Since the angzarr repositories were merged (core,
  router, embedded, build and platform are crates of one workspace,
  github.com/angzarr-io/angzarr), every angzarr crate is pinned to the same
  commit, `71b453a`, and rust-support (the former rust-support) to `d543b4e`,
  which is on it. The earlier pins (core `3535a54a`, embedded `bce1ff2`,
  build `2e072571`) are gone from that repository.
- **rust-support**, shared with ctxloom and taskloom and bumped in its order
  (rust-support, then taskloom and ctxloom, each green): `cli-support`,
  `run-within`, `process-env`, `duration-text`, `arch-lint`.

## Architecture
In ctxloom's layout, `crates/<ring>/<role>/<crate>`:

| Role | Crate | Holds |
|---|---|---|
| core/wire | `cherenkov-run-wire` | the prost types and generated serde of `cherenkov.run.v1`, `cherenkov.mutant.v1` and `cherenkov.output.v1` |
| core/domain | `cherenkov-mutation` (today `cherenkov-core`) | the mutation domain: concepts, operators, operator sets, the planner, verdicts and ids; `classify` and `combine`; the mutant plan's domain types, generated |
| core/domain | `cherenkov-run-domain` | the run and mutant aggregates and the fan-out and fan-in policies, from `markup_domain`: the run judges its programs' preparation and records `RunReady` and finishes; the mutant decides its verdict |
| core/read-model | `cherenkov-run-reads` | live runs (resume, completion, and each run's programs' preparation, which the runner reads), the verdict projection |
| core/ports | `cherenkov-run-ports` | the edge to the core (handle, report, read, wait for a commit); the executor port, moved out of `cherenkov-mutation`, with typed observations and `Diagnostic` errors; the language and source ports |
| core/application | `cherenkov-run-application` | plan a run, prepare its programs, the runner projector (a mutant's program runs, each recorded as a fact through the kernel, stopping at `MutantDecided`), resume or abandon |
| adapters/driven | `cherenkov-exec-dap`, `cherenkov-syntax`, `cherenkov-fs`, `cherenkov-run-store` | executors, each parsing its own runner's output; parsing; sources; the store with Cherenkov's codec |
| adapters/driving | `cherenkov-cli` | the commands; the output projector (JSON Lines or text) |
| root/lib | `cherenkov-wiring` | `App` over the store, the registry, clock, ids, runtime, the bus and its subscriptions |
| root/bin | `cherenkov` | `main` |

`cherenkov-dap-types` and the test-only `cherenkov-fidelity` get their
roles in 5.8.

**Naming.** The names follow the angzarr and ctxloom conventions: lower-case
kebab-case, `cherenkov-<role>`, with the ring carried by the directory
(`crates/<ring>/<role>/<crate>`), which `arch-lint` holds; a crate's name
does not repeat its ring.
- `cherenkov-core` becomes `cherenkov-mutation`, named for what it models,
  as ctxloom names a domain crate (`runs`, `sessions`, `trees`). Two crates
  named for "the domain" was the confusion: `cherenkov-mutation` is the
  mutation domain and `cherenkov-run-domain` the run aggregates. Its port
  traits (`SyntaxPort`, the source reader, `MutantExecutor`) move to
  `cherenkov-run-ports`, as the table says.
- The `run-` prefix names the run layer's crates (`run-wire`, `run-domain`,
  `run-reads`, `run-ports`, `run-application`, `run-store`); the ring is in
  the path. It stays.
- An execution adapter is `cherenkov-exec-<mechanism>`, named for the
  protocol it drives, not the language: `cherenkov-exec-dap` today (GDB,
  Delve and debugpy over DAP), `exec-jdwp`, `exec-cdp` and the like when
  the language evaluation's adapters are built
  (`language-contract-evaluation.md`). A language is data under
  `profiles/<lang>/`, not a crate.
- `cherenkov-dap-types` is the protocol's types, a driven-adapter support
  crate; `cherenkov-fidelity` is test-only and sits in no ring.

- **The spool** is the event store plus the bus's positions; the bus holds
  no events.
- **The bus** serves projector subscriptions (the runners with n
  instances; the output, verdict projection and live runs) and policy
  subscriptions (fan out and fan in, one instance each). Nothing runs
  inline in the kernel.
- **One writer per stream**: a mutant's is the fan-out policy, then its
  runner's facts (one runner per mutant at a time); a run's is its
  planning call and preparation facts, then the fan-in policy.
- **Errors** are `Diagnostic` everywhere. **Async**: tokio, created at the
  root; executors stay synchronous on the blocking pool.

## The store
angzarr-platform's store-jsonl, reworked into a store with its own engine
(its format 2):

- **Segments**: numbered `seg-NNNNNN.jsonl`, append-only, rolled at 32 to
  64 MB, read newest-first. A torn tail is only ever in the newest.
- **Lines**: one per operation, keys in a fixed order, the stream, expected
  sequence, page sequences and command ids lifted into the envelope, and a
  CRC32C last. A failing last line is a torn tail, truncated; a failing
  earlier line is corruption.
- **Catalog on open**: heads, forks and editions, page offsets, each
  stream's command ids (so `find_command` reads nothing for a new command),
  positions, outbox and dead letters, held in memory. It is written as a
  checkpoint line every few MB and at close; open reads backwards from the
  newest segment's tail to the newest checkpoint and folds the lines after
  it, falling back to an older checkpoint if one is bad. Pages are read
  with `pread` when asked for. Measured shape: 0.1 to 0.3 s to open at 100
  runs, flat as history grows, against 150 to 200 s today.
- **Snapshots are latest-only**: kept in memory and written out whole beside
  each checkpoint, replacing the last, never appended (appended, Cherenkov's
  snapshot after every write costs about 70 MB a run). A crash loses at
  most the snapshots since the last checkpoint; those streams fold from an
  older one.
- **Positions**, 57% of the lines, are all in each checkpoint, so compaction
  can drop the superseded ones from old segments. Events are never dropped.
- **The store knows no types.** A payload is embedded verbatim as nested
  JSON (`RawValue`), never base64 or a JSON string.
- **The codec layer** beside it holds the type registry, filled from each
  wire crate's generated registration, and turns the `Any` bytes of an
  `EventPage` or snapshot into JSON and back (microseconds a page). An
  unknown type is kept as `{"@type": .., "@base64": ..}`, bit-exact.
  Removing the bytes step would need angzarr core's `EventPage` to carry a
  typed or JSON payload: an option, not planned.
- **One process writes**, under a directory lock. **DuckDB** is a separate,
  read-only tool for queries and exports, and how a second process reads:
  it scans the segments with no lock. It is never in the live path (a point
  lookup there is 1 to 3 ms against 0.2 µs in memory).
- **The committed artifact is the verdict projection**: one line per mutant,
  sorted by id, holding nothing that varies between identical runs. Event
  segments are not committed (about 0.9 GB at 100 runs).
- **Why a rework, not a new store**: about 45% of store-jsonl survives (the
  lock, file seam, commit-then-append, poisoning, commit signal, error
  catalog, most of the schema). The engine over a catalog is new in either
  crate (about 2,250 lines and as many of tests). A second store would
  duplicate what survives and leave a format neither DuckDB nor a diff can
  read. Format 1 journals are refused, not migrated: they are pre-release.

## The bus interface
The event-store-backed bus in process and the distributed bus
interoperate behind one API: a transport-neutral subscription trait in
angzarr core, beside angzarr-ports and the router, which angzarr-embedded's
`Bus` and the distributed coordinator each implement. It is compatible with
a reworked form of how embedded runs policies today (inline in
`Kernel::handle`).

Where it stands: angzarr-embedded's `Bus` is a concrete struct that serves
`ProjectorHandler`s only, and it drops the handler's response. Distributed
policy delivery (`PolicyService.React`, the coordinator's outbox, `Deliver`)
is specified in protos and design docs but not built; `Deliver` returns
unimplemented. No bus or subscriber trait exists in angzarr core.

The trait covers:
- **Handler kind**: a projector, a sink returning nothing but "handled"
  (which may record facts through the kernel as a side effect, as the
  runners do), or a policy, whose outputs are commands or facts. Cherenkov
  needs both: projector subscriptions (the runners with n instances, the
  output, verdict and live-runs projectors) and policy subscriptions (fan
  out and fan in, one instance each).
- **Output routing and command-id derivation**, in one place: the kernel in
  process, `Deliver` distributed.
- **An atomic commit** of the position and the outgoing messages: one store
  write in process, the `Outbox` distributed.
- **Ack and credit**: n instances, one event of a stream in flight.
- **Positions**: one mark for each stream.
- **Wake source**: `CommitNotify` or the `Watch` token in process, a remote
  stream distributed.
- **Scope**: all streams of some domains.
- **Purged streams**: a position on a stream that no longer exists.
- **Failure**: hold the event and retry, or dead-letter it and report
  through `OnPolicyBug`.

Building blocks already there: the router's `PolicyComponent` and
`ProjectorHandler`; angzarr-ports' `Processed`, `PositionStore`, `Outbox`,
`Watch`, `CommitNotify`, `ListStreams` and `DeadLetterStore`; the
coordinator's `Commands::deliver`.

## Phases
Each phase ends with `just check` and `just test-slow` green and the
fixtures' `expected-verdicts.txt` unchanged. Upstream items are the owner's
own, in the named repository, merged to main before Cherenkov pins them.

1–4. Done (above).

5. **Adoption**, in steps. 5.1 and the store's upstream half run alongside
   5.2 to 5.6.

**5.0 Pins.** Done. One angzarr core with the in-memory store fix: the
root `Cargo.toml`'s pins and `Cargo.lock`, one revision of each angzarr
source in the lock. The 2,000-mutant measurement (`CHERENKOV_MEASURE`):
a fact on the run's stream costs about 270 µs falling to 50 µs, a whole run
0.95 s (phase 4: 0.6 rising to 1.9 ms, 3.0 s); recorded in
`runlayer-concurrency.md`. angzarr-platform (now the `os` and `file-system` crates of the angzarr
workspace) is needed only with the store (5.7).

**5.1 angzarr-build: domain types and JSON** (upstream, a dependency
phase). Goal: the generator produces every type Cherenkov needs, and its
JSON. Upstream (angzarr-build, angzarr-codegen; the annotations' option
definitions in angzarr core):
- `markup_domain` maps a message of another package, angzarr's `UUID`, and
  the plan's shapes (oneofs, enums, repeated enums for flag sets);
- serde generated from the JSON annotations (field naming such as
  snake_case, enum value names such as `not-called`), replacing pbjson and
  closing its defects: no `null` for non-finite floats, unknown enum
  numbers kept, deterministic map order, nested `Any` expanded, timestamps
  in `Z`, `UUID` as text;
- a generated registration of a wire crate's types for the codec.

The domain and JSON generator is merged (angzarr-build `3abf75e`):
`markup_domain_with` for messages of other packages, `UUID` and id
newtypes, `Duration`, oneofs, nested types and `Default`; `angzarr_build::json`,
with the options 50107 to 50111 in `io.angzarr.codegen.v1`. A component's
state is its anchor and snapshots are the implementation's choice
(`markup::Snapshots`, angzarr-embedded `cc3c07d`), not an option.

Exit: a round-trip fuzz over every field kind and the cases above passes;
ctxloom and taskloom build unchanged on it. Unblocks: 5.2, the codec.

**5.2 Protos with options and enums.** Done. Goal: a typed vocabulary, its
domain types generated. What was done (`proto/cherenkov/`, the wire, domain,
application and CLI crates):
- enums `Verdict` (all of `output-events.md`'s, with the core's
  `unevaluable` and `evaluator-raised`), `Tier`, `Selection` and `RunStatus`,
  each with an unspecified zero (nothing was removed, so nothing is
  `reserved`), named in kebab-case by the JSON options; tallies are a
  repeated `VerdictCount`. The message that held a verdict is now `Outcome`
  (a message and an enum cannot share a name in a package). The core's
  `Verdict::name`, `Tier::name` and `Selection::name` and its
  `reason_code(verdict-name, ..)` went: the names come from the generated
  enums, and a reason's code from `unviable_code` and `unsupported_code` by
  the verdict it is decided under;
- one refusal message per rejection, each under its `refusal_status`:
  `AlreadyPlanned` (the run), `NoRun`, `NoMutant` and `PlannedForRun` (the
  mutant). `rejection!` went: `RunRejection` and `MutantRejection` are
  plain enums with a code and a catalog message, `refuse!` makes a handler's
  refusal, and `TryFrom<&TypedRefusal>` reads one back by its type, as
  ctxloom's `refusal.rs` does;
- `exit` left `RunFinished` (field 3 and the name are reserved): the CLI
  maps the status to the exit code (`Exit::OK`, `Exit::FAILED`) and still
  prints it;
- the documents of `output-events.md` are the messages of
  `cherenkov.output.v1`, a `Document` whose oneof is tagged `event`, and
  `angzarr_build::json` writes their JSON (snake_case, `reason` for the
  selection's reason, nulls, the flattened mutant, killers and executors).
  The application builds `Document`s from the events; the CLI's text and
  deficiency sinks read them typed. Output of the fixtures is the same,
  field for field, but for two things: a duration is a 32-bit
  millisecond count in the documents (the canonical JSON writes a 64-bit
  integer as text, and the generator has no option for a number), and a
  mutant's `description` is written before its `original` and `mutated`
  (JSON keys follow the field numbers);
- `markup_domain_with` in the domain crate's `build.rs`, a layer per
  package; `own_domain_type!`, the wire crate's `events.rs` and its trial
  decoding went (a stream is read as the layer's event enums). The run
  fields of the mutant's messages are a `RunId`. A mutant's id is a content
  hash and not a UUID, so there is no `MutantId` newtype to generate: that
  is 5.3's, when the id is derived into the plan;
- snapshots under `markup::Snapshots`: `Run` and `Mutant` snapshot as their
  domain `RunSnapshot` and `MutantSnapshot`, with the checks `restore` made
  (and one more: tallies in strictly ascending order of verdict). The
  domain crate is version 0.2, because its state changed;
- `PlanMutant` without a run or a mutant is refused; a mutant's root that
  cannot be derived is an error (`UnderivableRoot`), not the nil UUID.

`encodings.txt` was regenerated once, in the commit that changed the
protos. The measurement (`CHERENKOV_MEASURE=2000`, release) took about 2.1
seconds on a machine at a load of 35 where the commit before the pin took
about 1.5; most of the difference came with the angzarr-embedded pin. Upstream: none beyond 5.1. Unblocks: 5.3 and 5.4. Every wire
change lands before 5.7, while no store outlives the process.

**5.3 Events sufficient to execute a mutant.** Done. Goal: a runner needs
only the streams. What was done:
- `MutantPlan` (action, edit, targets, spans, positions, requirements,
  operands) is proto (`proto/cherenkov/mutant/v1/plan.proto`), its domain
  types generated in `cherenkov-run-domain`; `PlanRun`, `MutantsPlanned`,
  `PlanMutant` and `MutantPlanned` carry it in an `ExecutableMutant`, beside
  the mutant as a reader needs it. **Deviation:** the planner's own types in
  `cherenkov-core` are not replaced by the generated ones. The core stays
  free of the wire (its architecture test allows no dependency beyond
  `bitflags`, `serde` and `sha2`), and the generated structs derive neither
  `Copy` nor `Eq` nor `Hash`, which the planner uses (`Pos`, `Span`,
  `Target` and `Action` are keys and `Copy`). `plan::encode` and
  `plan::decode` are the one place the two meet; every plan of every
  fixture and operator set, and a varied program with a site for each kind
  of action and edit, round-trips through proto equal. The core's
  `&'static str` operator and mutation names come back from the catalogs, so
  a plan naming an operator the planner does not have is refused by its name.
  Tokens and captures are proto enums numbered in the planner's `ALL` order,
  held to its names by a test. A prost enum field named `from` shadows
  `From::from` in the generated conversions (prost gives it an accessor), so
  `ReplaceToken` has `original` and `replacement`.
- **The mutant's id** is a 16-hex content hash and not a UUID, so
  `id_type` cannot generate it. It stays a `string` in the protos: the
  planner that derives it (`MutantId::derive`, in the core) is its one
  validator, `mutant_root` derives the stream root from it, and an id that
  cannot be derived is `UnderivableRoot`. A newtype in the domain crate
  would validate nothing the root derivation does not.
- Preparation is facts on the run. A program's `PreparationReported` holds
  its raw baseline (`Observation`, the executor port's `RawOutcome` as
  proto, with `observation::encode` and `decode` beside the plan's), the
  failing tests of a baseline that did not pass, and, when it did, what
  recording found: the tests, the selection and which tests call each
  mutant. The run judges it: a baseline that did not pass records
  `ProgramFailed` and finishes the run, failed, with the reason it always
  had; one that did records `ProgramPrepared` with the time one mutant may
  run in the program (`Budget::for_mutant` of the baseline's wall time, the
  run's to say) and its driven-test map by mutant id.
- With its last `ProgramPrepared` the run records `RunReady`, which carries
  the mutants with their plans (the run holds the plans in its state, and
  so in its snapshot, until then; after it, its snapshot is as small as
  before). `PlanRun` says how many programs there are (none, for a dry
  run, which is never ready).
- Fan out triggers on `RunReady`, not on `MutantsPlanned`, so a mutant
  appears on its stream only once its programs exist.
- The runner takes no plans and no `Programs`: it reads the plan from the
  `MutantPlanned` it is handed and what the run prepared from the run's
  stream, once for each run it serves. A test runs a mutant from a store
  holding only its streams.
- The live-runs read model is 5.4's, below. Run **resume** (finding the
  unfinished run again and continuing it) is 5.7's.

`encodings.txt` was regenerated twice in 5.3, each time in the commit that
changed the protos: for the plan (new lines for the plan, each arm of its
choices, and the executable mutant; `PlanRun`, `MutantsPlanned`, `PlanMutant`
and `MutantPlanned` longer by the plans they carry), and for the
preparation (`PreparationReported` and `ProgramPrepared` with the new
fields and the old `baseline_ms` of the fact reserved, `RunReady`,
`Observation` in each kind, `RunStarted`, `PlanRun` and the run snapshot with
the program count, the prepared programs, the plans and readiness).
**JSON:** the generated JSON covers the output package only. The generated
readers of `Edit`, `Action` and `MutantPlan` pass the complexity gate's line
limit, which holds generated code too, and nothing prints a plan; the
store's codec (5.7) will want them, and needs the generator to split what it
writes.

**5.4 Verdicts in the Mutant aggregate; runners as projectors and both
sagas as bus policies.** Done. Goal: the runner executes and reports; the
domain decides; policies only move work across domains.

Upstream, first, was the bus interface: angzarr core's `angzarr-bus` (the
`Bus` trait, `Subscription`, `Hub`, the conformance suite) and
angzarr-embedded's `Bus` over the kernel, with `Subscription::ephemeral`.
Pins: angzarr core `3535a54a`, router `fd7c87b9`, embedded `bce1ff2`,
angzarr-build `2e072571`. What was done, on it:
- each program's run of a mutant is a fact on its stream
  (`ProgramRunReported`: the observation, the killers and the tests it ran
  with, or that no test calls it, or that the debugger failed), recorded as
  `ProgramRan`. The mutant folds them and decides once for each asking run
  (`judge`, with the core's `classify` and `combine`), at the first run
  that detects it or cannot apply it, or at the last program's, and
  records `MutantDecided`. Each `Kernel::handle` reply carries the events
  recorded (`Reported::events`), and the runner runs no further program
  once `MutantDecided` is among them: no widening of the bus trait.
  `VerdictReported`, `Decided` and the worker's verdict logic are gone;
  the verdict paths (killed, survived, timeout, not called, not reached,
  unsupported, unviable, debugger failed, early stop, a program told twice,
  facts out of order, an unreadable observation) are tests on angzarr's
  `Harness`. `PlanMutant` and `MutantPlanned` carry the run's program count.
- the runner (`runner.rs`) is a projector subscription with an instance
  for each worker, over the mutant domain. A stream has one event in flight
  at a time, so a mutant is on one instance, and the instances are the
  worker pool. It records each program's result with the fact's id derived
  from the event that planned the mutant and the program. A mutant handed
  again runs only the programs with no fact, and not at all when decided.
- fan out and fan in are `BusPolicy` subscriptions with one instance each
  (`policies.rs`), registered nowhere in the kernel; their outputs commit
  with their position through the store's outbox.
- `host.rs` is the one place the bus is built and the four subscriptions
  made (the `Wait`, the retry policy, the push and the poll), written to be
  read as the reference.
- the live-runs read model (`live.rs`) is one ephemeral projector over
  the runs' streams, rebuilt from the first event each serve: which runs
  are unfinished, what each owes, how each ended. It is what ends the
  serve (the bus is asked whether to go on each time it is idle: a run that
  has ended, or nothing served, which is a run whose mutants the bus gave
  up on, reported as `Stalled { owed }`), and it is the piece ctxloom's
  `PendingModel` becomes.
- a test serves, stops half way (the first bus dropped with a mutant
  running) and serves a new `Bus`, runner and model on the same store
  (`restart.test.rs`): no mutant a runner finished is run again, the one the
  crash caught runs again, every mutant is decided once, and none is lost.

What went, and why. The `FanIn` projector, the `MutationTest` projector's
deciding, `sagas.rs` and the inline fan-out are replaced as above. The
`stalled` heuristic (idle twice over a poll with nothing handled) and
`Cell` flags between the follower and the serve went: the bus says when it
is idle, and the live-runs model says whether the run ended, so nothing is
timed. The first handler failure ending the run went: a failure the kernel
classes as retryable is held and retried by the bus after a delay
(`host::RETRY`), one it classes as a bug is dead-lettered and passed (the
run then ends `Stalled`, naming how many mutants it owes).

**Deviations and what is left.**
- The executor port still returns raw outcomes, and `Explain` (the
  libtest parser the CLI hands the workers) stays: typed observations from
  each adapter, and Go runs not meeting the libtest parser, are the next
  step and independent of the bus. `NOT_CALLED` and `ERROR` were gone
  already (5.2).
- Both policies and the runner are registered in `host::serve`, not
  generated: `register` waits for the codegen to cover policies.
- cli-support (rust-support `d543b4e`) is on the same angzarr commit as the
  rest, so the CLI uses `angzarr-diagnostic` and `angzarr-presentation`
  directly and a cause of the run layer is held as `Report::of(cause)`; the
  `cli-diagnostic` and `cli-presentation` aliases and the bridge between
  two cores are gone, and the lock holds one angzarr.
- **Speed.** The bus scans its subscription's streams on each commit
  (`StoreFeed::scan`: list the domain's streams, read each head) and picks
  the next delivery by walking them, so a run of n mutants costs O(n)
  for each commit, O(n squared) in all. Measured below; the change is
  angzarr-bus's and is the owner's to decide.

Measured (`CHERENKOV_MEASURE=2000`, release, instant mutants on 8
workers): the fan in, one fact on the run's stream, costs the same as
before, 430 us falling to 73 us over the run; a whole run took 1.11 s before
(load 2.4) and 9.6 s after (load 2.6), 0.58 s at 500 mutants, 2.5 s at
1,000 and 9.6 s at 2,000 (a doubling costs four times). The cost is in
the bus's scans, not the aggregates.

**Change feed (2026-11-20).** The bus follows the store with
`Following::Changes(core.store())` (angzarr `71b453a`); `RunStore` requires
`Changes`, and `FullScan` is no longer used (no natural place for an explicit
fallback: every `RunStore` has a feed). Measured with `CHERENKOV_MEASURE`,
release, instant mutants on 8 workers, in-memory store, whole run:

| mutants | `FullScan` (load 6.5) | `Changes` (load 2.6 to 3.8) |
| --- | --- | --- |
| 500 | 106 ms | 109 ms |
| 1,000 | 282 ms | 278 ms |
| 2,000 | 817 ms | 797 ms |

Against the 9.6 s of 2,000 mutants above (the old per-commit scan) both are
a large step; the change feed over the newer `FullScan` is within noise
here, because the in-memory store lists streams cheaply. The fan in costs
363 us per fact over the first 500 facts of 2,000 and 59 us over the last 500.

> **Still slow (owner: "that still feels slow").** The harness above does
> not show the run on the bus slower than before it: 0.80 s at 2,000 instant
> mutants against 1.11 s recorded before the bus (load 2.4; another day and
> commit, so only roughly comparable). Whatever feels slow is not in this
> harness. 2,000 instant mutants still cost about 400 us each end to end,
> and the dominant cost is not separated: no profiler is installed here
> (`perf`, `samply` absent), so none of the following is measured to be the
> cause. Open items:
> - the fan in's per-fact cost is highest while the run's snapshot holds the
>   plans and falls as mutants decide (363 to 59 us); stream load versus
>   snapshot decoding versus kernel is not separated.
> - the kernel and snapshotting per input (each fact loads, folds and may
>   snapshot).
> - a real store (SQLite, the journal) is not measured at all; every figure
>   is the in-memory store.
> - a real run (build, test processes) was not compared before and after the
>   bus; only instant mutants were.
> - angzarr-bus: a feed that yields to the runtime stalled the 64-mutant
>   run with 8 workers; fixed upstream (`71b453a`), and the test store's
>   feed yields again (`yielding.test.rs`).

Exit: `workers.rs` names no verdict; `FanIn`, `NOT_CALLED` and `ERROR` are
gone; no policy runs inline; a runner stops at `MutantDecided`; a `Harness`
test per verdict path. Not met: a killed Go fixture mutant naming its
killer (the Go run's output is still parsed by the libtest parser; with
typed observations it will). Unblocks: 5.6, phase 6.

**5.5 App and core_link.** Done. Goal: the root hosts the core; the
application sees ports. What was done:
- `cherenkov-run-ports` (core/ports): `Edge` (handle a book, read a stream,
  arm a wait for the next commit; `command` and `report` are provided
  methods) and `Serving` (serve a set of bus subscriptions until told to
  stop), with errors as angzarr's `Report`. The executor port stays where it
  is until 5.8.
- `cherenkov-embedded-edge` (driven adapter, as ctxloom's `embedded-edge`):
  `EmbeddedEdge<S>` is `angzarr_embedded_host::App` over one store
  (`App::builder(..).on_retryable(..).wait_with(..).start()`, so the owed
  policy work is resumed at start) and implements both ports; the bus is
  built in `serve` on `app.kernel()` and `app.store()` (no change to `App`
  was needed). It knows no domain (the registry is the caller's) and no
  clock (the caller's sample stamps each call and the bus's records).
  `POLL`, `RETRY`, `TokioWait` and the `RunStore` bound moved here from the
  application.
- `cherenkov-wiring` (root/lib): the clock and ids (`sample`, `new_run`),
  the registry (`cherenkov_run_domain::register`), the current-thread
  runtime, and `run`, which the CLI calls. `layer::run` is async over a
  core and takes the run's id.
- `RunCore`, `Undelivered` (a bus-served policy runs no chain in a call, so
  a call has no dead letters of its own; the bus reports its own as served
  failures) and `core.rs` went; the stream covers are `covers.rs`. Every
  application test runs on `testing::new_core` (`Samples` for the calls,
  a counter for run ids).
- **Not done: `FaultyStore` in place of `Yielding`.** `Yielding` yields to
  the runtime inside every store call, which is what lets the
  one-writer-per-stream test interleave instances on one thread; `FaultyStore`
  has hooks at a call but cannot yield, and does not push commits. It stays
  (now with `ListStreams`) and is 5.10's.
- The adapter is a crate of its own, not in the wiring, because the
  application's tests need the same `App` the root uses and the wiring
  depends on the application.

Exit met: the application depends on neither `uuid` nor tokio's runtime
builder (tests excepted) and reads no clock; its tests run on `Samples`.
Measured (`CHERENKOV_MEASURE`, release, instant mutants on 8 workers, at a
load of 23): 500 mutants 158 ms, 1,000 414 ms, 2,000 1.03 s (the figures
above were taken at a load of 2.6 to 3.8, so not comparable).
Unblocks: 5.6, 5.7.

**5.6 Projectors for the output and the verdict projection; live runs.**
Goal: output and run state are read models. Changes:
- live runs, folded from the run streams: which runs are unfinished (for
  resume), completion, and each run's programs' preparation, which the
  runner reads (owed mutants need no read model: the bus's positions are
  the owed work);
- the output projector in the CLI renders each event from the page it is
  handed, with no re-read; `Printer`, `Follow`, `render.rs`, `Sink` and
  `serde_json` leave the application;
- completion is the run's `RunFinished`, seen by the live-runs read model;
  `stalled` goes (nothing owed and unfinished is a dead letter or a held
  failure, reported as one); a failing handler is served again;
- the verdict projection, written at a run's end from its own message's
  generated serde.

Upstream: none. Exit: the output tests (`jq` round trip, text
order-independent) pass unchanged; a constant number of store reads per
decision; a run whose handler fails once completes; two runs of a fixture
write identical verdict files. Unblocks: 5.7.

**5.7 The durable store and resume.** Goal: a run survives the process.
Upstream (angzarr-platform): store-jsonl's format 2 and the codec layer,
exiting on the contract suite, a differential test against the in-memory
store with reopens at random points, a kill -9 loop, and open under 0.5 s
at 100 runs of 2,000 mutants. Changes: `cherenkov-run-store` opens it with
Cherenkov's codec; the wiring resumes at start, automatically when the
unfinished run's plan matches (else the old run is recorded abandoned):
the bus serves each subscription from its positions, so the runners are
handed again every `MutantPlanned` they had not finished and the policies
their unhandled triggers, and what is recorded twice is a duplicate by its
derived id;
SQL files for the DuckDB CLI (queries, the projection's export). Exit: a
fixture run killed partway and run again with the same plan resumes on its
own, runs no decided mutant again, and writes the same verdict file as an
uninterrupted run; run again with a changed plan, it records the old run
abandoned. Unblocks: phase 6.

**5.8 Layout, names and arch-lint.** Goal: the rings above, held, under the
names in "Naming". Changes: crates moved to
`crates/<ring>/<role>/<crate>`; `cherenkov-core` renamed
`cherenkov-mutation` (a mechanical rename of the crate, its imports and the
error codes that carry the crate's name, done in one commit with no other
change, after the complexity refactors and the gate-reliability work have
landed, so it does not collide with them), and its port traits moved to
`cherenkov-run-ports`; `arch-rules.toml` from ctxloom's; the CLI split from
the root; `arch-lint` in `just check`. Crates added in 5.3 to 5.7 go in flat
under their final names and move here. Upstream: none. Exit: `arch-lint`
passes with no exception the owner has not ruled on; no `cherenkov-core`
remains in the tree, the docs or the error catalogs (grep); the fixtures'
verdict files and `encodings.txt` are unchanged by the rename. Unblocks:
phase 8.

**5.9 Diagnostic everywhere.** Changes: `ExecError`, `SyntaxError`,
`TokenTableError`, `ProfileError`, `DapError`, `RpcError`, `IsaError`, the
patcher's and layouts' refusals and the run layer's string reasons become
`Diagnostic`s with catalogs and catalog tests; notices go through
cli-support and honour `--format json`. Exit: no `reason: String` in a
run-layer error; no `eprintln!` in the CLI but the failure reporter's.

**5.10 angzarr test support.** Changes: the remaining tests onto `Harness`,
`Samples` and `FaultyStore`; replay fixtures pinning snapshots and folds
across rules versions; crash and re-serve tests on the durable store. Exit:
no test builds its own kernel, clock or ids.

6. **Incremental runs.** The outcome facts already carry what a verdict
   depends on (the plan, each program run, the tests selected); add the
   test binary's hash to the preparation facts, as a `program` domain keyed
   by it if reuse needs one. Reuse is the Mutant aggregate's decision:
   planned for a run whose inputs match an earlier decision's, it decides
   at once, and the runners run only the rest (handed a mutant already
   decided, a runner records nothing). Exit: a rerun with no changes runs no
   mutants and writes a byte-identical verdict file; one changed function
   reruns only what depends on it.
7. **`--fast`**: recorded on `PlanRun` (always false today, `layer.rs:271`);
   the runner stops at the first failing test, the outcome fact marks the
   kill list partial, and the output's `complete` comes from it (always
   true today, `render.rs:124`).
8. **Extract and share**: the `core_link` analogue and stream following,
   written to ctxloom's shape in 5.5 and 5.6 so extraction is a move, go to
   `rust-support` or angzarr-embedded's host, with Cherenkov's cargo and
   libtest tooling; then all three applications use the shared versions.

## Upstream work, by repository
- **angzarr core**: the JSON annotations' definitions (5.1); the
  transport-neutral subscription trait and its conformance suite (5.4);
  optionally, `EventPage` with a typed or JSON payload. Later, not needed by
  Cherenkov: the distributed coordinator's implementation of the trait,
  for both handler kinds (`PolicyService.React`, the outbox, `Deliver`).
- **angzarr-build, angzarr-codegen** (crates of angzarr now): the
  `markup_domain` gaps, generated serde, codec registration (5.1), built on
  `feat/domain-serde` and awaiting merge.
- **angzarr-embedded**: its `Bus` implementing the subscription trait, with
  both projector and policy subscriptions (5.4); optionally, `App` hosting
  the bus (5.5).
- **angzarr** (`store-jsonl`, formerly angzarr-platform): store-jsonl format 2 and the codec
  layer (5.7).
- **rust-support**: the extraction (8).

## Open questions
1. **Early stop: the aggregate or the runner?** Recommendation: the
   aggregate; a fact per program run costs microseconds against seconds of
   tests, and the stop rule is part of the verdict.
2. **Where the driven-test map lives.** Recommendation: on the run's
   preparation facts, measured at 2,000 mutants; a `program` domain only
   when phase 6 keys reuse by binary hash.
3. **Plans on both the run's and each mutant's stream.** The fan-out policy
   needs them on the run; reuse needs them on the mutant. Recommendation:
   keep both; "nothing stored twice" holds for the spool, not for events.
4. **What the verdict file holds.** Recommendation: the latest finished
   run's mutants, each with id, verdict, file, line, column, function,
   operator, original and mutated text, and killing tests' names sorted; no
   times, run ids or tiers.
5. **Output documents: proto or CLI structs?** Decided (5.2): proto, in a
   package of their own (`cherenkov.output.v1`), with snake_case
   annotations, so one generator writes all of Cherenkov's JSON.
6. **DuckDB: a crate, or SQL for its CLI?** Recommendation: SQL files and
   `just` recipes; a crate only when a command needs one, never in the main
   binary.
7. **A bad old line in a segment.** Recommendation: it fails the reads of
   its stream, not the open, and a store check command reports it.
8. **Generated `register` and `descriptors`.** Recommendation: `register`
   once it covers policies (hand registration drifts from the protos);
   `descriptors` when a tool reads them.

## Order with other work
- 5.0 first; 5.1 upstream at once, with 5.2's design (enums, refusal
  messages) beside it.
- The bus interface (angzarr core's trait, then angzarr-embedded's `Bus`)
  landed before 5.4, which moved both policies and the runners onto it;
  fan out was inline, on `RunReady`, between 5.3 and 5.4.
- The store rework, the long item (three to five weeks), starts once 5.1's
  codec registration is settled and runs beside 5.2 to 5.6, which stay on
  the in-memory store.
- The language and debugger port extraction can run alongside: the run
  layer depends only on the executor port, reshaped in 5.4, moved in 5.8.

## Risks
- **The upstream critical path**: 5.1 gates 5.2 to 5.4; the bus interface
  gates 5.4; the store gates 5.7; ctxloom and taskloom must stay green on
  all three.
- **Wire churn after 5.7** needs upcasters or new package versions; the
  enums and facts are cheapest to get right in 5.2 to 5.4.
- **The store's rules drift** from the in-memory store's; the differential
  test with reopens guards it, since the contract suite never reopens.
- **Generated serde** keeps pbjson's traps unless 5.1 closes them.
- **Speed**: more facts per mutant, the fan-out and fan-in hops and durable
  syncs all add latency to a path that must stay an order of magnitude
  faster than cargo-mutants. Group commit, and the measurement after each
  phase.
- **DuckDB** reads a live segment as of its scan, with no documented row
  order and a torn line as a row of nulls: for queries, never decisions.
- **Stable ids**: resume and reuse are only as sound as the mutant ids and
  the plan match.
- **Incremental reuse** is only as sound as its key: start by hashing the
  whole test binary.
- **Pins drift** from ctxloom's and taskloom's; bump together, in
  rust-support's order.
- **Build requirements**: `protoc` and git dependencies; no crates.io
  publication until angzarr publishes.
