use std::io::Cursor;

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
fn test_printed_settles_at_once_when_the_run_has_printed_its_verdict() {
    for last in ["PASS\n", "FAIL\n"] {
        let printed = Printed::default();
        printed.push("--- FAIL: TestX (0.00s)\n");
        printed.push(last);
        let start = Instant::now();
        let text = printed.settled(Duration::from_secs(5));
        assert!(text.ends_with(last), "{text}");
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[test]
fn test_printed_without_a_verdict_settles_when_the_output_goes_quiet() {
    let printed = Printed::default();
    printed.push("panic: boom\n");
    let start = Instant::now();
    assert_eq!(printed.settled(Duration::from_secs(5)), "panic: boom\n");
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn test_printed_waits_for_the_verdict_a_slow_pipe_delivers() {
    let printed = Arc::new(Printed::default());
    printed.push("--- FAIL: TestX (0.00s)\n");
    let writer = Arc::clone(&printed);
    let late = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        writer.push("    x_test.go:6: boom\n");
        thread::sleep(Duration::from_millis(40));
        writer.push("FAIL\n");
    });
    let text = printed.settled(Duration::from_secs(5));
    late.join().unwrap();
    assert!(text.ends_with("boom\nFAIL\n"), "{text}");
}

#[test]
fn test_printed_hands_over_its_text_once() {
    let printed = Printed::default();
    printed.push("PASS\n");
    assert_eq!(printed.settled(Duration::from_secs(1)), "PASS\n");
    assert_eq!(printed.settled(Duration::from_millis(10)), "");
}
