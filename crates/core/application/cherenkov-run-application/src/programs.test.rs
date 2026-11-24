use std::time::Duration;

use cherenkov_mutation::execution::{RawOutcome, Tier};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Driven as Chosen, Recording, Selection as Picked};

use super::*;
use crate::test_plans::plans;
use cherenkov_run_domain::observation;

fn preparation(recording: Option<Recording>) -> Preparation {
    Preparation {
        program: 1,
        describe: Some("target/debug/deps/calc-0123".to_string()),
        baseline: RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: Duration::from_millis(400),
        },
        baseline_failures: Vec::new(),
        took: Duration::from_millis(900),
        recording,
    }
}

fn recording(plans: &[MutantPlan]) -> Recording {
    Recording {
        tests: Some(4),
        selection: Picked::RecordedWithFailures {
            failed_tests: vec!["flaky".to_string()],
        },
        driven: Some(vec![
            Chosen::Tests(vec!["test_a".to_string()]),
            Chosen::NotCalled,
        ]),
    }
    .tap(|r| assert_eq!(r.driven.as_ref().map(Vec::len), Some(plans.len())))
}

trait Tap: Sized {
    fn tap(self, check: impl FnOnce(&Self)) -> Self {
        check(&self);
        self
    }
}

impl Tap for Recording {}

#[test]
fn test_a_preparation_is_reported_with_its_baseline_as_observed_and_what_recording_found() {
    let plans = plans();
    let fact = report(preparation(Some(recording(&plans))), &plans);
    assert_eq!(fact.index, 1);
    assert_eq!(fact.program, "target/debug/deps/calc-0123");
    assert_eq!(fact.prepared_ms, 900);
    assert_eq!(fact.tests, Some(4));
    assert_eq!(fact.selection, Selection::RecordedWithFailures);
    assert_eq!(fact.failed_tests, ["flaky"]);
    assert_eq!(
        observation::decode(fact.baseline.unwrap()),
        Ok(RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: Duration::from_millis(400)
        })
    );
    let drives = fact.drives.unwrap().drives;
    assert_eq!(
        drives.iter().map(|d| d.mutant.as_str()).collect::<Vec<_>>(),
        [plans[0].id.0.as_str(), plans[1].id.0.as_str()]
    );
}

#[test]
fn test_a_preparation_whose_baseline_failed_is_reported_without_a_recording() {
    let failing = Preparation {
        baseline: RawOutcome::Exited {
            code: 101,
            tier: Tier::P,
            wall: Duration::from_millis(40),
        },
        baseline_failures: vec!["test_a at tests/calc.rs:5:5: boom".to_string()],
        ..preparation(None)
    };
    let fact = report(failing, &plans());
    assert_eq!(
        fact.baseline_failures,
        ["test_a at tests/calc.rs:5:5: boom"]
    );
    assert_eq!((fact.tests, fact.drives), (None, None));
}

#[test]
fn test_the_programs_a_runner_reads_come_back_in_index_order_with_their_budgets_and_drives() {
    let plans = plans();
    let prepared =
        |index: u32, budget_ms: u64, name: &str, drives: Option<Drives>| ProgramPrepared {
            program: name.to_string(),
            baseline_ms: 0,
            prepared_ms: 0,
            tests: None,
            selection: Selection::Recorded,
            failed_tests: Vec::new(),
            selection_reason: None,
            selection_why: None,
            index,
            budget_ms,
            drives,
        };
    let fact = report(preparation(Some(recording(&plans))), &plans);
    let programs = programs(vec![
        prepared(1, 7_500, "it-2", fact.drives),
        prepared(0, 2_000, "", None),
    ]);
    assert_eq!(programs.budgets_ms().collect::<Vec<_>>(), [2_000, 7_500]);
    assert_eq!(programs.name(0), None, "an unnamed program");
    assert_eq!(programs.name(1), Some("it-2"));
    assert_eq!(
        programs.driven(1, &plans[0].id.0),
        Some(Chosen::Tests(vec!["test_a".to_string()]))
    );
    assert_eq!(programs.driven(1, &plans[1].id.0), Some(Chosen::NotCalled));
    assert_eq!(
        programs.driven(0, &plans[0].id.0),
        None,
        "a program that cannot tell"
    );
}
