//! A restart: the bus served, stopped half way, and served again by a new
//! bus on the same store. What the stored positions and the streams hold is
//! all the second serve has, as after a crash on a durable store (phase 5.7
//! adds the store; the code is the same): nothing a runner finished is run
//! again, nothing a policy owed is lost, and the run ends as it would have.

use crate::testing::{Core, new_core, new_run};
use cherenkov_run_ports::Edge;

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::v1::Uuid;
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};
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
use crate::workers::{Killer, Worker};

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
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        if self.ledger.begin(&plan.id.0) == self.hold {
            // Released when the test lets go of the gate's sender.
            let _ = self.gate.lock().unwrap().recv();
        }
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101),
            _ => exited(0),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }
}

fn explain(_: &str) -> Vec<Killer> {
    Vec::new()
}

/// A runner of one worker over `core`, whose executor holds on its `hold`th
/// run of the test.
fn runner(
    core: &Arc<Core>,
    ledger: &Arc<Ledger>,
    hold: usize,
    gate: &Arc<Mutex<Receiver<()>>>,
) -> Runner<Core> {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Executor {
        ledger: ledger.clone(),
        hold,
        gate: gate.clone(),
    });
    Runner::new(
        core.clone(),
        explain,
        Pool::new(vec![Worker::new(vec![executor])]),
    )
}

/// A run of `MUTANTS` mutants, planned and ready: its one program prepared,
/// and nothing served yet.
async fn ready_run(core: &Core) -> (Uuid, Vec<MutantPlan>) {
    let plans = many(MUTANTS);
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
async fn decided(core: &Core, plans: &[MutantPlan]) -> BTreeMap<String, usize> {
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
async fn serve(
    core: &Core,
    runner: Runner<Core>,
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
