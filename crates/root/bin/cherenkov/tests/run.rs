//! `cherenkov run` on the fixtures under GDB and Delve gives the
//! hand-labelled verdicts, as JSON Lines events or as text, in completion
//! order. Slow; run with `just test-slow`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cli_support::Exit;

use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../fixtures")
}

fn dlv() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join("go/bin/dlv")
}

/// `cherenkov run dir args`, which must succeed. The run is kept in memory
/// unless `args` name a `--store`: the tests that run one fixture at once
/// must not share its store.
fn run(dir: &Path, args: &[&str]) -> Output {
    let kept = match args.contains(&"--store") {
        true => None,
        false => Some("--ephemeral"),
    };
    let out = Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .arg("run")
        .arg(dir)
        .args(args)
        .args(kept)
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

/// A mutant's verdict is a fact about the mutant, not about how busy the
/// machine was: the same corpus run twice decides every mutant the same way,
/// each by the same tier. Slow (two runs of the fixture), outside `just
/// check`; run it under load to mean something (`just test-determinism`).
#[test]
#[ignore = "slow: needs gdb and cargo; two runs of the rust fixture"]
fn test_run_twice_decides_every_mutant_of_the_rust_fixture_the_same_way() {
    let first = run(&fixtures().join("rust"), &["--operators", "breadth"]);
    let second = run(&fixtures().join("rust"), &["--operators", "breadth"]);
    let (a, b) = (decided_lines(&first.stdout), decided_lines(&second.stdout));
    assert!(!a.is_empty(), "the fixture decided no mutant");
    let differing: Vec<String> = a
        .iter()
        .filter(|l| !b.contains(l))
        .map(|l| format!("first only: {l}"))
        .chain(
            b.iter()
                .filter(|l| !a.contains(l))
                .map(|l| format!("second only: {l}")),
        )
        .collect();
    assert!(
        differing.is_empty(),
        "verdicts differ between identical runs:\n{}",
        differing.join("\n")
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
    // place, and both values. total has another aor mutant (`a * b`), and
    // decisions are printed in completion order (docs/output-events.md), so
    // the one meant is picked by what it changes, never by its place.
    let dir = fixtures().join("rust");
    let out = run(&dir, &["--test", "test_total"]);
    let aors: Vec<Value> = decided(&out.stdout)
        .into_iter()
        .filter(|d| d["function"] == "total" && d["operator"] == "aor")
        .collect();
    assert!(
        aors.iter().any(|d| d["mutated"] == "*"),
        "the sibling the order could put first: {aors:?}"
    );
    let aor = aors.into_iter().find(|d| d["mutated"] == "-").unwrap();
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
#[ignore = "slow: needs dlv and go"]
fn test_run_reports_what_killed_a_go_mutant() {
    // Total's aor mutant (`a + b` -> `a - b`) fails TestTotal, whose
    // `t.Fatal("Total")` is at calc_test.go:7: Go has no got/expected.
    let out = run(&fixtures().join("go"), &["--dlv", dlv().to_str().unwrap()]);
    let aor = decided(&out.stdout)
        .into_iter()
        .find(|d| d["function"] == "Total" && d["operator"] == "aor")
        .unwrap();
    assert_eq!(aor["verdict"], "killed");
    assert_eq!(aor["killed_by"][0]["test"], "TestTotal");
    assert_eq!(aor["killed_by"][0]["location"], "calc_test.go:7");
    assert_eq!(aor["killed_by"][0]["message"], "Total");
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

/// The multi-package fixture workspace (a standalone one, apart from
/// `fixtures/`, which the plan tests walk as a mixed tree).
fn workspace_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-workspace")
}

/// A path for a diff of this test alone.
fn unique_diff(tag: &str) -> PathBuf {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("cherenkov-{tag}-{}-{n}.diff", std::process::id()))
}

/// `cherenkov run` on the fixture workspace, one run at a time: they share
/// its target directory and the viability check's scratch copy in it.
fn run_workspace(args: &[&str]) -> Output {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    run(&workspace_fixture(), args)
}

/// A diff that changes `double`'s body line (line 4) in the fixture
/// workspace's `core` package and nothing else.
fn core_diff() -> PathBuf {
    let path = unique_diff("core");
    std::fs::write(
        &path,
        "--- a/crates/root/bin/cherenkov/fixtures/rust-workspace/core/src/lib.rs\n\
         +++ b/crates/root/bin/cherenkov/fixtures/rust-workspace/core/src/lib.rs\n\
         @@ -3,3 +3,3 @@\n pub fn double(x: i32) -> i32 {\n-    x + x\n+    x * 2\n }\n",
    )
    .unwrap();
    path
}

/// The test programs a text run prepared (stderr's `prepared in` lines),
/// by the package they belong to (`ws_core`, `ws_util`, `ws_app`).
fn prepared_packages(stderr: &[u8]) -> Vec<String> {
    let mut found: Vec<String> = String::from_utf8_lossy(stderr)
        .lines()
        .filter(|l| l.contains(": prepared in "))
        .flat_map(|l| {
            ["ws_core", "ws_util", "ws_app"]
                .into_iter()
                .filter(|p| l.contains(p))
        })
        .map(String::from)
        .collect();
    found.sort();
    found
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_of_a_diff_in_one_package_prepares_only_that_package_s_programs() {
    let diff = core_diff();
    let out = run_workspace(&["--format", "text", "--in-diff", diff.to_str().unwrap()]);
    let _ = std::fs::remove_file(&diff);
    assert_eq!(prepared_packages(&out.stderr), ["ws_core"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    for phase in [
        "build",
        "viability check",
        "preparation and recording",
        "mutants",
    ] {
        assert!(
            stderr.contains(&format!("phase {phase}: ")),
            "{phase}: {stderr}"
        );
    }
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_of_a_diff_with_test_workspace_prepares_every_package_s_programs() {
    let diff = core_diff();
    let out = run_workspace(&[
        "--format",
        "text",
        "--in-diff",
        diff.to_str().unwrap(),
        "--test-workspace",
        "true",
    ]);
    let _ = std::fs::remove_file(&diff);
    assert_eq!(
        prepared_packages(&out.stderr),
        ["ws_app", "ws_core", "ws_util"]
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_of_a_diff_with_test_package_judges_by_that_package_s_tests_alone() {
    let diff = core_diff();
    let out = run_workspace(&[
        "--format",
        "text",
        "--in-diff",
        diff.to_str().unwrap(),
        "--test-package",
        "ws-app",
    ]);
    let _ = std::fs::remove_file(&diff);
    assert_eq!(prepared_packages(&out.stderr), ["ws_app"]);
}

/// A diff that changes `app`'s test `test_run_doubles_an_even_number` only
/// (its body line, line 13).
fn app_test_diff() -> PathBuf {
    let path = unique_diff("app");
    std::fs::write(
        &path,
        "--- a/crates/root/bin/cherenkov/fixtures/rust-workspace/app/src/lib.rs\n\
         +++ b/crates/root/bin/cherenkov/fixtures/rust-workspace/app/src/lib.rs\n\
         @@ -12,3 +12,3 @@\n fn test_run_doubles_an_even_number() {\n-        assert_eq!(run(4), 7);\n+        assert_eq!(run(4), 8);\n     }\n",
    )
    .unwrap();
    path
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_of_a_changed_test_runs_the_mutants_it_calls_with_that_test_alone() {
    let diff = app_test_diff();
    let out = run_workspace(&["--format", "json", "--in-diff", diff.to_str().unwrap()]);
    let _ = std::fs::remove_file(&diff);
    let called: std::collections::BTreeSet<String> = decided(&out.stdout)
        .iter()
        .map(|d| d["function"].as_str().unwrap().to_string())
        .collect();
    // The changed test calls run, is_even and double; nothing calls triple
    // but core's own test, which did not change: its mutants are counted,
    // not listed.
    assert_eq!(
        called,
        ["double", "is_even", "run"].map(String::from).into()
    );
    let finished = documents(&out.stdout)
        .into_iter()
        .find(|d| d["event"] == "run.finished")
        .unwrap();
    // `triple`'s five mutants and `id_of`'s one.
    assert_eq!(finished["counts"]["not-called"], 6);
    let stderr = documents(&out.stdout)
        .iter()
        .filter(|d| d["event"] == "program.prepared")
        .count();
    assert_eq!(stderr, 1, "only app's program is prepared");
}

/// A diff that changes `id_of`'s body (line 16 of core).
fn id_diff() -> PathBuf {
    let path = unique_diff("id");
    std::fs::write(
        &path,
        "--- a/crates/root/bin/cherenkov/fixtures/rust-workspace/core/src/lib.rs\n\
         +++ b/crates/root/bin/cherenkov/fixtures/rust-workspace/core/src/lib.rs\n\
         @@ -15,3 +15,3 @@\n pub fn id_of(x: i32) -> Id {\n-    Id(x + 0)\n+    Id(x)\n }\n",
    )
    .unwrap();
    path
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_run_of_a_diff_has_rustc_reject_the_mutant_it_cannot_compile_and_writes_a_verdict_file() {
    let diff = id_diff();
    let verdicts =
        std::env::temp_dir().join(format!("cherenkov-verdicts-{}.jsonl", std::process::id()));
    let out = run_workspace(&[
        "--format",
        "text",
        "--in-diff",
        diff.to_str().unwrap(),
        "--verdicts",
        verdicts.to_str().unwrap(),
    ]);
    let _ = std::fs::remove_file(&diff);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(" id_of "), "{stdout}");
    assert!(
        stdout.contains("unviable"),
        "rustc rejects `Default::default()` of Id: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1 of "),
        "the viability check found it: {stderr}"
    );
    assert!(
        stderr.contains("changed-test mutants are judged by the changed tests alone"),
        "{stderr}"
    );
    let written = std::fs::read_to_string(&verdicts).unwrap();
    let _ = std::fs::remove_file(&verdicts);
    assert!(
        written.contains("\"id_of\"") || written.contains("id_of"),
        "{written}"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_a_run_killed_with_sigkill_is_carried_on_and_ends_as_an_uninterrupted_run_does() {
    use std::io::{BufRead, BufReader};
    let dir = fixtures().join("rust");
    let scratch = std::env::temp_dir().join(format!("cherenkov-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let store = scratch.join("runs.sqlite");
    let store_arg = store.to_str().unwrap();

    // The first process is killed as soon as it has decided a mutant a test
    // called, with the rest owed.
    let mut child = Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .arg("run")
        .arg(&dir)
        .args([
            "--store",
            store_arg,
            "--jobs",
            "1",
            "--operators",
            "breadth",
            "--format",
            "json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut seen = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let doc: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let called = doc["event"] == "mutant.decided" && doc["verdict"] != "not-called";
        assert_ne!(
            doc["event"], "run.finished",
            "the run ended before it was killed"
        );
        seen.push(doc);
        if called {
            break;
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(!seen.is_empty());

    let resumed = run(
        &dir,
        &[
            "--store",
            store_arg,
            "--jobs",
            "1",
            "--operators",
            "breadth",
        ],
    );
    let docs = documents(&resumed.stdout);
    assert_eq!(docs.first().unwrap()["event"], "run.started");
    assert_eq!(docs.first().unwrap()["run"], seen[0]["run"], "the same run");
    assert_eq!(docs.last().unwrap()["status"], "complete");
    assert_eq!(
        decided_lines(&resumed.stdout),
        sorted_lines(fixtures().join("rust/expected-verdicts.txt"))
    );
    let ids: Vec<&str> = docs
        .iter()
        .filter(|d| d["event"] == "mutant.decided")
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    let mut once = ids.clone();
    once.sort();
    once.dedup();
    assert_eq!(ids.len(), once.len(), "each mutant is decided once");

    // A run after that one is a new run: the first finished.
    let after = run(
        &dir,
        &[
            "--store",
            store_arg,
            "--jobs",
            "1",
            "--operators",
            "breadth",
        ],
    );
    assert_ne!(documents(&after.stdout)[0]["run"], seen[0]["run"]);
    let _ = std::fs::remove_dir_all(&scratch);
}
