//! `cherenkov run` on the fixtures under GDB and Delve gives the
//! hand-labelled verdicts, as JSON Lines events or as text, in completion
//! order. Slow; run with `just test-slow`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cli_support::Exit;

use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn dlv() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join("go/bin/dlv")
}

/// `cherenkov run dir args`, which must succeed.
fn run(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .arg("run")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// `stdout` read by `jq` one line at a time: each line must be one JSON
/// document. Returns what `filter` makes of the documents, one per line.
fn jq_lines(stdout: &[u8], filter: &str) -> Vec<String> {
    use std::io::Write;
    let mut jq = Command::new("jq")
        .args(["-R", "-r", &format!("fromjson | {filter}")])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("jq on PATH");
    jq.stdin.take().unwrap().write_all(stdout).unwrap();
    let out = jq.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "a line is not one JSON document: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

/// Every event of a JSON run's stdout.
fn documents(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8(stdout.to_vec())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

/// The decided mutants of a JSON run that a test called, in the
/// hand-labelled verdict files' `site verdict [tier]` form, sorted.
fn decided_lines(stdout: &[u8]) -> Vec<String> {
    let mut lines = jq_lines(
        stdout,
        r#"select(.event == "mutant.decided" and .verdict != "not-called")
           | "\(.file):\(.line):\(.column) \(.function // "-") \(.operator) \(.verdict)\(if .tier then " " + .tier else "" end)""#,
    );
    lines.sort();
    lines
}

/// The decided mutants of a text run that a test called, without their
/// ids, sorted.
fn text_lines(stdout: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = String::from_utf8(stdout.to_vec())
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.ends_with(" not-called"))
        .map(|l| l.split_once(' ').unwrap().1.to_string())
        .collect();
    lines.sort();
    lines
}

fn sorted_lines(path: PathBuf) -> Vec<String> {
    let mut lines: Vec<String> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    lines.sort();
    lines
}

/// The decided mutants of a JSON run.
fn decided(stdout: &[u8]) -> Vec<Value> {
    documents(stdout)
        .into_iter()
        .filter(|d| d["event"] == "mutant.decided")
        .collect()
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_gives_the_rust_fixture_s_hand_labelled_verdicts_as_json_lines() {
    let out = run(&fixtures().join("rust"), &["--operators", "breadth"]);
    assert_eq!(
        decided_lines(&out.stdout),
        sorted_lines(fixtures().join("rust/expected-verdicts.txt"))
    );
    let events = jq_lines(&out.stdout, ".event");
    assert_eq!(events.first().map(String::as_str), Some("run.started"));
    assert_eq!(events.last().map(String::as_str), Some("run.finished"));
    let finished = documents(&out.stdout).pop().unwrap();
    assert_eq!(
        (finished["status"].clone(), finished["exit"].clone()),
        ("complete".into(), 0.into())
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_as_text_gives_the_rust_fixture_s_hand_labelled_verdicts() {
    let out = run(
        &fixtures().join("rust"),
        &["--operators", "breadth", "--format", "text"],
    );
    assert_eq!(
        text_lines(&out.stdout),
        sorted_lines(fixtures().join("rust/expected-verdicts.txt"))
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_run_gives_the_go_fixture_s_hand_labelled_verdicts_as_json_lines() {
    let out = run(&fixtures().join("go"), &["--dlv", dlv().to_str().unwrap()]);
    assert_eq!(
        decided_lines(&out.stdout),
        sorted_lines(fixtures().join("go/expected-verdicts.txt"))
    );
}

#[test]
fn test_run_without_a_go_or_cargo_project_fails_with_message() {
    let out = Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .args(["run", fixtures().join("python").to_str().unwrap()])
        .args(["--format", "text"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(i32::from(Exit::REFUSED.code(false))),
        "declined"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no go.mod or Cargo.toml"), "{stderr}");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_with_a_selected_test_runs_only_that_test() {
    // Only test_total runs: total's mutants are killed as before; every
    // other function's mutants are not called (the selected test never
    // calls them), never "survived", which would blame the test.
    let out = run(
        &fixtures().join("rust"),
        &["--operators", "breadth", "--test", "test_total"],
    );
    let called: Vec<(String, String)> = decided(&out.stdout)
        .iter()
        .filter(|d| d["verdict"] != "not-called")
        .map(|d| {
            (
                d["function"].as_str().unwrap().to_string(),
                d["verdict"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(called.iter().all(|(f, _)| f == "total"), "{called:?}");
    let mut total: Vec<&str> = called.iter().map(|(_, v)| v.as_str()).collect();
    total.sort();
    assert_eq!(total, ["killed", "killed", "survived"]);
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_reports_what_killed_each_mutant() {
    // total's aor mutant (`a + b` -> `a - b`) gives -1 where test_total
    // expects 5: the report shows the change, the test, the assertion's
    // place, and both values.
    let dir = fixtures().join("rust");
    let out = run(&dir, &["--test", "test_total"]);
    let aor = decided(&out.stdout)
        .into_iter()
        .find(|d| d["function"] == "total" && d["operator"] == "aor")
        .unwrap();
    assert_eq!(
        (aor["original"].clone(), aor["mutated"].clone()),
        ("+".into(), "-".into())
    );
    assert_eq!(aor["killed_by"][0]["test"], "test_total");
    assert_eq!(aor["killed_by"][0]["location"], "tests/calc.rs:5:5");
    assert_eq!(
        (
            aor["killed_by"][0]["got"].clone(),
            aor["killed_by"][0]["expected"].clone()
        ),
        ("-1".into(), "5".into())
    );
    let text = run(&dir, &["--test", "test_total", "--format", "text"]);
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert!(
        stdout.contains("\n  killed by test_total at tests/calc.rs:5:5: got -1, expected 5\n"),
        "{stdout}"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_mutates_only_code_the_tests_call_never_the_tests() {
    // `called` is driven by the test; `never_called` is not; the test file
    // (a `#[cfg(test)]` module in its own file) and its helper are tests.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-driven");
    let out = run(&dir, &[]);
    let decided = decided(&out.stdout);
    let called: Vec<&Value> = decided
        .iter()
        .filter(|d| d["verdict"] != "not-called")
        .collect();
    assert!(!called.is_empty());
    assert!(
        decided.iter().all(|d| d["file"] == "src/lib.rs"),
        "{decided:?}"
    );
    assert!(
        called.iter().all(|d| matches!(
            d["function"].as_str(),
            Some("called" | "compute" | "table" | "bump")
        )),
        "{called:?}"
    );
    // `compute` runs once, for the test that checks nothing; the test that
    // checks reads the kept value. Every test counts as calling it, so its
    // mutants are killed, never "survived".
    let compute: Vec<&Value> = called
        .iter()
        .copied()
        .filter(|d| d["function"] == "compute")
        .collect();
    assert!(
        !compute.is_empty() && compute.iter().all(|d| d["verdict"] == "killed"),
        "{compute:?}"
    );
    assert!(decided.iter().any(|d| d["verdict"] == "not-called"));
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_reports_how_long_each_mutant_and_each_program_took() {
    // Where a run's time goes (killed, survived, unreached, timed out) is
    // measured per mutant, and per test program: its baseline, and
    // recording which tests call what.
    let out = run(&fixtures().join("rust"), &[]);
    let docs = documents(&out.stdout);
    assert!(
        docs.iter()
            .filter(|d| d["event"] == "mutant.decided")
            .all(|d| d["took_ms"].is_u64())
    );
    let prepared: Vec<&Value> = docs
        .iter()
        .filter(|d| d["event"] == "program.prepared")
        .collect();
    assert!(!prepared.is_empty());
    for p in &prepared {
        assert!(
            p["prepared_ms"].is_u64() && p["baseline_ms"].is_u64(),
            "{p}"
        );
        assert!(p["tests"].is_u64(), "counted: {p}");
        assert_eq!(p["selection"], "recorded", "{p}");
    }
    // The fixture's library has no tests; its integration tests have.
    assert!(
        prepared.iter().any(|p| p["tests"].as_u64() > Some(0)),
        "{prepared:?}"
    );
    // As text, each mutant's time is beneath its line.
    let text = run(&fixtures().join("rust"), &["--format", "text"]);
    let stdout = String::from_utf8(text.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(' ') {
            continue;
        }
        let details = lines[i + 1..].iter().take_while(|l| l.starts_with(' '));
        assert!(
            details
                .into_iter()
                .any(|l| l.starts_with("  took ") && l.ends_with(" ms")),
            "no time for {line}"
        );
    }
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(
        stderr
            .lines()
            .any(|l| l.contains(": prepared in ") && l.contains(" ms, baseline ")),
        "{stderr}"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_decides_mutants_no_test_executes_without_running_them() {
    // `bump`'s test never takes the branch: the `+=` there is never
    // executed. The recording sees that, so no run is spent learning it.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-driven");
    let out = run(&dir, &["--operators", "cargo-mutants"]);
    let decided = decided(&out.stdout);
    let unreached: Vec<&Value> = decided
        .iter()
        .filter(|d| d["verdict"] == "not-reached")
        .collect();
    assert!(unreached.is_empty(), "{unreached:?}");
    let finished = documents(&out.stdout).pop().unwrap();
    assert!(
        finished["counts"]["not-called"]
            .as_u64()
            .is_some_and(|n| n > 0),
        "the unexecuted mutants are counted: {finished}"
    );
}
