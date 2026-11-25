//! A mutant decides its own verdict from what each test program made of it.
//! One test for each way a verdict comes about, on angzarr's `Harness`: a
//! mutant planned (given), a program's run reported (when), what the
//! mutant recorded (then).

use std::time::Duration;

use angzarr_embedded::Registry;
use angzarr_embedded::markup::pack_message;
use angzarr_embedded::testing::Harness;
use angzarr_embedded::v1::command_page;
use angzarr_embedded::v1::{CommandBook, CommandPage, Cover, Fact, FactOrigin, Uuid};
use cherenkov_mutation::execution::{RawOutcome, Tier as CoreTier};
use coded_diagnostic::{Arg, Report};

use crate::__MutantEvent as MutantEvent;
use crate::failure::Failure;
use crate::mutant::{
    DebuggerFailed, ExecutableMutant, KilledBy, Observation, Outcome, PlannedMutant, ProgramRun,
    ProgramRunReported, ProgramRunResult, Ran, RunId, Unit,
};
use crate::why;
use crate::{Mutant, MutantDecided, MutantPlanned, ProgramRan, Verdict, mutant_root, observation};

fn harness() -> Harness<Mutant> {
    let mut registry = Registry::new();
    registry.aggregate::<Mutant>(Mutant::default).unwrap();
    let cover = Cover {
        domain: crate::MUTANT_DOMAIN.to_string(),
        root: Some(mutant_root("m").unwrap()),
        edition: None,
    };
    Harness::new(registry, cover)
}

fn run() -> RunId {
    RunId::from(Uuid::from_bytes([7; 16]))
}

fn planned(programs: u32) -> MutantEvent {
    MutantEvent::MutantPlanned(MutantPlanned {
        run: Some(run()),
        mutant: Some(ExecutableMutant {
            mutant: Some(PlannedMutant {
                id: "m".to_string(),
                ..PlannedMutant::default()
            }),
            plan: Some(crate::mutant::MutantPlan::default()),
        }),
        programs,
    })
}

fn exited(code: i32, wall_ms: u64) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: CoreTier::P,
        wall: Duration::from_millis(wall_ms),
    }
}

fn observed(raw: &RawOutcome) -> Observation {
    observation::encode(raw)
}

/// Program `program` ran the mutant and observed `raw`, in `took_ms`, with
/// `tests` selected.
fn ran(program: u32, raw: &RawOutcome, took_ms: u64, tests: &[&str]) -> ProgramRun {
    ProgramRun {
        program,
        program_name: format!("prog-{program}"),
        took_ms,
        tests: tests.iter().map(ToString::to_string).collect(),
        result: ProgramRunResult::Ran(Ran {
            observation: Some(observed(raw)),
            killed_by: Vec::new(),
        }),
    }
}

fn not_called(program: u32) -> ProgramRun {
    ProgramRun {
        program,
        program_name: String::new(),
        took_ms: 0,
        tests: Vec::new(),
        result: ProgramRunResult::NotCalled(Unit {}),
    }
}

fn killer() -> KilledBy {
    KilledBy {
        test: "calc::test_total".to_string(),
        location: Some("tests/calc.rs:5:5".to_string()),
        message: "assertion failed".to_string(),
        got: Some("-1".to_string()),
        expected: Some("5".to_string()),
    }
}

/// `report` as the fact that tells it to the mutant, under `external_id`.
fn fact_book(harness: &Harness<Mutant>, report: ProgramRun, external_id: &str) -> CommandBook {
    let fact = ProgramRunReported {
        run: Some(run()),
        ran: Some(report),
    };
    CommandBook {
        cover: Some(harness.cover().clone()),
        page: Some(CommandPage {
            body: Some(command_page::Body::Fact(Fact {
                payload: Some(pack_message(&fact)),
                origin: Some(FactOrigin {
                    external_id: external_id.to_string(),
                    description: "test".to_string(),
                }),
            })),
            ..CommandPage::default()
        }),
    }
}

/// `report` told to the mutant, under its program's id: what it recorded.
fn told(harness: &mut Harness<Mutant>, report: ProgramRun) -> Vec<MutantEvent> {
    let book = fact_book(
        harness,
        report.clone(),
        &format!("program {}", report.program),
    );
    harness.when_book(book).events()
}

/// The decision among `events`, if there is one.
fn decision(events: &[MutantEvent]) -> Option<&MutantDecided> {
    events.iter().find_map(|event| match event {
        MutantEvent::MutantDecided(decided) => Some(decided),
        _ => None,
    })
}

fn outcome(events: &[MutantEvent]) -> Outcome {
    decision(events)
        .and_then(|decided| decided.verdict.clone())
        .unwrap_or_else(|| panic!("decided: {events:?}"))
}

#[test]
fn test_a_run_that_fails_a_test_kills_the_mutant_with_its_killers_and_decides_at_once() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let mut report = ran(0, &exited(101, 31), 31, &["calc::test_total"]);
    if let ProgramRunResult::Ran(ran) = &mut report.result {
        ran.killed_by = vec![killer()];
    }
    let events = told(&mut harness, report);
    // Two programs, one run: the second is not waited for.
    assert!(matches!(events[0], MutantEvent::ProgramRan(_)));
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::Killed);
    assert_eq!(decided.tier, Some(crate::Tier::P));
    assert_eq!(decided.took_ms, 31);
    assert_eq!(decided.tests_run, ["calc::test_total"]);
    assert_eq!(decided.killed_by, [killer()]);
    assert_eq!(decided.reason, None);
    assert_eq!(decision(&events).unwrap().programs, ["prog-0"]);
}

#[test]
fn test_a_mutant_every_program_passes_survives_with_the_time_of_them_all() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let first = told(&mut harness, ran(0, &exited(0, 10), 10, &[]));
    assert_eq!(
        first.len(),
        1,
        "one program is not every program: {first:?}"
    );
    let second = told(&mut harness, ran(1, &exited(0, 20), 20, &["it::test_b"]));
    let decided = outcome(&second);
    assert_eq!(decided.verdict, Verdict::Survived);
    assert_eq!(decided.took_ms, 30);
    assert_eq!(decided.tests_run, ["it::test_b"]);
    assert_eq!(decided.killed_by, Vec::<KilledBy>::new());
    assert_eq!(decision(&second).unwrap().programs, ["prog-0", "prog-1"]);
}

#[test]
fn test_a_run_that_hangs_times_the_mutant_out_at_once() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let hung = RawOutcome::TimedOut {
        tier: CoreTier::B,
        wall: Duration::from_secs(2),
    };
    let events = told(&mut harness, ran(0, &hung, 2_000, &[]));
    let decided = outcome(&events);
    assert_eq!(
        (decided.verdict, decided.tier),
        (Verdict::Timeout, Some(crate::Tier::B))
    );
}

#[test]
fn test_a_mutant_no_program_calls_is_not_called_when_the_last_says_so() {
    let mut harness = harness();
    harness.given([planned(2)]);
    assert_eq!(told(&mut harness, not_called(0)).len(), 1);
    let events = told(&mut harness, not_called(1));
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::NotCalled);
    assert_eq!(
        (decided.tier, decided.reason, decided.why),
        (None, None, None)
    );
}

/// The reason an outcome records, read back as a report.
fn said(outcome: &Outcome) -> Option<Report> {
    outcome.why.as_ref().map(|why| why::decode(Some(why), ""))
}

/// What the run records of a debugger that failed for `cause`.
fn debugger_failed(cause: Report) -> Report {
    Report::new("cherenkov-run-domain-debugger-failed").arg("cause", Arg::nested(&cause))
}

#[test]
fn test_a_mutant_whose_code_no_run_reached_is_not_reached_with_the_reason() {
    let mut harness = harness();
    harness.given([planned(1)]);
    let missed = RawOutcome::NotReached {
        why: Report::new("cherenkov-exec-dap-never-ran"),
    };
    let events = told(&mut harness, ran(0, &missed, 3, &[]));
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::NotReached);
    assert_eq!(
        said(&decided),
        Some(Report::new("cherenkov-exec-dap-never-ran"))
    );
    assert_eq!(decided.reason, None);
}

#[test]
fn test_a_mutant_an_executor_cannot_apply_is_unsupported_at_once_with_the_reason() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let refused = RawOutcome::Unsupported {
        why: Report::new("cherenkov-exec-dap-inlined"),
    };
    let events = told(&mut harness, ran(0, &refused, 1, &[]));
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::Unsupported);
    assert_eq!(
        said(&decided),
        Some(Report::new("cherenkov-exec-dap-inlined"))
    );
}

#[test]
fn test_a_mutant_that_would_not_compile_is_unviable_at_once_with_the_reason() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let refused = RawOutcome::Unviable {
        why: Report::new("cherenkov-exec-dap-no-default").arg("name", "Settle"),
    };
    let events = told(&mut harness, ran(0, &refused, 1, &[]));
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::Unviable);
    assert_eq!(
        said(&decided),
        Some(Report::new("cherenkov-exec-dap-no-default").arg("name", "Settle"))
    );
}

#[test]
fn test_a_debugger_that_fails_on_a_mutant_makes_it_unsupported_saying_what_failed() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let events = told(
        &mut harness,
        ProgramRun {
            result: ProgramRunResult::Failed(DebuggerFailed {
                why: None,
                reason: "evaluate: dap connection closed".to_string(),
            }),
            ..ran(0, &exited(0, 1), 5, &["calc::test_total"])
        },
    );
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::Unsupported);
    assert_eq!(
        said(&decided),
        Some(debugger_failed(Report::of(&Failure::Recorded {
            reason: "evaluate: dap connection closed".to_string()
        })))
    );
    assert_eq!((decided.tier, decided.took_ms), (None, 5));
}

#[test]
fn test_a_coded_debugger_failure_is_carried_whole_in_the_reason() {
    let mut harness = harness();
    harness.given([planned(2)]);
    let events = told(
        &mut harness,
        ProgramRun {
            result: ProgramRunResult::Failed(DebuggerFailed {
                reason: String::new(),
                why: Some(crate::why::encode(
                    &coded_diagnostic::Report::new("cherenkov-exec-dap-rpc")
                        .arg("detail", "closed"),
                )),
            }),
            ..ran(0, &exited(0, 1), 5, &["calc::test_total"])
        },
    );
    let decided = outcome(&events);
    assert_eq!(decided.verdict, Verdict::Unsupported);
    assert_eq!(
        said(&decided),
        Some(debugger_failed(
            Report::new("cherenkov-exec-dap-rpc").arg("detail", "closed")
        ))
    );
}

#[test]
fn test_a_mutant_is_decided_once_and_a_run_told_after_records_nothing() {
    let mut harness = harness();
    harness.given([planned(2)]);
    told(&mut harness, ran(0, &exited(101, 1), 1, &[]));
    let after = told(&mut harness, ran(1, &exited(0, 1), 1, &[]));
    assert!(after.is_empty(), "{after:?}");
}

#[test]
fn test_a_program_told_twice_under_two_ids_counts_once() {
    let mut harness = harness();
    harness.given([planned(2)]);
    told(&mut harness, ran(0, &exited(0, 1), 1, &[]));
    let again = ran(0, &exited(0, 99), 99, &[]);
    let book = fact_book(&harness, again, "again");
    let events = harness.when_book(book).events();
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn test_programs_told_out_of_order_decide_as_if_in_order() {
    // The first to detect, in the order of the programs, decides.
    let mut harness = harness();
    harness.given([planned(2)]);
    let late = told(&mut harness, ran(1, &exited(101, 1), 1, &[]));
    assert_eq!(outcome(&late).verdict, Verdict::Killed);
}

#[test]
fn test_a_run_with_no_observation_is_a_bug_of_whoever_told_it() {
    let mut harness = harness();
    harness.given([planned(1)]);
    let silent = ProgramRun {
        result: ProgramRunResult::Ran(Ran::default()),
        ..ran(0, &exited(0, 1), 1, &[])
    };
    let book = fact_book(&harness, silent, "silent");
    assert!(harness.when_book(book).handled().is_err());
    assert_eq!(harness.pages().len(), 1, "nothing recorded");
}

#[test]
fn test_what_the_programs_made_of_a_mutant_is_recorded_beside_its_decision() {
    let mut harness = harness();
    harness.given([planned(1)]);
    let report = ran(0, &exited(0, 4), 4, &[]);
    let events = told(&mut harness, report.clone());
    let [
        MutantEvent::ProgramRan(recorded),
        MutantEvent::MutantDecided(_),
    ] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(
        recorded,
        &ProgramRan {
            run: Some(run()),
            ran: Some(report)
        }
    );
}
