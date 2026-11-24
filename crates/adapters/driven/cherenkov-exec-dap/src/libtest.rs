//! Per-test outcomes from libtest's human-readable output
//! (`test name ... ok`). The stable machine-readable format is unstable in
//! libtest, so the text form is read.

use std::collections::BTreeMap;

use cherenkov_mutation::execution::{Failure as CoreFailure, TestReport, TestResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TestOutcome {
    Ok,
    Failed,
    Ignored,
}

/// Each test named in `stdout` with its outcome. A test whose result line
/// never appears (the process died mid-test) is absent.
pub fn parse(stdout: &str) -> BTreeMap<String, TestOutcome> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("test ")?;
            let (name, result) = rest.split_once(" ... ")?;
            let outcome = match result.trim_end() {
                "ok" => TestOutcome::Ok,
                "FAILED" => TestOutcome::Failed,
                r if r == "ignored" || r.starts_with("ignored,") => TestOutcome::Ignored,
                _ => return None,
            };
            Some((name.to_string(), outcome))
        })
        .collect()
}

/// What a failing test reported: where it panicked, the panic message, and
/// for an equality assertion the two values (`left`, `right`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// `file:line:col` of the panic.
    pub location: Option<String>,
    pub message: String,
    pub left: Option<String>,
    pub right: Option<String>,
}

/// Each failing test's report, from libtest's `---- name stdout ----`
/// sections.
pub fn failures(stdout: &str) -> BTreeMap<String, Failure> {
    let mut out = BTreeMap::new();
    let mut lines = stdout.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(name) = line
            .strip_prefix("---- ")
            .and_then(|l| l.strip_suffix(" stdout ----"))
        else {
            continue;
        };
        let mut section = Vec::new();
        while let Some(next) = lines.peek() {
            if next.starts_with("---- ") || *next == "failures:" {
                break;
            }
            section.push(lines.next().unwrap());
        }
        out.insert(name.to_string(), failure(&section));
    }
    out
}

fn failure(section: &[&str]) -> Failure {
    let mut location = None;
    let mut message = Vec::new();
    let mut left: Option<Vec<&str>> = None;
    let mut right: Option<Vec<&str>> = None;
    let mut in_panic = false;
    for line in section {
        // `thread 'name' panicked at f:l:c:`, or with the thread id after the
        // name (`thread 'name' (1234) panicked at`) in newer toolchains.
        if let Some(at) = line
            .strip_prefix("thread '")
            .and_then(|l| l.split_once(" panicked at "))
            .map(|(_, at)| at)
        {
            location = Some(at.trim_end_matches(':').to_string());
            in_panic = true;
            continue;
        }
        if !in_panic {
            continue;
        }
        if line.is_empty() || line.starts_with("note: ") {
            in_panic = false;
        } else if let Some(v) = line.strip_prefix("  left: ") {
            left = Some(vec![v]);
        } else if let Some(v) = line.strip_prefix(" right: ") {
            right = Some(vec![v]);
        } else if let Some(r) = right.as_mut() {
            r.push(line);
        } else if let Some(l) = left.as_mut() {
            l.push(line);
        } else {
            message.push(*line);
        }
    }
    Failure {
        location,
        message: message.join("\n"),
        left: left.map(|l| l.join("\n")),
        right: right.map(|r| r.join("\n")),
    }
}

/// Each test `stdout` reports, by name: its result line, and for a failing
/// one the panic its section gives (a failing test whose result line was
/// lost to a crash is still named by its section; one with no section has
/// no place and no values).
pub fn reports(stdout: &str) -> Vec<TestReport> {
    let mut failed = failures(stdout);
    let mut reports: Vec<TestReport> = parse(stdout)
        .into_iter()
        .map(|(test, outcome)| {
            let result = match outcome {
                TestOutcome::Ok => TestResult::Passed,
                TestOutcome::Ignored => TestResult::Ignored,
                TestOutcome::Failed => TestResult::Failed(seen(failed.remove(&test))),
            };
            TestReport { test, result }
        })
        .collect();
    reports.extend(failed.into_iter().map(|(test, f)| TestReport {
        test,
        result: TestResult::Failed(seen(Some(f))),
    }));
    reports.sort_by(|a, b| a.test.cmp(&b.test));
    reports
}

/// A failure as the run layer reads it: libtest's left and right are the
/// value the mutant produced and the value the test expects.
fn seen(failure: Option<Failure>) -> CoreFailure {
    match failure {
        Some(f) => CoreFailure {
            location: f.location,
            message: f.message,
            got: f.left,
            expected: f.right,
        },
        None => CoreFailure {
            message: "the test failed".to_string(),
            ..CoreFailure::default()
        },
    }
}

/// The tests a libtest binary lists (`--list`): `name: test` lines.
pub fn listed(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.strip_suffix(": test"))
        .map(String::from)
        .collect()
}

/// The tests of a libtest binary, by running it with `--list`; `None`
/// when it does not run or is not one.
pub fn list_tests(binary: &std::path::Path) -> Option<Vec<String>> {
    let out = std::process::Command::new(binary)
        .args(["--list", "--format", "terse"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| listed(&String::from_utf8_lossy(&out.stdout)))
}
