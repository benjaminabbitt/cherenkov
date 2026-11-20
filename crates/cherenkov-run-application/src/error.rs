//! Why the run layer could not run a run: coded, with named data, never
//! English (the messages are the catalog's).

use std::fmt;

use angzarr_diagnostic::{Arg, Diagnostic, Report, code};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunLayerError {
    /// The kernel or its store failed, or refused what the run sent it.
    Kernel { cause: Report },
    /// The run's kernel or bus did not take what the run registers.
    Misconfigured { cause: Report },
    /// The run's async runtime did not start.
    Runtime { reason: String },
    /// The workers could not run the mutants (a test program that does
    /// not prepare, a debugger missing).
    Workers { reason: String },
    /// Steps a call set off were dead-lettered, never recorded.
    Undelivered { letters: u64 },
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

impl fmt::Display for RunLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for RunLayerError {}

impl Diagnostic for RunLayerError {
    fn code(&self) -> &str {
        match self {
            RunLayerError::Kernel { .. } => code!("kernel"),
            RunLayerError::Misconfigured { .. } => code!("misconfigured"),
            RunLayerError::Runtime { .. } => code!("runtime"),
            RunLayerError::Workers { .. } => code!("workers"),
            RunLayerError::Undelivered { .. } => code!("undelivered"),
            RunLayerError::Stalled { .. } => code!("stalled"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            RunLayerError::Kernel { cause } | RunLayerError::Misconfigured { cause } => {
                vec![("cause", Arg::nested(cause))]
            }
            RunLayerError::Runtime { reason } | RunLayerError::Workers { reason } => {
                vec![("reason", reason.as_str().into())]
            }
            RunLayerError::Undelivered { letters } => vec![("letters", (*letters).into())],
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
/// crate's, the domain's refusals, and the kernel's and its store's for a
/// nested cause.
pub const CATALOG_SOURCES: [(&str, &str); 4] = [
    EN_US,
    cherenkov_run_domain::EN_US,
    angzarr_embedded::EN_US,
    angzarr_embedded::angzarr_ports::EN_US,
];

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let cause = || Report::new("angzarr-embedded-x");
    vec![
        Box::new(RunLayerError::Kernel { cause: cause() }),
        Box::new(RunLayerError::Misconfigured { cause: cause() }),
        Box::new(RunLayerError::Runtime { reason: "r".into() }),
        Box::new(RunLayerError::Workers { reason: "r".into() }),
        Box::new(RunLayerError::Undelivered { letters: 2 }),
        Box::new(RunLayerError::Stalled { owed: 3 }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
