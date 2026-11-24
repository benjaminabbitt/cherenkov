use std::time::Duration;

use cherenkov_mutation::execution::{
    Budget, Failure, Observed, Outcome, RawOutcome, Tier, TestReport, TestResult, classify, combine,
};
use cherenkov_mutation::verdict::Verdict;

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(40),
    }
}

#[test]
fn test_classify_passing_tests_is_survived() {
    assert_eq!(
        classify(&exited(0)),
        Outcome {
            verdict: Verdict::Survived,
            tier: Some(Tier::P),
            detail: None
        }
    );
}

#[test]
fn test_classify_failing_or_crashing_tests_is_killed_with_exit_code() {
    for code in [1, 2, -11] {
        assert_eq!(
            classify(&exited(code)),
            Outcome {
                verdict: Verdict::Killed,
                tier: Some(Tier::P),
                detail: Some(format!("exit code {code}"))
            }
        );
    }
}

#[test]
fn test_classify_timed_out_is_timeout() {
    let raw = RawOutcome::TimedOut {
        tier: Tier::P,
        wall: Duration::from_secs(9),
    };
    assert_eq!(classify(&raw).verdict, Verdict::Timeout);
    assert_eq!(classify(&raw).tier, Some(Tier::P));
}

#[test]
fn test_classify_unsupported_keeps_the_reason_and_has_no_tier() {
    let raw = RawOutcome::Unsupported {
        reason: "no conditional jump on line 12".into(),
    };
    assert_eq!(
        classify(&raw),
        Outcome {
            verdict: Verdict::UnsupportedByExecutor,
            tier: None,
            detail: Some("no conditional jump on line 12".into()),
        }
    );
}

#[test]
fn test_budget_is_ten_times_baseline_plus_five_seconds() {
    assert_eq!(
        Budget::from_baseline(Duration::from_millis(200)).timeout,
        Duration::from_secs(7)
    );
    assert_eq!(
        Budget::from_baseline(Duration::ZERO).timeout,
        Duration::from_secs(5)
    );
}

#[test]
fn test_a_mutant_s_budget_is_a_small_multiple_of_the_baseline_with_a_floor() {
    assert_eq!(
        Budget::for_mutant(Duration::from_millis(40)).timeout,
        Duration::from_secs(2),
        "the floor covers the debugger's own overhead"
    );
    assert_eq!(
        Budget::for_mutant(Duration::from_secs(3)).timeout,
        Duration::from_secs(15)
    );
}

#[test]
fn test_a_run_that_evaluates_at_every_hit_gets_three_times_the_budget() {
    let budget = Budget::for_mutant(Duration::from_millis(40));
    assert_eq!(budget.evaluating().timeout, Duration::from_secs(6));
}

#[test]
fn test_classify_crash_caught_by_the_debugger_is_killed_with_its_message() {
    let raw = RawOutcome::Crashed {
        kind: "panic".into(),
        message: "runtime error: index out of range [2] with length 2".into(),
        tier: Tier::P,
        wall: Duration::from_millis(30),
    };
    assert_eq!(
        classify(&raw),
        Outcome {
            verdict: Verdict::Killed,
            tier: Some(Tier::P),
            detail: Some("panic: runtime error: index out of range [2] with length 2".into()),
        }
    );
}

// --- Outcomes across several test binaries (Rust: one per test target).

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn exit(code: i32, wall: u64) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: ms(wall),
    }
}

fn not_reached() -> RawOutcome {
    RawOutcome::NotReached {
        reason: "not in this binary".into(),
    }
}

#[test]
fn test_classify_not_reached_keeps_the_reason() {
    assert_eq!(
        classify(&not_reached()),
        Outcome {
            verdict: Verdict::NotReached,
            tier: None,
            detail: Some("not in this binary".into())
        }
    );
}

#[test]
fn test_combine_first_detection_wins() {
    let crashed = RawOutcome::Crashed {
        kind: "signal".into(),
        message: "SIGABRT".into(),
        tier: Tier::P,
        wall: ms(5),
    };
    assert_eq!(
        combine(vec![exit(0, 10), exit(101, 20), crashed.clone()]),
        exit(101, 20)
    );
    assert_eq!(
        combine(vec![not_reached(), crashed.clone(), exit(101, 1)]),
        crashed
    );
    let timeout = RawOutcome::TimedOut {
        tier: Tier::P,
        wall: ms(9),
    };
    assert_eq!(combine(vec![exit(0, 1), timeout.clone()]), timeout);
}

#[test]
fn test_combine_all_passing_is_one_pass_with_total_wall_time() {
    assert_eq!(
        combine(vec![exit(0, 10), not_reached(), exit(0, 15)]),
        exit(0, 25)
    );
}

#[test]
fn test_combine_nothing_reached_is_not_reached() {
    assert_eq!(combine(vec![not_reached(), not_reached()]), not_reached());
    assert_eq!(
        combine(vec![]),
        RawOutcome::NotReached {
            reason: "no test binary".into()
        }
    );
}

#[test]
fn test_combine_unsupported_anywhere_without_a_detection_is_unsupported() {
    let unsupported = RawOutcome::Unsupported {
        reason: "no tier P rule".into(),
    };
    assert_eq!(combine(vec![exit(0, 1), unsupported.clone()]), unsupported);
    assert_eq!(combine(vec![unsupported.clone(), exit(1, 1)]), exit(1, 1));
}

#[test]
fn test_classify_unviable_keeps_the_reason_has_no_tier_and_is_not_scored() {
    // The mutated source would not compile (cargo-mutants' "unviable"):
    // no test can say anything about it.
    let o = classify(&RawOutcome::Unviable {
        reason: "Remote has no Default".into(),
    });
    assert_eq!(o.verdict, Verdict::Unviable);
    assert_eq!(o.tier, None);
    assert_eq!(o.detail.as_deref(), Some("Remote has no Default"));
    assert!(!o.verdict.counts_in_score() && !o.verdict.detected());
}

#[test]
fn test_combine_unviable_anywhere_without_a_detection_is_unviable() {
    let got = combine(vec![
        RawOutcome::NotReached {
            reason: "elsewhere".into(),
        },
        RawOutcome::Unviable {
            reason: "no Default".into(),
        },
    ]);
    assert!(matches!(got, RawOutcome::Unviable { .. }), "{got:?}");
}

fn failed(test: &str, location: Option<&str>, message: &str) -> TestReport {
    TestReport {
        test: test.into(),
        result: TestResult::Failed(Failure {
            location: location.map(Into::into),
            message: message.into(),
            got: None,
            expected: None,
        }),
    }
}

#[test]
fn test_bare_observation_reports_no_tests() {
    let observed = Observed::bare(exited(1));
    assert_eq!(observed.outcome, exited(1));
    assert!(observed.tests.is_empty());
    assert!(!observed.failed());
}

#[test]
fn test_observation_with_a_failed_test_is_failed_and_one_that_only_passed_is_not() {
    let passed = TestReport {
        test: "a".into(),
        result: TestResult::Passed,
    };
    let ignored = TestReport {
        test: "b".into(),
        result: TestResult::Ignored,
    };
    let only_passes = Observed {
        outcome: exited(0),
        tests: vec![passed.clone(), ignored],
    };
    assert!(!only_passes.failed());
    let mixed = Observed {
        outcome: exited(1),
        tests: vec![passed, failed("c", None, "boom")],
    };
    assert!(mixed.failed());
}

#[test]
fn test_failing_reports_are_only_the_failed_ones() {
    let observed = Observed {
        outcome: exited(1),
        tests: vec![
            TestReport {
                test: "a".into(),
                result: TestResult::Passed,
            },
            failed("c", None, "boom"),
        ],
    };
    assert_eq!(observed.failures(), vec![failed("c", None, "boom")]);
}

#[test]
fn test_report_line_names_the_place_and_the_message() {
    assert_eq!(
        failed("TestX", Some("x_test.go:6"), "Total").line(),
        "TestX at x_test.go:6: Total"
    );
    assert_eq!(failed("TestX", None, "Total").line(), "TestX: Total");
}

#[test]
fn test_report_line_of_an_equality_failure_gives_both_values() {
    let report = TestReport {
        test: "t".into(),
        result: TestResult::Failed(Failure {
            location: Some("a.rs:1:2".into()),
            message: "assertion".into(),
            got: Some("-1".into()),
            expected: Some("5".into()),
        }),
    };
    assert_eq!(report.line(), "t at a.rs:1:2: got -1, expected 5");
}

#[test]
fn test_report_line_of_a_test_that_did_not_fail_is_its_name_and_result() {
    let report = TestReport {
        test: "t".into(),
        result: TestResult::Passed,
    };
    assert_eq!(report.line(), "t: passed");
}
