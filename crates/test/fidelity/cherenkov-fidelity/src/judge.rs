//! Does a run-time mutant behave like its compiled source mutant? Judged per
//! test: the source mutant is the ground truth (docs/mutation-operators.md,
//! "How mutants are judged").

use std::collections::BTreeMap;

use cherenkov_exec_dap::libtest::TestOutcome;

/// How one run of the test programs ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// The test programs finished; each test's outcome.
    Tests(BTreeMap<String, TestOutcome>),
    /// Stopped at the time limit.
    TimedOut,
    /// A test program died (a fatal signal), with what it said about it.
    Crashed(String),
    /// The source mutant failed to build: no behaviour to compare.
    DoesNotCompile,
    /// The debugger judged the mutant unviable without running it: the
    /// source must then not compile either.
    Unviable,
}

impl Run {
    fn ending(&self) -> String {
        match self {
            Run::Tests(_) => "tests reported".into(),
            Run::TimedOut => "timed out".into(),
            Run::Crashed(kind) => format!("crashed ({kind})"),
            Run::DoesNotCompile => "does not compile".into(),
            Run::Unviable => "judged unviable without a run".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disagreement {
    /// One test, different outcomes.
    Test {
        test: String,
        debugger: TestOutcome,
        source: TestOutcome,
    },
    /// A test only one run reported.
    Missing { test: String, from_debugger: bool },
    /// The runs ended differently (timeout, crash, tests reported).
    Ending { debugger: String, source: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    Faithful,
    Unfaithful(Vec<Disagreement>),
    /// The source mutant does not compile, so there is nothing to judge.
    Unviable,
}

pub fn judge(debugger: &Run, source: &Run) -> Judgement {
    match (debugger, source) {
        (_, Run::DoesNotCompile) => Judgement::Unviable,
        (Run::Unviable, compiled) => Judgement::Unfaithful(vec![Disagreement::Ending {
            debugger: Run::Unviable.ending(),
            source: compiled.ending(),
        }]),
        (Run::Tests(d), Run::Tests(s)) => {
            let mut out = Vec::new();
            for (test, dv) in d {
                match s.get(test) {
                    Some(sv) if sv == dv => {}
                    Some(sv) => out.push(Disagreement::Test {
                        test: test.clone(),
                        debugger: *dv,
                        source: *sv,
                    }),
                    None => out.push(Disagreement::Missing {
                        test: test.clone(),
                        from_debugger: false,
                    }),
                }
            }
            for test in s.keys().filter(|t| !d.contains_key(*t)) {
                out.push(Disagreement::Missing {
                    test: test.clone(),
                    from_debugger: true,
                });
            }
            if out.is_empty() {
                Judgement::Faithful
            } else {
                Judgement::Unfaithful(out)
            }
        }
        (Run::TimedOut, Run::TimedOut) => Judgement::Faithful,
        (Run::Crashed(_), Run::Crashed(_)) => Judgement::Faithful,
        (d, s) => Judgement::Unfaithful(vec![Disagreement::Ending {
            debugger: d.ending(),
            source: s.ending(),
        }]),
    }
}
