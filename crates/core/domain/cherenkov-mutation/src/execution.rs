//! Running mutants: the pure classification of what an executor observed
//! into verdicts (design sections 4.1 and 7). The executor port is
//! `cherenkov_run_ports::executor`. Executors report facts; only the core
//! decides verdicts.

use std::time::Duration;

use coded_diagnostic::{Arg, Diagnostic, code};

/// What an executor says of a mutant it did not run: the type of the
/// `why` of [`RawOutcome`]'s refusals.
pub use coded_diagnostic::Report;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// test binary that does not link the function); `why` says how.
    NotReached { why: Report },
    /// The executor cannot apply this plan; `why` says why.
    Unsupported { why: Report },
    /// The mutated source would not compile (a value of a type that has no
    /// such constructor); `why` says why.
    Unviable { why: Report },
}

/// Why a mutant was reached by no program: there was none to run it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreached {
    /// The mutant's outcomes are none: no test binary.
    NoTestBinary,
}

impl std::fmt::Display for Unreached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Unreached {}

impl Diagnostic for Unreached {
    fn code(&self) -> &str {
        match self {
            Unreached::NoTestBinary => code!("no-test-binary"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

/// A verdict with the facts it was derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    /// Every verdict records the tier that applied the mutant (design section 7).
    pub tier: Option<Tier>,
    /// How a mutant was killed: the exit code or the crash.
    pub detail: Option<String>,
    /// Why a mutant was not run, or not reached.
    pub why: Option<Report>,
}

/// What an executor observed of one run: the outcome of the process, and
/// the tests the program reported (none when the executor cannot read
/// them). Plain data; only the core decides what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub outcome: RawOutcome,
    pub tests: Vec<TestReport>,
    /// The last of the program's output (see [`output_tail`]), for a person
    /// diagnosing a failure. No verdict, test report or decision is read
    /// from it.
    pub output_tail: Option<String>,
}

/// How much of a program's output an observation keeps.
pub const OUTPUT_TAIL_BYTES: usize = 16 * 1024;

/// The last [`OUTPUT_TAIL_BYTES`] of `output`, cut on a character boundary
/// and led by a marker saying how many bytes were left out; none for no
/// output. For diagnosis only: nothing decides from it.
pub fn output_tail(_output: &str) -> Option<String> {
    None
}

impl Observed {
    /// An outcome with no test reports.
    pub fn bare(outcome: RawOutcome) -> Observed {
        Observed {
            outcome,
            tests: Vec::new(),
            output_tail: None,
        }
    }

    /// This observation, carrying the tail of the output it came from.
    pub fn with_output(mut self, output: &str) -> Observed {
        self.output_tail = output_tail(output);
        self
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
    /// Whether the test failed.
    pub fn is_failed(&self) -> bool {
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
            why: None,
        },
        RawOutcome::Exited { code, tier, .. } => Outcome {
            verdict: Verdict::Killed,
            tier: Some(*tier),
            detail: Some(format!("exit code {code}")),
            why: None,
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
            why: None,
        },
        RawOutcome::TimedOut { tier, .. } => Outcome {
            verdict: Verdict::Timeout,
            tier: Some(*tier),
            detail: None,
            why: None,
        },
        RawOutcome::NotReached { why } => Outcome {
            verdict: Verdict::NotReached,
            tier: None,
            detail: None,
            why: Some(why.clone()),
        },
        RawOutcome::Unsupported { why } => Outcome {
            verdict: Verdict::UnsupportedByExecutor,
            tier: None,
            detail: None,
            why: Some(why.clone()),
        },
        RawOutcome::Unviable { why } => Outcome {
            verdict: Verdict::Unviable,
            tier: None,
            detail: None,
            why: Some(why.clone()),
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
                why: Report::of(&Unreached::NoTestBinary),
            }),
    }
}
