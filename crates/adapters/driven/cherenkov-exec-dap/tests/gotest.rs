use cherenkov_exec_dap::gotest::reports;
use cherenkov_mutation::execution::{Failure, TestReport, TestResult};

fn failed(test: &str, location: Option<&str>, message: &str) -> TestReport {
    TestReport {
        test: test.to_string(),
        result: TestResult::Failed(Failure {
            location: location.map(str::to_string),
            message: message.to_string(),
            got: None,
            expected: None,
        }),
    }
}

#[test]
fn test_a_failed_test_is_named_with_the_place_and_message_of_its_first_report() {
    let out = "--- FAIL: TestTotal (0.00s)\n    calc_test.go:6: Total\nFAIL\n";
    assert_eq!(
        reports(out),
        vec![failed("TestTotal", Some("calc_test.go:6"), "Total")]
    );
}

#[test]
fn test_a_passing_run_reports_no_test() {
    assert!(reports("PASS\n").is_empty());
    assert!(reports("").is_empty());
}

#[test]
fn test_each_failed_test_of_a_run_is_named() {
    let out = "--- FAIL: TestA (0.00s)\n    a_test.go:3: first\n    a_test.go:4: second\n--- FAIL: TestB (0.01s)\n    b_test.go:9: other\nFAIL\n";
    assert_eq!(
        reports(out),
        vec![
            failed("TestA", Some("a_test.go:3"), "first"),
            failed("TestB", Some("b_test.go:9"), "other"),
        ]
    );
}

#[test]
fn test_a_failed_subtest_is_named_with_its_own_report_and_its_parent_with_none() {
    let out = "--- FAIL: TestA (0.00s)\n    --- FAIL: TestA/sub (0.00s)\n        a_test.go:9: bad\nFAIL\n";
    assert_eq!(
        reports(out),
        vec![
            failed("TestA", None, "the test failed"),
            failed("TestA/sub", Some("a_test.go:9"), "bad"),
        ]
    );
}

#[test]
fn test_a_multi_line_message_is_kept_whole() {
    let out = "--- FAIL: TestA (0.00s)\n    a_test.go:3: got\n        more\n    a_test.go:4: later\nFAIL\n";
    assert_eq!(
        reports(out),
        vec![failed("TestA", Some("a_test.go:3"), "got\nmore")]
    );
}

#[test]
fn test_a_failed_test_with_no_report_line_has_no_place() {
    let out = "--- FAIL: TestPanics (0.00s)\npanic: boom\n\ngoroutine 7 [running]:\n";
    assert_eq!(
        reports(out),
        vec![failed("TestPanics", None, "the test failed")]
    );
}

#[test]
fn test_verbose_passes_and_skips_are_reported_too() {
    let out = "--- PASS: TestA (0.00s)\n--- SKIP: TestB (0.00s)\n    b_test.go:2: why\n";
    assert_eq!(
        reports(out),
        vec![
            TestReport {
                test: "TestA".to_string(),
                result: TestResult::Passed
            },
            TestReport {
                test: "TestB".to_string(),
                result: TestResult::Ignored
            },
        ]
    );
}

#[test]
fn test_a_name_with_parentheses_is_read_up_to_the_duration_s_paren() {
    let out = "--- FAIL: TestA/a_(b) (0.00s)\n    a_test.go:3: x\n";
    assert_eq!(
        reports(out)[0].test,
        "TestA/a_(b)",
        "{:?}",
        reports(out)
    );
}

#[test]
fn test_a_log_line_with_a_colon_but_no_place_is_not_a_location() {
    let out = "--- FAIL: TestA (0.00s)\n    note: something\n    a_test.go:3: real\n";
    assert_eq!(
        reports(out),
        vec![failed("TestA", Some("a_test.go:3"), "real")]
    );
}
