//! Against a real Delve and Go toolchain on the Go fixture. Slow; run with
//! `just test-slow`. Needs `dlv` and `go` on PATH or in `$HOME/go/bin`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use cherenkov_exec_dap::delve::{
    DelveConfig, DelveExecutor, DelveSession, RunEnd, build_go_test_binary,
};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::native::NativeSession;
use cherenkov_exec_dap::tierp::{Patch, derive};
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::executor::MutantExecutor;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../fixtures/go")
        .canonicalize()
        .unwrap()
}

fn tool(name: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap())
        .join("go/bin")
        .join(name);
    if home.exists() {
        return home;
    }
    std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .find(|c| c.exists())
        })
        .unwrap_or_else(|| panic!("{name} not found"))
}

/// The fixture's test binary, built once per test process into a temp dir.
fn test_binary() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("cherenkov-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("calc.test");
        build_go_test_binary(&tool("go"), &fixture(), &out).unwrap();
        out
    })
}

fn config() -> DelveConfig {
    DelveConfig {
        dlv: tool("dlv"),
        test_binary: test_binary().to_path_buf(),
        test_args: Vec::new(),
    }
}

fn plan(operator: &str, line: u32) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let go = LanguageId::new("go");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == go)
        .unwrap();
    let src = std::fs::read_to_string(fixture().join("calc.go")).unwrap();
    let file = syntax
        .analyze(&go, Path::new("calc.go"), Arc::from(src.as_str()))
        .unwrap();
    plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .find(|p| p.operator == operator && p.stop_at.line == line)
        .unwrap()
}

fn budget() -> Budget {
    Budget {
        timeout: Duration::from_secs(30),
    }
}

fn negate_is_small(session: &mut DelveSession) -> Patch {
    let file = fixture().join("calc.go");
    let code = session.function_code(&file, 12).unwrap().unwrap();
    derive(
        &plan("negate-condition", 12),
        &code.file,
        &code.insns,
        &Isa::amd64(),
    )
    .unwrap()
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_patch_arms_and_disarms_byte_identical() {
    let mut s = DelveSession::start(&config()).unwrap();
    let patch = negate_is_small(&mut s);
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
#[ignore = "slow: needs dlv and go"]
fn test_live_arm_refuses_when_memory_does_not_hold_the_original() {
    let mut s = DelveSession::start(&config()).unwrap();
    let mut patch = negate_is_small(&mut s);
    let before = s
        .read(patch.regions[0].addr, patch.regions[0].original.len())
        .unwrap();
    patch.regions[0].original[0] ^= 0xff;
    assert!(s.arm(&patch).is_err());
    assert_eq!(
        s.read(patch.regions[0].addr, before.len()).unwrap(),
        before,
        "nothing written"
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_unmutated_run_exits_zero_and_patched_run_reports_exit_code() {
    let mut s = DelveSession::start(&config()).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(30)).unwrap(),
        RunEnd::Exited(0)
    );
    let mut s = DelveSession::start(&config()).unwrap();
    let patch = negate_is_small(&mut s);
    s.arm(&patch).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(30)).unwrap(),
        RunEnd::Exited(1)
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_executor_baseline_passes_and_mutants_kill_or_survive() {
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    let code = |raw: RawOutcome| match raw {
        RawOutcome::Exited { code, tier, .. } => {
            assert_eq!(tier, Tier::P);
            code
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(code(exec.baseline(budget()).unwrap().outcome), 0);
    assert_ne!(
        code(
            exec.run(&plan("negate-condition", 12), budget())
                .unwrap()
                .outcome
        ),
        0,
        "negation is killed"
    );
    assert_eq!(
        code(exec.run(&plan("ror", 12), budget()).unwrap().outcome),
        0,
        "boundary swap survives: no test at 10"
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_executor_reports_unpatchable_plan_as_unsupported() {
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    // Multiplication has no same-length machine-code swap.
    let mut p = plan("aor", 6);
    p.edit = cherenkov_mutation::edit::Edit::ReplaceToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        from: cherenkov_mutation::token::Token::Mul,
        with: cherenkov_mutation::token::Token::Div,
    };
    let raw = exec.run(&p, budget()).unwrap().outcome;
    assert_eq!(
        raw,
        RawOutcome::Unsupported {
            reason: "no tier B evaluator here (tier P: no tier P rule for aor mul -> div)".into()
        }
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_executor_reports_the_go_tests_a_mutant_failed_and_none_for_a_pass() {
    use cherenkov_mutation::execution::{Failure, TestReport, TestResult};
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    // `a + b` -> `a - b` fails TestTotal's `t.Fatal("Total")`.
    let killed = exec.run(&plan("aor", 6), budget()).unwrap();
    assert!(
        matches!(killed.outcome, RawOutcome::Exited { code: 1, .. }),
        "{killed:?}"
    );
    assert_eq!(
        killed.tests,
        vec![TestReport {
            test: "TestTotal".to_string(),
            result: TestResult::Failed(Failure {
                location: Some("calc_test.go:7".to_string()),
                message: "Total".to_string(),
                got: None,
                expected: None,
            }),
        }]
    );
    let baseline = exec.baseline(budget()).unwrap();
    assert!(
        baseline.tests.is_empty(),
        "a quiet pass names no test: {baseline:?}"
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_run_past_budget_times_out() {
    // A 1 ms budget cannot fit even the unmutated run.
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    let raw = exec
        .baseline(Budget {
            timeout: Duration::from_millis(1),
        })
        .unwrap();
    assert!(
        matches!(raw.outcome, RawOutcome::TimedOut { tier: Tier::P, .. }),
        "{raw:?}"
    );
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_panicking_mutant_is_caught_by_delve_as_a_crash_with_its_message() {
    // `j <= len(xs)` indexes past the end. Delve stops on the unrecovered
    // panic, so the run ends at once, under the debugger, with the stack
    // that locates the crash and the test that hit it.
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    match exec.run(&plan("ror", 28), budget()).unwrap().outcome {
        RawOutcome::Crashed {
            kind,
            message,
            tier,
            ..
        } => {
            assert_eq!(kind, "panic");
            assert!(
                message.contains("calc.Sum") && message.contains("calc.go:29"),
                "{message}"
            );
            assert!(message.contains("calc.TestSum"), "{message}");
            assert_eq!(tier, Tier::P);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_a_go_test_that_panics_is_still_named_among_the_failed() {
    // Go prints `--- FAIL` before it re-panics, and Delve stops at the
    // panic after that: the crash names its test.
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    let observed = exec.run(&plan("ror", 28), budget()).unwrap();
    assert!(
        matches!(observed.outcome, RawOutcome::Crashed { .. }),
        "{observed:?}"
    );
    let names: Vec<&str> = observed.tests.iter().map(|t| t.test.as_str()).collect();
    assert_eq!(names, ["TestSum"], "{observed:?}");
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_dropping_a_session_stops_delve() {
    let s = DelveSession::start(&config()).unwrap();
    let pid = s.pid();
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    drop(s);
    let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| !s.contains(") Z "))
        .unwrap_or(false);
    assert!(!alive, "delve {pid} still running after drop");
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_looping_mutant_times_out_and_is_torn_down_promptly() {
    // `i = i - 1` in CountUp never ends the loop.
    let mut exec = DelveExecutor::new(config(), Isa::amd64());
    let start = std::time::Instant::now();
    let raw = exec
        .run(
            &plan("aor", 21),
            Budget {
                timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
    assert!(
        matches!(raw.outcome, RawOutcome::TimedOut { .. }),
        "{raw:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(8),
        "teardown took {:?}",
        start.elapsed()
    );
}

fn gone(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |s| s.contains(") Z "))
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_dropping_a_timed_out_session_kills_its_looping_debuggee() {
    let mut s = DelveSession::start(&config()).unwrap();
    let debuggee = s.debuggee_pid();
    let file = fixture().join("calc.go");
    let code = s.function_code(&file, 21).unwrap().unwrap();
    let patch = derive(&plan("aor", 21), &code.file, &code.insns, &Isa::amd64()).unwrap();
    s.arm(&patch).unwrap();
    assert_eq!(
        s.run_to_exit(Duration::from_secs(1)).unwrap(),
        RunEnd::TimedOut
    );
    drop(s);
    std::thread::sleep(Duration::from_millis(300));
    assert!(gone(debuggee), "debuggee {debuggee} still running");
}

fn selecting(test: &str) -> DelveConfig {
    DelveConfig {
        test_args: cherenkov_exec_dap::toolchain::go_test_args(&[test.to_string()]),
        ..config()
    }
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_delve_mutant_the_selected_test_never_executes_is_not_reached() {
    // TestTotal never calls IsSmall: its mutant must not "survive".
    let mut exec = DelveExecutor::new(selecting("TestTotal"), Isa::amd64());
    let raw = exec
        .run(&plan("negate-condition", 12), budget())
        .unwrap()
        .outcome;
    assert!(matches!(raw, RawOutcome::NotReached { .. }), "{raw:?}");
}

#[test]
#[ignore = "slow: needs dlv and go"]
fn test_live_delve_reached_mutant_is_judged_by_the_selected_test() {
    let mut exec = DelveExecutor::new(selecting("TestIsSmall"), Isa::amd64());
    let raw = exec
        .run(&plan("negate-condition", 12), budget())
        .unwrap()
        .outcome;
    assert!(
        matches!(raw, RawOutcome::Exited { code, .. } if code != 0),
        "negation is killed: {raw:?}"
    );
}
