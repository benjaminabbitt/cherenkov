//! Why the run layer could not run a run: coded, with named data, never
//! English (the messages are the catalog's).

use std::fmt;

use cherenkov_run_ports::EdgeError;
use coded_diagnostic::{Arg, Diagnostic, Report, code};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunLayerError {
    /// The kernel or its store failed, or refused what the run sent it.
    Kernel { cause: Report },
    /// The core could not take a call for now (its store down, a write that
    /// lost its race): worth making again.
    Unavailable { cause: Report },
    /// The run's store could not be opened (a file that is not a store, a
    /// directory that cannot be written).
    Store { cause: Report },
    /// The run's kernel or bus did not take what the run registers.
    Misconfigured { cause: Report },
    /// The run's async runtime did not start (the root's); `detail` is what
    /// the operating system said.
    Runtime { detail: String },
    /// The workers could not run the mutants (a test program that does
    /// not prepare, a debugger missing): `program` is the one that did not
    /// prepare, where the failure is a program's.
    Workers {
        program: Option<String>,
        cause: Report,
    },
    /// Nothing is left to serve, and the run has not finished: `owed`
    /// mutants are planned and undecided, ones the bus gave up on.
    Stalled { owed: u64 },
}

impl RunLayerError {
    /// The kernel's `cause`, captured.
    pub fn kernel(cause: &dyn Diagnostic) -> RunLayerError {
        RunLayerError::Kernel {
            cause: Report::of(cause),
        }
    }
}

/// What the core answered with a failure, as the run layer's: one worth
/// making again is `Unavailable`.
impl From<EdgeError> for RunLayerError {
    fn from(error: EdgeError) -> RunLayerError {
        match error.retryable() {
            true => RunLayerError::Unavailable { cause: error.cause },
            false => RunLayerError::Kernel { cause: error.cause },
        }
    }
}

impl fmt::Display for RunLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for RunLayerError {}

impl Diagnostic for RunLayerError {
    fn code(&self) -> &str {
        match self {
            RunLayerError::Kernel { .. } => code!("kernel"),
            RunLayerError::Unavailable { .. } => code!("unavailable"),
            RunLayerError::Store { .. } => code!("store"),
            RunLayerError::Misconfigured { .. } => code!("misconfigured"),
            RunLayerError::Runtime { .. } => code!("runtime"),
            RunLayerError::Workers { program: None, .. } => code!("workers"),
            RunLayerError::Workers {
                program: Some(_), ..
            } => code!("workers-program"),
            RunLayerError::Stalled { .. } => code!("stalled"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            RunLayerError::Kernel { cause }
            | RunLayerError::Unavailable { cause }
            | RunLayerError::Store { cause }
            | RunLayerError::Misconfigured { cause } => {
                vec![("cause", Arg::nested(cause))]
            }
            RunLayerError::Runtime { detail } => vec![("detail", detail.as_str().into())],
            RunLayerError::Workers {
                program: None,
                cause,
            } => vec![("cause", Arg::nested(cause))],
            RunLayerError::Workers {
                program: Some(program),
                cause,
            } => vec![
                ("program", program.as_str().into()),
                ("cause", Arg::nested(cause)),
            ],
            RunLayerError::Stalled { owed } => vec![("owed", (*owed).into())],
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// The en-US messages of every crate a run layer error can carry: this
/// crate's, the domain's refusals, the planner's and the syntax port's, and
/// the kernel's and its store's for a nested cause.
pub const CATALOG_SOURCES: [(&str, &str); 6] = [
    EN_US,
    cherenkov_mutation::error::EN_US,
    cherenkov_run_ports::syntax::EN_US,
    cherenkov_run_domain::EN_US,
    angzarr_embedded::EN_US,
    angzarr_embedded::angzarr_ports::EN_US,
];

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let cause = || Report::new("angzarr-embedded-x");
    let own: Vec<Box<dyn Diagnostic>> = vec![
        Box::new(crate::session::FileProblem::Unreadable { detail: "d".into() }),
        Box::new(crate::session::FileProblem::Analysis {
            cause: Report::new("cherenkov-run-ports-incomplete"),
        }),
        Box::new(crate::session::FileProblem::SyntaxErrors),
        Box::new(RunLayerError::Kernel { cause: cause() }),
        Box::new(RunLayerError::Unavailable { cause: cause() }),
        Box::new(RunLayerError::Store { cause: cause() }),
        Box::new(RunLayerError::Misconfigured { cause: cause() }),
        Box::new(RunLayerError::Runtime { detail: "r".into() }),
        Box::new(RunLayerError::Workers {
            program: None,
            cause: cause(),
        }),
        Box::new(RunLayerError::Workers {
            program: Some("p".into()),
            cause: cause(),
        }),
        Box::new(RunLayerError::Stalled { owed: 3 }),
    ];
    own.into_iter()
        .chain(crate::reason::diagnostic_samples())
        .collect()
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
