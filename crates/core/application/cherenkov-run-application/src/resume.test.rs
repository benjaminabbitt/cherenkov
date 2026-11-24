//! Starting a run on a store that holds earlier ones: an unfinished run of
//! the same plan is resumed, any other unfinished run is abandoned, a
//! finished run is left alone, and what a run prints and writes is its own.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::v1::Uuid;
use cherenkov_mutation::execution::{Budget, RawOutcome, Tier};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::{RunStatus, Verdict};
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_run_ports::{Edge, Report};
use cherenkov_run_wire::output::VerdictLine;

use crate::covers::{mutant_cover, run_cover};
use crate::host::Readers;
use crate::layer::{Settings, Started, Workers, plan_run, start};
use crate::test_plans::many;
use crate::testing::{Core, new_core, new_run, recording, silent};
use crate::verdicts::VerdictFile;
use crate::workers::Killer;

const MUTANTS: usize = 6;

/// Every run of an executor, in order: the ids of the plans it was handed.
#[derive(Default)]
struct Ledger(Mutex<Vec<String>>);

impl Ledger {
    fn begin(&self, id: &str) -> usize {
        let mut ran = self.0.lock().unwrap();
        ran.push(id.to_string());
        ran.len()
    }

    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn runs_of(&self, id: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|ran| *ran == id)
            .count()
    }
}

/// Kills the mutants in `a`; on its `hold`th run of the ledger, waits until
/// the gate's sender is dropped: a mutant the crash caught running.
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

/// One worker, holding on the ledger's `hold`th run.
fn workers(ledger: &Arc<Ledger>, hold: usize, gate: &Arc<Mutex<Receiver<()>>>) -> Workers {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Executor {
        ledger: ledger.clone(),
        hold,
        gate: gate.clone(),
    });
    Workers {
        sets: vec![vec![executor]],
        jobs: 1,
        explain,
    }
}

/// The one operator set of a run.
fn operators(language: &str, set: &str) -> Vec<cherenkov_run_domain::OperatorSet> {
    vec![cherenkov_run_domain::OperatorSet {
        language: language.to_string(),
        set: set.to_string(),
    }]
}

fn settings() -> Settings {
    Settings {
        root: "/project".to_string(),
        operators: operators("rust", "breadth"),
        jobs: 1,
    }
}

/// Workers that never hold.
fn free(ledger: &Arc<Ledger>) -> Workers {
    let (_, gate) = channel();
    workers(ledger, usize::MAX, &Arc::new(Mutex::new(gate)))
}

/// Start a run of `plans` on `core` with the given workers, printing nothing.
async fn started(
    core: &Arc<Core>,
    fresh: &Uuid,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
) -> Started {
    start(
        core,
        fresh.clone(),
        plans,
        settings,
        workers,
        Readers::printing(silent()),
    )
    .await
    .unwrap()
}

/// The run's events.
async fn events(core: &Core, run: &Uuid) -> Vec<RunEvent> {
    core.events(&run_cover(run), 0)
        .await
        .unwrap()
        .iter()
        .filter_map(run_event)
        .collect()
}

/// How the run ended, if its stream says it did.
fn ended(events: &[RunEvent]) -> Option<RunStatus> {
    events.iter().find_map(|event| match event {
        RunEvent::RunFinished(finished) => Some(finished.status),
        _ => None,
    })
}

/// A run planned by an earlier process that died before doing anything
/// more: planned for one test program, with the settings and plans given.
async fn earlier(core: &Core, plans: &[MutantPlan], settings: &Settings) -> Uuid {
    let run = new_run();
    core.command(run_cover(&run), &plan_run(plans, settings, 1))
        .await
        .unwrap();
    run
}

/// Start a run and let it go on until `ledger` shows `hold` mutants begun,
/// then drop it, as a crash does, and let the held executor go.
async fn crashed_at(
    core: &Arc<Core>,
    ledger: &Arc<Ledger>,
    hold: usize,
    plans: &[MutantPlan],
) -> Uuid {
    let (release, gate): (Sender<()>, Receiver<()>) = channel();
    let gate = Arc::new(Mutex::new(gate));
    let fresh = new_run();
    let silent = Readers::printing(silent());
    let asked = settings();
    let first = start(
        core,
        fresh.clone(),
        plans,
        &asked,
        Some(workers(ledger, hold, &gate)),
        silent,
    );
    let waiting = async {
        while ledger.count() < hold {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::select! {
        done = first => panic!("the first run ended on its own: {:?}", done.map(|s| s.status)),
        timed = tokio::time::timeout(Duration::from_secs(30), waiting) => timed.expect("the mutants began"),
    }
    drop(release);
    fresh
}

#[tokio::test]
async fn test_a_run_killed_part_way_is_resumed_by_the_next_with_the_same_plan() {
    let core = Arc::new(new_core().await);
    let plans = many(MUTANTS);
    let ledger = Arc::new(Ledger::default());
    let first = crashed_at(&core, &ledger, 4, &plans).await;
    let before = ledger.count();
    assert_eq!(before, 4);

    let (output, printed) = recording();
    let again = start(
        &core,
        new_run(),
        &plans,
        &settings(),
        Some(free(&ledger)),
        Readers::printing(output),
    )
    .await
    .unwrap();

    assert!(again.resumed, "it carried on the run it found");
    assert_eq!(again.run, first);
    assert_eq!(again.status, Some(RunStatus::Complete));
    let run = events(&core, &first).await;
    let decisions = run
        .iter()
        .filter(|e| matches!(e, RunEvent::DecisionRecorded(_)))
        .count();
    let starts = run
        .iter()
        .filter(|e| matches!(e, RunEvent::RunStarted(_)))
        .count();
    assert_eq!((decisions, starts), (MUTANTS, 1));
    // Three were decided before the crash, and ran once; the fourth, caught
    // running, ran again; the rest ran once. No mutant is lost.
    assert_eq!(ledger.count(), MUTANTS + 1);
    let twice = plans
        .iter()
        .filter(|p| ledger.runs_of(&p.id.0) == 2)
        .count();
    assert_eq!(twice, 1, "only the mutant the crash caught");
    // The output printed the run from its start to its finish.
    let printed = printed.lock().unwrap();
    assert_eq!(printed.first(), Some(&"run.started"));
    assert_eq!(printed.last(), Some(&"run.finished"));
    assert_eq!(
        printed.iter().filter(|n| **n == "mutant.decided").count(),
        MUTANTS
    );
}

#[tokio::test]
async fn test_a_run_with_another_plan_abandons_the_unfinished_one_and_never_runs_its_mutants() {
    let core = Arc::new(new_core().await);
    let plans = many(MUTANTS);
    let ledger = Arc::new(Ledger::default());
    let first = crashed_at(&core, &ledger, 4, &plans).await;

    let fewer = &plans[..4];
    let again = started(&core, &new_run(), fewer, &settings(), Some(free(&ledger))).await;

    assert!(!again.resumed);
    assert_ne!(again.run, first);
    assert_eq!(again.status, Some(RunStatus::Complete));
    let old = events(&core, &first).await;
    let [
        ..,
        RunEvent::RunAbandoned(_),
        RunEvent::RunFinished(finished),
    ] = &old[..]
    else {
        panic!(
            "the old run's tail: {:?}",
            old.iter().rev().take(2).collect::<Vec<_>>()
        );
    };
    assert_eq!(finished.status, RunStatus::Abandoned);
    // The new run's four mutants ran for it; the old run's pending ones
    // did not run for the old run: 4 before the crash, 4 now.
    assert_eq!(ledger.count(), 4 + 4);
    // The old run is not decided any further: its stream did not grow.
    assert_eq!(events(&core, &first).await.len(), old.len());
}

#[tokio::test]
async fn test_settings_that_change_what_is_run_abandon_the_unfinished_run_and_the_workers_do_not() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let planned = earlier(&core, &plans, &settings()).await;

    let more_workers = Settings {
        jobs: 8,
        ..settings()
    };
    let ledger = Arc::new(Ledger::default());
    let same = started(
        &core,
        &new_run(),
        &plans,
        &more_workers,
        Some(free(&ledger)),
    )
    .await;
    assert!(same.resumed, "a different worker count is the same run");
    assert_eq!(same.run, planned);

    for changed in [
        Settings {
            operators: operators("rust", "all"),
            ..settings()
        },
        Settings {
            operators: operators("go", "breadth"),
            ..settings()
        },
        Settings {
            root: "/other".into(),
            ..settings()
        },
    ] {
        let unfinished = earlier(&core, &plans, &settings()).await;
        let ran = started(&core, &new_run(), &plans, &changed, Some(free(&ledger))).await;
        assert!(!ran.resumed, "{changed:?}");
        assert_eq!(
            ended(&events(&core, &unfinished).await),
            Some(RunStatus::Abandoned)
        );
    }
}

#[tokio::test]
async fn test_of_two_unfinished_runs_of_the_plan_the_newest_is_resumed_and_the_older_abandoned() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let older = earlier(&core, &plans, &settings()).await;
    let newer = earlier(&core, &plans, &settings()).await;
    let ledger = Arc::new(Ledger::default());
    let ran = started(&core, &new_run(), &plans, &settings(), Some(free(&ledger))).await;
    assert_eq!(ran.run, newer);
    assert_eq!(
        ended(&events(&core, &older).await),
        Some(RunStatus::Abandoned)
    );
}

#[tokio::test]
async fn test_a_finished_run_is_never_resumed_or_reprinted_and_a_run_after_it_starts_afresh() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let ledger = Arc::new(Ledger::default());
    let first = started(&core, &new_run(), &plans, &settings(), Some(free(&ledger))).await;
    assert_eq!(first.status, Some(RunStatus::Complete));

    let (output, printed) = recording();
    let written = Written::default();
    let readers = Readers::new(output, Some(Arc::new(written.clone())));
    let second = start(
        &core,
        new_run(),
        &plans,
        &settings(),
        Some(free(&ledger)),
        readers,
    )
    .await
    .unwrap();

    assert!(!second.resumed);
    assert_ne!(second.run, first.run);
    assert_eq!(second.status, Some(RunStatus::Complete));
    assert_eq!(ledger.count(), 4, "both runs ran their two mutants");
    // The second run printed itself and nothing of the first.
    let printed = printed.lock().unwrap().clone();
    assert_eq!(printed.iter().filter(|n| **n == "run.started").count(), 1);
    assert_eq!(printed.iter().filter(|n| **n == "run.finished").count(), 1);
    // And wrote the file once, for itself.
    let writes = written.0.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].len(), 2);
    // The first run's stream is as it ended.
    assert_eq!(
        ended(&events(&core, &first.run).await),
        Some(RunStatus::Complete)
    );
}

#[tokio::test]
async fn test_a_mutant_two_runs_planned_is_decided_for_each_of_them() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let ledger = Arc::new(Ledger::default());
    for _ in 0..2 {
        started(&core, &new_run(), &plans, &settings(), Some(free(&ledger))).await;
    }
    let pages = core
        .events(&mutant_cover(&plans[0].id.0).unwrap(), 0)
        .await
        .unwrap();
    let decided = pages
        .iter()
        .filter(|page| {
            matches!(mutant_event(page), Some(MutantEvent::MutantDecided(d))
            if d.verdict.as_ref().map(|o| o.verdict) == Some(Verdict::Killed))
        })
        .count();
    assert_eq!(decided, 2);
}

#[tokio::test]
async fn test_a_dry_run_leaves_the_runs_of_a_store_as_they_are() {
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let unfinished = earlier(&core, &plans, &settings()).await;
    let other = Settings {
        operators: operators("rust", "all"),
        ..settings()
    };
    let dry = started(&core, &new_run(), &plans, &other, None).await;
    assert!(!dry.resumed);
    assert_eq!(dry.status, None);
    assert_eq!(
        ended(&events(&core, &unfinished).await),
        None,
        "not abandoned"
    );
}

/// A file that keeps what it is written.
#[derive(Clone, Default)]
struct Written(Arc<Mutex<Vec<Vec<VerdictLine>>>>);

impl VerdictFile for Written {
    fn write(&self, lines: &[VerdictLine]) -> Result<(), Report> {
        self.0.lock().unwrap().push(lines.to_vec());
        Ok(())
    }
}

#[tokio::test]
async fn test_a_run_planned_for_one_program_is_not_the_run_of_workers_driving_two() {
    // How many test programs a run prepares is part of its plan: a project
    // whose tests are now built as two programs is not the one that was run.
    let core = Arc::new(new_core().await);
    let plans = many(2);
    let planned = earlier(&core, &plans, &settings()).await;
    let ledger = Arc::new(Ledger::default());
    let (_, gate) = channel();
    let gate = Arc::new(Mutex::new(gate));
    let two = |ledger: &Arc<Ledger>| {
        let program = || -> Box<dyn MutantExecutor + Send> {
            Box::new(Executor {
                ledger: ledger.clone(),
                hold: usize::MAX,
                gate: gate.clone(),
            })
        };
        Workers {
            sets: vec![vec![program(), program()]],
            jobs: 1,
            explain,
        }
    };
    let ran = started(&core, &new_run(), &plans, &settings(), Some(two(&ledger))).await;
    assert!(!ran.resumed);
    assert_ne!(ran.run, planned);
    assert_eq!(
        ended(&events(&core, &planned).await),
        Some(RunStatus::Abandoned)
    );
    assert_eq!(ran.status, Some(RunStatus::Complete));
}
