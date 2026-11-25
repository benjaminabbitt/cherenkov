//! A restart: the bus served, stopped half way, and served again by a new
//! bus on the same store. What the stored positions and the streams hold is
//! all the second serve has, as after a crash on a durable store (phase 5.7
//! adds the store; the code is the same): nothing a runner finished is run
//! again, nothing a policy owed is lost, and the run ends as it would have.

use crate::testing::{new_core, new_core_on, new_run};
use crate::yielding::{Crash, Yielding};
use angzarr_embedded::{OnRetryable, SnapshotStore, StreamRef};
use cherenkov_embedded_edge::{EmbeddedEdge, RunStore};
use cherenkov_run_ports::Edge;

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::v1::Uuid;
use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, Tier};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::{PlanRun, RunStatus};
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};

use crate::covers::{mutant_cover, run_cover};
use crate::host::{self, Readers};
use crate::live::LiveRuns;
use crate::pool::Pool;
use crate::runner::Runner;
use crate::test_plans::{executables, many, prepared};
use crate::testing::silent;
use crate::workers::Worker;

const MUTANTS: usize = 6;

/// What every executor of a test did, in the order it was done: the plan
/// ids it began running.
#[derive(Default)]
struct Ledger {
    started: Mutex<Vec<String>>,
}

impl Ledger {
    fn begin(&self, id: &str) -> usize {
        let mut started = self.started.lock().unwrap();
        started.push(id.to_string());
        started.len()
    }

    fn count(&self) -> usize {
        self.started.lock().unwrap().len()
    }

    fn runs_of(&self, id: &str) -> usize {
        let started = self.started.lock().unwrap();
        started.iter().filter(|ran| *ran == id).count()
    }
}

/// Kills the mutants in `a`, lets the rest survive, and, on its `hold`th run
/// since the test began, does not return until released: a mutant caught
/// by the crash, running.
struct Executor {
    ledger: Arc<Ledger>,
    hold: usize,
    gate: Arc<Mutex<Receiver<()>>>,
}

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(1),
    }
}

impl MutantExecutor for Executor {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        if self.ledger.begin(&plan.id.0) == self.hold {
            // Released when the test lets go of the gate's sender.
            let _ = self.gate.lock().unwrap().recv();
        }
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101).into(),
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

/// A runner of one worker over `core`, whose executor holds on its `hold`th
/// run of the test.
fn runner<S: RunStore>(
    core: &Arc<EmbeddedEdge<S>>,
    ledger: &Arc<Ledger>,
    hold: usize,
    gate: &Arc<Mutex<Receiver<()>>>,
) -> Runner<EmbeddedEdge<S>> {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Executor {
        ledger: ledger.clone(),
        hold,
        gate: gate.clone(),
    });
    Runner::new(core.clone(), Pool::new(vec![Worker::new(vec![executor])]))
}

/// A run of `MUTANTS` mutants, planned and ready: its one program prepared,
/// and nothing served yet.
async fn ready_run<S: RunStore>(core: &EmbeddedEdge<S>) -> (Uuid, Vec<MutantPlan>) {
    ready_run_of(core, MUTANTS).await
}

/// A run of `mutants` mutants, planned and ready.
async fn ready_run_of<S: RunStore>(
    core: &EmbeddedEdge<S>,
    mutants: usize,
) -> (Uuid, Vec<MutantPlan>) {
    let plans = many(mutants);
    let run = new_run();
    let plan = PlanRun {
        programs: 1,
        mutants: executables(&plans),
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    core.report(run_cover(&run), &prepared(), "program 0")
        .await
        .unwrap();
    (run, plans)
}

/// The mutants of `plans` with a decision on their streams, by id.
async fn decided<S: RunStore>(
    core: &EmbeddedEdge<S>,
    plans: &[MutantPlan],
) -> BTreeMap<String, usize> {
    let mut decided = BTreeMap::new();
    for plan in plans {
        let pages = core
            .events(&mutant_cover(&plan.id.0).unwrap(), 0)
            .await
            .unwrap();
        let decisions = pages
            .iter()
            .filter(|page| matches!(mutant_event(page), Some(MutantEvent::MutantDecided(_))))
            .count();
        decided.insert(plan.id.0.clone(), decisions);
    }
    decided
}

/// Wait, a tick at a time, until `done`, for as long as a test may take.
async fn until(mut done: impl FnMut() -> bool) {
    let waiting = async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(30), waiting)
        .await
        .expect("the condition came about");
}

#[tokio::test]
async fn test_a_serve_stopped_half_way_is_finished_by_a_new_bus_with_no_mutant_run_twice_and_none_lost()
 {
    let core = Arc::new(new_core().await);
    let (run, plans) = ready_run(&core).await;
    let ledger = Arc::new(Ledger::default());
    let (release, gate): (Sender<()>, Receiver<()>) = channel();
    let gate = Arc::new(Mutex::new(gate));
    let instances = NonZeroUsize::MIN;

    // The first serve: its runner holds on the fourth mutant it begins.
    let hold = 4;
    let first_live = LiveRuns::default();
    let first = serve(
        core.as_ref(),
        runner(&core, &ledger, hold, &gate),
        instances,
        &first_live,
        |_| true,
    );
    tokio::select! {
        served = first => panic!("the first serve ended on its own: {served:?}"),
        () = until(|| ledger.count() == hold) => {}
    }
    // That is the crash: the serve is dropped with a mutant running. Let
    // it go, so the thread it holds ends.
    drop(release);

    let before = decided(&core, &plans).await;
    let decided_before: Vec<&String> = before
        .iter()
        .filter(|(_, decisions)| **decisions == 1)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        decided_before.len(),
        hold - 1,
        "the three mutants finished before the fourth began: {before:?}"
    );
    assert_eq!(
        first_live.status(&run),
        None,
        "the run was not over when the serve died"
    );

    // The second serve: a new bus, a new runner, a new model of the live
    // runs, on the same store. It stops when the run has ended.
    let second_live = LiveRuns::default();
    let ended = second_live.clone();
    serve(
        core.as_ref(),
        runner(&core, &ledger, usize::MAX, &gate),
        instances,
        &second_live,
        |served| ended.status(&run).is_none() && !served.is_empty(),
    )
    .await
    .unwrap();

    // Rebuilt from the first event, the model says the run ended.
    assert_eq!(second_live.status(&run), Some(RunStatus::Complete));
    assert_eq!(second_live.owed(&run), 0);
    // No mutant is lost: each is decided, once.
    let after = decided(&core, &plans).await;
    assert!(after.values().all(|decisions| *decisions == 1), "{after:?}");
    // None is run twice that was finished: the three decided before the
    // crash ran once. The fourth was running, and ran again.
    for id in &decided_before {
        assert_eq!(ledger.runs_of(id), 1, "{id} was finished before the crash");
    }
    let rerun: Vec<&String> = after.keys().filter(|id| ledger.runs_of(id) == 2).collect();
    assert_eq!(
        rerun.len(),
        1,
        "only the mutant the crash caught: {rerun:?}"
    );
    assert_eq!(ledger.count(), MUTANTS + 1);
    // The run's stream says so: each decision once, and one finish.
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    let events: Vec<RunEvent> = pages.iter().filter_map(run_event).collect();
    let recorded = events
        .iter()
        .filter(|e| matches!(e, RunEvent::DecisionRecorded(_)))
        .count();
    let finished = events
        .iter()
        .filter(|e| matches!(e, RunEvent::RunFinished(_)))
        .count();
    assert_eq!((recorded, finished), (MUTANTS, 1));
}

/// What an uninterrupted run of `SWEPT` mutants came to, and how many event
/// writes the serve made: the sweep's reference.
struct Reference {
    run_events: Vec<&'static str>,
    writes: u64,
}

const SWEPT: usize = 3;

fn names_of_run(pages: &[angzarr_embedded::v1::EventPage]) -> Vec<&'static str> {
    pages
        .iter()
        .filter_map(run_event)
        .map(|event| crate::testing::event_name(&event))
        .collect()
}

/// A run of `SWEPT` mutants on a store that crashes the serve as `crash`
/// says, served (never released) until the store crashes or the run ends,
/// then served again, clean, on the same store, until the run ends.
/// Returns the ledger of what ran, the core and the run.
async fn crash_and_reserve(
    crash: Crash,
) -> (
    Arc<EmbeddedEdge<Yielding>>,
    Uuid,
    Vec<MutantPlan>,
    Arc<Ledger>,
    BTreeMap<String, usize>,
) {
    let store = Yielding::default();
    let core = Arc::new(new_core_on(store.clone(), OnRetryable::default()).await);
    let (run, plans) = ready_run_of(&core, SWEPT).await;
    let ledger = Arc::new(Ledger::default());
    let (_release, gate): (Sender<()>, Receiver<()>) = channel();
    let gate = Arc::new(Mutex::new(gate));
    let instances = NonZeroUsize::MIN;

    store.arm(crash);
    let live = LiveRuns::default();
    let ended = live.clone();
    let first = serve(
        core.as_ref(),
        runner(&core, &ledger, usize::MAX, &gate),
        instances,
        &live,
        |served| ended.status(&run).is_none() && !served.is_empty(),
    );
    tokio::select! {
        served = first => panic!("{crash:?}: the serve ended before the store died: {served:?}"),
        () = until(|| store.crashed()) => {}
    }
    let before = decided(&core, &plans).await;
    store.disarm();

    let live = LiveRuns::default();
    let ended = live.clone();
    serve(
        core.as_ref(),
        runner(&core, &ledger, usize::MAX, &gate),
        instances,
        &live,
        |served| ended.status(&run).is_none() && !served.is_empty(),
    )
    .await
    .unwrap();
    (core, run, plans, ledger, before)
}

#[tokio::test]
async fn test_a_serve_dropped_at_any_write_is_finished_by_a_new_serve_that_loses_and_repeats_nothing()
 {
    let reference = {
        let store = Yielding::default();
        let core = Arc::new(new_core_on(store.clone(), OnRetryable::default()).await);
        let (run, _) = ready_run_of(&core, SWEPT).await;
        let ledger = Arc::new(Ledger::default());
        let (_release, gate): (Sender<()>, Receiver<()>) = channel();
        let gate = Arc::new(Mutex::new(gate));
        store.arm(Crash::Never);
        let live = LiveRuns::default();
        let ended = live.clone();
        serve(
            core.as_ref(),
            runner(&core, &ledger, usize::MAX, &gate),
            NonZeroUsize::MIN,
            &live,
            |served| ended.status(&run).is_none() && !served.is_empty(),
        )
        .await
        .unwrap();
        let pages = core.events(&run_cover(&run), 0).await.unwrap();
        Reference {
            run_events: names_of_run(&pages),
            writes: store.writes(),
        }
    };
    assert!(
        reference.writes > SWEPT as u64,
        "the sweep has writes to cut"
    );

    for write in 1..=reference.writes {
        for crash in [Crash::Before(write), Crash::After(write)] {
            let (core, run, plans, ledger, before) = crash_and_reserve(crash).await;
            let context = format!("{crash:?}");
            // Every mutant is decided once, on its own stream.
            let after = decided(&core, &plans).await;
            assert!(after.values().all(|n| *n == 1), "{context}: {after:?}");
            // The run holds what an uninterrupted run held: each decision
            // once and one finish.
            let pages = core.events(&run_cover(&run), 0).await.unwrap();
            assert_eq!(names_of_run(&pages), reference.run_events, "{context}");
            // A snapshot is never ahead of its stream, and only the stream
            // the crash fell in (between its event write and its snapshot
            // write, with no later write to catch it up) is behind.
            let mut covers = vec![run_cover(&run)];
            covers.extend(plans.iter().map(|p| mutant_cover(&p.id.0).unwrap()));
            let mut behind = 0;
            for cover in &covers {
                let stream = StreamRef::try_from(cover).unwrap();
                let head = core.events(cover, 0).await.unwrap().len() as u64 - 1;
                let snapshot = core.store().read_snapshot(stream).await.unwrap();
                let at = snapshot.map(|s| s.sequence);
                assert!(at <= Some(head), "{context}: {cover:?} {at:?} of {head}");
                behind += usize::from(at < Some(head));
            }
            assert!(behind <= 1, "{context}: {behind} snapshots behind");
            // A mutant decided before the crash was not run again; at
            // most the one the crash caught ran twice, and none thrice.
            for (id, decisions) in &before {
                if *decisions == 1 {
                    assert_eq!(ledger.runs_of(id), 1, "{context}: {id} was decided");
                }
            }
            let twice = plans
                .iter()
                .filter(|p| ledger.runs_of(&p.id.0) == 2)
                .count();
            assert!(twice <= 1, "{context}: {twice} mutants ran twice");
            assert!(
                plans
                    .iter()
                    .all(|p| (1..=2).contains(&ledger.runs_of(&p.id.0))),
                "{context}"
            );
        }
    }
}

/// The bus follows a store through its change feed, so a run's store must
/// have one: this does not compile for a `RunStore` without it.
fn change_feed_of<S: cherenkov_embedded_edge::RunStore>(
    store: &S,
) -> &dyn angzarr_embedded::angzarr_bus::ChangeSource {
    store
}

#[tokio::test]
async fn test_a_run_on_the_bus_completes_following_the_stores_change_feed() {
    let core = Arc::new(new_core().await);
    let _feed = change_feed_of(core.store());
    let (run, _plans) = ready_run(&core).await;
    let live = LiveRuns::default();
    let ended = live.clone();
    let ledger = Arc::new(Ledger::default());
    let (_release, gate): (Sender<()>, Receiver<()>) = channel();
    let gate = Arc::new(Mutex::new(gate));
    serve(
        core.as_ref(),
        runner(&core, &ledger, usize::MAX, &gate),
        NonZeroUsize::MIN,
        &live,
        |served| ended.status(&run).is_none() && !served.is_empty(),
    )
    .await
    .unwrap();
    assert_eq!(live.status(&run), Some(RunStatus::Complete));
    assert_eq!(ledger.count(), MUTANTS, "each mutant ran once");
}

/// The runner, both policies and the live runs on the bus `core` hosts,
/// served until `more` says no.
async fn serve<S: RunStore>(
    core: &EmbeddedEdge<S>,
    runner: Runner<EmbeddedEdge<S>>,
    instances: NonZeroUsize,
    live: &LiveRuns,
    more: impl FnMut(&angzarr_embedded::angzarr_bus::Served) -> bool + Send,
) -> Result<(), crate::error::RunLayerError> {
    let runners = Some((Box::new(runner) as Box<dyn DynProjectorHandler>, instances));
    host::serve(
        core,
        host::subscriptions(live, Readers::printing(silent()), runners),
        more,
    )
    .await
}
