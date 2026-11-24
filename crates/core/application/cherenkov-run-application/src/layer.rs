//! A run on the run layer, end to end. The run is planned; unless it is a
//! dry run, its test programs are prepared, each told to the run as it is
//! ready, and the run, with its last program prepared, is ready and hands
//! its mutants on. The bus serves ([`host`]) from the start: the output
//! prints each event as the run records it (a mutant's decision is on the
//! run's stream, whole), the runners, one instance per worker, run the
//! mutants once they are handed on, and both policies move the work between
//! the run and its mutants, until the run ends.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use angzarr_embedded::angzarr_bus::Outcome;
use angzarr_embedded::v1::Uuid;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::MutantExecutor;
use cherenkov_run_ports::{Edge, Listing, Serving, Subscriptions};
use coded_diagnostic::Report;
use tokio::sync::mpsc::unbounded_channel;

use crate::covers::run_cover;
use crate::error::RunLayerError;
use crate::host::{self, Readers};
use crate::live::LiveRuns;
use crate::pool::Pool;
use crate::programs;
use crate::reason::Reason;
use crate::resume;
use crate::runner::Runner;
use crate::workers::{self, Failed, Preparation, Worker};
use cherenkov_run_domain::events::{RunEvent, run_event};
use cherenkov_run_domain::plan;
use cherenkov_run_domain::why;
use cherenkov_run_domain::{
    ExecutableMutant, OperatorSet, PlanRun, PlannedMutant, PreparationFailed, RunStatus,
};

/// What a run is planned with.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub root: String,
    pub operators: Vec<OperatorSet>,
    pub jobs: usize,
}

/// What the runner drives: one executor per test program for each
/// worker, and how many programs to prepare at once.
pub struct Workers {
    pub sets: Vec<Vec<Box<dyn MutantExecutor + Send>>>,
    pub jobs: usize,
}

/// What a started run came to: which run it was, how it ended, and whether
/// it carried on a run an earlier process left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub run: Uuid,
    /// How it ended; none for a run that has not finished (a dry run).
    pub status: Option<RunStatus>,
    pub resumed: bool,
}

impl Workers {
    /// How many test programs each worker drives.
    fn programs(&self) -> usize {
        self.sets.first().map_or(0, Vec::len)
    }
}

/// Run `plans` on `core`: the unfinished run an earlier process left, if the
/// store holds one planned as these are (the same settings and mutants, each
/// with its plan), else a new run `fresh`. Every other unfinished run is
/// abandoned first, so the store holds one live run. A run given no workers
/// (a dry run) and a run of no mutants are planned afresh and touch no other
/// run: they carry nothing on and leave nothing owed. See [`run`], which
/// plans a run that is new.
pub async fn start<C: Serving + Listing + 'static>(
    core: &Arc<C>,
    fresh: Uuid,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Started, RunLayerError> {
    let Some(workers) = workers.filter(|_| !plans.is_empty()) else {
        let status = run(core, fresh.clone(), plans, settings, None, readers).await?;
        return Ok(Started {
            run: fresh,
            status,
            resumed: false,
        });
    };
    let wanted = plan_run(plans, settings, workers.programs());
    match resume::settle(core.as_ref(), &wanted).await? {
        Some(earlier) => {
            let live = LiveRuns::default();
            let readers = readers.of(earlier.clone());
            work(core, &earlier, plans, workers, &live, readers).await?;
            Ok(Started {
                status: live.status(&earlier),
                run: earlier,
                resumed: true,
            })
        }
        None => {
            let status = run(core, fresh.clone(), plans, settings, Some(workers), readers).await?;
            Ok(Started {
                run: fresh,
                status,
                resumed: false,
            })
        }
    }
}

/// Plan `plans` as the new run `run` with `settings` on `core` and, given
/// `workers`, run it; with none, a dry run: planned, never run. The bus is
/// served from the start, and the `readers` print each event of the run the
/// moment it is recorded and write its verdict file when it finishes. Returns the status the run finished with, none
/// for a run that has not finished (a dry run). A test program that does
/// not prepare is recorded (the program, the run finished as failed)
/// before its error is returned.
pub async fn run<C: Serving + 'static>(
    core: &Arc<C>,
    run: Uuid,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Option<RunStatus>, RunLayerError> {
    let programs = workers.as_ref().map_or(0, Workers::programs);
    core.command(run_cover(&run), &plan_run(plans, settings, programs))
        .await?;
    let live = LiveRuns::default();
    let readers = readers.of(run.clone());
    match workers.filter(|_| !plans.is_empty()) {
        Some(workers) => work(core, &run, plans, workers, &live, readers).await?,
        None => print(core.as_ref(), &live, readers).await?,
    }
    Ok(live.status(&run))
}

/// Serve the output alone until it has printed what the run recorded: a dry
/// run, or a run of no mutants.
async fn print<C: Serving>(
    core: &C,
    live: &LiveRuns,
    readers: Readers,
) -> Result<(), RunLayerError> {
    host::serve(core, host::subscriptions(live, readers, None), |served| {
        !served.is_empty()
    })
    .await
}

/// Prepare the run's test programs, recording each on the run, which
/// judges them and, with the last, is ready, handing its mutants on; and,
/// meanwhile and after, serve the bus until the run ends. The first worker
/// prepares; the others are the runner's from the start, and the first
/// joins them when the preparation is done.
async fn work<C: Serving + 'static>(
    core: &Arc<C>,
    run: &Uuid,
    plans: &[MutantPlan],
    workers: Workers,
    live: &LiveRuns,
    readers: Readers,
) -> Result<(), RunLayerError> {
    let Workers { mut sets, jobs } = workers;
    if sets.is_empty() {
        return Err(RunLayerError::Workers {
            program: None,
            cause: Report::of(&Reason::NoWorkers),
        });
    }
    let first = sets.remove(0);
    if first.is_empty() {
        return Err(RunLayerError::Workers {
            program: None,
            cause: Report::of(&Reason::NoTestPrograms),
        });
    }
    let instances = NonZeroUsize::MIN.saturating_add(sets.len());
    let pool = Pool::new(sets.into_iter().map(Worker::new).collect());
    let runner = Runner::new(core.clone(), pool.clone()).knowing(live.clone());
    let subscriptions = host::subscriptions(live, readers, Some((Box::new(runner), instances)));
    let progress = Progress::default();
    let prepare = async {
        let prepared = prepare_programs(core.as_ref(), run, plans, first, jobs).await;
        progress.prepared(&prepared);
        prepared.map(|first| pool.give(Worker::new(first)))
    };
    let (prepared, served) = tokio::join!(
        prepare,
        serve(core.as_ref(), run, subscriptions, live, &progress)
    );
    prepared?;
    served
}

/// How far the preparation of the run's test programs is, for the serve
/// that waits on it. Whether the bus has seen what the preparation
/// recorded is the live runs model's to say, not this flag's: the bus may
/// be idle just before a commit and asked just after it.
#[derive(Default)]
struct Progress {
    done: AtomicBool,
    aborted: AtomicBool,
}

impl Progress {
    /// The preparation ended with `result`. A failure the run's stream says
    /// (a program that did not prepare finishes the run) is observed like
    /// the rest; any other has recorded nothing, and nothing will come.
    fn prepared<T>(&self, result: &Result<T, RunLayerError>) {
        let recorded = matches!(result, Ok(_) | Err(RunLayerError::Workers { .. }));
        self.aborted.store(!recorded, Ordering::SeqCst);
        self.done.store(true, Ordering::SeqCst);
    }

    /// Whether the bus is to go on, idle as it is. It is, until the run
    /// ends (the live runs model has seen `RunFinished`): a delivery the bus
    /// holds for its retry is served again after its delay, and a step to
    /// come is committed by a handler that has finished. It is not once the
    /// bus has given up on a delivery (`given_up`: a dead letter, which the
    /// run owes for good) or the preparation recorded nothing and will not.
    fn more(&self, live: &LiveRuns, run: &Uuid, given_up: bool) -> bool {
        if self.aborted.load(Ordering::SeqCst) || given_up {
            return false;
        }
        !self.done.load(Ordering::SeqCst) || live.status(run).is_none()
    }
}

/// Prepare the run's test programs on `first`, the first worker's set,
/// recording each on the run as it is ready; the set comes back. A program
/// the run refuses (its unmutated tests fail) or that could not be
/// prepared finishes the run, failed, and is this call's error.
async fn prepare_programs<E: Edge>(
    core: &E,
    run: &Uuid,
    plans: &[MutantPlan],
    mut first: Vec<Box<dyn MutantExecutor + Send>>,
    jobs: usize,
) -> Result<Vec<Box<dyn MutantExecutor + Send>>, RunLayerError> {
    let planned = plans.to_vec();
    let (to, mut told) = unbounded_channel();
    let preparing = tokio::task::spawn_blocking(move || {
        workers::prepare(&planned, &mut first, jobs, &mut |p| {
            let _ = to.send(p);
        });
        first
    });
    while let Some(prepared) = told.recv().await {
        match prepared {
            Ok(preparation) => report_prepared(core, run, plans, preparation).await?,
            Err(Failed { program, why }) => {
                let fact = PreparationFailed {
                    program,
                    reason: String::new(),
                    why: Some(why::encode(&why)),
                };
                core.report(run_cover(run), &fact, "preparation failed")
                    .await?;
            }
        }
    }
    let first = match preparing.await {
        Ok(first) => first,
        Err(_) => return Err(panicked(core, run).await),
    };
    refused(core, run).await?;
    Ok(first)
}

/// The error of a preparation that panicked: the run is told, which finishes
/// it failed, as for any program that does not prepare.
async fn panicked<E: Edge>(core: &E, run: &Uuid) -> RunLayerError {
    let cause = Report::of(&Reason::PreparationPanicked);
    let fact = PreparationFailed {
        program: None,
        reason: String::new(),
        why: Some(why::encode(&cause)),
    };
    match core
        .report(run_cover(run), &fact, "preparation panicked")
        .await
    {
        Ok(_) => RunLayerError::Workers {
            program: None,
            cause,
        },
        Err(failed) => failed.into(),
    }
}

/// Tell the run what preparing one program found.
async fn report_prepared<E: Edge>(
    core: &E,
    run: &Uuid,
    plans: &[MutantPlan],
    preparation: Preparation,
) -> Result<(), RunLayerError> {
    let external_id = format!("program {}", preparation.program);
    let fact = programs::report(preparation, plans);
    core.report(run_cover(run), &fact, &external_id).await?;
    Ok(())
}

/// The run's error, if its stream says a program did not prepare.
async fn refused<E: Edge>(core: &E, run: &Uuid) -> Result<(), RunLayerError> {
    let failed = core
        .events(&run_cover(run), 0)
        .await?
        .iter()
        .find_map(|page| match run_event(page) {
            Some(RunEvent::ProgramFailed(failed)) => Some(failed),
            _ => None,
        });
    failed.map_or(Ok(()), |failed| {
        Err(RunLayerError::Workers {
            program: failed.program,
            cause: why::decode(failed.why.as_ref(), &failed.reason),
        })
    })
}

/// Serve the bus until `run` ends. The bus asks whether to go on each time
/// it is idle ([`Progress::more`]); a run that did not end by then is owed
/// for good, and is reported as what the bus gave up on, else as stalled.
async fn serve<C: Serving>(
    core: &C,
    run: &Uuid,
    subscriptions: Subscriptions,
    live: &LiveRuns,
    progress: &Progress,
) -> Result<(), RunLayerError> {
    let mut failures = Vec::new();
    let ended = live.clone();
    let asking = run.clone();
    host::serve(core, subscriptions, |served| {
        failures.extend(served.failures().iter().cloned());
        let given_up = failures
            .iter()
            .any(|failed| failed.outcome != Outcome::Held);
        progress.more(&ended, &asking, given_up)
    })
    .await?;
    if live.status(run).is_some() {
        return Ok(());
    }
    Err(match failures.into_iter().next() {
        Some(failed) => RunLayerError::Kernel {
            cause: failed.cause,
        },
        None => RunLayerError::Stalled {
            owed: u64::try_from(live.owed(run)).unwrap_or(u64::MAX),
        },
    })
}

/// The command that plans `plans` with `settings`, for a run that prepares
/// `programs` test programs before its mutants run (none: a dry run).
pub(crate) fn plan_run(plans: &[MutantPlan], settings: &Settings, programs: usize) -> PlanRun {
    PlanRun {
        programs: u32::try_from(programs).unwrap_or(u32::MAX),
        root: settings.root.clone(),
        operators: settings.operators.clone(),
        jobs: u32::try_from(settings.jobs).unwrap_or(u32::MAX),
        fast: false,
        mutants: plans.iter().map(planned).collect(),
    }
}

/// A plan as the run records it: the mutant as a reader needs it, and the
/// plan as a runner needs it.
fn planned(plan: &MutantPlan) -> ExecutableMutant {
    ExecutableMutant {
        mutant: Some(summary(plan)),
        plan: Some(plan::encode(plan)),
    }
}

/// A plan as a reader needs it: where it is and what it changes.
fn summary(plan: &MutantPlan) -> PlannedMutant {
    PlannedMutant {
        id: plan.id.0.clone(),
        // As the plan's own listing spells it, on every platform.
        file: plan.file.to_string_lossy().replace('\\', "/"),
        line: plan.stop_at.line,
        column: plan.stop_at.col,
        end_line: plan.site.end.line,
        end_column: plan.site.end.col,
        function: plan.function.clone(),
        operator: plan.operator.to_string(),
        description: plan.change(),
        original: plan.original.clone(),
        mutated: plan.mutated.clone(),
    }
}

#[cfg(test)]
#[path = "layer.test.rs"]
mod tests;
