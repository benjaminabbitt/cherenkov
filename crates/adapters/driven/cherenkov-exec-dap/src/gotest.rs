//! Per-test results from Go's plain test output (no `-test.v`): the
//! `--- FAIL: TestX (0.00s)` line of each failed test, and under it the
//! first `file.go:line: message` the test logged. Go prints no values to
//! compare, so a failure has neither `got` nor `expected`.

use cherenkov_mutation::execution::{Failure, TestReport, TestResult};

/// How a test with no report line says it failed.
const NO_REPORT: &str = "the test failed";

/// Each test the output of a Go test binary names, in the order it names
/// them. A passed test is named only by verbose output.
pub fn reports(output: &str) -> Vec<TestReport> {
    let lines: Vec<&str> = output.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let (kind, test) = header(line)?;
            let result = match kind {
                Kind::Pass => TestResult::Passed,
                Kind::Skip => TestResult::Ignored,
                Kind::Fail => TestResult::Failed(failure(&own_lines(&lines, i))),
            };
            Some(TestReport {
                test: test.to_string(),
                result,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Kind {
    Pass,
    Fail,
    Skip,
}

/// How many spaces `line` is indented.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// A `--- FAIL: name (0.00s)` line (indented for a subtest): the result
/// and the name.
fn header(line: &str) -> Option<(Kind, &str)> {
    let rest = line.trim_start().strip_prefix("--- ")?;
    let (kind, rest) = rest.split_once(": ")?;
    let kind = match kind {
        "PASS" => Kind::Pass,
        "FAIL" => Kind::Fail,
        "SKIP" => Kind::Skip,
        _ => return None,
    };
    let (name, _took) = rest.rsplit_once(" (")?;
    Some((kind, name))
}

/// The lines the test at `at` printed itself: those indented under its
/// header, less its subtests' sections.
fn own_lines<'a>(lines: &[&'a str], at: usize) -> Vec<&'a str> {
    let depth = indent(lines[at]);
    let mut own = Vec::new();
    let mut skipping_under = None;
    for line in lines[at + 1..]
        .iter()
        .take_while(|l| !l.trim().is_empty() && indent(l) > depth)
    {
        if skipping_under.is_some_and(|d| indent(line) > d) {
            continue;
        }
        skipping_under = header(line).map(|_| indent(line));
        if skipping_under.is_none() {
            own.push(*line);
        }
    }
    own
}

/// The first place-and-message the lines give, with the lines under it
/// that carry no place of their own as the rest of its message.
fn failure(own: &[&str]) -> Failure {
    let Some(first) = own.iter().position(|l| located(l).is_some()) else {
        return Failure {
            message: NO_REPORT.to_string(),
            ..Failure::default()
        };
    };
    let (location, head) = located(own[first]).unwrap_or_default();
    let mut message = vec![head];
    message.extend(
        own[first + 1..]
            .iter()
            .take_while(|l| located(l).is_none())
            .map(|l| l.trim()),
    );
    Failure {
        location: Some(location.to_string()),
        message: message.join("\n"),
        got: None,
        expected: None,
    }
}

/// `file.go:line: message`, as `(file.go:line, message)`.
fn located(line: &str) -> Option<(&str, &str)> {
    let (place, message) = line.trim_start().split_once(": ")?;
    let (file, number) = place.rsplit_once(':')?;
    let place_like = file.ends_with(".go") && !number.is_empty();
    (place_like && number.bytes().all(|b| b.is_ascii_digit())).then_some((place, message))
}
