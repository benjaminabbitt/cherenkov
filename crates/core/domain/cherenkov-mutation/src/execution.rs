//! Running mutants: the pure classification of what an executor observed
//! into verdicts (design sections 4.1 and 7). The executor port is
//! `cherenkov_run_ports::executor`. Executors report facts; only the core
//! decides verdicts.

use std::time::Duration;

use serde::Serialize;

use crate::verdict::Verdict;

/// How a mutant was applied (design section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// Pure DAP: evaluate plus goto.
    A,
    /// DAP plus a native escape for jumps and returns.
    B,
    /// Instruction patch, armed once.
    P,
}

/// Limits for one mutant run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub timeout: Duration,
}

impl Budget {
    /// Ten times the unmutated run under the same backend, plus five
    /// seconds of slack for debugger startup jitter.
    pub fn from_baseline(baseline: Duration) -> Budget {
        Budget {
            timeout: baseline * 10 + Duration::from_secs(5),
        }
    }
}

impl Budget {
    /// One mutant's run: five times the unmutated run, never under two
    /// seconds (the debugger's own overhead). A mutant over it is hung, and
    /// is killed by timeout: the unmutated program does not take that long.
    pub fn for_mutant(baseline: Duration) -> Budget {
        Budget {
            timeout: (baseline * 5).max(Duration::from_secs(2)),
        }
    }
}

impl Budget {
    /// The budget of a run that stops and evaluates at every hit of the
    /// mutated site (tier B): each hit costs the debugger's round trip, so a
    /// mutant that merely runs a hot site many times is not mistaken for a
    /// hung one.
    pub fn evaluating(self) -> Budget {
        Budget {
            timeout: self.timeout * 3,
        }
    }
}

/// What an executor observed. Facts only; no verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum RawOutcome {
    /// The test process ran to completion with this exit code.
    Exited {
        code: i32,
        tier: Tier,
        wall: Duration,
    },
    /// The debugger caught the process crashing (Go: unrecovered panic or
    /// fatal error) and stopped it there, before it could exit.
    Crashed {
        kind: String,
        message: String,
        tier: Tier,
        wall: Duration,
    },
    /// The run exceeded the budget and was killed.
    TimedOut { tier: Tier, wall: Duration },
    /// The mutated code is not in what this executor runs (for example, a
    /// test binary that does not link the function).
    NotReached { reason: String },
    /// The executor cannot apply this plan; `reason` says why.
    Unsupported { reason: String },
    /// The mutated source would not compile (a value of a type that has no
    /// such constructor); `reason` says why.
    Unviable { reason: String },
}

/// A verdict with the facts it was derived from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub verdict: Verdict,
    /// Every verdict records the tier that applied the mutant (design section 7).
    pub tier: Option<Tier>,
    pub detail: Option<String>,
}

/// What an executor observed of one run: the outcome of the process, and
/// the tests the program reported (none when the executor cannot read
/// them). Plain data; only the core decides what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub outcome: RawOutcome,
    pub tests: Vec<TestReport>,
}

impl Observed {
    /// An outcome with no test reports.
    pub fn bare(outcome: RawOutcome) -> Observed {
        Observed {
            outcome,
            tests: Vec::new(),
        }
    }

    /// Whether any test reports as failed.
    pub fn failed(&self) -> bool {
        self.tests.iter().any(TestReport::is_failed)
    }

    /// The reports of the tests that failed, in the order reported.
    pub fn failures(&self) -> Vec<TestReport> {
        self.tests
            .iter()
            .filter(|t| t.is_failed())
            .cloned()
            .collect()
    }
}

impl From<RawOutcome> for Observed {
    fn from(outcome: RawOutcome) -> Observed {
        Observed::bare(outcome)
    }
}

/// One test's result as the test program reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReport {
    pub test: String,
    pub result: TestResult,
}

impl TestReport {
    fn is_failed(&self) -> bool {
        matches!(self.result, TestResult::Failed(_))
    }

    /// `test at location: got X, expected Y`, or `test at location:
    /// message`, for a test that failed; `test: passed` or `test: ignored`.
    pub fn line(&self) -> String {
        let TestResult::Failed(failure) = &self.result else {
            let said = match self.result {
                TestResult::Ignored => "ignored",
                _ => "passed",
            };
            return format!("{}: {said}", self.test);
        };
        let at = failure
            .location
            .as_ref()
            .map(|l| format!(" at {l}"))
            .unwrap_or_default();
        match (&failure.got, &failure.expected) {
            (Some(got), Some(expected)) => {
                format!("{}{at}: got {got}, expected {expected}", self.test)
            }
            _ => format!("{}{at}: {}", self.test, failure.message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestResult {
    Passed,
    Ignored,
    Failed(Failure),
}

/// How a test failed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Failure {
    /// `file:line[:col]` of the failure.
    pub location: Option<String>,
    pub message: String,
    /// For an equality assertion, the value the mutant produced.
    pub got: Option<String>,
    /// For an equality assertion, the value the test expects.
    pub expected: Option<String>,
}

/// Exit code 0 is a pass; anything else (test failure, panic, crash) kills.
pub fn classify(raw: &RawOutcome) -> Outcome {
    match raw {
        RawOutcome::Exited { code: 0, tier, .. } => Outcome {
            verdict: Verdict::Survived,
            tier: Some(*tier),
            detail: None,
        },
        RawOutcome::Exited { code, tier, .. } => Outcome {
            verdict: Verdict::Killed,
            tier: Some(*tier),
            detail: Some(format!("exit code {code}")),
        },
        RawOutcome::Crashed {
            kind,
            message,
            tier,
            ..
        } => Outcome {
            verdict: Verdict::Killed,
            tier: Some(*tier),
            detail: Some(format!("{kind}: {message}")),
        },
        RawOutcome::TimedOut { tier, .. } => Outcome {
            verdict: Verdict::Timeout,
            tier: Some(*tier),
            detail: None,
        },
        RawOutcome::NotReached { reason } => Outcome {
            verdict: Verdict::NotReached,
            tier: None,
            detail: Some(reason.clone()),
        },
        RawOutcome::Unsupported { reason } => Outcome {
            verdict: Verdict::UnsupportedByExecutor,
            tier: None,
            detail: Some(reason.clone()),
        },
        RawOutcome::Unviable { reason } => Outcome {
            verdict: Verdict::Unviable,
            tier: None,
            detail: Some(reason.clone()),
        },
    }
}

/// One mutant's outcome across several test binaries, in run order. The
/// first detection (failure, crash, timeout) decides; otherwise an
/// unsupported binary makes the mutant unsupported; otherwise every binary
/// that reached the code passed, and their wall times add up; and if none
/// reached it, it is not reached.
pub fn combine(outcomes: Vec<RawOutcome>) -> RawOutcome {
    let detected = |o: &RawOutcome| match o {
        RawOutcome::Exited { code, .. } => *code != 0,
        RawOutcome::Crashed { .. } | RawOutcome::TimedOut { .. } => true,
        RawOutcome::NotReached { .. }
        | RawOutcome::Unsupported { .. }
        | RawOutcome::Unviable { .. } => false,
    };
    if let Some(first) = outcomes.iter().find(|o| detected(o)) {
        return first.clone();
    }
    if let Some(refused) = outcomes.iter().find(|o| {
        matches!(
            o,
            RawOutcome::Unsupported { .. } | RawOutcome::Unviable { .. }
        )
    }) {
        return refused.clone();
    }
    let passes: Vec<(Tier, Duration)> = outcomes
        .iter()
        .filter_map(|o| match o {
            RawOutcome::Exited { tier, wall, .. } => Some((*tier, *wall)),
            _ => None,
        })
        .collect();
    match passes.first() {
        Some(&(tier, _)) => RawOutcome::Exited {
            code: 0,
            tier,
            wall: passes.iter().map(|(_, w)| *w).sum(),
        },
        None => outcomes
            .into_iter()
            .next()
            .unwrap_or(RawOutcome::NotReached {
                reason: "no test binary".into(),
            }),
    }
}
