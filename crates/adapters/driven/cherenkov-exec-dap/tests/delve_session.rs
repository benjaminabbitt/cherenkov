//! Delve session behaviour that needs no real Delve.

use coded_diagnostic::Diagnostic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cherenkov_exec_dap::delve::{DelveConfig, DelveSession, RunEnd, outcome_of};
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};

#[test]
fn test_start_fails_fast_when_the_debugger_exits_without_listening() {
    let config = DelveConfig {
        dlv: PathBuf::from("/bin/false"),
        test_binary: PathBuf::from("/nonexistent"),
        test_args: Vec::new(),
    };
    let start = Instant::now();
    let err = DelveSession::start(&config).err().expect("start must fail");
    assert_eq!(err.code(), "cherenkov-exec-dap-session-not-listening");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

fn budget(ms: u64) -> Budget {
    Budget {
        timeout: Duration::from_millis(ms),
    }
}

#[test]
fn test_outcome_within_budget_keeps_how_the_run_ended() {
    let wall = Duration::from_millis(40);
    assert_eq!(
        outcome_of(RunEnd::Exited(1), wall, budget(100)),
        RawOutcome::Exited {
            code: 1,
            tier: Tier::P,
            wall
        }
    );
    assert_eq!(
        outcome_of(
            RunEnd::Crashed {
                kind: "panic".into(),
                message: "m".into()
            },
            wall,
            budget(100)
        ),
        RawOutcome::Crashed {
            kind: "panic".into(),
            message: "m".into(),
            tier: Tier::P,
            wall
        }
    );
    assert_eq!(
        outcome_of(RunEnd::TimedOut, wall, budget(100)),
        RawOutcome::TimedOut {
            tier: Tier::P,
            wall
        }
    );
}

#[test]
fn test_outcome_past_budget_is_a_timeout_however_the_run_ended() {
    let wall = Duration::from_millis(150);
    for end in [
        RunEnd::Exited(0),
        RunEnd::Crashed {
            kind: "panic".into(),
            message: String::new(),
        },
    ] {
        assert_eq!(
            outcome_of(end, wall, budget(100)),
            RawOutcome::TimedOut {
                tier: Tier::P,
                wall
            }
        );
    }
    let at_limit = Duration::from_millis(100);
    assert_eq!(
        outcome_of(RunEnd::Exited(0), at_limit, budget(100)),
        RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: at_limit
        }
    );
}
