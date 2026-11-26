//! Against real GDB (DAP) and cargo on the Rust fixture. Slow; run with
//! `just test-slow`. Needs `gdb` 14+ and `cargo` on PATH.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cherenkov_exec_dap::gdb::{GdbConfig, GdbExecutor, GdbSession};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::native::{NativeSession, RunEnd};
use cherenkov_exec_dap::reason::Reason;
use cherenkov_exec_dap::tierp::derive;
use cherenkov_exec_dap::toolchain::build_rust_test_binaries;
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::executor::MutantExecutor;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../fixtures/rust")
        .canonicalize()
        .unwrap()
}

/// The fixture's test binaries: [lib unit tests, tests/calc.rs], sorted.
fn binaries() -> &'static [PathBuf] {
    static BINS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    BINS.get_or_init(|| {
        let mut bins = build_rust_test_binaries(Path::new("cargo"), &fixture()).unwrap();
        bins.sort();
        bins
    })
}

/// The integration test binary, which links the library and runs its tests.
fn integration() -> PathBuf {
    integration_of(binaries())
}

/// The binary of `bins` that runs the fixture's integration tests. The
/// library's own unit-test binary is not it: the linker drops the library
/// functions nothing there calls. Binaries' names are hashes, so their
/// order says nothing about which is which.
fn integration_of(bins: &[PathBuf]) -> PathBuf {
    bins.iter()
        .find(|b| {
            let out = std::process::Command::new(b)
                .arg("--list")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).contains("test_total")
        })
        .expect("an integration test binary")
        .clone()
}

fn config(binary: PathBuf) -> GdbConfig {
    GdbConfig {
        gdb: PathBuf::from("gdb"),
        test_binary: binary,
        test_args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        source_root: None,
    }
}

fn plan(operator: &str, line: u32) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = std::fs::read_to_string(fixture().join("src/lib.rs")).unwrap();
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src.as_str()))
        .unwrap();
    plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .find(|p| p.operator == operator && p.stop_at.line == line)
        .unwrap()
}

fn budget() -> Budget {
    Budget {
        timeout: Duration::from_secs(60),
    }
}

fn code(raw: RawOutcome) -> i32 {
    match raw {
        RawOutcome::Exited { code, tier, .. } => {
            assert_eq!(tier, Tier::P);
            code
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_finds_the_function_of_a_source_line() {
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 10)
        .unwrap()
        .expect("is_small is linked");
    assert!(code.file.ends_with("src/lib.rs"), "{}", code.file);
    assert!(code.insns.iter().any(|i| i.line == 10));
    assert!(
        code.insns.iter().all(|i| i.line >= 9 && i.line <= 14),
        "only is_small's lines"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_reports_code_missing_from_a_binary() {
    let mut s = GdbSession::start(&config(integration())).unwrap();
    assert_eq!(
        s.function_code(&fixture().join("src/no_such.rs"), 3)
            .unwrap(),
        None
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_a_file_missing_from_the_binary_costs_at_most_one_lookup_for_all_its_lines() {
    // A workspace asks every test program about every planned file; most
    // programs hold few of them. Searching on line by line for a file the
    // binary does not hold cost minutes per file in large binaries.
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let missing = fixture().join("src/no_such.rs");
    for line in 1..=20 {
        assert_eq!(s.function_code(&missing, line).unwrap(), None);
    }
    let lookups = s.line_lookups().unwrap();
    assert!(lookups <= 1, "{lookups} lookups");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_locating_any_line_costs_at_most_one_lookup() {
    // A line with no code (a comment, a signature) is located by the next
    // line with code; asking GDB about each line in between cost hundreds
    // of thousands of lookups in a workspace's large binaries.
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let file = fixture().join("src/lib.rs");
    let lines = std::fs::read_to_string(&file).unwrap().lines().count() as u32;
    let mut found = 0;
    for line in 1..=lines {
        found += usize::from(s.function_code(&file, line).unwrap().is_some());
    }
    assert!(found > 0, "the fixture's functions are located");
    let lookups = s.line_lookups().unwrap();
    assert!(
        lookups <= u64::from(lines),
        "{lookups} lookups for {lines} lines"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_locating_a_line_again_asks_gdb_nothing() {
    // Several mutants share a line (1.7 per line in ctxloom); each is
    // located in every test program.
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let file = fixture().join("src/lib.rs");
    let first = s.function_code(&file, 10).unwrap();
    let lookups = s.line_lookups().unwrap();
    assert_eq!(s.function_code(&file, 10).unwrap(), first);
    assert_eq!(s.line_lookups().unwrap(), lookups);
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_a_function_is_disassembled_once_for_all_its_lines() {
    // A large function (an async state machine) has many mutated lines;
    // disassembling it again for each cost minutes per test program.
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let file = fixture().join("src/lib.rs");
    let at_10 = s.function_code(&file, 10).unwrap().expect("is_small");
    let at_13 = s.function_code(&file, 13).unwrap().expect("is_small");
    assert_eq!(at_10, at_13);
    assert_eq!(s.disassemblies(), 1);
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_a_process_killed_by_a_signal_is_a_crash_not_a_pass() {
    // GDB's DAP reports a death by signal as "exited" with code 0.
    let mut cfg = config(integration());
    cfg.test_args = vec![
        "--ignored".into(),
        "--exact".into(),
        "test_killed_outright".into(),
    ];
    let mut s = GdbSession::start(&cfg).unwrap();
    let (end, _) = s.run_reaching(&[], Duration::from_secs(60)).unwrap();
    // GDB names the signal when it reports the death; when it reports
    // nothing, the process is found gone and no one can say which signal.
    assert!(
        matches!(&end, RunEnd::Crashed { kind, .. } if kind == "SIGKILL" || kind == "killed"),
        "{end:?}"
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_an_evaluation_that_never_finishes_is_refused_quickly() {
    // Tier B evaluates with the program's other threads stopped: a call
    // that waits on one of them never returns. The mutated program itself
    // would not hang, so this is a refusal, not a timeout, and it must not
    // hold the run past the evaluation's own limit.
    use cherenkov_exec_dap::native::Effect;
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 4)
        .unwrap()
        .expect("total is linked");
    let stop = code
        .insns
        .iter()
        .filter(|i| i.line == 4)
        .map(|i| i.pc)
        .min()
        .unwrap();
    let effect = Effect::Assign {
        target: "s".into(),
        value: "count_up(9223372036854775807)".into(),
    };
    let started = std::time::Instant::now();
    let run = s
        .run_evaluating(&[stop], &effect, Duration::from_secs(120))
        .unwrap()
        .expect("GDB evaluates");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    let why = run.refused.expect("refused");
    assert!(matches!(why, Reason::EvaluationHung { .. }), "{why}");
    // GDB is stuck in the call: the session is not reused.
    assert!(!s.restart().unwrap());
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_patch_arms_and_disarms_byte_identical() {
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 10)
        .unwrap()
        .unwrap();
    let patch = derive(
        &plan("negate-condition", 10),
        &code.file,
        &code.insns,
        &Isa::amd64(),
    )
    .unwrap();
    let window = s.read(patch.regions[0].addr - 4, 16).unwrap();
    s.arm(&patch).unwrap();
    assert_eq!(
        s.read(patch.regions[0].addr, patch.regions[0].replacement.len())
            .unwrap(),
        patch.regions[0].replacement
    );
    s.disarm(&patch).unwrap();
    assert_eq!(s.read(patch.regions[0].addr - 4, 16).unwrap(), window);
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_unmutated_run_exits_zero_and_failing_run_exits_101() {
    let mut s = GdbSession::start(&config(integration())).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(60)).unwrap(),
        RunEnd::Exited(0)
    );
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 10)
        .unwrap()
        .unwrap();
    let patch = derive(
        &plan("negate-condition", 10),
        &code.file,
        &code.insns,
        &Isa::amd64(),
    )
    .unwrap();
    s.arm(&patch).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(60)).unwrap(),
        RunEnd::Exited(101)
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_executor_kills_and_survives_as_labelled() {
    let mut exec = GdbExecutor::new(config(integration()), Isa::amd64());
    assert_eq!(code(exec.baseline(budget()).unwrap().outcome), 0);
    assert_eq!(
        code(
            exec.run(&plan("negate-condition", 10), budget())
                .unwrap()
                .outcome
        ),
        101
    );
    assert_eq!(
        code(exec.run(&plan("ror", 10), budget()).unwrap().outcome),
        0,
        "no test at 10"
    );
    assert_eq!(
        code(
            exec.run(&plan("delete-statement", 5), budget())
                .unwrap()
                .outcome
        ),
        0,
        "log is a no-op"
    );
    assert_eq!(
        code(exec.run(&plan("ror", 18), budget()).unwrap().outcome),
        101
    );
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_executor_reports_a_binary_without_the_code_as_not_reached() {
    let mut exec = GdbExecutor::new(config(integration()), Isa::amd64());
    let mut p = plan("negate-condition", 10);
    p.file = PathBuf::from("src/no_such.rs");
    assert!(matches!(
        exec.run(&p, budget()).unwrap().outcome,
        RawOutcome::NotReached { .. }
    ));
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_finds_code_in_a_binary_built_at_another_path() {
    // Debug info names the path the binary was built at, not the project
    // root a run is given (copied target directories, CI caches).
    let guard = RemovedOnDrop(
        std::env::temp_dir().join(format!("cherenkov-built-elsewhere-{}", std::process::id())),
    );
    let elsewhere = &guard.0;
    let _ = std::fs::remove_dir_all(elsewhere);
    std::fs::create_dir_all(elsewhere.join("src")).unwrap();
    std::fs::create_dir_all(elsewhere.join("tests")).unwrap();
    for f in ["Cargo.toml", "src/lib.rs", "tests/calc.rs"] {
        std::fs::copy(fixture().join(f), elsewhere.join(f)).unwrap();
    }
    let bins = build_rust_test_binaries(Path::new("cargo"), elsewhere).unwrap();
    let mut exec = GdbExecutor::new(
        GdbConfig {
            gdb: PathBuf::from("gdb"),
            test_binary: integration_of(&bins),
            test_args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            source_root: None,
        },
        Isa::amd64(),
    );
    let raw = exec
        .run(&plan("negate-condition", 10), budget())
        .unwrap()
        .outcome;
    assert!(
        matches!(raw, RawOutcome::Exited { code: 101, .. }),
        "{raw:?}"
    );
}

/// A scratch directory removed however the test ends, a panic included:
/// mutation runs make these tests fail, and each left a copy behind.
struct RemovedOnDrop(PathBuf);

impl Drop for RemovedOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gone(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |s| s.contains(") Z "))
}

/// Busy threads, several for each core, until the guard is dropped: a
/// machine too loaded for anything to get a core to itself.
struct Hogs(Arc<std::sync::atomic::AtomicBool>);

impl Hogs {
    fn start() -> Hogs {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        for _ in 0..cores * 4 {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            });
        }
        Hogs(stop)
    }
}

impl Drop for Hogs {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_a_looping_mutant_is_timed_out_on_a_loaded_machine_too() {
    let _hogs = Hogs::start();
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 19)
        .unwrap()
        .unwrap();
    let patch = derive(&plan("aor", 19), &code.file, &code.insns, &Isa::amd64()).unwrap();
    s.arm(&patch).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(1)).unwrap(),
        RunEnd::TimedOut
    );
    // Stretched by the wait, never past the backstop (6 s) and its slack.
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(1) && took < Duration::from_secs(10),
        "{took:?}"
    );
    // The run says what the load took from it, for its verdict to net out.
    assert!(s.lost_to_load() > Duration::ZERO && s.lost_to_load() <= Duration::from_secs(5));
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_dropping_a_timed_out_session_kills_its_looping_inferior() {
    let mut s = GdbSession::start(&config(integration())).unwrap();
    let inferior = s.debuggee_pid();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 19)
        .unwrap()
        .unwrap();
    let patch = derive(&plan("aor", 19), &code.file, &code.insns, &Isa::amd64()).unwrap();
    s.arm(&patch).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(1)).unwrap(),
        RunEnd::TimedOut
    );
    drop(s);
    std::thread::sleep(Duration::from_millis(300));
    assert!(gone(inferior), "inferior {inferior} still running");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_cancelling_a_run_in_flight_ends_gdb_and_its_inferior() {
    use cherenkov_exec_dap::cancel::Watch;
    use cherenkov_run_ports::executor::Cancel;
    let watch = std::sync::Arc::new(Watch::default());
    let mut s = GdbSession::start_watched(&config(integration()), &watch).unwrap();
    let inferior = s.debuggee_pid();
    let code = s
        .function_code(&fixture().join("src/lib.rs"), 19)
        .unwrap()
        .unwrap();
    let patch = derive(&plan("aor", 19), &code.file, &code.insns, &Isa::amd64()).unwrap();
    s.arm(&patch).unwrap();
    // The mutant loops: the run is in flight, blocked in gdb's `continue`.
    let (done, ended) = std::sync::mpsc::channel();
    let runner = std::thread::spawn(move || {
        let end = s.run_to_exit(Duration::from_secs(600));
        let _ = done.send(end.is_err());
        s
    });
    std::thread::sleep(Duration::from_secs(1));
    watch.cancel();
    let errored = ended.recv_timeout(Duration::from_secs(5));
    let session = runner.join().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let left = !gone(inferior);
    drop(session);
    assert_eq!(errored, Ok(true), "the blocked run did not return an error");
    assert!(!left, "inferior {inferior} still running after the cancel");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_run_capturing_returns_the_test_binarys_stdout() {
    let mut exec = GdbExecutor::new(config(integration()), Isa::amd64());
    let (raw, out) = exec
        .run_capturing(&plan("negate-condition", 10), budget())
        .unwrap();
    assert!(
        matches!(raw, RawOutcome::Exited { code: 101, .. }),
        "{raw:?}"
    );
    assert!(out.contains("test test_is_small ... FAILED"), "{out}");
    assert!(out.contains("test test_total ... ok"), "{out}");
    let (raw, out) = exec.baseline_capturing(budget()).unwrap();
    assert!(matches!(raw, RawOutcome::Exited { code: 0, .. }), "{raw:?}");
    assert!(out.contains("test test_is_small ... ok"), "{out}");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_run_reports_each_test_typed_with_what_the_failed_one_saw() {
    use cherenkov_mutation::execution::TestResult;
    let mut exec = GdbExecutor::new(config(integration()), Isa::amd64());
    let observed = exec.run(&plan("negate-condition", 10), budget()).unwrap();
    let result = |name: &str| {
        observed
            .tests
            .iter()
            .find(|t| t.test == name)
            .map(|t| t.result.clone())
    };
    assert_eq!(result("test_total"), Some(TestResult::Passed));
    let Some(TestResult::Failed(failed)) = result("test_is_small") else {
        panic!("{observed:?}");
    };
    assert!(failed.location.is_some(), "{failed:?}");
}

fn selecting(test: &str) -> GdbConfig {
    GdbConfig {
        test_args: vec![test.to_string(), "--exact".to_string()],
        ..config(integration())
    }
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_mutant_the_selected_test_never_executes_is_not_reached() {
    // test_total never calls is_small: its mutant must not "survive".
    let mut exec = GdbExecutor::new(selecting("test_total"), Isa::amd64());
    let raw = exec
        .run(&plan("negate-condition", 10), budget())
        .unwrap()
        .outcome;
    assert!(matches!(raw, RawOutcome::NotReached { .. }), "{raw:?}");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_reached_mutants_are_judged_by_the_test_as_before() {
    let mut exec = GdbExecutor::new(selecting("test_is_small"), Isa::amd64());
    assert_eq!(
        code(
            exec.run(&plan("negate-condition", 10), budget())
                .unwrap()
                .outcome
        ),
        101
    );
    // Reached, and test_is_small cannot tell `<` from `<=` at 10: survived.
    let raw = exec.run(&plan("ror", 10), budget()).unwrap().outcome;
    assert!(matches!(raw, RawOutcome::Exited { code: 0, .. }), "{raw:?}");
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_records_which_tests_call_each_function_and_runs_only_those() {
    use cherenkov_run_ports::executor::Driven;
    let mut exec = GdbExecutor::new(config(integration()), Isa::amd64());
    let total = plan("aor", 4);
    let small = plan("negate-condition", 10);
    let recording = exec
        .recording(&[total.clone(), small.clone()], budget())
        .unwrap();
    assert_eq!(
        recording.selection,
        cherenkov_run_ports::executor::Selection::Recorded
    );
    assert!(recording.tests.is_some_and(|n| n >= 2), "{recording:?}");
    let driven = recording.driven.unwrap();
    assert_eq!(
        driven,
        [
            Driven::Tests(vec!["test_total".to_string()]),
            Driven::Tests(vec!["test_is_small".to_string()]),
        ]
    );
    // Narrowed to a test that never calls `total`, its mutant is not
    // reached; narrowed to the one that does, it is killed.
    exec.select_tests(Some(&["test_is_small".to_string()]));
    assert!(matches!(
        exec.run(&total, budget()).unwrap().outcome,
        RawOutcome::NotReached { .. }
    ));
    exec.select_tests(Some(&["test_total".to_string()]));
    assert_eq!(code(exec.run(&total, budget()).unwrap().outcome), 101);
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_configured_variables_add_to_the_program_s_environment() {
    // GDB's DAP launch replaces the environment with the one given: the
    // configured variables must come on top of ours, not instead of it.
    let mut c = config(integration());
    c.env = vec![("CHERENKOV_PROBE".into(), "1".into())];
    let mut s = GdbSession::start(&c).unwrap();
    assert_eq!(
        s.environment_variable("CHERENKOV_PROBE")
            .unwrap()
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        s.environment_variable("PATH").unwrap(),
        std::env::var("PATH").ok()
    );
}

#[test]
fn test_every_signal_but_faults_and_the_debugger_s_own_is_passed_to_the_program() {
    use cherenkov_exec_dap::gdb::PASSED_SIGNALS;
    // The program's own signal use (Rust ignores SIGPIPE; tests use SIGCHLD,
    // SIGTERM, SIGUSR1, timers) is its business: passed, never a stop. A
    // passed signal that kills the program is still a crash, seen at its
    // exit.
    for name in [
        "SIGPIPE", "SIGCHLD", "SIGTERM", "SIGUSR1", "SIGALRM", "SIGWINCH",
    ] {
        assert!(PASSED_SIGNALS.contains(&name), "{name}");
    }
    // Faults are crashes (Rust catches SIGSEGV and SIGBUS only to report a
    // stack overflow); the debugger's own and the uncatchable are not ours
    // to pass.
    for name in [
        "SIGSEGV", "SIGBUS", "SIGILL", "SIGFPE", "SIGABRT", "SIGINT", "SIGTRAP", "SIGKILL",
        "SIGSTOP",
    ] {
        assert!(!PASSED_SIGNALS.contains(&name), "{name}");
    }
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_gdb_a_signal_the_program_ignores_is_not_a_crash() {
    // Writing to a pipe whose reader is gone raises SIGPIPE, which Rust
    // ignores: the write fails and the test goes on.
    let mut cfg = config(integration());
    cfg.test_args = vec![
        "--ignored".into(),
        "--exact".into(),
        "test_writes_to_a_closed_pipe".into(),
    ];
    let mut s = GdbSession::start(&cfg).unwrap();
    let (end, _) = s.run_reaching(&[], Duration::from_secs(60)).unwrap();
    assert_eq!(end, RunEnd::Exited(0));
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_live_a_test_binary_without_debug_info_is_an_error_not_every_mutant_skipped() {
    // A project whose test profile has `debug = 0` (angzarr's) gives no
    // line table: nothing can be located, and every mutant would quietly
    // count as never called.
    let target = test_dir::TestDir::new("cherenkov-no-debug");
    let out = std::process::Command::new("cargo")
        .args(["test", "--no-run", "--message-format=json"])
        .current_dir(fixture())
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_STRIP", "true")
        .env("CARGO_PROFILE_DEV_STRIP", "true")
        .output()
        .unwrap();
    let binary = cherenkov_exec_dap::toolchain::test_binaries_from_cargo_json(
        &String::from_utf8_lossy(&out.stdout),
    )
    .into_iter()
    .find(|b| {
        let l = std::process::Command::new(b)
            .arg("--list")
            .output()
            .unwrap();
        String::from_utf8_lossy(&l.stdout).contains("test_total")
    })
    .unwrap();
    let mut exec = GdbExecutor::new(config(binary), Isa::amd64());
    let got = exec.driven(&[plan("aor", 4)], budget());
    let err = got.expect_err("no debug info must be an error").to_string();
    assert!(err.contains("debug"), "{err}");
}
