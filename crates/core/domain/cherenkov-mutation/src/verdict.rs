//! Verdict classes (design section 7).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Killed,
    Survived,
    Timeout,
    NotReached,
    Unevaluable,
    EvaluatorRaised,
    UnsupportedByExecutor,
    /// The mutated source would not compile, as cargo-mutants calls it:
    /// there is no such program for the tests to run.
    Unviable,
}

impl Verdict {
    pub const ALL: [Verdict; 8] = [
        Verdict::Killed,
        Verdict::Survived,
        Verdict::Timeout,
        Verdict::NotReached,
        Verdict::Unevaluable,
        Verdict::EvaluatorRaised,
        Verdict::UnsupportedByExecutor,
        Verdict::Unviable,
    ];

    /// Whether the verdict enters the mutation score.
    pub fn counts_in_score(self) -> bool {
        matches!(self, Verdict::Killed | Verdict::Survived | Verdict::Timeout)
    }

    /// Whether the verdict counts as detected by the tests.
    pub fn detected(self) -> bool {
        matches!(self, Verdict::Killed | Verdict::Timeout)
    }
}
