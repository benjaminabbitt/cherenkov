//! Check run-time mutants against their compiled source mutants, one by one.

use std::time::{Duration, Instant};

use cherenkov_core::execution::{Budget, Driven, MutantExecutor, RawOutcome, Tier, classify};
use cherenkov_core::plan::{MutantPlan, render_source};
use cherenkov_core::profile::Profile;

use crate::judge::{Judgement, Run, judge};
use crate::source::ScratchProject;
use cherenkov_exec_dap::libtest;

/// What checking one mutant found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Judged(Judgement),
    /// The executor has no way to run this mutant yet.
    Unsupported(String),
    /// The mutant has no source form to compile (an unknown default value).
    NoSourceForm,
}

/// The debugger side: one executor per test program, with its baseline.
pub struct Debugger {
    executors: Vec<(Box<dyn MutantExecutor>, Budget, Run)>,
    /// Per executor, which of its tests call each plan's function (by plan
    /// id), from its recording run; `None` runs every test.
    drives: Vec<Option<std::collections::HashMap<String, Driven>>>,
    /// The tier that applied each mutant (by plan id) in the last run.
    tiers: std::collections::HashMap<String, Tier>,
}

/// The first, unmutated run is given this long.
const FIRST_RUN: Duration = Duration::from_secs(120);

impl Debugger {
    /// Baseline every executor: its time budget, and its tests' outcomes for
    /// a program that does not contain the mutated code.
    pub fn new(executors: Vec<Box<dyn MutantExecutor>>) -> Result<Debugger, String> {
        let mut out = Vec::new();
        for mut ex in executors {
            let (raw, output) = ex
                .baseline_with_output(Budget::from_baseline(FIRST_RUN))
                .map_err(|e| e.to_string())?;
            let RawOutcome::Exited { code: 0, wall, .. } = raw else {
                return Err(format!(
                    "unmutated tests do not pass under the debugger: {raw:?}"
                ));
            };
            out.push((
                ex,
                Budget::from_baseline(wall),
                Run::Tests(libtest::parse(&output)),
            ));
        }
        let drives = vec![None; out.len()];
        Ok(Debugger {
            executors: out,
            drives,
            tiers: std::collections::HashMap::new(),
        })
    }

    /// Record, per test program, which tests call each plan's function, so
    /// each mutant runs as the CLI runs it: only where its function is
    /// called, with only those tests. The tests left out keep their
    /// baseline outcome, so a selection that drops a detecting test shows
    /// as a disagreement with the compiled mutant.
    pub fn record(&mut self, plans: &[MutantPlan]) -> Result<(), String> {
        for ((ex, budget, _), drives) in self.executors.iter_mut().zip(&mut self.drives) {
            *drives = ex
                .driven(plans, *budget)
                .map_err(|e| e.to_string())?
                .map(|d| plans.iter().map(|p| p.id.to_string()).zip(d).collect());
        }
        Ok(())
    }

    /// The tier that applied `plan` in the last test program that ran it;
    /// `None` when it was not applied (refused, unviable, not reached).
    pub fn tier(&self, plan: &MutantPlan) -> Option<Tier> {
        self.tiers.get(&plan.id.to_string()).copied()
    }

    /// Every test program's outcome under the run-time mutant.
    fn run(&mut self, plan: &MutantPlan) -> Result<Result<Run, String>, String> {
        let mut tests = std::collections::BTreeMap::new();
        for ((ex, budget, baseline), drives) in self.executors.iter_mut().zip(&self.drives) {
            let drive = drives
                .as_ref()
                .and_then(|d| d.get(&plan.id.to_string()).cloned())
                .unwrap_or(Driven::Every);
            if let Run::Tests(t) = baseline {
                tests.extend(t.clone());
            }
            match drive {
                Driven::NotCalled => continue,
                Driven::Tests(t) => ex.select_tests(Some(&t)),
                Driven::Every => ex.select_tests(None),
            }
            let (raw, output) = ex
                .run_with_output(plan, *budget)
                .map_err(|e| format!("{} {}: {e}", plan.id, plan.site()))?;
            if let Some(tier) = classify(&raw).tier {
                self.tiers.insert(plan.id.to_string(), tier);
            }
            let run = match raw {
                RawOutcome::Unsupported { reason } => return Ok(Err(reason)),
                RawOutcome::Unviable { .. } => return Ok(Ok(Run::Unviable)),
                RawOutcome::TimedOut { .. } => return Ok(Ok(Run::TimedOut)),
                RawOutcome::Crashed { kind, .. } => return Ok(Ok(Run::Crashed(kind))),
                // Not in this program: its tests run as unmutated.
                RawOutcome::NotReached { .. } if output.is_empty() => baseline.clone(),
                _ => Run::Tests(libtest::parse(&output)),
            };
            if let Run::Tests(t) = run {
                tests.extend(t);
            }
        }
        Ok(Ok(Run::Tests(tests)))
    }
}

/// Check each plan: run it under the debugger, compile and run its source
/// form, and judge the two. `source_of` reads a planned file.
pub fn check(
    plans: &[MutantPlan],
    profile: &Profile,
    source_of: &dyn Fn(&MutantPlan) -> std::io::Result<String>,
    debugger: &mut Debugger,
    scratch: &ScratchProject,
    test_args: &[String],
    mut report: impl FnMut(&MutantPlan, &Checked),
) -> Result<(), String> {
    let started = Instant::now();
    let baseline = scratch
        .run_baseline(test_args, FIRST_RUN)
        .map_err(|e| e.to_string())?;
    if !matches!(baseline, Run::Tests(_)) {
        return Err(format!(
            "unmutated source build does not pass: {baseline:?}"
        ));
    }
    let source_budget = Budget::from_baseline(started.elapsed()).timeout;
    debugger.record(plans)?;
    for plan in plans {
        let original = source_of(plan).map_err(|e| format!("{}: {e}", plan.file.display()))?;
        let checked = match render_source(&original, plan, profile) {
            None => Checked::NoSourceForm,
            Some(mutated) => match debugger.run(plan)? {
                Err(reason) => Checked::Unsupported(reason),
                Ok(at_runtime) => {
                    let compiled = scratch
                        .run_mutant(&plan.file, &mutated, test_args, source_budget)
                        .map_err(|e| e.to_string())?;
                    Checked::Judged(judge(&at_runtime, &compiled))
                }
            },
        };
        report(plan, &checked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use cherenkov_core::execution::ExecError;
    use cherenkov_core::operator_set::named;
    use cherenkov_core::plan::plan_file;
    use cherenkov_core::ports::SyntaxPort;
    use cherenkov_core::syntax::LanguageId;
    use cherenkov_syntax::TreeSitterSyntax;

    use super::*;

    /// An executor whose baseline passes and whose mutants all end as `raw`.
    struct Fixed(RawOutcome);

    impl MutantExecutor for Fixed {
        fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
            Ok(self.0.clone())
        }

        fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
            Ok(RawOutcome::Exited {
                code: 0,
                tier: Tier::A,
                wall: Duration::from_millis(1),
            })
        }
    }

    fn a_plan() -> MutantPlan {
        let syntax = TreeSitterSyntax::builtin().unwrap();
        let rust = LanguageId::new("rust");
        let profile = syntax
            .profiles()
            .into_iter()
            .find(|p| p.lang == rust)
            .unwrap();
        let src = include_str!("../fixtures/rust-ops/src/lib.rs");
        let file = syntax
            .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
            .unwrap();
        let ops = named("cargo-mutants").unwrap().operators();
        plan_file(&file, &profile, &ops).plans.remove(0)
    }

    fn debugger_of(mutant_ends: RawOutcome) -> Debugger {
        Debugger::new(vec![Box::new(Fixed(mutant_ends))]).unwrap()
    }

    #[test]
    fn test_tier_is_the_one_that_applied_the_mutant_in_the_last_run() {
        let plan = a_plan();
        let mut debugger = debugger_of(RawOutcome::Exited {
            code: 101,
            tier: Tier::P,
            wall: Duration::from_millis(1),
        });
        assert_eq!(debugger.tier(&plan), None, "before any run");
        debugger.run(&plan).unwrap().unwrap();
        assert_eq!(debugger.tier(&plan), Some(Tier::P));
    }

    #[test]
    fn test_tier_is_none_for_a_mutant_no_tier_applied() {
        let plan = a_plan();
        let mut debugger = debugger_of(RawOutcome::NotReached {
            reason: "not called".into(),
        });
        debugger.run(&plan).unwrap().unwrap();
        assert_eq!(debugger.tier(&plan), None);
    }
}
