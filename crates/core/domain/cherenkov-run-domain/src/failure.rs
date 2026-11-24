//! The run domain's own reasons for a failure it records, coded.

use std::fmt;

use angzarr_diagnostic::{Arg, Diagnostic, code};

/// Why the run domain failed a program or a run itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Failure {
    /// A reason recorded as text, before reasons were coded: carried so
    /// that a reader holds a diagnostic whatever the stream recorded.
    Recorded { reason: String },
    /// The unmutated tests of a program do not pass under the debugger.
    BaselineFailed { outcome: String, failing: String },
    /// [`Failure::BaselineFailed`], for a program that has a name.
    ProgramBaselineFailed {
        program: String,
        outcome: String,
        failing: String,
    },
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for Failure {}

impl Diagnostic for Failure {
    fn code(&self) -> &str {
        match self {
            Failure::Recorded { .. } => code!("recorded-reason"),
            Failure::BaselineFailed { .. } => code!("baseline-failed"),
            Failure::ProgramBaselineFailed { .. } => code!("program-baseline-failed"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Failure::Recorded { reason } => vec![("reason", reason.as_str().into())],
            Failure::BaselineFailed { outcome, failing } => vec![
                ("outcome", outcome.as_str().into()),
                ("failing", failing.as_str().into()),
            ],
            Failure::ProgramBaselineFailed {
                program,
                outcome,
                failing,
            } => vec![
                ("program", program.as_str().into()),
                ("outcome", outcome.as_str().into()),
                ("failing", failing.as_str().into()),
            ],
        }
    }
}

/// One value of every failure, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(Failure::Recorded { reason: "r".into() }),
        Box::new(Failure::BaselineFailed {
            outcome: "o".into(),
            failing: "f".into(),
        }),
        Box::new(Failure::ProgramBaselineFailed {
            program: "p".into(),
            outcome: "o".into(),
            failing: "f".into(),
        }),
    ]
}
