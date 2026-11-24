//! The executor port's provided methods.

use std::time::Duration;

use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, Tier};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Driven, ExecError, MutantExecutor, Selection};
use coded_diagnostic::{Arg, Diagnostic, Report};

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(40),
    }
}

#[test]
fn test_an_exec_error_is_its_cause_code_and_args() {
    let cause = Report::new("x-failed").arg("detail", "dlv not found");
    let error = ExecError::of(&cause);
    assert_eq!(error.code(), "x-failed");
    assert_eq!(Report::of(&error), cause);
    assert_eq!(error.to_string(), "x-failed detail=\"dlv not found\"");
}

#[test]
fn test_an_exec_error_holds_its_cause_nested() {
    let inner = Report::new("x-rpc").arg("detail", "closed");
    let outer = Report::new("x-session").arg("cause", Arg::nested(&inner));
    assert_eq!(ExecError::of(&outer).cause, outer);
}

/// Says which tests call what through `driven` alone, as an executor
/// written before recordings were reported does.
struct DrivenOnly(Option<Vec<Driven>>);

impl MutantExecutor for DrivenOnly {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn driven(&mut self, _: &[MutantPlan], _: Budget) -> Result<Option<Vec<Driven>>, ExecError> {
        Ok(self.0.clone())
    }
}

#[test]
fn test_a_recording_is_what_driven_says_and_every_test_where_it_cannot_tell() {
    let budget = Budget::from_baseline(Duration::from_millis(10));
    let recorded = DrivenOnly(Some(vec![Driven::NotCalled]))
        .recording(&[], budget)
        .unwrap();
    assert_eq!(recorded.tests, None, "not counted");
    assert_eq!(recorded.selection, Selection::Recorded);
    assert_eq!(recorded.driven, Some(vec![Driven::NotCalled]));
    let blind = DrivenOnly(None).recording(&[], budget).unwrap();
    assert!(
        matches!(blind.selection, Selection::EveryTest { .. }),
        "{blind:?}"
    );
    assert_eq!(blind.driven, None);
}
