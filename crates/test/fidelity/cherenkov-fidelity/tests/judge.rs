//! Judging a run-time mutant against its compiled source mutant: they agree
//! when every test both runs reports the same outcome, or when both runs end
//! the same abnormal way (timeout, crash) before tests can report.

use std::collections::BTreeMap;

use cherenkov_exec_dap::libtest::TestOutcome::{self, Failed, Ok as Pass};
use cherenkov_fidelity::judge::{Disagreement, Judgement, Run, judge};

fn tests(rows: &[(&str, TestOutcome)]) -> Run {
    Run::Tests(
        rows.iter()
            .map(|(n, o)| (n.to_string(), *o))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn test_same_outcome_for_every_test_is_faithful() {
    let a = tests(&[("t1", Failed), ("t2", Pass)]);
    assert_eq!(judge(&a, &a.clone()), Judgement::Faithful);
}

#[test]
fn test_a_test_that_differs_is_reported() {
    let debugger = tests(&[("t1", Failed), ("t2", Pass)]);
    let source = tests(&[("t1", Pass), ("t2", Pass)]);
    assert_eq!(
        judge(&debugger, &source),
        Judgement::Unfaithful(vec![Disagreement::Test {
            test: "t1".into(),
            debugger: Failed,
            source: Pass,
        }])
    );
}

#[test]
fn test_a_test_only_one_run_reports_is_a_disagreement() {
    let debugger = tests(&[("t1", Pass)]);
    let source = tests(&[("t1", Pass), ("t2", Failed)]);
    assert_eq!(
        judge(&debugger, &source),
        Judgement::Unfaithful(vec![Disagreement::Missing {
            test: "t2".into(),
            from_debugger: true,
        }])
    );
}

#[test]
fn test_both_timing_out_agree_and_one_timing_out_does_not() {
    assert_eq!(judge(&Run::TimedOut, &Run::TimedOut), Judgement::Faithful);
    assert_eq!(
        judge(&Run::TimedOut, &tests(&[("t", Pass)])),
        Judgement::Unfaithful(vec![Disagreement::Ending {
            debugger: "timed out".into(),
            source: "tests reported".into(),
        }])
    );
}

#[test]
fn test_a_source_mutant_that_does_not_compile_is_unviable() {
    assert_eq!(
        judge(&tests(&[("t", Pass)]), &Run::DoesNotCompile),
        Judgement::Unviable
    );
}

#[test]
fn test_unviable_under_the_debugger_is_faithful_only_if_the_source_does_not_compile() {
    assert_eq!(
        judge(&Run::Unviable, &Run::DoesNotCompile),
        Judgement::Unviable
    );
    assert!(matches!(
        judge(&Run::Unviable, &Run::Tests(Default::default())),
        Judgement::Unfaithful(_)
    ));
}
