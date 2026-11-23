//! Which test programs judge which mutants. A workspace's mutants are
//! judged by the tests of some of its packages (the mutated one's, as
//! cargo-mutants does by default); the program of any other package says
//! nothing of them, so it is neither recorded with them nor run with them.

use std::collections::BTreeSet;
use std::sync::Arc;

use cherenkov_core::execution::{Budget, Driven, ExecError, MutantExecutor, RawOutcome, Recording};
use cherenkov_core::id::MutantId;
use cherenkov_core::plan::MutantPlan;

/// A test program that judges only the mutants `judged` holds: for any
/// other, the recording says [`Driven::NotCalled`], which the run layer
/// takes to say that the program has nothing to tell of it. A program that
/// cannot record (it answers none) is left as it is: the run layer allows no
/// answers beside a selection of every test.
pub struct OnlyJudged<E> {
    inner: E,
    judged: Arc<BTreeSet<MutantId>>,
}

impl<E> OnlyJudged<E> {
    pub fn new(inner: E, judged: Arc<BTreeSet<MutantId>>) -> Self {
        OnlyJudged { inner, judged }
    }

    /// The plans this program judges.
    fn mine(&self, plans: &[MutantPlan]) -> Vec<MutantPlan> {
        plans
            .iter()
            .filter(|p| self.judged.contains(&p.id))
            .cloned()
            .collect()
    }

    /// `found` for the judged plans, as the answer for all `plans`, in
    /// order.
    fn spread(&self, plans: &[MutantPlan], found: Vec<Driven>) -> Vec<Driven> {
        let mut found = found.into_iter();
        plans
            .iter()
            .map(|p| {
                if self.judged.contains(&p.id) {
                    found.next().unwrap_or(Driven::Every)
                } else {
                    Driven::NotCalled
                }
            })
            .collect()
    }
}

impl<E: MutantExecutor> MutantExecutor for OnlyJudged<E> {
    fn run(&mut self, plan: &MutantPlan, budget: Budget) -> Result<RawOutcome, ExecError> {
        self.inner.run(plan, budget)
    }
    fn baseline(&mut self, budget: Budget) -> Result<RawOutcome, ExecError> {
        self.inner.baseline(budget)
    }
    fn run_with_output(
        &mut self,
        plan: &MutantPlan,
        budget: Budget,
    ) -> Result<(RawOutcome, String), ExecError> {
        self.inner.run_with_output(plan, budget)
    }
    fn baseline_with_output(&mut self, budget: Budget) -> Result<(RawOutcome, String), ExecError> {
        self.inner.baseline_with_output(budget)
    }
    fn driven(
        &mut self,
        plans: &[MutantPlan],
        budget: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        let found = self.inner.driven(&self.mine(plans), budget)?;
        Ok(found.map(|found| self.spread(plans, found)))
    }
    fn recording(&mut self, plans: &[MutantPlan], budget: Budget) -> Result<Recording, ExecError> {
        let mut recording = self.inner.recording(&self.mine(plans), budget)?;
        recording.driven = recording
            .driven
            .take()
            .map(|found| self.spread(plans, found));
        Ok(recording)
    }
    fn select_tests(&mut self, tests: Option<&[String]>) {
        self.inner.select_tests(tests);
    }
    fn describe(&self) -> Option<String> {
        self.inner.describe()
    }
    fn close_session(&mut self) {
        self.inner.close_session();
    }
}
