//! A run on the durable store, its serve dropped at any one event write and
//! served again on the same file: no mutant is lost, none is decided twice,
//! none that was decided is run again (but the one the crash caught), and
//! the run's stream is the stream of a run that was never interrupted.
//!
//! The serve dies at write `n` in two ways: before the write reaches the
//! store (it does not land), and after it landed with its answer lost. Every
//! `n` of a run is tried, each on a file of its own. `killed.rs` kills a real
//! process; this reaches each commit boundary, which a kill at a moment
//! cannot be made to.

use std::mem::Discriminant;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod common;

use angzarr_embedded::OnRetryable;
use angzarr_embedded::angzarr_ports::{ListStreams, SnapshotStore, StreamRef};
use cherenkov_mutation::execution::{Budget, Observed};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_application::covers::{mutant_cover, run_cover};
use cherenkov_run_application::layer::{self, Started, Workers};
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::{RUN_DOMAIN, RunStatus};
use cherenkov_run_ports::Edge;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_wiring::Sampling;
use common::crashing::{Crash, Crashing, Fate};
use common::{Kept, exited, plans_of, readers, settings};
use test_dir::TestDir;

const MUTANTS: usize = 3;

/// Every program run, by mutant id, in the order it began.
type Ledger = Arc<Mutex<Vec<String>>>;

/// Kills the mutants in `a`, lets the rest survive, and notes each it runs.
struct Counted(Ledger);

impl MutantExecutor for Counted {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        self.0.lock().unwrap().push(plan.id.0.clone());
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101).into(),
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

fn workers(ledger: &Ledger) -> Workers {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Counted(ledger.clone()));
    Workers {
        sets: vec![vec![executor]],
        jobs: 1,
    }
}

type Core = cherenkov_embedded_edge::EmbeddedEdge<Crashing<cherenkov_run_store::SqliteStore>>;

/// A core over the store file at `path`, dying as `fate` says.
async fn core_on(path: &std::path::Path, fate: &Arc<Fate>, sampling: &Sampling) -> Arc<Core> {
    let store = cherenkov_run_store::open(path).await.unwrap();
    let store = Crashing::new(store, fate.clone());
    Arc::new(
        cherenkov_wiring::core(store, OnRetryable::default(), sampling)
            .await
            .unwrap(),
    )
}

async fn started(core: &Arc<Core>, ledger: &Ledger, sampling: &Sampling) -> Started {
    let plans = plans_of(MUTANTS);
    layer::start(
        core,
        sampling.run(),
        &plans,
        &settings(),
        Some(workers(ledger)),
        readers(&Kept::default()),
    )
    .await
    .unwrap()
}

/// Wait, a tick at a time, until `done`, for as long as a test may take.
async fn until(mut done: impl FnMut() -> bool) {
    let waiting = async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), waiting)
        .await
        .expect("the condition came about");
}

/// The kinds of the events of `run`, in order.
async fn run_kinds(core: &Core, run: &angzarr_embedded::v1::Uuid) -> Vec<Discriminant<RunEvent>> {
    let pages = core.events(&run_cover(run), 0).await.unwrap();
    pages
        .iter()
        .filter_map(run_event)
        .map(|event| std::mem::discriminant(&event))
        .collect()
}

/// How many decisions each mutant's stream holds.
async fn decisions(core: &Core) -> Vec<usize> {
    let mut counts = Vec::new();
    for plan in plans_of(MUTANTS) {
        let pages = core
            .events(&mutant_cover(&plan.id.0).unwrap(), 0)
            .await
            .unwrap();
        counts.push(
            pages
                .iter()
                .filter_map(mutant_event)
                .filter(|event| matches!(event, MutantEvent::MutantDecided(_)))
                .count(),
        );
    }
    counts
}

/// How many run streams the store holds.
async fn run_streams(core: &Core) -> usize {
    let streams = core.store().list_streams().await.unwrap();
    streams
        .iter()
        .filter(|cover| cover.domain == RUN_DOMAIN)
        .count()
}

/// The run the store holds, if it holds one.
async fn first_run(core: &Core) -> Option<angzarr_embedded::v1::Uuid> {
    let streams = core.store().list_streams().await.unwrap();
    streams
        .into_iter()
        .find(|cover| cover.domain == RUN_DOMAIN)
        .and_then(|cover| cover.root)
}

/// The streams whose snapshot is behind their head, having asserted that
/// none is ahead of it.
async fn snapshots_behind(core: &Core, run: &angzarr_embedded::v1::Uuid, context: &str) -> usize {
    let mut covers = vec![run_cover(run)];
    covers.extend(
        plans_of(MUTANTS)
            .iter()
            .map(|p| mutant_cover(&p.id.0).unwrap()),
    );
    let mut behind = 0;
    for cover in &covers {
        let stream = StreamRef::try_from(cover).unwrap();
        let head = core.events(cover, 0).await.unwrap().len() as u64 - 1;
        let snapshot = core.store().read_snapshot(stream).await.unwrap();
        let at = snapshot.map(|s| s.sequence);
        assert!(at <= Some(head), "{context}: {cover:?} {at:?} of {head}");
        behind += usize::from(at < Some(head));
    }
    behind
}

/// What an uninterrupted run left: the kinds of its run stream's events and
/// how many event writes it took.
struct Reference {
    run_kinds: Vec<Discriminant<RunEvent>>,
    writes: u64,
}

async fn reference() -> Reference {
    let dir = TestDir::new("cherenkov-crash-reference");
    let (fate, sampling) = (
        Arc::new(Fate::default()),
        cherenkov_wiring::testing::sampling(),
    );
    let core = core_on(&dir.join("runs.sqlite"), &fate, &sampling).await;
    fate.arm(Crash::Never);
    let ledger = Ledger::default();
    let whole = started(&core, &ledger, &sampling).await;
    assert_eq!(whole.status, Some(RunStatus::Complete));
    Reference {
        run_kinds: run_kinds(&core, &whole.run).await,
        writes: fate.writes(),
    }
}

/// What the first serve left when it died.
struct Aftermath {
    /// How many decisions each mutant's stream held.
    decided: Vec<usize>,
    first_run: Option<angzarr_embedded::v1::Uuid>,
    /// Whether the run it left was over.
    finished: bool,
}

/// The serve on a store armed with `crash`, dropped when the store dies.
async fn serve_until_dead(
    path: &std::path::Path,
    crash: Crash,
    ledger: &Ledger,
    sampling: &Sampling,
    reference: &Reference,
) -> Aftermath {
    let fate = Arc::new(Fate::default());
    let core = core_on(path, &fate, sampling).await;
    fate.arm(crash);
    tokio::select! {
        done = started(&core, ledger, sampling) => panic!("{crash:?}: the serve ended before the store died: {done:?}"),
        () = until(|| fate.crashed()) => {}
    }
    let first_run = first_run(&core).await;
    let finished = match &first_run {
        Some(run) => run_kinds(&core, run).await == reference.run_kinds,
        None => false,
    };
    Aftermath {
        decided: decisions(&core).await,
        first_run,
        finished,
    }
}

/// A mutant decided before the crash was not run again; at most the one the
/// crash caught ran twice, and none ran thrice or not at all.
fn assert_run_no_more_than_the_crash_caught(ran: &[String], decided: &[usize], context: &str) {
    let plans = plans_of(MUTANTS);
    let runs_of = |id: &str| ran.iter().filter(|ran| *ran == id).count();
    for (plan, decided) in plans.iter().zip(decided) {
        let times = runs_of(&plan.id.0);
        assert!(
            (1..=2).contains(&times),
            "{context}: {} ran {times} times",
            plan.id.0
        );
        if *decided == 1 {
            assert_eq!(
                times, 1,
                "{context}: {} was decided before the crash",
                plan.id.0
            );
        }
    }
    let twice = plans.iter().filter(|p| runs_of(&p.id.0) == 2).count();
    assert!(twice <= 1, "{context}: {twice} mutants ran twice");
}

/// The serve dropped at `crash`, then served again on the same file.
async fn crash_then_serve_again(crash: Crash, reference: &Reference) {
    let context = format!("{crash:?}");
    let dir = TestDir::new("cherenkov-crash");
    let path = dir.join("runs.sqlite");
    let sampling = cherenkov_wiring::testing::sampling();
    let ledger = Ledger::default();
    let left = serve_until_dead(&path, crash, &ledger, &sampling, reference).await;

    let core = core_on(&path, &Arc::new(Fate::default()), &sampling).await;
    let again = started(&core, &ledger, &sampling).await;
    assert_eq!(again.status, Some(RunStatus::Complete), "{context}");

    // A crash after the run's last write leaves a run that is over: the
    // serve that follows is a new run, as after any finished one, and the
    // run that was over is as an uninterrupted run's stream says.
    if left.finished {
        assert!(!again.resumed, "{context}");
        assert_eq!(run_streams(&core).await, 2, "{context}");
        let run = left.first_run.unwrap();
        assert_eq!(
            run_kinds(&core, &run).await,
            reference.run_kinds,
            "{context}"
        );
        return;
    }
    assert_eq!(again.resumed, left.first_run.is_some(), "{context}");
    // One run, as an uninterrupted one's stream says it ended.
    assert_eq!(run_streams(&core).await, 1, "{context}");
    assert_eq!(
        run_kinds(&core, &again.run).await,
        reference.run_kinds,
        "{context}"
    );
    // Each mutant decided once.
    assert_eq!(decisions(&core).await, vec![1; MUTANTS], "{context}");
    let ran = ledger.lock().unwrap().clone();
    assert_run_no_more_than_the_crash_caught(&ran, &left.decided, &context);
    // A snapshot is never ahead of its stream, and only the stream the
    // crash fell in (between its event write and its snapshot, with no
    // later write to catch it up) is behind.
    let behind = snapshots_behind(&core, &again.run, &context).await;
    assert!(behind <= 1, "{context}: {behind} snapshots behind");
}

/// The sweep's futures hold a whole run each, which a debug build lays out
/// past the default test stack: it runs on a thread of its own.
#[test]
fn test_a_serve_dropped_at_any_write_is_finished_on_the_same_file_losing_and_repeating_nothing() {
    let sweep = || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let reference = reference().await;
            assert!(
                reference.writes > MUTANTS as u64,
                "the sweep has writes to cut"
            );
            for write in 1..=reference.writes {
                for crash in [Crash::Before(write), Crash::After(write)] {
                    crash_then_serve_again(crash, &reference).await;
                }
            }
        });
    };
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(sweep)
        .unwrap()
        .join()
        .unwrap();
}
