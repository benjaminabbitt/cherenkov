use angzarr_embedded::markup::DomainMessage;
use angzarr_embedded::markup::decode_message;
use angzarr_embedded::testing::{Harness, block_on_ready};
use angzarr_embedded::v1::{
    Command, CommandBook, CommandPage, Cover, Fact, FactOrigin, command_page,
};
use angzarr_embedded::{Domain, Handled, Registry, SnapshotStore, StreamRef};

use std::time::Duration;

use cherenkov_mutation::execution::{RawOutcome, Tier as CoreTier};

use crate::events::{MutantEvent, RunEvent, mutant_event, run_event};
use crate::mutant::Observation;

use super::*;

/// Whether a fact was recorded, or was a duplicate of one on the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Applied,
    Duplicate,
}

/// The `n`th run's id: the same in every test, never nil.
fn id(n: u8) -> Uuid {
    Uuid::from_bytes([n; 16])
}

/// The id of the run `run_harness` holds.
fn run_id() -> Uuid {
    id(1)
}

fn run_harness() -> Harness<Run> {
    let mut registry = Registry::new();
    registry.aggregate::<Run>(Run::default).unwrap();
    Harness::new(registry, run_cover(&run_id()))
}

fn mutant_harness(id: &str) -> Harness<Mutant> {
    let mut registry = Registry::new();
    registry.aggregate::<Mutant>(Mutant::default).unwrap();
    Harness::new(registry, mutant_cover(id))
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

/// Sending a command or a fact to the harness's stream, as a client does.
trait Sends {
    fn handle(&mut self, body: command_page::Body) -> Result<Delivery, String>;

    fn command<C: DomainMessage>(&mut self, command: &C) -> Result<(), String> {
        let body = command_page::Body::Command(Command {
            payload: Some(pack_message(command)),
            handling: None,
        });
        self.handle(body).map(drop)
    }

    fn report<F: DomainMessage>(
        &mut self,
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
        self.handle(body)
    }
}

impl<D: Domain> Sends for Harness<D> {
    fn handle(&mut self, body: command_page::Body) -> Result<Delivery, String> {
        let book = CommandBook {
            cover: Some(self.cover().clone()),
            page: Some(CommandPage {
                body: Some(body),
                ..CommandPage::default()
            }),
        };
        match self.when_book(book).handled() {
            Ok(Handled::Applied(_)) => Ok(Delivery::Applied),
            Ok(Handled::Duplicate(..)) => Ok(Delivery::Duplicate),
            Err(e) => Err(e.to_string()),
        }
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
        selection_why: None,
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
        baseline_output_tail: None,
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

fn run_events(core: &Harness<Run>) -> Vec<RunEvent> {
    core.pages().iter().filter_map(run_event).collect()
}

fn mutant_events(core: &Harness<Mutant>) -> Vec<MutantEvent> {
    core.pages().iter().filter_map(mutant_event).collect()
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

#[test]
fn test_planning_a_run_records_its_start_then_its_mutants_in_plan_order() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    let events = run_events(&core);
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

#[test]
fn test_a_run_is_planned_once() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    assert!(core.command(&plan(&["b"])).is_err());
    assert_eq!(run_events(&core).len(), 2);
}

#[test]
fn test_a_run_of_no_mutants_finishes_with_its_plan() {
    let mut core = run_harness();
    core.command(&plan(&[])).unwrap();
    let events = run_events(&core);
    let finished = finishes(&events);
    assert_eq!(finished.len(), 1, "{events:?}");
    assert_eq!(finished[0].status, RunStatus::Complete);
}

#[test]
fn test_each_decision_is_counted_and_the_last_finishes_the_run_with_its_tallies() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b", "c"])).unwrap();
    for (id, v) in [("a", Verdict::Killed), ("b", Verdict::Survived)] {
        core.report(&decision(id, v), id).unwrap();
    }
    assert!(finishes(&run_events(&core)).is_empty(), "c is owed");
    core.report(&decision("c", Verdict::Killed), "c").unwrap();
    let events = run_events(&core);
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

#[test]
fn test_a_decision_redelivered_under_its_id_is_a_duplicate() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    let first = core.report(&decision("a", Verdict::Killed), "a");
    let again = core.report(&decision("a", Verdict::Killed), "a");
    assert_eq!(first, Ok(Delivery::Applied));
    assert_eq!(again, Ok(Delivery::Duplicate));
}

#[test]
fn test_a_prepared_program_is_recorded_with_its_budget_from_its_baseline() {
    let mut core = run_harness();
    core.command(&plan_for(&["a"], 2)).unwrap();
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
        selection_why: None,
        tests: Some(3),
        selection: Selection::RecordedWithFailures,
        failed_tests: vec!["test_flaky".to_string()],
        selection_reason: None,
        drives: Some(drives.clone()),
        ..preparation_at(1, "target/debug/deps/calc-1")
    };
    core.report(&prepared, "program 1").unwrap();
    assert_eq!(
        run_events(&core).last(),
        Some(&RunEvent::ProgramPrepared(ProgramPrepared {
            selection_why: None,
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

#[test]
fn test_the_last_program_prepared_makes_the_run_ready_with_its_mutants_in_plan_order() {
    let mut core = run_harness();
    core.command(&plan_for(&["c", "a", "b"], 2)).unwrap();
    let ready = |events: &[RunEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, RunEvent::RunReady(_)))
            .count()
    };
    core.report(&preparation_at(1, "it-2"), "program 1")
        .unwrap();
    assert_eq!(ready(&run_events(&core)), 0, "one of two");
    // Told again under another id, a program prepared is not counted twice.
    core.report(&preparation_at(1, "it-2"), "again").unwrap();
    assert_eq!(ready(&run_events(&core)), 0, "the same one");
    core.report(&preparation_at(0, "calc-1"), "program 0")
        .unwrap();
    let events = run_events(&core);
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

#[test]
fn test_a_failed_baselines_output_tail_is_on_the_program_failed_event() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let failing = PreparationReported {
        selection_why: None,
        baseline: Some(observation::encode(&RawOutcome::Exited {
            code: 101,
            tier: CoreTier::P,
            wall: Duration::from_millis(40),
        })),
        baseline_output_tail: Some("thread 'a' panicked\n".to_string()),
        ..preparation("calc-1")
    };
    core.report(&failing, "program 0").unwrap();
    let events = run_events(&core);
    let [
        ..,
        RunEvent::ProgramFailed(program),
        RunEvent::RunFinished(_),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(
        program.output_tail.as_deref(),
        Some("thread 'a' panicked\n")
    );
}

#[test]
fn test_a_baseline_that_fails_finishes_the_run_failed_and_it_is_never_ready() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let failing = PreparationReported {
        selection_why: None,
        baseline: Some(observation::encode(&RawOutcome::Exited {
            code: 101,
            tier: CoreTier::P,
            wall: Duration::from_millis(40),
        })),
        baseline_failures: vec!["calc::test_total at tests/calc.rs:5:5: boom".to_string()],
        ..preparation("calc-1")
    };
    core.report(&failing, "program 0").unwrap();
    let events = run_events(&core);
    let [
        ..,
        RunEvent::ProgramFailed(program),
        RunEvent::RunFinished(finished),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(program.program.as_deref(), Some("calc-1"));
    assert_eq!(program.reason, "", "a coded failure leaves the text empty");
    let why = program.why.as_ref().expect("a coded reason");
    assert_eq!(why.code, "cherenkov-run-domain-program-baseline-failed");
    let args: std::collections::BTreeMap<String, String> =
        angzarr_embedded::v1::Diagnostic::from(why.clone())
            .flat_args()
            .into_iter()
            .collect();
    assert_eq!(args["program"], "calc-1");
    assert!(args["outcome"].contains("code: 101"), "{why:?}");
    assert!(
        args["failing"].contains("calc::test_total at tests/calc.rs:5:5: boom"),
        "{why:?}"
    );
    assert_eq!(finished.status, RunStatus::Failed);
    assert!(!events.iter().any(|e| matches!(e, RunEvent::RunReady(_))));
}

#[test]
fn test_a_baseline_the_executor_did_not_report_is_a_bug_of_the_reporter() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let silent = PreparationReported {
        selection_why: None,
        baseline: None,
        ..preparation("calc-1")
    };
    assert!(core.report(&silent, "program 0").is_err());
    assert_eq!(run_events(&core).len(), 2, "nothing recorded");
}

#[test]
fn test_a_dry_run_is_never_ready() {
    let mut core = run_harness();
    core.command(&plan_for(&["a"], 0)).unwrap();
    core.report(&preparation("calc-1"), "stray").unwrap();
    let events = run_events(&core);
    assert!(!events.iter().any(|e| matches!(e, RunEvent::RunReady(_))));
}

#[test]
fn test_a_program_that_does_not_prepare_is_recorded_and_finishes_the_run_failed() {
    // A reader of the stream learns why the run stopped, and that it is
    // over, from the stream alone.
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let failed = PreparationFailed {
        why: None,
        program: Some("calc-1".to_string()),
        reason: "unmutated tests do not pass".to_string(),
    };
    core.report(&failed, "preparation failed").unwrap();
    let events = run_events(&core);
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

fn why() -> angzarr::Diagnostic {
    crate::why::encode(&Report::new("cherenkov-exec-dap-session").arg("detail", "closed"))
}

/// A converted `Report` with several args, one a nested cause: its map is
/// ordered by key, so its bytes are the same every time.
fn why_many() -> angzarr::Diagnostic {
    let cause = Report::new("cherenkov-exec-dap-rpc").arg("detail", "connection closed");
    why::encode(
        &Report::new("cherenkov-exec-dap-session")
            .arg("program", "calc-0123")
            .arg("count", 3_u32)
            .arg("delta", -3_i32)
            .arg("cause", coded_diagnostic::Arg::nested(&cause)),
    )
}

/// A reason whose cause has no args of its own.
fn why_bare() -> angzarr::Diagnostic {
    why::encode(&Report::new("cherenkov-exec-dap-session").arg(
        "cause",
        coded_diagnostic::Arg::nested(&Report::new("cherenkov-exec-dap-closed")),
    ))
}

#[test]
fn test_a_report_with_several_args_encodes_to_the_same_bytes_every_time() {
    let bytes = |why: angzarr::Diagnostic| {
        prost::Message::encode_to_vec(&angzarr_embedded::v1::Diagnostic::from(why))
    };
    let first = bytes(why_many());
    for _ in 0..16 {
        assert_eq!(bytes(why_many()), first);
    }
}

#[test]
fn test_a_coded_failure_is_recorded_as_it_was_reported() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let failed = PreparationFailed {
        why: Some(why()),
        program: Some("calc-1".to_string()),
        reason: String::new(),
    };
    core.report(&failed, "preparation failed").unwrap();
    let events = run_events(&core);
    let [.., RunEvent::ProgramFailed(program), _] = &events[..] else {
        panic!("{events:?}");
    };
    assert_eq!(program.why, Some(why()));
    assert_eq!(program.reason, "");
}

#[test]
fn test_a_coded_selection_reason_is_kept_in_the_prepared_program() {
    let mut core = run_harness();
    core.command(&plan_for(&["a"], 1)).unwrap();
    let prepared = PreparationReported {
        selection_reason: None,
        selection_why: Some(why()),
        ..preparation_at(0, "calc-1")
    };
    core.report(&prepared, "program 0").unwrap();
    let events = run_events(&core);
    let Some(RunEvent::ProgramPrepared(program)) = events
        .iter()
        .find(|e| matches!(e, RunEvent::ProgramPrepared(_)))
    else {
        panic!("{events:?}");
    };
    assert_eq!(program.selection_why, Some(why()));
}

#[test]
fn test_nothing_is_recorded_after_the_run_finishes() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    let after = run_events(&core).len();
    let failed = PreparationFailed {
        why: None,
        program: None,
        reason: "late".to_string(),
    };
    core.report(&failed, "late").unwrap();
    core.report(&preparation("calc-9"), "program 9").unwrap();
    assert_eq!(run_events(&core).len(), after);
}

#[test]
fn test_a_run_told_it_is_abandoned_records_why_and_finishes_abandoned_with_its_tallies() {
    // A run a newer plan has superseded is over, and its stream says so.
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    let abandoned = Abandoned {
        why: None,
        reason: "the plan changed".to_string(),
    };
    core.report(&abandoned, "abandoned").unwrap();
    let events = run_events(&core);
    let [
        ..,
        RunEvent::RunAbandoned(why),
        RunEvent::RunFinished(finished),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(why.reason, "the plan changed");
    assert_eq!(finished.status, RunStatus::Abandoned);
    let counts: Vec<(Verdict, u32)> = finished
        .counts
        .iter()
        .map(|tally| (tally.verdict, tally.count))
        .collect();
    assert_eq!(counts, [(Verdict::Killed, 1)]);
}

#[test]
fn test_a_run_already_over_is_not_abandoned_and_one_told_twice_is_abandoned_once() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    let over = run_events(&core).len();
    let abandoned = Abandoned {
        why: None,
        reason: "late".to_string(),
    };
    core.report(&abandoned, "abandoned").unwrap();
    assert_eq!(
        run_events(&core).len(),
        over,
        "finished runs stay as they ended"
    );

    let mut live = run_harness();
    live.command(&plan(&["a"])).unwrap();
    for _ in 0..2 {
        live.report(&abandoned, "abandoned").unwrap();
    }
    let abandonments = run_events(&live)
        .iter()
        .filter(|e| matches!(e, RunEvent::RunAbandoned(_)))
        .count();
    assert_eq!(abandonments, 1);
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
        why: None,
        every_test: false,
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

#[test]
fn test_a_mutant_is_planned_once_per_asking_run() {
    let mut core = mutant_harness("a");
    let (first, second) = (id(1), id(2));
    core.command(&plan_mutant(&first, "a")).unwrap();
    assert!(core.command(&plan_mutant(&first, "a")).is_err());
    core.command(&plan_mutant(&second, "a")).unwrap();
    let runs: Vec<Option<RunId>> = mutant_events(&core)
        .into_iter()
        .map(|e| match e {
            MutantEvent::MutantPlanned(p) => p.run,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(runs, [Some(RunId::from(first)), Some(RunId::from(second))]);
}

#[test]
fn test_a_mutant_is_decided_once_per_asking_run_as_planned_with_its_inputs() {
    let mut core = mutant_harness("a");
    let (asked, other) = (id(1), id(2));
    core.command(&plan_mutant(&asked, "a")).unwrap();
    for (run, verdict, external) in [
        (&asked, Verdict::Killed, "1"),
        (&asked, Verdict::Survived, "2"),
        (&other, Verdict::Killed, "3"),
    ] {
        core.report(&reported(run, verdict), external).unwrap();
    }
    let decided: Vec<MutantDecided> = mutant_events(&core)
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
            // The reported program selects no tests: it ran every test.
            verdict: Some(Outcome {
                every_test: true,
                ..outcome(Verdict::Killed)
            }),
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
    let problems = coded_diagnostic::catalog::check(EN_US.0, EN_US.1, &crate::diagnostic_samples());
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
            RunEvent::RunAbandoned(e) => run.run_abandoned(e),
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
fn run_states(core: &Harness<Run>) -> (Run, Run) {
    let replayed = replay_run(run_events(core));
    let restored = run_restored(&replayed).unwrap();
    (restored, replayed)
}

#[test]
fn test_restoring_a_run_s_snapshot_at_each_point_equals_its_replay() {
    let mut core = run_harness();
    core.command(&plan(&["c", "a", "b"])).unwrap();
    let (restored, replayed) = run_states(&core);
    assert!(replayed.live(), "just planned");
    assert_eq!(restored, replayed, "just planned");
    core.report(&preparation("calc-1"), "prepared").unwrap();
    for (id, v) in [("a", Verdict::Killed), ("c", Verdict::Survived)] {
        core.report(&decision(id, v), id).unwrap();
    }
    let (restored, replayed) = run_states(&core);
    assert!(replayed.live(), "partly decided");
    assert_eq!(restored, replayed, "partly decided");
    core.report(&decision("b", Verdict::Killed), "b").unwrap();
    let (restored, replayed) = run_states(&core);
    assert!(replayed.finished(), "finished");
    assert_eq!(restored, replayed, "finished");
}

#[test]
fn test_restoring_a_failed_run_s_snapshot_equals_its_replay_with_mutants_owed() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    let failed = PreparationFailed {
        why: None,
        program: Some("calc-2".to_string()),
        reason: "did not build".to_string(),
    };
    core.report(&failed, "failed").unwrap();
    let (restored, replayed) = run_states(&core);
    assert!(replayed.finished());
    assert_eq!(restored, replayed);
}

#[test]
fn test_restoring_a_mutant_s_snapshot_at_each_point_equals_its_replay() {
    let mut core = mutant_harness("a");
    let (first, second) = (id(1), id(2));
    let states = |events| {
        let replayed = replay_mutant(events);
        (mutant_restored(&replayed).unwrap(), replayed)
    };
    core.command(&plan_mutant(&first, "a")).unwrap();
    let (restored, replayed) = states(mutant_events(&core));
    assert_eq!(restored, replayed, "just planned");
    let mut other = plan_mutant(&second, "a");
    if let Some(m) = other.mutant.as_mut().and_then(|e| e.mutant.as_mut()) {
        m.line = 9;
    }
    core.command(&other).unwrap();
    core.report(&reported(&second, Verdict::Killed), "2")
        .unwrap();
    let (restored, replayed) = states(mutant_events(&core));
    assert_eq!(restored, replayed, "decided for one run of two");
    core.report(&reported(&first, Verdict::Survived), "1")
        .unwrap();
    let (restored, replayed) = states(mutant_events(&core));
    assert_eq!(restored, replayed, "decided for both");
}

/// The tallies of `counts`, in the order given.
fn tally(counts: &[(Verdict, u32)]) -> Vec<VerdictCount> {
    counts
        .iter()
        .map(|&(verdict, count)| VerdictCount { verdict, count })
        .collect()
}

/// A run, started, with `planned` mutants and `counts` tallied.
fn run_snapshot(planned: u32, counts: &[(Verdict, u32)]) -> RunSnapshot {
    RunSnapshot {
        started_at: Some(Timestamp {
            seconds: 1,
            nanos: 0,
        }),
        planned,
        counts: tally(counts),
        finished: false,
        ..RunSnapshot::default()
    }
}

#[test]
fn test_restoring_a_run_from_an_invalid_snapshot_is_refused_and_leaves_it_unchanged() {
    let unstarted = RunSnapshot {
        started_at: None,
        ..run_snapshot(1, &[])
    };
    for (snapshot, refusal) in [
        (
            run_snapshot(1, &[(Verdict::Killed, 1), (Verdict::Survived, 1)]),
            SnapshotRejection::TalliesExceedPlan,
        ),
        (
            RunSnapshot {
                prepared: vec![1, 0],
                ..run_snapshot(1, &[])
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
            run_snapshot(2, &[(Verdict::Survived, 1), (Verdict::Killed, 1)]),
            SnapshotRejection::TalliesUnordered,
        ),
        (
            run_snapshot(2, &[(Verdict::Killed, 1), (Verdict::Killed, 2)]),
            SnapshotRejection::TalliesUnordered,
        ),
    ] {
        let mut run = Run::default();
        run.restore(run_snapshot(3, &[(Verdict::Killed, 2)]))
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
fn stored_snapshot<D: Domain>(core: &Harness<D>) -> (angzarr_embedded::v1::Snapshot, u64) {
    let stream = StreamRef::try_from(core.cover()).unwrap();
    let snapshot = block_on_ready(core.store().read_snapshot(stream)).unwrap();
    let last = u64::try_from(core.pages().len()).unwrap() - 1;
    (snapshot.expect("a snapshot"), last)
}

#[test]
fn test_handling_a_run_s_input_stores_its_snapshot_at_the_head() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    let (snapshot, last) = stored_snapshot(&core);
    assert_eq!(snapshot.sequence, last);
    let state: RunSnapshot = decode_message(&snapshot.state.unwrap()).unwrap().unwrap();
    let mut restored = Run::default();
    restored.restore(state).unwrap();
    assert_eq!(restored, replay_run(run_events(&core)));
}

#[test]
fn test_handling_a_mutant_s_input_stores_its_snapshot_at_the_head() {
    let mut core = mutant_harness("a");
    let run = run_id();
    core.command(&plan_mutant(&run, "a")).unwrap();
    core.report(&reported(&run, Verdict::Killed), "1").unwrap();
    let (snapshot, last) = stored_snapshot(&core);
    assert_eq!(snapshot.sequence, last);
    let state: MutantSnapshot = decode_message(&snapshot.state.unwrap()).unwrap().unwrap();
    let mut restored = Mutant::default();
    restored.restore(state).unwrap();
    assert_eq!(restored, replay_mutant(mutant_events(&core)));
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
        refusal_line(&PreparationFailed {
            program: None,
            reason: String::new(),
            why: Some(why()),
        }),
        refusal_line(&ProgramFailed {
            program: Some("calc-1".to_string()),
            reason: String::new(),
            why: Some(why()),
            output_tail: None,
        }),
        refusal_line(&ProgramFailed {
            program: None,
            reason: String::new(),
            why: Some(why_many()),
            output_tail: None,
        }),
        refusal_line(&ProgramFailed {
            program: None,
            reason: String::new(),
            why: Some(why_bare()),
            output_tail: None,
        }),
    ];
    let expected: Vec<&str> = include_str!("encodings.txt").lines().collect();
    assert_eq!(lines, expected);
}

#[test]
fn test_a_plan_without_a_run_a_mutant_or_the_mutant_s_plan_is_refused_and_records_nothing() {
    let mut core = mutant_harness("a");
    let run = run_id();
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
        let error = core.command(&command).expect_err("refused");
        assert!(
            error.ends_with(&format!("refusal_type={refusal}")),
            "{error}"
        );
    }
    assert!(mutant_events(&core).is_empty());
}

#[test]
fn test_a_snapshot_stored_under_the_rules_before_the_vocabulary_is_passed_over() {
    // The tallies of a snapshot became verdicts and counts, so the state
    // changed: only a snapshot of this minor version resumes a stream.
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let catalog = registry.catalog();
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

#[test]
fn test_a_decision_is_recorded_on_the_run_with_the_decision_whole() {
    let mut core = run_harness();
    let run = run_id();
    core.command(&plan(&["a"])).unwrap();
    core.report(&preparation("p"), "p").unwrap();
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
    core.report(&fact, "a").unwrap();
    let recorded = run_events(&core)
        .into_iter()
        .find_map(|event| match event {
            RunEvent::DecisionRecorded(d) => Some(d),
            _ => None,
        })
        .unwrap();
    assert_eq!(recorded.decision, Some(decided));
}

/// The bytes of a ready run of `n` mutants' snapshot after its first decision.
fn snapshot_size_after_first_decision(n: usize) -> usize {
    let mut core = run_harness();
    let ids: Vec<String> = (0..n).map(|i| format!("mutant-{i:05}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    core.command(&plan(&refs)).unwrap();
    core.report(&preparation("calc-1"), "prepared").unwrap();
    core.report(&decision(&ids[0], Verdict::Killed), "first")
        .unwrap();
    let replayed = replay_run(run_events(&core));
    assert!(replayed.ready() && replayed.live());
    prost::Message::encode_to_vec(&replayed.snapshot().expect("it offers one").to_wire()).len()
}

#[test]
fn test_a_ready_run_s_snapshot_does_not_grow_with_the_mutants_planned() {
    let small = snapshot_size_after_first_decision(200);
    let large = snapshot_size_after_first_decision(400);
    assert!(
        large <= small + 4,
        "200 mutants: {small} bytes, 400 mutants: {large} bytes"
    );
}

#[test]
fn test_a_decision_told_to_an_abandoned_run_with_a_mutant_owed_records_nothing() {
    let mut core = run_harness();
    core.command(&plan(&["a", "b"])).unwrap();
    let abandoned = Abandoned {
        why: None,
        reason: "the plan changed".to_string(),
    };
    core.report(&abandoned, "abandoned").unwrap();
    let before = run_events(&core).len();
    core.report(&decision("a", Verdict::Killed), "a").unwrap();
    assert_eq!(run_events(&core).len(), before);
}

#[test]
fn test_restoring_an_unstarted_snapshot_that_holds_nothing_is_accepted() {
    let mut run = Run::default();
    assert_eq!(run.restore(RunSnapshot::default()), Ok(()));
}

#[test]
fn test_a_purge_tombstone_in_either_stream_is_no_run_or_mutant_event_and_is_skipped() {
    let closed = angzarr_embedded::angzarr_ports::tombstone::page(7);
    assert!(run_event(&closed).is_none());
    assert!(mutant_event(&closed).is_none());
}

#[cfg(feature = "purge")]
#[test]
fn test_a_run_stream_purged_to_a_tombstone_is_planned_again_not_refused_as_already_planned() {
    let mut core = run_harness();
    core.command(&plan(&["a"])).unwrap();
    let stream = StreamRef::try_from(core.cover()).unwrap();
    let head = u64::try_from(core.pages().len()).unwrap();
    let purged = block_on_ready(angzarr_embedded::Purge::purge(
        core.store().inner(),
        stream,
        head,
    ))
    .unwrap();
    assert_eq!(purged, head);
    assert!(
        core.pages()
            .last()
            .is_some_and(angzarr_embedded::angzarr_ports::tombstone::is)
    );
    core.command(&plan(&["b"]))
        .expect("a purged run plans afresh");
    assert_eq!(run_events(&core).len(), 2, "its new start and plan only");
}
