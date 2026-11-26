//! The executor port: what runs a mutant. An executor applies one plan, runs
//! the tests and reports raw facts (`cherenkov_mutation::execution`); only the
//! core decides verdicts.

use cherenkov_mutation::execution::{Budget, Observed};
use cherenkov_mutation::plan::MutantPlan;
use std::sync::Arc;

use coded_diagnostic::{Arg, Diagnostic, Report, code};

/// An executor applies one plan, runs the tests, and reports facts.
/// Errors are infrastructure failures (the debugger or harness broke), not
/// mutant outcomes, and never count in the score.
pub trait MutantExecutor {
    /// Run the plan's mutant: what the process did and the tests it
    /// reported ([`Observed::bare`] for a backend that cannot read them).
    fn run(&mut self, plan: &MutantPlan, budget: Budget) -> Result<Observed, ExecError>;
    /// The unmutated reference run under the same backend.
    fn baseline(&mut self, budget: Budget) -> Result<Observed, ExecError>;
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
                    why: Report::new(code!("no-recording")),
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
    /// A handle that ends this executor's session from another thread
    /// while a call is in flight. A backend with nothing to end hands back
    /// [`NoCancel`].
    fn canceller(&self) -> Arc<dyn Cancel> {
        Arc::new(NoCancel)
    }
}

/// Ends an executor's session from another thread, at once.
pub trait Cancel: Send + Sync {
    /// Idempotent. The call the owner is blocked in returns an error soon after;
    /// every later call on the executor fails without starting a session.
    fn cancel(&self);
}

/// A [`Cancel`] that does nothing: for an executor with no session to end.
pub struct NoCancel;

impl Cancel for NoCancel {
    fn cancel(&self) {}
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
    /// Every mutant runs every test, for `why`.
    EveryTest { why: Report },
}

/// Why an executor failed: what failed, captured. It names no recovery;
/// the layers closer to the user add one if they have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError {
    pub cause: Report,
}

impl ExecError {
    /// The failure `cause` reports.
    pub fn of(cause: &dyn Diagnostic) -> ExecError {
        ExecError {
            cause: Report::of(cause),
        }
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for ExecError {}

/// An executor's failure is its cause: the same code and args, so what
/// holds one holds the cause's own message.
impl Diagnostic for ExecError {
    fn code(&self) -> &str {
        self.cause.code()
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        self.cause.args()
    }
}
