use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::markup::DomainMessage;
use angzarr_embedded::markup::decode_message;
use angzarr_embedded::v1::{
    Command, CommandBook, CommandPage, Cover, Fact, FactOrigin, command_page,
};
use angzarr_embedded::{EventStore, Handled, Kernel, Registry, Sampled, SnapshotStore, StreamRef};

use std::time::Duration;

use cherenkov_core::execution::{RawOutcome, Tier as CoreTier};

use crate::events::{MutantEvent, RunEvent, mutant_event, run_event};
use crate::mutant::Observation;

use super::*;

/// Whether a fact was recorded, or was a duplicate of one on the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Applied,
    Duplicate,
}

/// The two aggregates served by the kernel over a store in memory.
struct Core {
    registry: Registry,
    store: MemoryStore,
}

fn core() -> Core {
    let mut registry = Registry::new();
    registry.aggregate::<Run>(Run::default).unwrap();
    registry.aggregate::<Mutant>(Mutant::default).unwrap();
    Core {
        registry,
        store: MemoryStore::new(),
    }
}

fn sample() -> Sampled {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    Sampled {
        now: Timestamp {
            seconds: i64::try_from(since.as_secs()).unwrap(),
            nanos: i32::try_from(since.subsec_nanos()).unwrap(),
        },
        command_id: Uuid::from_bytes(*uuid::Uuid::now_v7().as_bytes()),
    }
}

fn new_run() -> Uuid {
    Uuid::from_bytes(*uuid::Uuid::now_v7().as_bytes())
}

fn run_cover(run: &Uuid) -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: Some(run.clone()),
        edition: None,
    }
}

fn mutant_cover(id: &str) -> Cover {
    Cover {
        domain: MUTANT_DOMAIN.to_string(),
        root: Some(mutant_root(id).unwrap()),
        edition: None,
    }
}

impl Core {
    async fn handle(&self, cover: Cover, body: command_page::Body) -> Result<Delivery, String> {
        let book = CommandBook {
            cover: Some(cover),
            page: Some(CommandPage {
                body: Some(body),
                ..CommandPage::default()
            }),
        };
        match Kernel::new(&self.registry)
            .handle_any(&self.store, book, &sample())
            .await
        {
            Ok(Handled::Applied(_)) => Ok(Delivery::Applied),
            Ok(Handled::Duplicate(..)) => Ok(Delivery::Duplicate),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn command<C: DomainMessage>(&self, cover: Cover, command: &C) -> Result<(), String> {
        let body = command_page::Body::Command(Command {
            payload: Some(pack_message(command)),
            handling: None,
        });
        self.handle(cover, body).await.map(drop)
    }

    async fn report<F: DomainMessage>(
        &self,
        cover: Cover,
        fact: &F,
        external_id: &str,
    ) -> Result<Delivery, String> {
        let body = command_page::Body::Fact(Fact {
            payload: Some(pack_message(fact)),
            origin: Some(FactOrigin {
                external_id: external_id.to_string(),
                description: "test".to_string(),
            }),
        });
        self.handle(cover, body).await
    }

    async fn events(
        &self,
        cover: &Cover,
        from: u64,
    ) -> Result<Vec<angzarr_embedded::v1::EventPage>, String> {
        let stream = StreamRef::try_from(cover).map_err(|e| e.to_string())?;
        let slice = self
            .store
            .read(stream, from, None)
            .await
            .map_err(|e| e.to_string())?;
        Ok(slice.pages)
    }
}

fn mutant(id: &str) -> PlannedMutant {
    PlannedMutant {
        id: id.to_string(),
        file: "src/lib.rs".to_string(),
        line: 3,
        column: 5,
        end_line: 3,
        end_column: 10,
        function: Some("total".to_string()),
        operator: "aor".to_string(),
        description: "in-place".to_string(),
        original: "+".to_string(),
        mutated: Some("-".to_string()),
    }
}

/// `id`'s mutant with its plan, as the run plans it.
fn executable(id: &str) -> ExecutableMutant {
    ExecutableMutant {
        mutant: Some(mutant(id)),
        plan: Some(crate::mutant::MutantPlan {
            id: id.to_string(),
            ..Default::default()
        }),
    }
}

/// An unmutated run that passed, in a second and a half.
fn passed() -> Observation {
    observation::encode(&RawOutcome::Exited {
        code: 0,
        tier: CoreTier::P,
        wall: Duration::from_millis(1_500),
    })
}

/// A program prepared with every test selected, as `program`, the
/// `index`th of the run's.
fn preparation_at(index: u32, program: &str) -> PreparationReported {
    PreparationReported {
        program: program.to_string(),
        prepared_ms: 2,
        tests: None,
        selection: Selection::EveryTest,
        failed_tests: Vec::new(),
        selection_reason: Some("no listing".to_string()),
        index,
        baseline: Some(passed()),
        baseline_failures: Vec::new(),
        drives: None,
    }
}

fn preparation(program: &str) -> PreparationReported {
    preparation_at(0, program)
}

/// A plan of `ids` for a run of one test program.
fn plan(ids: &[&str]) -> PlanRun {
    plan_for(ids, 1)
}

/// A plan of `ids` for a run of `programs` test programs.
fn plan_for(ids: &[&str], programs: u32) -> PlanRun {
    PlanRun {
        programs,
        root: "/project".to_string(),
        operators: vec![OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string(),
        }],
        jobs: 2,
        fast: false,
        mutants: ids.iter().map(|id| executable(id)).collect(),
    }
}

fn decision(id: &str, verdict: Verdict) -> DecisionReported {
    DecisionReported {
        mutant: id.to_string(),
        verdict,
        decision: None,
    }
}

async fn run_events(core: &Core, run: &Uuid) -> Vec<RunEvent> {
    core.events(&run_cover(run), 0)
        .await
        .unwrap()
        .iter()
        .filter_map(run_event)
        .collect()
}

async fn mutant_events(core: &Core, id: &str) -> Vec<MutantEvent> {
    core.events(&mutant_cover(id), 0)
        .await
        .unwrap()
        .iter()
        .filter_map(mutant_event)
        .collect()
}

fn finishes(events: &[RunEvent]) -> Vec<&RunFinished> {
    events
        .iter()
        .filter_map(|e| match e {
            RunEvent::RunFinished(f) => Some(f),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn test_planning_a_run_records_its_start_then_its_mutants_in_plan_order() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    let [
        RunEvent::RunStarted(started),
        RunEvent::MutantsPlanned(planned),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(started.mutants, 2);
    assert_eq!(
        started.operators,
        vec![OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string()
        }]
    );
    assert!(started.started_at.is_some());
    let ids: Vec<&str> = planned
        .mutants
        .iter()
        .filter_map(|m| m.mutant.as_ref())
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, ["a", "b"]);
}

#[tokio::test]
async fn test_a_run_is_planned_once() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    assert!(core.command(run_cover(&run), &plan(&["b"])).await.is_err());
    assert_eq!(run_events(&core, &run).await.len(), 2);
}

#[tokio::test]
async fn test_a_run_of_no_mutants_finishes_with_its_plan() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&[])).await.unwrap();
    let events = run_events(&core, &run).await;
    let finished = finishes(&events);
    assert_eq!(finished.len(), 1, "{events:?}");
    assert_eq!(finished[0].status, RunStatus::Complete);
}

#[tokio::test]
async fn test_each_decision_is_counted_and_the_last_finishes_the_run_with_its_tallies() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b", "c"]))
        .await
        .unwrap();
    for (id, v) in [("a", Verdict::Killed), ("b", Verdict::Survived)] {
        core.report(run_cover(&run), &decision(id, v), id)
            .await
            .unwrap();
    }
    assert!(
        finishes(&run_events(&core, &run).await).is_empty(),
        "c is owed"
    );
    core.report(run_cover(&run), &decision("c", Verdict::Killed), "c")
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    let decided: Vec<(&str, Verdict)> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::DecisionRecorded(d) => Some((d.mutant.as_str(), d.verdict)),
            _ => None,
        })
        .collect();
    assert_eq!(
        decided,
        [
            ("a", Verdict::Killed),
            ("b", Verdict::Survived),
            ("c", Verdict::Killed)
        ]
    );
    let finished = finishes(&events);
    assert_eq!(finished.len(), 1);
    let counts: Vec<(Verdict, u32)> = finished[0]
        .counts
        .iter()
        .map(|tally| (tally.verdict, tally.count))
        .collect();
    assert_eq!(counts, [(Verdict::Killed, 2), (Verdict::Survived, 1)]);
    assert_eq!(finished[0].status, RunStatus::Complete);
    assert!(matches!(events.last(), Some(RunEvent::RunFinished(_))));
}

#[tokio::test]
async fn test_a_decision_redelivered_under_its_id_is_a_duplicate() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    let first = core
        .report(run_cover(&run), &decision("a", Verdict::Killed), "a")
        .await;
    let again = core
        .report(run_cover(&run), &decision("a", Verdict::Killed), "a")
        .await;
    assert_eq!(first, Ok(Delivery::Applied));
    assert_eq!(again, Ok(Delivery::Duplicate));
}

#[tokio::test]
async fn test_a_mutant_is_counted_once_and_only_for_its_own_run() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    for (id, v, external) in [
        ("a", Verdict::Killed, "a#1"),
        ("a", Verdict::Survived, "a#2"),
        ("z", Verdict::Killed, "z"),
    ] {
        core.report(run_cover(&run), &decision(id, v), external)
            .await
            .unwrap();
    }
    let decided = run_events(&core, &run)
        .await
        .into_iter()
        .filter(|e| matches!(e, RunEvent::DecisionRecorded(_)))
        .count();
    assert_eq!(decided, 1);
}

#[tokio::test]
async fn test_a_prepared_program_is_recorded_with_its_budget_from_its_baseline() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan_for(&["a"], 2))
        .await
        .unwrap();
    let drives = Drives {
        drives: vec![Drive {
            mutant: "a".to_string(),
            driven: Some(Driven {
                kind: DrivenKind::Tests(TestList {
                    tests: vec!["calc::test_total".to_string()],
                }),
            }),
        }],
    };
    let prepared = PreparationReported {
        tests: Some(3),
        selection: Selection::RecordedWithFailures,
        failed_tests: vec!["test_flaky".to_string()],
        selection_reason: None,
        drives: Some(drives.clone()),
        ..preparation_at(1, "target/debug/deps/calc-1")
    };
    core.report(run_cover(&run), &prepared, "program 1")
        .await
        .unwrap();
    assert_eq!(
        run_events(&core, &run).await.last(),
        Some(&RunEvent::ProgramPrepared(ProgramPrepared {
            program: "target/debug/deps/calc-1".to_string(),
            baseline_ms: 1_500,
            prepared_ms: 2,
            tests: Some(3),
            selection: Selection::RecordedWithFailures,
            failed_tests: vec!["test_flaky".to_string()],
            selection_reason: None,
            index: 1,
            // Five times the baseline: a mutant over it is hung.
            budget_ms: 7_500,
            drives: Some(drives),
        })),
        "one of two programs is not every program"
    );
}

#[tokio::test]
async fn test_the_last_program_prepared_makes_the_run_ready_with_its_mutants_in_plan_order() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan_for(&["c", "a", "b"], 2))
        .await
        .unwrap();
    let ready = |events: &[RunEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, RunEvent::RunReady(_)))
            .count()
    };
    core.report(run_cover(&run), &preparation_at(1, "it-2"), "program 1")
        .await
        .unwrap();
    assert_eq!(ready(&run_events(&core, &run).await), 0, "one of two");
    // Told again under another id, a program prepared is not counted twice.
    core.report(run_cover(&run), &preparation_at(1, "it-2"), "again")
        .await
        .unwrap();
    assert_eq!(ready(&run_events(&core, &run).await), 0, "the same one");
    core.report(run_cover(&run), &preparation_at(0, "calc-1"), "program 0")
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    let [.., RunEvent::ProgramPrepared(_), RunEvent::RunReady(ready)] = &events[..] else {
        panic!("{events:?}");
    };
    assert_eq!(ready.programs, 2);
    let ids: Vec<&str> = ready
        .mutants
        .iter()
        .filter_map(|m| m.mutant.as_ref())
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(ids, ["c", "a", "b"]);
    assert!(ready.mutants.iter().all(|m| m.plan.is_some()));
}

#[tokio::test]
async fn test_a_baseline_that_fails_finishes_the_run_failed_and_it_is_never_ready() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    let failing = PreparationReported {
        baseline: Some(observation::encode(&RawOutcome::Exited {
            code: 101,
            tier: CoreTier::P,
            wall: Duration::from_millis(40),
        })),
        baseline_failures: vec!["calc::test_total at tests/calc.rs:5:5: boom".to_string()],
        ..preparation("calc-1")
    };
    core.report(run_cover(&run), &failing, "program 0")
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    let [
        ..,
        RunEvent::ProgramFailed(program),
        RunEvent::RunFinished(finished),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(program.program.as_deref(), Some("calc-1"));
    assert!(
        program
            .reason
            .starts_with("unmutated tests do not pass under the debugger (calc-1)"),
        "{}",
        program.reason
    );
    assert!(program.reason.contains("code: 101"), "{}", program.reason);
    assert!(
        program
            .reason
            .contains("calc::test_total at tests/calc.rs:5:5: boom"),
        "{}",
        program.reason
    );
    assert_eq!(finished.status, RunStatus::Failed);
    assert!(!events.iter().any(|e| matches!(e, RunEvent::RunReady(_))));
}

#[tokio::test]
async fn test_a_baseline_the_executor_did_not_report_is_a_bug_of_the_reporter() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    let silent = PreparationReported {
        baseline: None,
        ..preparation("calc-1")
    };
    assert!(
        core.report(run_cover(&run), &silent, "program 0")
            .await
            .is_err()
    );
    assert_eq!(run_events(&core, &run).await.len(), 2, "nothing recorded");
}

#[tokio::test]
async fn test_a_dry_run_is_never_ready() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan_for(&["a"], 0))
        .await
        .unwrap();
    core.report(run_cover(&run), &preparation("calc-1"), "stray")
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    assert!(!events.iter().any(|e| matches!(e, RunEvent::RunReady(_))));
}

#[tokio::test]
async fn test_a_program_that_does_not_prepare_is_recorded_and_finishes_the_run_failed() {
    // A reader of the stream learns why the run stopped, and that it is
    // over, from the stream alone.
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    let failed = PreparationFailed {
        program: Some("calc-1".to_string()),
        reason: "unmutated tests do not pass".to_string(),
    };
    core.report(run_cover(&run), &failed, "preparation failed")
        .await
        .unwrap();
    let events = run_events(&core, &run).await;
    let [
        ..,
        RunEvent::ProgramFailed(program),
        RunEvent::RunFinished(finished),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(program.program.as_deref(), Some("calc-1"));
    assert_eq!(program.reason, "unmutated tests do not pass");
    assert_eq!(finished.status, RunStatus::Failed);
}

#[tokio::test]
async fn test_nothing_is_recorded_after_the_run_finishes() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    core.report(run_cover(&run), &decision("a", Verdict::Killed), "a")
        .await
        .unwrap();
    let after = run_events(&core, &run).await.len();
    let failed = PreparationFailed {
        program: None,
        reason: "late".to_string(),
    };
    core.report(run_cover(&run), &failed, "late").await.unwrap();
    core.report(run_cover(&run), &preparation("calc-9"), "program 9")
        .await
        .unwrap();
    assert_eq!(run_events(&core, &run).await.len(), after);
}

fn plan_mutant(run: &Uuid, id: &str) -> PlanMutant {
    PlanMutant {
        run: Some(RunId::from(run.clone())),
        mutant: Some(executable(id)),
        programs: 1,
    }
}

fn outcome(verdict: Verdict) -> Outcome {
    Outcome {
        verdict,
        took_ms: 0,
        tests_run: Vec::new(),
        killed_by: Vec::new(),
        reason: None,
        tier: Some(Tier::P),
    }
}

/// Program `program` not called by the mutant.
fn program_run(program: u32) -> ProgramRun {
    ProgramRun {
        program,
        program_name: String::new(),
        took_ms: 0,
        tests: Vec::new(),
        result: ProgramRunResult::NotCalled(Unit {}),
    }
}

/// The one program of the run's run of the mutant: its tests fail with the
/// mutant in place (`Killed`) or pass (`Survived`).
fn reported(run: &Uuid, verdict: Verdict) -> ProgramRunReported {
    let code = match verdict {
        Verdict::Killed => 101,
        _ => 0,
    };
    ProgramRunReported {
        run: Some(RunId::from(run.clone())),
        ran: Some(ProgramRun {
            program: 0,
            program_name: "calc-1".to_string(),
            took_ms: 0,
            tests: Vec::new(),
            result: ProgramRunResult::Ran(Ran {
                observation: Some(observation::encode(&RawOutcome::Exited {
                    code,
                    tier: CoreTier::P,
                    wall: Duration::ZERO,
                })),
                killed_by: Vec::new(),
            }),
        }),
    }
}

#[tokio::test]
async fn test_a_mutant_is_planned_once_per_asking_run() {
    let core = core();
    let (first, second) = (new_run(), new_run());
    core.command(mutant_cover("a"), &plan_mutant(&first, "a"))
        .await
        .unwrap();
    assert!(
        core.command(mutant_cover("a"), &plan_mutant(&first, "a"))
            .await
            .is_err()
    );
    core.command(mutant_cover("a"), &plan_mutant(&second, "a"))
        .await
        .unwrap();
    let runs: Vec<Option<RunId>> = mutant_events(&core, "a")
        .await
        .into_iter()
        .map(|e| match e {
            MutantEvent::MutantPlanned(p) => p.run,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(runs, [Some(RunId::from(first)), Some(RunId::from(second))]);
}

#[tokio::test]
async fn test_a_mutant_is_decided_once_per_asking_run_as_planned_with_its_inputs() {
    let core = core();
    let (asked, other) = (new_run(), new_run());
    core.command(mutant_cover("a"), &plan_mutant(&asked, "a"))
        .await
        .unwrap();
    for (run, verdict, external) in [
        (&asked, Verdict::Killed, "1"),
        (&asked, Verdict::Survived, "2"),
        (&other, Verdict::Killed, "3"),
    ] {
        core.report(mutant_cover("a"), &reported(run, verdict), external)
            .await
            .unwrap();
    }
    let decided: Vec<MutantDecided> = mutant_events(&core, "a")
        .await
        .into_iter()
        .filter_map(|e| match e {
            MutantEvent::MutantDecided(d) => Some(d),
            MutantEvent::MutantPlanned(_) | MutantEvent::ProgramRan(_) => None,
        })
        .collect();
    assert_eq!(
        decided,
        [MutantDecided {
            mutant: Some(mutant("a")),
            verdict: Some(outcome(Verdict::Killed)),
            run: Some(RunId::from(asked)),
            programs: vec!["calc-1".to_string()],
        }]
    );
}

#[test]
fn test_a_mutant_s_stream_is_rooted_by_its_id_alone() {
    assert_eq!(
        mutant_root("0123456789abcdef"),
        mutant_root("0123456789abcdef")
    );
    assert_ne!(
        mutant_root("0123456789abcdef").unwrap(),
        mutant_root("0123456789abcdee").unwrap()
    );
}

#[test]
fn test_a_root_that_cannot_be_derived_is_an_error_and_not_the_nil_uuid() {
    // The derivation refuses a domain holding the name's separator.
    assert_eq!(
        root_of("run:time", "abc"),
        Err(UnderivableRoot {
            domain: "run:time".to_string(),
            key: "abc".to_string(),
        })
    );
    assert_ne!(mutant_root("abc"), Ok(Uuid::default()));
}

#[test]
fn test_every_refusal_has_a_message_using_exactly_its_args() {
    let problems =
        angzarr_diagnostic::catalog::check(EN_US.0, EN_US.1, &crate::diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}

/// The run `events` fold to, applied one by one.
fn replay_run(events: Vec<RunEvent>) -> Run {
    let mut run = Run::default();
    for event in events {
        match event {
            RunEvent::RunStarted(e) => run.started(e),
            RunEvent::MutantsPlanned(e) => run.planned(e),
            RunEvent::ProgramPrepared(e) => run.program_prepared(e),
            RunEvent::RunReady(e) => run.run_ready(e),
            RunEvent::ProgramFailed(e) => run.program_failed(e),
            RunEvent::DecisionRecorded(e) => run.decision_recorded(e),
            RunEvent::RunFinished(e) => run.run_finished(e),
        }
    }
    run
}

/// The mutant `events` fold to, applied one by one.
fn replay_mutant(events: Vec<MutantEvent>) -> Mutant {
    let mut mutant = Mutant::default();
    for event in events {
        match event {
            MutantEvent::MutantPlanned(e) => mutant.planned(e),
            MutantEvent::ProgramRan(e) => mutant.program_was_run(e),
            MutantEvent::MutantDecided(e) => mutant.decided(e),
        }
    }
    mutant
}

/// `run` snapshotted, through its wire bytes, and restored on a fresh run.
fn run_restored(run: &Run) -> Result<Run, Report> {
    let bytes = prost::Message::encode_to_vec(&run.snapshot().expect("it offers one").to_wire());
    let wire =
        <<RunSnapshot as DomainMessage>::Wire as prost::Message>::decode(&bytes[..]).unwrap();
    let snapshot = RunSnapshot::from_wire(wire).unwrap();
    let mut restored = Run::default();
    restored.restore(snapshot)?;
    Ok(restored)
}

/// `mutant` snapshotted, through its wire bytes, and restored on a fresh
/// mutant.
fn mutant_restored(mutant: &Mutant) -> Result<Mutant, Report> {
    let bytes = prost::Message::encode_to_vec(&mutant.snapshot().expect("it offers one").to_wire());
    let wire =
        <<MutantSnapshot as DomainMessage>::Wire as prost::Message>::decode(&bytes[..]).unwrap();
    let snapshot = MutantSnapshot::from_wire(wire).unwrap();
    let mut restored = Mutant::default();
    restored.restore(snapshot)?;
    Ok(restored)
}

/// `core`'s run as it stands, restored from its snapshot, beside the same
/// run replayed.
async fn run_states(core: &Core, run: &Uuid) -> (Run, Run) {
    let replayed = replay_run(run_events(core, run).await);
    let restored = run_restored(&replayed).unwrap();
    (restored, replayed)
}

#[tokio::test]
async fn test_restoring_a_run_s_snapshot_at_each_point_equals_its_replay() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["c", "a", "b"]))
        .await
        .unwrap();
    let (restored, replayed) = run_states(&core, &run).await;
    assert!(replayed.live(), "just planned");
    assert_eq!(restored, replayed, "just planned");
    core.report(run_cover(&run), &preparation("calc-1"), "prepared")
        .await
        .unwrap();
    for (id, v) in [("a", Verdict::Killed), ("c", Verdict::Survived)] {
        core.report(run_cover(&run), &decision(id, v), id)
            .await
            .unwrap();
    }
    let (restored, replayed) = run_states(&core, &run).await;
    assert!(replayed.live(), "partly decided");
    assert_eq!(restored, replayed, "partly decided");
    core.report(run_cover(&run), &decision("b", Verdict::Killed), "b")
        .await
        .unwrap();
    let (restored, replayed) = run_states(&core, &run).await;
    assert!(replayed.finished(), "finished");
    assert_eq!(restored, replayed, "finished");
}

#[tokio::test]
async fn test_restoring_a_failed_run_s_snapshot_equals_its_replay_with_mutants_owed() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    core.report(run_cover(&run), &decision("a", Verdict::Killed), "a")
        .await
        .unwrap();
    let failed = PreparationFailed {
        program: Some("calc-2".to_string()),
        reason: "did not build".to_string(),
    };
    core.report(run_cover(&run), &failed, "failed")
        .await
        .unwrap();
    let (restored, replayed) = run_states(&core, &run).await;
    assert!(replayed.finished());
    assert_eq!(restored, replayed);
}

#[tokio::test]
async fn test_restoring_a_mutant_s_snapshot_at_each_point_equals_its_replay() {
    let core = core();
    let (first, second) = (new_run(), new_run());
    let states = |events| {
        let replayed = replay_mutant(events);
        (mutant_restored(&replayed).unwrap(), replayed)
    };
    core.command(mutant_cover("a"), &plan_mutant(&first, "a"))
        .await
        .unwrap();
    let (restored, replayed) = states(mutant_events(&core, "a").await);
    assert_eq!(restored, replayed, "just planned");
    let mut other = plan_mutant(&second, "a");
    if let Some(m) = other.mutant.as_mut().and_then(|e| e.mutant.as_mut()) {
        m.line = 9;
    }
    core.command(mutant_cover("a"), &other).await.unwrap();
    core.report(mutant_cover("a"), &reported(&second, Verdict::Killed), "2")
        .await
        .unwrap();
    let (restored, replayed) = states(mutant_events(&core, "a").await);
    assert_eq!(restored, replayed, "decided for one run of two");
    core.report(mutant_cover("a"), &reported(&first, Verdict::Survived), "1")
        .await
        .unwrap();
    let (restored, replayed) = states(mutant_events(&core, "a").await);
    assert_eq!(restored, replayed, "decided for both");
}

/// The tallies of `counts`, in the order given.
fn tally(counts: &[(Verdict, u32)]) -> Vec<VerdictCount> {
    counts
        .iter()
        .map(|&(verdict, count)| VerdictCount { verdict, count })
        .collect()
}

/// A run, started, with `undecided` owed and `counts` tallied.
fn run_snapshot(undecided: &[&str], counts: &[(Verdict, u32)]) -> RunSnapshot {
    RunSnapshot {
        started_at: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        undecided: undecided.iter().map(ToString::to_string).collect(),
        counts: tally(counts),
        finished: false,
        ..RunSnapshot::default()
    }
}

#[test]
fn test_restoring_a_run_from_an_invalid_snapshot_is_refused_and_leaves_it_unchanged() {
    let unstarted = RunSnapshot {
        started_at: None,
        ..run_snapshot(&["a"], &[])
    };
    for (snapshot, refusal) in [
        (
            run_snapshot(&["b", "a"], &[]),
            SnapshotRejection::UndecidedUnordered,
        ),
        (
            run_snapshot(&["a", "a"], &[]),
            SnapshotRejection::UndecidedUnordered,
        ),
        (
            RunSnapshot {
                prepared: vec![1, 0],
                ..run_snapshot(&["a"], &[])
            },
            SnapshotRejection::PreparedUnordered,
        ),
        (unstarted, SnapshotRejection::Unstarted),
        (
            RunSnapshot {
                finished: true,
                ..RunSnapshot::default()
            },
            SnapshotRejection::Unstarted,
        ),
        (
            RunSnapshot {
                counts: tally(&[(Verdict::Killed, 1)]),
                ..RunSnapshot::default()
            },
            SnapshotRejection::Unstarted,
        ),
        (
            run_snapshot(&[], &[(Verdict::Survived, 1), (Verdict::Killed, 1)]),
            SnapshotRejection::TalliesUnordered,
        ),
        (
            run_snapshot(&[], &[(Verdict::Killed, 1), (Verdict::Killed, 2)]),
            SnapshotRejection::TalliesUnordered,
        ),
    ] {
        let mut run = Run::default();
        run.restore(run_snapshot(&["z"], &[(Verdict::Killed, 2)]))
            .unwrap();
        let before = run.clone();
        assert_eq!(
            run.restore(snapshot.clone()),
            Err(Report::of(&refusal)),
            "{snapshot:?}"
        );
        assert_eq!(run, before, "{snapshot:?}");
    }
}

#[test]
fn test_restoring_a_mutant_from_an_invalid_snapshot_is_refused_and_leaves_it_unchanged() {
    let entry = |run: &str, planned: bool| MutantRun {
        run: run.to_string(),
        mutant: planned.then(|| mutant("a")),
        decided: false,
        ..MutantRun::default()
    };
    for (runs, refusal) in [
        (
            vec![entry("2", true), entry("1", true)],
            SnapshotRejection::RunsUnordered,
        ),
        (
            vec![entry("1", true), entry("1", true)],
            SnapshotRejection::RunsUnordered,
        ),
        (vec![entry("1", false)], SnapshotRejection::Unplanned),
        (
            vec![MutantRun {
                ran: vec![program_run(0), program_run(0)],
                ..entry("1", true)
            }],
            SnapshotRejection::RanRepeated,
        ),
    ] {
        let mut mutant = Mutant::default();
        mutant
            .restore(MutantSnapshot {
                runs: vec![entry("0", true)],
            })
            .unwrap();
        let before = mutant.clone();
        let snapshot = MutantSnapshot { runs };
        assert_eq!(
            mutant.restore(snapshot.clone()),
            Err(Report::of(&refusal)),
            "{snapshot:?}"
        );
        assert_eq!(mutant, before, "{snapshot:?}");
    }
}

/// The snapshot `core`'s store holds for `cover`, and the sequence of the
/// stream's last event.
async fn stored_snapshot(core: &Core, cover: &Cover) -> (angzarr_embedded::v1::Snapshot, u64) {
    let stream = StreamRef::try_from(cover).unwrap();
    let snapshot = core.store.read_snapshot(stream).await.unwrap();
    let last = u64::try_from(core.events(cover, 0).await.unwrap().len()).unwrap() - 1;
    (snapshot.expect("a snapshot"), last)
}

#[tokio::test]
async fn test_handling_a_run_s_input_stores_its_snapshot_at_the_head() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    core.report(run_cover(&run), &decision("a", Verdict::Killed), "a")
        .await
        .unwrap();
    let (snapshot, last) = stored_snapshot(&core, &run_cover(&run)).await;
    assert_eq!(snapshot.sequence, last);
    let state: RunSnapshot = decode_message(&snapshot.state.unwrap()).unwrap().unwrap();
    let mut restored = Run::default();
    restored.restore(state).unwrap();
    assert_eq!(restored, replay_run(run_events(&core, &run).await));
}

#[tokio::test]
async fn test_handling_a_mutant_s_input_stores_its_snapshot_at_the_head() {
    let core = core();
    let run = new_run();
    core.command(mutant_cover("a"), &plan_mutant(&run, "a"))
        .await
        .unwrap();
    core.report(mutant_cover("a"), &reported(&run, Verdict::Killed), "1")
        .await
        .unwrap();
    let (snapshot, last) = stored_snapshot(&core, &mutant_cover("a")).await;
    assert_eq!(snapshot.sequence, last);
    let state: MutantSnapshot = decode_message(&snapshot.state.unwrap()).unwrap().unwrap();
    let mut restored = Mutant::default();
    restored.restore(state).unwrap();
    assert_eq!(restored, replay_mutant(mutant_events(&core, "a").await));
}

/// `message`'s wire form: its full name and its encoding as hex.
fn refusal_line<R: DomainMessage>(message: &R) -> String
where
    R::Wire: prost::Name,
{
    let hex: String = prost::Message::encode_to_vec(&message.to_wire())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{} {hex}", <R::Wire as prost::Name>::full_name())
        .trim_end()
        .to_string()
}

#[test]
fn test_every_refusal_encodes_to_the_bytes_it_always_has() {
    let lines = vec![
        refusal_line(&AlreadyPlanned {}),
        refusal_line(&NoRun {}),
        refusal_line(&NoMutant {}),
        refusal_line(&PlannedForRun {}),
    ];
    let expected: Vec<&str> = include_str!("encodings.txt").lines().collect();
    assert_eq!(lines, expected);
}

#[tokio::test]
async fn test_a_plan_without_a_run_a_mutant_or_the_mutant_s_plan_is_refused_and_records_nothing() {
    let core = core();
    let run = new_run();
    for (command, refusal) in [
        (
            PlanMutant {
                run: None,
                mutant: Some(executable("a")),
                programs: 1,
            },
            "cherenkov.mutant.v1.NoRun",
        ),
        (
            PlanMutant {
                run: Some(RunId::from(run.clone())),
                mutant: None,
                programs: 1,
            },
            "cherenkov.mutant.v1.NoMutant",
        ),
        (
            // Decided it could be, never run.
            PlanMutant {
                run: Some(RunId::from(run.clone())),
                mutant: Some(ExecutableMutant {
                    mutant: Some(mutant("a")),
                    plan: None,
                }),
                programs: 1,
            },
            "cherenkov.mutant.v1.NoMutant",
        ),
        (PlanMutant::default(), "cherenkov.mutant.v1.NoRun"),
    ] {
        let error = core
            .command(mutant_cover("a"), &command)
            .await
            .expect_err("refused");
        assert!(
            error.ends_with(&format!("refusal_type={refusal}")),
            "{error}"
        );
    }
    assert!(mutant_events(&core, "a").await.is_empty());
}

#[test]
fn test_a_snapshot_stored_under_the_rules_before_the_vocabulary_is_passed_over() {
    // The tallies of a snapshot became verdicts and counts, so the state
    // changed: only a snapshot of this minor version resumes a stream.
    let core = core();
    let catalog = core.registry.catalog();
    for (domain, state) in [
        (RUN_DOMAIN, pack_message(&RunSnapshot::default())),
        (MUTANT_DOMAIN, pack_message(&MutantSnapshot::default())),
    ] {
        let at = |rules_version: &str| angzarr_embedded::v1::Snapshot {
            sequence: 1,
            state: Some(state.clone()),
            rules_version: rules_version.to_string(),
        };
        let current = catalog.rules_version(domain).expect("registered");
        assert!(catalog.snapshot_usable(domain, &at(&current)), "{domain}");
        assert!(!catalog.snapshot_usable(domain, &at("0.1.0")), "{domain}");
    }
}

#[test]
fn register_puts_both_aggregates_in_a_registry() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut domains: Vec<&str> = registry.catalog().domains().collect();
    domains.sort_unstable();
    assert_eq!(domains, [MUTANT_DOMAIN, RUN_DOMAIN]);
}

#[test]
fn registering_twice_is_refused() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    assert!(register(&mut registry).is_err());
}

#[tokio::test]
async fn test_a_decision_is_recorded_on_the_run_with_the_decision_whole() {
    let core = core();
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a"])).await.unwrap();
    core.report(run_cover(&run), &preparation("p"), "p")
        .await
        .unwrap();
    let decided = MutantDecided {
        mutant: Some(PlannedMutant {
            id: "a".to_string(),
            file: "src/lib.rs".to_string(),
            ..PlannedMutant::default()
        }),
        verdict: Some(Outcome {
            killed_by: vec![KilledBy {
                test: "t".to_string(),
                location: None,
                message: String::new(),
                got: None,
                expected: None,
            }],
            ..outcome(Verdict::Killed)
        }),
        run: Some(RunId::from(run.clone())),
        programs: vec!["p".to_string()],
    };
    let fact = DecisionReported {
        decision: Some(decided.clone()),
        ..decision("a", Verdict::Killed)
    };
    core.report(run_cover(&run), &fact, "a").await.unwrap();
    let recorded = run_events(&core, &run)
        .await
        .into_iter()
        .find_map(|event| match event {
            RunEvent::DecisionRecorded(d) => Some(d),
            _ => None,
        })
        .unwrap();
    assert_eq!(recorded.decision, Some(decided));
}
