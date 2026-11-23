//! A test program judges only the mutants routed to it: the others are
//! never recorded in it and never run in it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cherenkov_core::execution::{
    Budget, Driven, ExecError, MutantExecutor, RawOutcome, Recording, Selection, Tier,
};
use cherenkov_core::id::MutantId;
use cherenkov_core::plan::MutantPlan;
use cherenkov_exec_dap::routing::OnlyJudged;

/// What the inner program was asked to record.
#[derive(Default, Clone)]
struct Seen(Arc<Mutex<Vec<String>>>);

struct Program(Seen);

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
    use cherenkov_core::ports::SyntaxPort;
    let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
    let rust = cherenkov_core::syntax::LanguageId::new("rust");
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
    let mut plan =
        cherenkov_core::plan::plan_file(&file, &profile, &cherenkov_core::operator::catalog())
            .plans
            .remove(0);
    plan.id = MutantId(id.into());
    plan
}

fn judging(ids: &[&str]) -> (OnlyJudged<Program>, Seen) {
    let seen = Seen::default();
    let judged: BTreeSet<MutantId> = ids.iter().map(|i| MutantId((*i).into())).collect();
    (
        OnlyJudged::new(Program(seen.clone()), Arc::new(judged)),
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
