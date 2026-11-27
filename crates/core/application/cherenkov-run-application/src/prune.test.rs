//! Pruning a store: the last N finished runs are kept, an unfinished run
//! never goes, and a mutant's stream, which every run that plans the
//! mutant appends to, goes only when no kept run plans it.

use std::sync::Arc;
use std::sync::Mutex;

use angzarr_embedded::angzarr_ports::tombstone;
use angzarr_embedded::v1::{Cover, EventPage, Uuid};
use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, Tier};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_run_ports::{Edge, EdgeError, Listed, Listing, Purging, Recovery, Report};

use crate::covers::{mutant_cover, run_cover};
use crate::error::RunLayerError;
use crate::host::Readers;
use crate::layer::{Settings, Workers, plan_run, start};
use crate::prune::{ATTEMPTS, prune};
use crate::test_plans::many;
use crate::testing::{Core, new_core, new_run, silent};

/// Kills nothing.
struct Surviving;

impl MutantExecutor for Surviving {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited().into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited().into())
    }
}

fn exited() -> RawOutcome {
    RawOutcome::Exited {
        code: 0,
        tier: Tier::P,
        wall: std::time::Duration::from_millis(1),
    }
}

fn settings() -> Settings {
    Settings {
        root: "/project".to_string(),
        operators: vec![cherenkov_run_domain::OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string(),
        }],
        jobs: 1,
    }
}

/// A run of `plans`, run to its end.
async fn finished(core: &Arc<Core>, plans: &[MutantPlan]) -> Uuid {
    let run = new_run();
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Surviving);
    let workers = Workers {
        sets: vec![vec![executor]],
        jobs: 1,
    };
    let started = start(
        core,
        run.clone(),
        plans,
        &settings(),
        Some(workers),
        Readers::printing(silent()),
    )
    .await
    .unwrap();
    assert!(started.status.is_some(), "the run finished");
    run
}

/// A run of `plans` that an earlier process left planned and unfinished.
async fn unfinished(core: &Core, plans: &[MutantPlan]) -> Uuid {
    let run = new_run();
    core.command(run_cover(&run), &plan_run(plans, &settings(), 1))
        .await
        .unwrap();
    run
}

/// The events a stream holds: a purge leaves its tombstone, which is none.
async fn held(core: &Core, cover: &Cover) -> usize {
    core.events(cover, 0)
        .await
        .unwrap()
        .iter()
        .filter(|page| !tombstone::is(page))
        .count()
}

async fn has_run(core: &Core, run: &Uuid) -> bool {
    held(core, &run_cover(run)).await > 0
}

async fn has_mutant(core: &Core, plans: &[MutantPlan], at: usize) -> bool {
    held(core, &mutant_cover(&plans[at].id.0).unwrap()).await > 0
}

#[tokio::test]
async fn test_prune_keeps_the_last_n_finished_runs_and_removes_the_older() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let runs = [
        finished(&core, &plans).await,
        finished(&core, &plans).await,
        finished(&core, &plans).await,
    ];
    // Made last: a later run of the plan would carry it on, or abandon it.
    unfinished(&core, &plans).await;
    let pruned = prune(&*core, 2, false).await.unwrap();
    assert_eq!(pruned.runs, [runs[0].clone()]);
    assert_eq!((pruned.kept_finished, pruned.kept_unfinished), (2, 1));
    assert!(!has_run(&core, &runs[0]).await);
    assert!(has_run(&core, &runs[1]).await);
    assert!(has_run(&core, &runs[2]).await);
}

#[tokio::test]
async fn test_prune_never_removes_an_unfinished_run_and_does_not_count_it_among_the_kept() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let old = finished(&core, &plans).await;
    let newest = finished(&core, &plans).await;
    // Made last: a later run of the plan would carry it on, or abandon it.
    let left = unfinished(&core, &plans).await;
    let pruned = prune(&*core, 1, false).await.unwrap();
    assert_eq!(pruned.runs, std::slice::from_ref(&old));
    assert_eq!((pruned.kept_finished, pruned.kept_unfinished), (1, 1));
    assert!(has_run(&core, &left).await, "the unfinished run stays");
    assert!(has_run(&core, &newest).await);
    assert!(!has_run(&core, &old).await);
    let pruned = prune(&*core, 0, false).await.unwrap();
    assert_eq!(pruned.runs, [newest]);
    assert!(has_run(&core, &left).await);
}

#[tokio::test]
async fn test_prune_removes_the_mutant_streams_only_the_removed_runs_planned() {
    let core = Arc::new(new_core().await);
    let plans = many(4);
    let old = finished(&core, &plans[..2]).await;
    finished(&core, &plans[2..]).await;
    let mut gone = held(&core, &run_cover(&old)).await;
    for plan in &plans[..2] {
        gone += held(&core, &mutant_cover(&plan.id.0).unwrap()).await;
    }
    let pruned = prune(&*core, 1, false).await.unwrap();
    assert_eq!(
        pruned.events as usize, gone,
        "every event of every stream removed"
    );
    assert_eq!(pruned.mutants, 2);
    assert!(!has_mutant(&core, &plans, 0).await);
    assert!(!has_mutant(&core, &plans, 1).await);
    assert!(has_mutant(&core, &plans, 2).await);
    assert!(has_mutant(&core, &plans, 3).await);
}

#[tokio::test]
async fn test_prune_keeps_a_mutant_stream_a_kept_run_shares_with_a_removed_one() {
    let core = Arc::new(new_core().await);
    let plans = many(4);
    finished(&core, &plans[..3]).await;
    finished(&core, &plans[1..]).await;
    let pruned = prune(&*core, 1, false).await.unwrap();
    assert_eq!(
        pruned.mutants, 1,
        "only mutant 0 was the removed run's alone"
    );
    assert!(!has_mutant(&core, &plans, 0).await);
    for shared in 1..4 {
        assert!(has_mutant(&core, &plans, shared).await, "mutant {shared}");
    }
}

#[tokio::test]
async fn test_prune_keeps_a_mutant_stream_an_unfinished_run_plans() {
    let core = Arc::new(new_core().await);
    let plans = many(3);
    finished(&core, &plans).await;
    unfinished(&core, &plans[1..]).await;
    let pruned = prune(&*core, 0, false).await.unwrap();
    assert_eq!(pruned.mutants, 1);
    assert!(!has_mutant(&core, &plans, 0).await);
    assert!(has_mutant(&core, &plans, 1).await);
    assert!(has_mutant(&core, &plans, 2).await);
}

#[tokio::test]
async fn test_a_dry_prune_says_what_a_prune_removes_and_removes_nothing() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let old = finished(&core, &plans).await;
    finished(&core, &plans).await;
    let before_run = held(&core, &run_cover(&old)).await;
    let before_mutant = held(&core, &mutant_cover(&plans[0].id.0).unwrap()).await;
    let dry = prune(&*core, 1, true).await.unwrap();
    assert_eq!(held(&core, &run_cover(&old)).await, before_run);
    assert_eq!(
        held(&core, &mutant_cover(&plans[0].id.0).unwrap()).await,
        before_mutant
    );
    let real = prune(&*core, 1, false).await.unwrap();
    assert_eq!(dry, real, "what it would remove is what it removes");
    assert_eq!(real.runs, [old]);
    assert!(real.events > 0);
}

#[tokio::test]
async fn test_prune_of_a_store_with_no_more_finished_runs_than_asked_removes_nothing() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    finished(&core, &plans).await;
    let pruned = prune(&*core, 1, false).await.unwrap();
    assert_eq!(pruned.runs, []);
    assert_eq!((pruned.mutants, pruned.events), (0, 0));
    assert!(
        prune(&new_core().await, 0, false)
            .await
            .unwrap()
            .runs
            .is_empty()
    );
}

#[tokio::test]
async fn test_a_second_prune_finds_the_streams_the_first_purged_already_gone() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let old = finished(&core, &plans).await;
    finished(&core, &plans).await;
    let first = prune(&*core, 1, false).await.unwrap();
    assert_eq!((first.runs, first.mutants), (vec![old], 0));
    let again = prune(&*core, 0, false).await.unwrap();
    assert_eq!(again.runs.len(), 1, "only the run that was kept is left");
    let last = prune(&*core, 0, false).await.unwrap();
    assert_eq!(last.runs, []);
    assert_eq!((last.mutants, last.events), (0, 0));
    assert_eq!((last.kept_finished, last.kept_unfinished), (0, 0));
}

#[tokio::test]
async fn test_a_run_after_a_prune_leaves_the_purged_runs_alone_and_plans_the_mutants_afresh() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let old = finished(&core, &plans).await;
    let before = held(&core, &mutant_cover(&plans[0].id.0).unwrap()).await;
    prune(&*core, 0, false).await.unwrap();
    assert!(!has_mutant(&core, &plans, 0).await);
    let next = finished(&core, &plans).await;
    let purged = core.events(&run_cover(&old), 0).await.unwrap();
    assert!(
        purged.last().is_some_and(tombstone::is),
        "the run that began on the store did not abandon a run already gone"
    );
    assert!(has_run(&core, &next).await);
    assert_eq!(
        held(&core, &mutant_cover(&plans[0].id.0).unwrap()).await,
        before,
        "the mutant is handled as a fresh one: only the new run's events"
    );
}

#[tokio::test]
async fn test_a_dry_prune_of_a_stream_purged_and_written_again_counts_only_the_events_it_holds() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    finished(&core, &plans).await;
    prune(&*core, 0, false).await.unwrap();
    finished(&core, &plans).await;
    let dry = prune(&*core, 0, true).await.unwrap();
    let real = prune(&*core, 0, false).await.unwrap();
    assert!(real.events > 0);
    assert_eq!(
        dry.events, real.events,
        "the erased events and the tombstone are not counted"
    );
}

/// A core whose first `races` purges lose to a writer, as the store says it:
/// refused, retryable, nothing removed.
struct Racing<'a> {
    core: &'a Core,
    races: Mutex<usize>,
    refusal: Recovery,
    purges: Mutex<usize>,
}

impl Racing<'_> {
    /// Whether this purge is one a writer got in front of.
    fn lose_race(&self) -> bool {
        let mut races = self.races.lock().unwrap();
        let lost = *races > 0;
        *races = races.saturating_sub(1);
        lost
    }
}

impl Edge for Racing<'_> {
    async fn handle(
        &self,
        book: angzarr_embedded::v1::CommandBook,
    ) -> Result<cherenkov_run_ports::Reported, EdgeError> {
        self.core.handle(book).await
    }

    async fn events(&self, stream: &Cover, from: u64) -> Result<Vec<EventPage>, EdgeError> {
        self.core.events(stream, from).await
    }

    fn next_commit(&self) -> angzarr_embedded::angzarr_ports::CommitWait {
        self.core.next_commit()
    }
}

impl Listing for Racing<'_> {
    async fn streams(&self, domain: &str) -> Result<Vec<Listed>, EdgeError> {
        self.core.streams(domain).await
    }

    async fn window(&self, s: &Cover, from: u64, to: u64) -> Result<Vec<EventPage>, EdgeError> {
        self.core.window(s, from, to).await
    }
}

impl Purging for Racing<'_> {
    async fn purge(&self, stream: &Cover, expected: u64) -> Result<u64, EdgeError> {
        *self.purges.lock().unwrap() += 1;
        if self.lose_race() {
            return Err(EdgeError {
                cause: Report::new("angzarr-conflict"),
                recovery: self.refusal,
            });
        }
        self.core.purge(stream, expected).await
    }
}

fn racing(core: &Core, races: usize, refusal: Recovery) -> Racing<'_> {
    Racing {
        core,
        races: Mutex::new(races),
        refusal,
        purges: Mutex::new(0),
    }
}

#[tokio::test]
async fn test_prune_reads_again_and_goes_on_when_a_purge_loses_a_race() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let old = finished(&core, &plans).await;
    finished(&core, &plans).await;
    let edge = racing(&core, 2, Recovery::Retry);
    let pruned = prune(&edge, 1, false).await.unwrap();
    assert_eq!(pruned.runs, std::slice::from_ref(&old));
    assert!(!has_run(&core, &old).await);
}

#[tokio::test]
async fn test_prune_gives_up_when_every_purge_loses_its_race_and_says_it_may_be_retried() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    finished(&core, &plans).await;
    finished(&core, &plans).await;
    let edge = racing(&core, usize::MAX, Recovery::Retry);
    let error = prune(&edge, 1, false).await.unwrap_err();
    assert!(
        matches!(error, RunLayerError::Unavailable { .. }),
        "{error:?}"
    );
    assert_eq!(
        *edge.purges.lock().unwrap(),
        ATTEMPTS,
        "it reads the store again after each lost race, and then gives up"
    );
}

#[tokio::test]
async fn test_prune_stops_at_a_purge_the_store_refuses_for_good() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    finished(&core, &plans).await;
    finished(&core, &plans).await;
    let edge = racing(&core, usize::MAX, Recovery::Bug);
    let error = prune(&edge, 1, false).await.unwrap_err();
    assert!(matches!(error, RunLayerError::Kernel { .. }), "{error:?}");
    assert_eq!(*edge.purges.lock().unwrap(), 1);
}
