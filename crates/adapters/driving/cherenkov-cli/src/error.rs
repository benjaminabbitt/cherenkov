//! Why a command could not do what was asked: coded, with named data
//! (the messages are `locales/en-US/messages.ftl`'s), and the exit each
//! ends the process with.

use std::fmt;

use angzarr_diagnostic::{Arg, Diagnostic, Report, code};
use cli_support::{Exit, ExitClass};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliError {
    /// No operator set has this name.
    UnknownOperatorSet { name: String, known: String },
    /// A custom operator set's file is not a set (not TOML of the right
    /// shape, or it names a mutation or skip kind that does not exist).
    BadOperatorSet { path: String, reason: String },
    /// The directory holds no project a run can execute.
    NoProject { dir: String },
    /// A tool the run needs is not where it was looked for.
    ToolMissing { tool: String },
    /// A file or directory could not be read (or a pattern for one is not
    /// valid).
    Unreadable { path: String, reason: String },
    /// The parsers did not load.
    Syntax { reason: String },
    /// The project's sources could not be planned.
    Planning { reason: String },
    /// The toolchain could not build or describe the project's tests.
    Toolchain { reason: String },
    /// The run could not run.
    Run { cause: Report },
    /// Another process holds the store a run or a prune was asked to use:
    /// `holder` says who.
    StoreLocked { path: String, holder: String },
    /// `store unlock` was asked of a store a process still holds.
    StoreHeld { path: String, holder: String },
    /// There is no store at the path a prune was asked to use.
    NoStore { path: String },
}

impl CliError {
    /// The run layer's `cause`, captured.
    pub fn run(cause: &dyn Diagnostic) -> CliError {
        CliError::Run {
            cause: Report::of(cause),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for CliError {}

impl Diagnostic for CliError {
    fn code(&self) -> &str {
        match self {
            CliError::UnknownOperatorSet { .. } => code!("unknown-operator-set"),
            CliError::BadOperatorSet { .. } => code!("bad-operator-set"),
            CliError::NoProject { .. } => code!("no-project"),
            CliError::ToolMissing { .. } => code!("tool-missing"),
            CliError::Unreadable { .. } => code!("unreadable"),
            CliError::Syntax { .. } => code!("syntax"),
            CliError::Planning { .. } => code!("planning"),
            CliError::Toolchain { .. } => code!("toolchain"),
            CliError::Run { .. } => code!("run"),
            CliError::StoreLocked { .. } => code!("store-locked"),
            CliError::StoreHeld { .. } => code!("store-held"),
            CliError::NoStore { .. } => code!("no-store"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            CliError::UnknownOperatorSet { name, known } => {
                vec![
                    ("name", name.as_str().into()),
                    ("known", known.as_str().into()),
                ]
            }
            CliError::NoProject { dir } => vec![("dir", dir.as_str().into())],
            CliError::ToolMissing { tool } => vec![("tool", tool.as_str().into())],
            CliError::BadOperatorSet { path, reason } | CliError::Unreadable { path, reason } => {
                vec![
                    ("path", path.as_str().into()),
                    ("reason", reason.as_str().into()),
                ]
            }
            CliError::Syntax { reason }
            | CliError::Planning { reason }
            | CliError::Toolchain { reason } => vec![("reason", reason.as_str().into())],
            CliError::Run { cause } => vec![("cause", Arg::nested(cause))],
            CliError::StoreLocked { path, holder } | CliError::StoreHeld { path, holder } => {
                vec![
                    ("path", path.as_str().into()),
                    ("holder", holder.as_str().into()),
                ]
            }
            CliError::NoStore { path } => vec![("path", path.as_str().into())],
        }
    }
}

impl ExitClass for CliError {
    /// A request the state forbids (no such set, no project, no tool) is
    /// declined; anything that broke on the way failed.
    fn exit(&self) -> Exit {
        match self {
            CliError::UnknownOperatorSet { .. }
            | CliError::BadOperatorSet { .. }
            | CliError::NoProject { .. }
            | CliError::ToolMissing { .. }
            | CliError::StoreLocked { .. }
            | CliError::StoreHeld { .. }
            | CliError::NoStore { .. } => Exit::REFUSED,
            _ => Exit::FAILED,
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let reason = || "r".to_string();
    vec![
        Box::new(CliError::UnknownOperatorSet {
            name: "nope".into(),
            known: "breadth".into(),
        }),
        Box::new(CliError::BadOperatorSet {
            path: "p".into(),
            reason: reason(),
        }),
        Box::new(CliError::NoProject { dir: "d".into() }),
        Box::new(CliError::ToolMissing { tool: "gdb".into() }),
        Box::new(CliError::Unreadable {
            path: "p".into(),
            reason: reason(),
        }),
        Box::new(CliError::Syntax { reason: reason() }),
        Box::new(CliError::Planning { reason: reason() }),
        Box::new(CliError::Toolchain { reason: reason() }),
        Box::new(CliError::Run {
            cause: Report::new("cherenkov-run-application-runtime").arg("reason", "r"),
        }),
        Box::new(CliError::StoreLocked {
            path: "p".into(),
            holder: "pid 7".into(),
        }),
        Box::new(CliError::StoreHeld {
            path: "p".into(),
            holder: "pid 7".into(),
        }),
        Box::new(CliError::NoStore { path: "p".into() }),
    ]
}

#[cfg(test)]
#[path = "error.test.rs"]
mod tests;
