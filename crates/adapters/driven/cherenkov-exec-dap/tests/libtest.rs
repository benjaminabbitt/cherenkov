use std::collections::BTreeMap;

use cherenkov_exec_dap::libtest::{TestOutcome, parse};

fn map(rows: &[(&str, TestOutcome)]) -> BTreeMap<String, TestOutcome> {
    rows.iter().map(|(n, o)| (n.to_string(), *o)).collect()
}

#[test]
fn test_parse_reads_each_test_result_line() {
    let out = "\nrunning 4 tests\ntest test_count_up ... ok\ntest a::b::test_x ... FAILED\ntest slow ... ignored\ntest test_total ... ok\n\nfailures:\n\n---- a::b::test_x stdout ----\nthread panicked\n\nfailures:\n    a::b::test_x\n\ntest result: FAILED. 2 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out\n";
    assert_eq!(
        parse(out),
        map(&[
            ("test_count_up", TestOutcome::Ok),
            ("a::b::test_x", TestOutcome::Failed),
            ("slow", TestOutcome::Ignored),
            ("test_total", TestOutcome::Ok),
        ])
    );
}

#[test]
fn test_parse_reads_ignored_with_a_reason() {
    assert_eq!(
        parse("test live ... ignored, slow: needs gdb\n"),
        map(&[("live", TestOutcome::Ignored)])
    );
}

#[test]
fn test_parse_ignores_other_lines_and_partial_runs() {
    // The process died after starting the second test: only the first counts.
    let out = "running 2 tests\ntest first ... ok\ntest second ... \nthread 'second' panicked at src/lib.rs:3:5\n";
    assert_eq!(parse(out), map(&[("first", TestOutcome::Ok)]));
}

#[test]
fn test_parse_handles_output_split_across_chunks_joined_without_newlines() {
    // Output events may split a line; callers join them before parsing.
    let joined = ["test fir", "st ... o", "k\n"].concat();
    assert_eq!(parse(&joined), map(&[("first", TestOutcome::Ok)]));
}

#[test]
fn test_parse_reads_bench_lines_as_nothing() {
    assert!(parse("test bench_x ... bench:       1,234 ns/iter (+/- 5)\n").is_empty());
}

// --- Failures: what each failing test saw.

use cherenkov_exec_dap::libtest::{Failure, failures};

const FAILED_RUN: &str = "\nrunning 3 tests\ntest test_total ... FAILED\ntest test_is_small ... FAILED\ntest test_first ... ok\n\nfailures:\n\n---- test_total stdout ----\n\nthread 'test_total' panicked at tests/calc.rs:5:5:\nassertion `left == right` failed\n  left: 6\n right: 5\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n---- test_is_small stdout ----\n\nthread 'test_is_small' panicked at tests/calc.rs:10:5:\nassertion failed: is_small(3)\n\n\nfailures:\n    test_is_small\n    test_total\n\ntest result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out\n";

#[test]
fn test_failures_give_each_failing_tests_location_and_message() {
    let got = failures(FAILED_RUN);
    assert_eq!(got.len(), 2);
    assert_eq!(
        got["test_is_small"],
        Failure {
            location: Some("tests/calc.rs:10:5".to_string()),
            message: "assertion failed: is_small(3)".to_string(),
            left: None,
            right: None,
        }
    );
}

#[test]
fn test_an_equality_assertion_gives_the_value_seen_and_the_value_expected() {
    // Under a mutant, `left` is what the mutated code produced and `right`
    // what the test expected (assert_eq!(actual, expected) by convention).
    let f = &failures(FAILED_RUN)["test_total"];
    assert_eq!(f.location.as_deref(), Some("tests/calc.rs:5:5"));
    assert_eq!(f.message, "assertion `left == right` failed");
    assert_eq!(f.left.as_deref(), Some("6"));
    assert_eq!(f.right.as_deref(), Some("5"));
}

#[test]
fn test_multi_line_values_are_kept_whole() {
    let out = "---- t stdout ----\n\nthread 't' panicked at src/lib.rs:9:9:\nassertion `left == right` failed: totals differ\n  left: [\n    1,\n    2,\n]\n right: [\n    1,\n]\n\nfailures:\n    t\n";
    let f = &failures(out)["t"];
    assert_eq!(f.message, "assertion `left == right` failed: totals differ");
    assert_eq!(f.left.as_deref(), Some("[\n    1,\n    2,\n]"));
    assert_eq!(f.right.as_deref(), Some("[\n    1,\n]"));
}

#[test]
fn test_no_failures_section_means_no_failures() {
    assert!(failures("test a ... ok\n\ntest result: ok. 1 passed\n").is_empty());
}

#[test]
fn test_panic_line_with_a_thread_id_is_read() {
    // The format of current toolchains (captured from a run under GDB).
    let out = "---- test_total stdout ----\n\nthread 'test_total' (1453796) panicked at tests/calc.rs:5:5:\nassertion `left == right` failed\n  left: -1\n right: 5\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    test_total\n";
    let f = &failures(out)["test_total"];
    assert_eq!(f.location.as_deref(), Some("tests/calc.rs:5:5"));
    assert_eq!(
        (f.left.as_deref(), f.right.as_deref()),
        (Some("-1"), Some("5"))
    );
}

#[test]
fn test_a_test_list_names_the_tests_and_not_the_benches_or_summary() {
    use cherenkov_exec_dap::libtest::listed;
    let out = "address::tests::test_a: test\nlock::tests::test_b: test\nbench_x: bench\n\n2 tests, 1 benchmark\n";
    assert_eq!(
        listed(out),
        ["address::tests::test_a", "lock::tests::test_b"]
    );
}

// --- Reports: each test's result, with what a failing one saw.

use cherenkov_exec_dap::libtest::reports;
use cherenkov_mutation::execution::{Failure as Seen, TestResult};

#[test]
fn test_reports_give_each_test_its_result_and_a_failing_one_what_it_saw() {
    let got = reports(FAILED_RUN);
    let names: Vec<&str> = got.iter().map(|r| r.test.as_str()).collect();
    assert_eq!(names, ["test_first", "test_is_small", "test_total"]);
    assert_eq!(got[0].result, TestResult::Passed);
    assert_eq!(
        got[2].result,
        TestResult::Failed(Seen {
            location: Some("tests/calc.rs:5:5".to_string()),
            message: "assertion `left == right` failed".to_string(),
            got: Some("6".to_string()),
            expected: Some("5".to_string()),
        })
    );
}

#[test]
fn test_reports_name_an_ignored_test_ignored() {
    let got = reports("test slow ... ignored, takes long\n");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].result, TestResult::Ignored);
}

#[test]
fn test_reports_name_a_failed_test_whose_section_is_missing_with_no_place() {
    let got = reports("test t ... FAILED\n");
    assert_eq!(got.len(), 1);
    let TestResult::Failed(f) = &got[0].result else {
        panic!("{got:?}");
    };
    assert_eq!((f.location.as_deref(), f.got.as_deref()), (None, None));
}

#[test]
fn test_reports_name_a_failing_section_whose_result_line_was_lost() {
    let out = "---- t stdout ----\nthread 't' panicked at a.rs:1:1:\nboom\n";
    let got = reports(out);
    assert_eq!(got.len(), 1);
    assert!(matches!(got[0].result, TestResult::Failed(_)), "{got:?}");
}

#[test]
fn test_reports_of_no_output_are_none() {
    assert!(reports("").is_empty());
}
