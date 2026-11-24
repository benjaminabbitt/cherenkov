//! A test program judges only the mutants routed to it: the others are
//! never recorded in it and never run in it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cherenkov_exec_dap::routing::OnlyJudged;
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Driven, ExecError, MutantExecutor, Recording, Selection};

/// What the inner program was asked to record.
#[derive(Default, Clone)]
struct Seen(Arc<Mutex<Vec<String>>>);

struct Program(Seen, Vec<Driven>);

impl MutantExecutor for Program {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: Duration::ZERO,
        })
    }
    fn baseline(&mut self, budget: Budget) -> Result<RawOutcome, ExecError> {
        self.run_unused(budget)
    }
    fn driven(
        &mut self,
        plans: &[MutantPlan],
        _: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        self.0
            .0
            .lock()
            .unwrap()
            .extend(plans.iter().map(|p| p.id.0.clone()));
        if !self.1.is_empty() {
            return Ok(Some(self.1.clone()));
        }
        Ok(Some(
            plans
                .iter()
                .map(|_| Driven::Tests(vec!["t".to_string()]))
                .collect(),
        ))
    }
}

impl Program {
    fn run_unused(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: Duration::ZERO,
        })
    }
}

fn plan_of(id: &str) -> MutantPlan {
    use cherenkov_run_ports::syntax::SyntaxPort;
    let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
    let rust = cherenkov_mutation::syntax::LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let file = syntax
        .analyze(
            &rust,
            std::path::Path::new("lib.rs"),
            Arc::from("pub fn f() -> bool { true }\n"),
        )
        .unwrap();
    let mut plan = cherenkov_mutation::plan::plan_file(
        &file,
        &profile,
        &cherenkov_mutation::operator::catalog(),
    )
    .plans
    .remove(0);
    plan.id = MutantId(id.into());
    plan
}

fn judging(ids: &[&str]) -> (OnlyJudged<Program>, Seen) {
    let seen = Seen::default();
    let judged: BTreeSet<MutantId> = ids.iter().map(|i| MutantId((*i).into())).collect();
    (
        OnlyJudged::new(Program(seen.clone(), Vec::new()), Arc::new(judged)),
        seen,
    )
}

const BUDGET: Budget = Budget {
    timeout: Duration::from_secs(1),
};

#[test]
fn test_a_program_records_only_the_mutants_routed_to_it_and_the_rest_are_not_called() {
    let (mut program, seen) = judging(&["mine"]);
    let plans = [plan_of("other"), plan_of("mine"), plan_of("other2")];
    let recording = program.recording(&plans, BUDGET).unwrap();
    assert_eq!(
        *seen.0.lock().unwrap(),
        ["mine"],
        "only its mutant recorded"
    );
    assert_eq!(
        recording.driven.unwrap(),
        [
            Driven::NotCalled,
            Driven::Tests(vec!["t".to_string()]),
            Driven::NotCalled
        ],
        "answers for every plan, in order"
    );
    assert_eq!(recording.selection, Selection::Recorded);
}

#[test]
fn test_a_program_that_cannot_record_is_left_as_it_is_with_every_test_for_every_mutant() {
    struct Blind;
    impl MutantExecutor for Blind {
        fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
            unreachable!()
        }
        fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
            unreachable!()
        }
    }
    let judged = Arc::new(BTreeSet::from([MutantId("mine".into())]));
    let mut program = OnlyJudged::new(Blind, judged);
    let plans = [plan_of("other"), plan_of("mine")];
    let recording: Recording = program.recording(&plans, BUDGET).unwrap();
    // The run layer's invariant: a recording without selection has no
    // answers to give; routing cannot be told through it.
    assert_eq!(recording.driven, None);
    assert!(matches!(recording.selection, Selection::EveryTest { .. }));
}

fn only_changed(
    answers: Vec<Driven>,
    ids: &[&str],
    only: &[&str],
    changed: &[&str],
) -> Vec<Driven> {
    let set =
        |l: &[&str]| -> BTreeSet<MutantId> { l.iter().map(|i| MutantId((*i).into())).collect() };
    let mut program = OnlyJudged::new(Program(Seen::default(), answers), Arc::new(set(ids)))
        .with_changed(
            Arc::new(set(only)),
            changed.iter().map(|c| (*c).to_string()).collect(),
        );
    let plans: Vec<MutantPlan> = ids.iter().map(|i| plan_of(i)).collect();
    program.recording(&plans, BUDGET).unwrap().driven.unwrap()
}

#[test]
fn test_a_mutant_only_the_changed_tests_judge_runs_the_changed_tests_that_call_it() {
    let got = only_changed(
        vec![
            Driven::Tests(vec!["a".into(), "changed".into()]),
            Driven::Tests(vec!["a".into(), "b".into()]),
            Driven::Every,
            Driven::NotCalled,
            Driven::Tests(vec!["a".into()]),
        ],
        &["m1", "m2", "m3", "m4", "app"],
        &["m1", "m2", "m3", "m4"],
        &["changed"],
    );
    assert_eq!(
        got,
        [
            Driven::Tests(vec!["changed".into()]),
            Driven::NotCalled,
            Driven::Tests(vec!["changed".into()]),
            Driven::NotCalled,
            Driven::Tests(vec!["a".into()]),
        ],
        "the mutant of changed application code keeps every test that calls it"
    );
}

/// A program that notes what it is asked.
#[derive(Default, Clone)]
struct Calls(Arc<Mutex<Vec<String>>>);

struct Noting(Calls);

impl MutantExecutor for Noting {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        unreachable!()
    }
    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        unreachable!()
    }
    fn driven(
        &mut self,
        plans: &[MutantPlan],
        _: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        let ids: Vec<String> = plans.iter().map(|p| p.id.0.clone()).collect();
        self.0
            .0
            .lock()
            .unwrap()
            .push(format!("driven {}", ids.join(",")));
        Ok(Some(plans.iter().map(|_| Driven::Every).collect()))
    }
    fn select_tests(&mut self, tests: Option<&[String]>) {
        self.0.0.lock().unwrap().push(format!("select {tests:?}"));
    }
    fn describe(&self) -> Option<String> {
        Some("the program".to_string())
    }
    fn close_session(&mut self) {
        self.0.0.lock().unwrap().push("close".to_string());
    }
}

fn noting() -> (OnlyJudged<Noting>, Calls) {
    let calls = Calls::default();
    let judged = Arc::new(BTreeSet::from([MutantId("mine".into())]));
    (OnlyJudged::new(Noting(calls.clone()), judged), calls)
}

#[test]
fn test_driven_asks_the_program_about_its_mutants_alone_and_answers_for_all() {
    let (mut program, calls) = noting();
    let plans = [plan_of("other"), plan_of("mine")];
    let driven = program.driven(&plans, BUDGET).unwrap();
    assert_eq!(driven, Some(vec![Driven::NotCalled, Driven::Every]));
    assert_eq!(*calls.0.lock().unwrap(), ["driven mine"]);
}

#[test]
fn test_a_program_s_test_selection_description_and_session_are_its_own() {
    let (mut program, calls) = noting();
    program.select_tests(Some(&["t".to_string()]));
    program.select_tests(None);
    program.close_session();
    assert_eq!(program.describe().as_deref(), Some("the program"));
    assert_eq!(
        *calls.0.lock().unwrap(),
        ["select Some([\"t\"])", "select None", "close"]
    );
}
