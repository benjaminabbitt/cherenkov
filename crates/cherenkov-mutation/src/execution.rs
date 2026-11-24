//! Running mutants: the executor port and the pure classification of what
//! an executor observed into verdicts (design sections 4.1 and 7).
//! Executors report facts; only the core decides verdicts.

use std::time::Duration;

use serde::Serialize;

use crate::plan::MutantPlan;
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

/// An executor applies one plan, runs the tests, and reports facts.
/// Errors are infrastructure failures (the debugger or harness broke), not
/// mutant outcomes, and never count in the score.
pub trait MutantExecutor {
    fn run(&mut self, plan: &MutantPlan, budget: Budget) -> Result<RawOutcome, ExecError>;
    /// The unmutated reference run under the same backend.
    fn baseline(&mut self, budget: Budget) -> Result<RawOutcome, ExecError>;
    /// [`MutantExecutor::run`], with the test program's output, so a kill
    /// can be explained by the failing test's report.
    fn run_with_output(
        &mut self,
        plan: &MutantPlan,
        budget: Budget,
    ) -> Result<(RawOutcome, String), ExecError> {
        self.run(plan, budget).map(|raw| (raw, String::new()))
    }
    /// [`MutantExecutor::baseline`], with the test program's output.
    fn baseline_with_output(&mut self, budget: Budget) -> Result<(RawOutcome, String), ExecError> {
        self.baseline(budget).map(|raw| (raw, String::new()))
    }
    /// For each plan, which of the selected tests call the function it is
    /// in: the code they drive, from one unmutated run. `None` when this
    /// backend cannot tell (every plan is then run with every test).
    fn driven(
        &mut self,
        _plans: &[MutantPlan],
        _budget: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        Ok(None)
    }
    /// [`MutantExecutor::driven`], with how it was found: how many tests
    /// the program has and whether the recording could select tests.
    fn recording(&mut self, plans: &[MutantPlan], budget: Budget) -> Result<Recording, ExecError> {
        let driven = self.driven(plans, budget)?;
        Ok(Recording {
            tests: None,
            selection: match driven {
                Some(_) => Selection::Recorded,
                None => Selection::EveryTest {
                    reason: "this executor does not record which tests call what".to_string(),
                },
            },
            driven,
        })
    }
    /// Run the next mutants with only these tests (by the runner's names);
    /// `None` returns to the selected ones. A backend that cannot select
    /// keeps running them all.
    fn select_tests(&mut self, _tests: Option<&[String]>) {}
    /// What this executor runs (a test binary's path), for messages.
    fn describe(&self) -> Option<String> {
        None
    }
    /// End this executor's debugger session, if one is open: it frees the
    /// debugger's memory. The next run starts a fresh one.
    fn close_session(&mut self) {}
}

/// Which tests call a plan's function, as one unmutated run saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Driven {
    /// No selected test calls it: the mutant says nothing about them.
    NotCalled,
    /// These tests call it, and only they can detect the mutant.
    Tests(Vec<String>),
    /// Called by every test, as far as the run could tell (a hot function,
    /// or a call where no test was on the stack).
    Every,
}

/// What one unmutated recording run found: `driven` is none exactly when
/// `selection` is [`Selection::EveryTest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// The program's tests, when the executor can list them.
    pub tests: Option<usize>,
    pub selection: Selection,
    /// For each plan, which tests call its function.
    pub driven: Option<Vec<Driven>>,
}

/// How a program's mutants choose their tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Each mutant runs the tests the recording saw call its function.
    Recorded,
    /// As recorded, except that these tests failed during the recording
    /// and run every mutant.
    RecordedWithFailures { failed_tests: Vec<String> },
    /// Every mutant runs every test, for `reason`.
    EveryTest { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError(pub String);

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExecError {}
