use std::io::Cursor;

use coded_diagnostic::Diagnostic;

use super::*;

fn split(stdout: &str) -> (Vec<String>, Vec<String>) {
    let mut found = Vec::new();
    let mut printed = Vec::new();
    split_stdout(
        Cursor::new(stdout.as_bytes().to_vec()),
        |addr| found.push(addr),
        |line| printed.push(line.to_string()),
    );
    (found, printed)
}

#[test]
fn test_split_stdout_gives_the_address_and_what_follows_the_banner_as_the_programs() {
    let (found, printed) = split(
        "warning: delve's own\nAPI server listening at: 127.0.0.1:41000\n--- FAIL: TestX (0.00s)\n    x_test.go:6: boom\nFAIL\n",
    );
    assert_eq!(found, ["127.0.0.1:41000"]);
    assert_eq!(
        printed,
        [
            "--- FAIL: TestX (0.00s)\n",
            "    x_test.go:6: boom\n",
            "FAIL\n"
        ],
        "each line with its newline, none of delve's own before the banner"
    );
}

#[test]
fn test_split_stdout_without_a_banner_finds_no_address_and_no_output() {
    assert_eq!(split("nothing here\nPASS\n"), (vec![], vec![]));
}

#[test]
fn test_split_stdout_keeps_a_last_line_with_no_newline_and_non_utf8_bytes() {
    let mut printed = Vec::new();
    split_stdout(
        Cursor::new(b"API server listening at: a:1\nok \xff\ntail".to_vec()),
        |_| {},
        |line| printed.push(line.to_string()),
    );
    assert_eq!(printed, ["ok \u{fffd}\n", "tail"]);
}

#[test]
fn test_split_stdout_takes_a_second_banner_for_the_programs_output() {
    let (found, printed) = split("API server listening at: a:1\nAPI server listening at: b:2\n");
    assert_eq!(found, ["a:1"]);
    assert_eq!(printed, ["API server listening at: b:2\n"]);
}

#[test]
fn test_printed_returns_what_the_run_printed_once_its_verdict_is_in() {
    for last in ["PASS\n", "FAIL\n"] {
        let printed = Printed::default();
        printed.push("--- FAIL: TestX (0.00s)\n");
        printed.push(last);
        let text = printed.await_verdict(Duration::from_secs(60));
        assert_eq!(text, format!("--- FAIL: TestX (0.00s)\n{last}"));
    }
}

#[test]
fn test_printed_waits_for_the_verdict_and_keeps_every_line_before_it() {
    // However the threads interleave, a wait that ends on the verdict has
    // every line the writer pushed before it.
    let printed = Arc::new(Printed::default());
    let writer = Arc::clone(&printed);
    let late = thread::spawn(move || {
        writer.push("--- FAIL: TestX (0.00s)\n");
        writer.push("    x_test.go:6: boom\n");
        writer.push("FAIL\n");
    });
    let text = printed.await_verdict(Duration::from_secs(60));
    late.join().unwrap();
    assert_eq!(
        text,
        "--- FAIL: TestX (0.00s)\n    x_test.go:6: boom\nFAIL\n"
    );
}

#[test]
fn test_printed_with_no_verdict_gives_what_it_has_when_the_bound_is_spent() {
    // A bound of nothing is spent at once: what was printed is returned
    // whole, never cut.
    let printed = Printed::default();
    printed.push("os.Exit says goodbye\n");
    assert_eq!(
        printed.await_verdict(Duration::ZERO),
        "os.Exit says goodbye\n"
    );
}

#[test]
fn test_printed_of_a_stopped_run_gives_what_it_has_once_quiet() {
    let printed = Printed::default();
    printed.push("panic: boom\n");
    assert_eq!(
        printed.await_quiet(Duration::from_millis(5), Duration::from_secs(60)),
        "panic: boom\n"
    );
}

#[test]
fn test_quiet_left_counts_from_the_last_output_by_the_clock_not_by_wakeups() {
    let at = Instant::now();
    let quiet = Duration::from_millis(100);
    assert_eq!(quiet_left(at, at, quiet), quiet);
    assert_eq!(
        quiet_left(at, at + Duration::from_millis(30), quiet),
        Duration::from_millis(70)
    );
    assert_eq!(quiet_left(at, at + quiet, quiet), Duration::ZERO);
    assert_eq!(quiet_left(at, at + 2 * quiet, quiet), Duration::ZERO);
}

#[test]
fn test_printed_over_its_cap_keeps_the_head_and_the_tail_with_the_verdict() {
    let printed = Printed::capped(20, 20);
    printed.push("first test report\n");
    for i in 0..1000 {
        printed.push(&format!("noise {i}\n"));
    }
    printed.push("FAIL\n");
    let text = printed.await_verdict(Duration::from_secs(60));
    assert!(text.starts_with("first test report\n"), "{text}");
    assert!(text.ends_with("noise 999\nFAIL\n"), "{text}");
    assert!(text.contains(OMITTED), "says it left lines out: {text}");
    assert!(text.len() < 200, "bounded: {} bytes", text.len());
}

#[test]
fn test_printed_under_its_cap_keeps_everything_and_says_nothing_omitted() {
    let printed = Printed::capped(100, 100);
    printed.push("a\n");
    printed.push("PASS\n");
    assert_eq!(printed.await_verdict(Duration::from_secs(60)), "a\nPASS\n");
}

#[test]
fn test_printed_stops_recording_once_its_output_is_taken() {
    let printed = Printed::default();
    printed.push("PASS\n");
    assert_eq!(printed.await_verdict(Duration::from_secs(60)), "PASS\n");
    printed.push("a looping mutant's noise\n");
    assert_eq!(printed.await_verdict(Duration::ZERO), "");
}

#[test]
fn test_exit_zero_with_no_verdict_is_an_incomplete_output_error() {
    // A bound of nothing is spent at once: no clock decides.
    let printed = Printed::default();
    printed.push("=== RUN   TestX\n");
    let e = printed
        .settle_exit(0, Duration::ZERO)
        .expect_err("a clean exit with no verdict");
    assert_eq!(e.code(), "cherenkov-exec-dap-session-output-incomplete");
}

#[test]
fn test_exit_nonzero_with_no_verdict_leaves_the_output_to_be_read() {
    // os.Exit / log.Fatal: no verdict is ever printed, and the output names
    // whoever failed.
    let printed = Printed::default();
    printed.push("--- FAIL: TestX (0.00s)\n");
    printed.settle_exit(1, Duration::ZERO).unwrap();
    assert_eq!(
        printed.await_verdict(Duration::ZERO),
        "--- FAIL: TestX (0.00s)\n"
    );
}

#[test]
fn test_exit_zero_with_the_verdict_in_keeps_the_output_untaken() {
    let printed = Printed::default();
    printed.push("PASS\n");
    printed.settle_exit(0, Duration::ZERO).unwrap();
    assert_eq!(printed.await_verdict(Duration::ZERO), "PASS\n");
}
