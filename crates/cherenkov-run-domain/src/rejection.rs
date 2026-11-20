//! Why an aggregate refused a command, and why a snapshot was refused.

use std::fmt;

use angzarr_diagnostic::{Arg, Diagnostic, code};

/// Why the run refused a command.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RunRejection {
    /// A plan for a run already planned.
    AlreadyPlanned,
}

impl fmt::Display for RunRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for RunRejection {
    fn code(&self) -> &str {
        match self {
            RunRejection::AlreadyPlanned => code!("already-planned"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

/// Why a mutant refused a command.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MutantRejection {
    /// A plan for no run.
    NoRun,
    /// A plan for no mutant.
    NoMutant,
    /// A plan for a run that already planned the mutant.
    PlannedForRun,
}

impl fmt::Display for MutantRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for MutantRejection {
    fn code(&self) -> &str {
        match self {
            MutantRejection::NoRun => code!("no-run"),
            MutantRejection::NoMutant => code!("no-mutant"),
            MutantRejection::PlannedForRun => code!("planned-for-run"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

/// Why a snapshot was refused: it holds what no stream folds to, so it was
/// not written from one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SnapshotRejection {
    /// A run's undecided mutants not in strictly ascending order.
    UndecidedUnordered,
    /// A run's tallies not in strictly ascending order of verdict.
    TalliesUnordered,
    /// A run's prepared programs not in strictly ascending order.
    PreparedUnordered,
    /// A run with undecided mutants, tallies or a finish, and no start.
    Unstarted,
    /// A mutant's runs not in strictly ascending order of their keys.
    RunsUnordered,
    /// One of a mutant's runs without the mutant as it was planned.
    Unplanned,
    /// One of a mutant's runs with a program's run twice.
    RanRepeated,
}

impl fmt::Display for SnapshotRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl Diagnostic for SnapshotRejection {
    fn code(&self) -> &str {
        match self {
            SnapshotRejection::UndecidedUnordered => code!("undecided-unordered"),
            SnapshotRejection::TalliesUnordered => code!("tallies-unordered"),
            SnapshotRejection::PreparedUnordered => code!("prepared-unordered"),
            SnapshotRejection::Unstarted => code!("unstarted"),
            SnapshotRejection::RunsUnordered => code!("runs-unordered"),
            SnapshotRejection::Unplanned => code!("unplanned"),
            SnapshotRejection::RanRepeated => code!("ran-repeated"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        Vec::new()
    }
}

/// Why a recorded plan or observation is not one the planner's types hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Unreadable {
    /// A field every plan has is not set.
    Missing {
        /// The field, by its path in the plan.
        field: &'static str,
    },
    /// An operator or a mutation the catalog does not have.
    Unknown {
        /// What kind of name it is.
        kind: &'static str,
        /// The name.
        name: String,
    },
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for Unreadable {}

impl Diagnostic for Unreadable {
    fn code(&self) -> &str {
        match self {
            Unreadable::Missing { .. } => code!("unreadable-missing"),
            Unreadable::Unknown { .. } => code!("unreadable-unknown"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Unreadable::Missing { field } => vec![("field", (*field).into())],
            Unreadable::Unknown { kind, name } => {
                vec![("kind", (*kind).into()), ("name", name.as_str().into())]
            }
        }
    }
}

/// A stream root that cannot be derived: the derivation refuses the
/// domain.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnderivableRoot {
    /// The domain whose stream it was.
    pub domain: String,
    /// What the root was to be derived from.
    pub key: String,
}

impl fmt::Display for UnderivableRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for UnderivableRoot {}

impl Diagnostic for UnderivableRoot {
    fn code(&self) -> &str {
        code!("underivable-root")
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        vec![
            ("domain", self.domain.as_str().into()),
            ("key", self.key.as_str().into()),
        ]
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every rejection, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(RunRejection::AlreadyPlanned),
        Box::new(MutantRejection::NoRun),
        Box::new(MutantRejection::NoMutant),
        Box::new(MutantRejection::PlannedForRun),
        Box::new(SnapshotRejection::UndecidedUnordered),
        Box::new(SnapshotRejection::TalliesUnordered),
        Box::new(SnapshotRejection::PreparedUnordered),
        Box::new(SnapshotRejection::Unstarted),
        Box::new(SnapshotRejection::RunsUnordered),
        Box::new(SnapshotRejection::Unplanned),
        Box::new(SnapshotRejection::RanRepeated),
        Box::new(Unreadable::Missing { field: "action" }),
        Box::new(Unreadable::Unknown {
            kind: "operator",
            name: "n".to_string(),
        }),
        Box::new(UnderivableRoot {
            domain: "d".to_string(),
            key: "k".to_string(),
        }),
    ]
}
