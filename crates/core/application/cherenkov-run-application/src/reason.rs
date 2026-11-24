//! The reasons the run layer gives for what it did not or could not do,
//! coded: the workers', the preparation's and the abandoning of a run.

use std::fmt;

use coded_diagnostic::{Arg, Diagnostic, code};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The run was handed no workers.
    NoWorkers,
    /// The first worker holds no test program.
    NoTestPrograms,
    /// Preparing was handed no executors.
    NoExecutors,
    /// A test program's preparation panicked.
    PreparationPanicked,
    /// A worker panicked running a mutant.
    WorkerPanicked,
    /// A worker has no test program of this index.
    NoProgram { program: u64 },
    /// A run prepared no test program.
    NonePrepared { run: String },
    /// A mutant was planned with no plan.
    NoPlan,
    /// A mutant was decided with no outcome.
    NoOutcome { mutant: String },
    /// A run's exit status says it passed, and its output reports failed
    /// tests.
    ExitDisagrees { tests: String },
    /// A program's tests are not recorded because its baseline did not pass.
    BaselineFailed,
    /// A later run of the same plan was resumed in this run's place.
    Resumed,
    /// The project's plan is not the one the run was planned with.
    PlanChanged,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Reason {}

impl Diagnostic for Reason {
    fn code(&self) -> &str {
        match self {
            Reason::NoWorkers => code!("no-workers"),
            Reason::NoTestPrograms => code!("no-test-programs"),
            Reason::NoExecutors => code!("no-executors"),
            Reason::PreparationPanicked => code!("preparation-panicked"),
            Reason::WorkerPanicked => code!("worker-panicked"),
            Reason::NoProgram { .. } => code!("no-program"),
            Reason::NonePrepared { .. } => code!("none-prepared"),
            Reason::NoPlan => code!("no-plan"),
            Reason::NoOutcome { .. } => code!("no-outcome"),
            Reason::ExitDisagrees { .. } => code!("exit-disagrees"),
            Reason::BaselineFailed => code!("baseline-failed"),
            Reason::Resumed => code!("resumed"),
            Reason::PlanChanged => code!("plan-changed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Reason::NoProgram { program } => vec![("program", (*program).into())],
            Reason::NonePrepared { run } => vec![("run", run.as_str().into())],
            Reason::NoOutcome { mutant } => vec![("mutant", mutant.as_str().into())],
            Reason::ExitDisagrees { tests } => vec![("tests", tests.as_str().into())],
            _ => Vec::new(),
        }
    }
}

/// One value of every reason, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(Reason::NoWorkers),
        Box::new(Reason::NoTestPrograms),
        Box::new(Reason::NoExecutors),
        Box::new(Reason::PreparationPanicked),
        Box::new(Reason::WorkerPanicked),
        Box::new(Reason::NoProgram { program: 2 }),
        Box::new(Reason::NonePrepared { run: "r".into() }),
        Box::new(Reason::NoPlan),
        Box::new(Reason::NoOutcome { mutant: "m".into() }),
        Box::new(Reason::ExitDisagrees {
            tests: "a, b".into(),
        }),
        Box::new(Reason::BaselineFailed),
        Box::new(Reason::Resumed),
        Box::new(Reason::PlanChanged),
    ]
}
