//! Check run-time mutants against their compiled source mutants, one by one.

use std::time::{Duration, Instant};

use cherenkov_mutation::execution::{Budget, RawOutcome, Tier, classify};
use cherenkov_mutation::plan::{MutantPlan, render_source};
use cherenkov_mutation::profile::Profile;
use cherenkov_run_ports::executor::{Driven, MutantExecutor};

use crate::judge::{Judgement, Run, judge};
use crate::source::ScratchProject;
use cherenkov_exec_dap::libtest::{self, TestOutcome};

/// What checking one mutant found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Judged(Judgement),
    /// The executor has no way to run this mutant yet.
    Unsupported(String),
    /// The mutant has no source form to compile (an unknown default value).
    NoSourceForm,
    /// No test reached the mutated code under the debugger, and the compiled
    /// mutant leaves every test as at baseline too: they agree, and nothing
    /// was exercised.
    NotExercised(String),
}

/// What the debugger established for one mutant, across the test programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// Every program that ran the mutant reported complete, consistent results.
    Ran(Run),
    /// The executor refuses this mutant, with its reason.
    Unsupported(String),
    /// No program contains the mutated code.
    NotReached(String),
    /// No test calls the mutated function.
    NotCalled,
    /// The output cannot give a verdict; what was missing.
    Incomplete(String),
}

impl Observed {
    /// What checking the mutant found, given the unmutated source's `baseline`
    /// and `source`, which compiles and runs the mutant. A mutant the
    /// debugger did not run while the compiled one is detected is the
    /// false-survivor signature: an error, as is output that cannot be
    /// judged.
    fn settle(
        self,
        plan: &MutantPlan,
        baseline: &Run,
        source: impl FnOnce() -> Result<Run, String>,
    ) -> Result<Checked, String> {
        match &self {
            Observed::Unsupported(reason) => Ok(Checked::Unsupported(reason.clone())),
            Observed::Incomplete(_) => Err(self.failure(plan)),
            Observed::Ran(run) => Ok(Checked::Judged(judge(run, &source()?))),
            Observed::NotReached(_) | Observed::NotCalled => {
                let compiled = source()?;
                match compiled {
                    Run::DoesNotCompile => Ok(Checked::Judged(Judgement::Unviable)),
                    _ if compiled == *baseline => Ok(Checked::NotExercised(self.failure(plan))),
                    _ => Err(format!(
                        "{}; the compiled mutant ends differently from baseline: {compiled:?}",
                        self.failure(plan)
                    )),
                }
            }
        }
    }

    /// Why this outcome cannot be judged, for the error naming the plan.
    fn failure(&self, plan: &MutantPlan) -> String {
        let id = plan.id.to_string();
        match self {
            Observed::Ran(_) | Observed::Unsupported(_) => String::new(),
            Observed::NotReached(reason) => format!(
                "{id} {}: the debugger did not reach the mutated code: {reason}",
                plan.site()
            ),
            Observed::NotCalled => format!(
                "{id} {}: no test calls the mutated function, so the debugger run says nothing",
                plan.site()
            ),
            Observed::Incomplete(why) => format!(
                "{id} {}: the debugger run cannot be judged, its output is incomplete: {why}",
                plan.site()
            ),
        }
    }
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

    /// Every test program's outcome under the run-time mutant. A program
    /// that was not reached or not called leaves its tests at baseline, but
    /// if every program is so, the mutant ran nowhere and says nothing.
    fn run(&mut self, plan: &MutantPlan) -> Result<Observed, String> {
        let mut tests = std::collections::BTreeMap::new();
        let mut reached = false;
        let mut not_reached = None;
        for ((ex, budget, baseline), drives) in self.executors.iter_mut().zip(&self.drives) {
            let drive = drives
                .as_ref()
                .and_then(|d| d.get(&plan.id.to_string()).cloned())
                .unwrap_or(Driven::Every);
            let baseline_tests = match baseline {
                Run::Tests(t) => t.clone(),
                _ => Default::default(),
            };
            tests.extend(baseline_tests.clone());
            let expected: Vec<String> = match &drive {
                Driven::NotCalled => continue,
                Driven::Tests(t) => {
                    ex.select_tests(Some(t));
                    t.clone()
                }
                Driven::Every => {
                    ex.select_tests(None);
                    baseline_tests.keys().cloned().collect()
                }
            };
            let (raw, output) = ex
                .run_with_output(plan, *budget)
                .map_err(|e| format!("{} {}: {e}", plan.id, plan.site()))?;
            if let Some(tier) = classify(&raw).tier {
                self.tiers.insert(plan.id.to_string(), tier);
            }
            match program_run(&raw, &output, &expected) {
                Program::Ends(observed) => return Ok(observed),
                Program::NotReached(reason) => not_reached = Some(reason),
                Program::Tests(t) => {
                    reached = true;
                    tests.extend(t);
                }
            }
        }
        Ok(match (reached, not_reached) {
            (true, _) => Observed::Ran(Run::Tests(tests)),
            (false, Some(reason)) => Observed::NotReached(reason),
            (false, None) => Observed::NotCalled,
        })
    }
}

/// What one test program made of a mutant.
enum Program {
    /// The mutant's whole outcome is decided by this program.
    Ends(Observed),
    /// The mutated code is not in this program.
    NotReached(String),
    /// The program ran and reported these tests, completely.
    Tests(std::collections::BTreeMap<String, TestOutcome>),
}

/// One program's run: its ending, its output, and the tests that were
/// to run. Output that does not agree with the ending, or does not cover
/// the tests, is incomplete, never a verdict.
fn program_run(raw: &RawOutcome, output: &str, expected: &[String]) -> Program {
    match raw {
        RawOutcome::Unsupported { reason } => Program::Ends(Observed::Unsupported(reason.clone())),
        RawOutcome::Unviable { .. } => Program::Ends(Observed::Ran(Run::Unviable)),
        RawOutcome::TimedOut { .. } => Program::Ends(Observed::Ran(Run::TimedOut)),
        RawOutcome::Crashed { kind, .. } => {
            Program::Ends(Observed::Ran(Run::Crashed(kind.clone())))
        }
        RawOutcome::NotReached { reason } => Program::NotReached(reason.clone()),
        RawOutcome::Exited { code, .. } => {
            let parsed = libtest::parse(output);
            match incomplete(*code, &parsed, expected) {
                Some(why) => Program::Ends(Observed::Incomplete(why)),
                None => Program::Tests(parsed),
            }
        }
    }
}

/// Why the parsed tests cannot be trusted for a program that exited with
/// `code`, if they cannot.
fn incomplete(
    code: i32,
    parsed: &std::collections::BTreeMap<String, TestOutcome>,
    expected: &[String],
) -> Option<String> {
    let missing: Vec<&str> = expected
        .iter()
        .filter(|t| !parsed.contains_key(*t))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Some(format!(
            "exit code {code}, but no result for test(s) {}",
            missing.join(", ")
        ));
    }
    let failed = parsed.values().any(|o| *o == TestOutcome::Failed);
    match (code, failed) {
        (0, true) => Some("exit code 0, but a test reported FAILED".into()),
        (0, false) | (_, true) => None,
        (code, false) => Some(format!("exit code {code}, but no test reported FAILED")),
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
            Some(mutated) => {
                let compile = || {
                    scratch
                        .run_mutant(&plan.file, &mutated, test_args, source_budget)
                        .map_err(|e| e.to_string())
                };
                debugger.run(plan)?.settle(plan, &baseline, compile)?
            }
        };
        report(plan, &checked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use cherenkov_mutation::operator_set::named;
    use cherenkov_mutation::plan::plan_file;
    use cherenkov_mutation::syntax::LanguageId;
    use cherenkov_run_ports::executor::ExecError;
    use cherenkov_run_ports::syntax::SyntaxPort;
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

    const BASELINE_OUTPUT: &str = "test a ... ok\ntest b ... ok\n";

    fn exited(code: i32) -> RawOutcome {
        RawOutcome::Exited {
            code,
            tier: Tier::B,
            wall: Duration::from_millis(1),
        }
    }

    /// An executor with two tests whose mutant run ends as `raw` with
    /// `output`, and which says `driven` of the plans.
    struct Scripted {
        raw: RawOutcome,
        output: &'static str,
        driven: Option<Driven>,
    }

    impl MutantExecutor for Scripted {
        fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
            Ok(self.raw.clone())
        }

        fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
            Ok(exited(0))
        }

        fn run_with_output(
            &mut self,
            _: &MutantPlan,
            _: Budget,
        ) -> Result<(RawOutcome, String), ExecError> {
            Ok((self.raw.clone(), self.output.to_string()))
        }

        fn baseline_with_output(&mut self, _: Budget) -> Result<(RawOutcome, String), ExecError> {
            Ok((exited(0), BASELINE_OUTPUT.to_string()))
        }

        fn driven(
            &mut self,
            plans: &[MutantPlan],
            _: Budget,
        ) -> Result<Option<Vec<Driven>>, ExecError> {
            Ok(self.driven.clone().map(|d| vec![d; plans.len()]))
        }
    }

    fn observe(raw: RawOutcome, output: &'static str, driven: Option<Driven>) -> Observed {
        let plan = a_plan();
        let mut debugger = Debugger::new(vec![Box::new(Scripted {
            raw,
            output,
            driven,
        })])
        .unwrap();
        debugger.record(std::slice::from_ref(&plan)).unwrap();
        debugger.run(&plan).unwrap()
    }

    fn tests_of(pairs: &[(&str, TestOutcome)]) -> Run {
        Run::Tests(pairs.iter().map(|(n, o)| (n.to_string(), *o)).collect())
    }

    #[test]
    fn test_run_reports_the_tests_when_output_is_complete_and_consistent() {
        let seen = observe(exited(0), BASELINE_OUTPUT, None);
        assert_eq!(
            seen,
            Observed::Ran(tests_of(&[("a", TestOutcome::Ok), ("b", TestOutcome::Ok)]))
        );
    }

    #[test]
    fn test_run_reports_a_failure_when_the_exit_code_and_a_failed_test_agree() {
        let seen = observe(exited(101), "test a ... ok\ntest b ... FAILED\n", None);
        assert_eq!(
            seen,
            Observed::Ran(tests_of(&[
                ("a", TestOutcome::Ok),
                ("b", TestOutcome::Failed)
            ]))
        );
    }

    #[test]
    fn test_run_is_incomplete_when_a_test_is_missing_from_the_output() {
        let seen = observe(exited(0), "test a ... ok\n", None);
        let Observed::Incomplete(why) = seen else {
            panic!("a test missing from the output must not count as a pass: {seen:?}");
        };
        assert!(why.contains('b'), "names the missing test: {why}");
    }

    #[test]
    fn test_run_is_incomplete_when_the_exit_code_fails_but_no_test_failed() {
        let seen = observe(exited(101), BASELINE_OUTPUT, None);
        let Observed::Incomplete(why) = seen else {
            panic!("exit 101 with every test ok must not be judged: {seen:?}");
        };
        assert!(why.contains("101"), "names the exit code: {why}");
    }

    #[test]
    fn test_run_is_incomplete_when_the_exit_code_passes_but_a_test_failed() {
        let seen = observe(exited(0), "test a ... ok\ntest b ... FAILED\n", None);
        assert!(matches!(seen, Observed::Incomplete(_)), "{seen:?}");
    }

    #[test]
    fn test_run_is_incomplete_when_the_selected_tests_are_missing_from_the_output() {
        let seen = observe(exited(0), "", Some(Driven::Tests(vec!["b".into()])));
        let Observed::Incomplete(why) = seen else {
            panic!("{seen:?}");
        };
        assert!(why.contains('b'), "{why}");
    }

    #[test]
    fn test_run_takes_unselected_tests_from_the_baseline() {
        let seen = observe(
            exited(101),
            "test b ... FAILED\n",
            Some(Driven::Tests(vec!["b".into()])),
        );
        assert_eq!(
            seen,
            Observed::Ran(tests_of(&[
                ("a", TestOutcome::Ok),
                ("b", TestOutcome::Failed)
            ]))
        );
    }

    #[test]
    fn test_run_reports_not_reached_even_when_the_program_printed_results() {
        let seen = observe(
            RawOutcome::NotReached {
                reason: "not called".into(),
            },
            BASELINE_OUTPUT,
            None,
        );
        assert_eq!(seen, Observed::NotReached("not called".into()));
    }

    #[test]
    fn test_run_reports_not_called_when_no_test_calls_the_function() {
        let seen = observe(exited(0), BASELINE_OUTPUT, Some(Driven::NotCalled));
        assert_eq!(seen, Observed::NotCalled);
    }

    #[test]
    fn test_run_reports_a_refusal_as_unsupported_with_its_reason() {
        let seen = observe(
            RawOutcome::Unsupported {
                reason: "no way".into(),
            },
            "",
            None,
        );
        assert_eq!(seen, Observed::Unsupported("no way".into()));
    }

    fn baseline_run() -> Run {
        tests_of(&[("a", TestOutcome::Ok), ("b", TestOutcome::Ok)])
    }

    fn killed_run() -> Run {
        tests_of(&[("a", TestOutcome::Ok), ("b", TestOutcome::Failed)])
    }

    #[test]
    fn test_settle_judges_a_ran_mutant_against_the_compiled_one() {
        let checked = Observed::Ran(baseline_run())
            .settle(&a_plan(), &baseline_run(), || Ok(killed_run()))
            .unwrap();
        assert!(matches!(checked, Checked::Judged(Judgement::Unfaithful(_))));
    }

    #[test]
    fn test_settle_reports_not_exercised_when_the_compiled_mutant_is_unaffected_too() {
        for observed in [
            Observed::NotCalled,
            Observed::NotReached("elsewhere".into()),
        ] {
            let checked = observed
                .settle(&a_plan(), &baseline_run(), || Ok(baseline_run()))
                .unwrap();
            assert!(matches!(checked, Checked::NotExercised(_)), "{checked:?}");
        }
    }

    #[test]
    fn test_settle_fails_loudly_when_the_debugger_ran_nothing_but_the_compiled_mutant_is_killed() {
        for observed in [
            Observed::NotCalled,
            Observed::NotReached("elsewhere".into()),
        ] {
            let plan = a_plan();
            let why = observed
                .settle(&plan, &baseline_run(), || Ok(killed_run()))
                .unwrap_err();
            assert!(why.contains(&plan.id.to_string()), "{why}");
        }
    }

    #[test]
    fn test_settle_leaves_an_unviable_source_mutant_unviable_whatever_the_debugger_saw() {
        let checked = Observed::NotCalled
            .settle(&a_plan(), &baseline_run(), || Ok(Run::DoesNotCompile))
            .unwrap();
        assert_eq!(checked, Checked::Judged(Judgement::Unviable));
    }

    #[test]
    fn test_settle_reports_a_refusal_as_unsupported_without_compiling() {
        let checked = Observed::Unsupported("no way".into())
            .settle(&a_plan(), &baseline_run(), || panic!("compiled"))
            .unwrap();
        assert_eq!(checked, Checked::Unsupported("no way".into()));
    }

    #[test]
    fn test_settle_fails_loudly_for_incomplete_output_even_when_the_compiled_mutant_is_unaffected()
    {
        let plan = a_plan();
        let why = Observed::Incomplete("test b missing".into())
            .settle(&plan, &baseline_run(), || Ok(baseline_run()))
            .unwrap_err();
        assert!(why.contains("test b missing"), "{why}");
        assert!(why.contains(&plan.id.to_string()), "{why}");
    }

    /// `check` over the rust-ops fixture's first mutant with a debugger that
    /// reaches nothing: how it ended and what it reported.
    fn checked_with_a_debugger_that_ran_nothing() -> (Result<(), String>, Vec<Checked>) {
        let plan = a_plan();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rust-ops");
        let scratch = ScratchProject::copy(&root, Path::new("cargo")).unwrap();
        let mut debugger = Debugger::new(vec![Box::new(Scripted {
            raw: RawOutcome::NotReached {
                reason: "elsewhere".into(),
            },
            output: "",
            driven: None,
        })])
        .unwrap();
        let profile = TreeSitterSyntax::builtin()
            .unwrap()
            .profiles()
            .into_iter()
            .find(|p| p.lang == LanguageId::new("rust"))
            .unwrap();
        let mut reported = Vec::new();
        let ended = check(
            &[plan],
            &profile,
            &|p| std::fs::read_to_string(root.join(&p.file)),
            &mut debugger,
            &scratch,
            &[],
            |_, c| reported.push(c.clone()),
        );
        (ended, reported)
    }

    #[test]
    fn test_check_fails_naming_the_plan_when_the_debugger_ran_nothing_and_the_source_mutant_is_killed()
     {
        // The rust-ops fixture's first mutant (`compare` returning a value)
        // is killed when compiled; a debugger that did not reach it is the
        // false-survivor signature.
        let (ended, reported) = checked_with_a_debugger_that_ran_nothing();
        let why = ended.expect_err("a false survivor must fail the check");
        assert!(reported.is_empty(), "{reported:?}");
        assert!(why.contains("did not reach"), "{why}");
        assert!(why.contains("compare"), "names the mutant's site: {why}");
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
        debugger.run(&plan).unwrap();
        assert_eq!(debugger.tier(&plan), Some(Tier::P));
    }

    #[test]
    fn test_tier_is_none_for_a_mutant_no_tier_applied() {
        let plan = a_plan();
        let mut debugger = debugger_of(RawOutcome::NotReached {
            reason: "not called".into(),
        });
        debugger.run(&plan).unwrap();
        assert_eq!(debugger.tier(&plan), None);
    }
}
