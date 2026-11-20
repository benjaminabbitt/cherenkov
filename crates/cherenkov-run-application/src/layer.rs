//! A run on the run layer, end to end. The run is planned; unless it is a
//! dry run, its test programs are prepared, each told to the run as it is
//! ready, and the run, with its last program prepared, is ready and hands
//! its mutants on. Then the bus serves ([`host`]) the runners, one instance
//! per worker, and both policies, until the run ends. Meanwhile the run's
//! stream is followed, and each event's documents handed to a [`Sink`] the
//! moment it is recorded; a mutant's decision is printed from its own
//! stream as the run records it.

use std::cell::Cell;
use std::io::Write;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use angzarr_embedded::OnRetryable;
use angzarr_embedded::v1::{EventPage, Uuid};
use cherenkov_core::execution::MutantExecutor;
use cherenkov_core::plan::MutantPlan;
use tokio::sync::mpsc::unbounded_channel;

use crate::core::{RunCore, RunStore, mutant_cover, new_run, run_cover};
use crate::error::RunLayerError;
use crate::follow::Follow;
use crate::host;
use crate::live::LiveRuns;
use crate::programs;
use crate::render::documents;
use crate::runner::Runner;
use crate::workers::{self, Explain, Failed, Preparation, Worker};
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::plan;
use cherenkov_run_domain::{
    ExecutableMutant, PlanRun, PlannedMutant, PreparationFailed, RunStatus,
};
use cherenkov_run_wire::output::Document;

/// How often the follower looks at the store when no commit woke it.
const POLL: Duration = host::POLL;

/// What a run is planned with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub root: String,
    pub language: String,
    pub operators: String,
    pub jobs: usize,
}

/// What the runner drives: one executor per test program for each
/// worker, how many programs to prepare at once, and how a test program's
/// output names its failing tests.
pub struct Workers {
    pub sets: Vec<Vec<Box<dyn MutantExecutor + Send>>>,
    pub jobs: usize,
    pub explain: Explain,
}

/// Where a followed run's documents go, each as soon as it is recorded.
pub trait Sink {
    fn document(&mut self, doc: &Document);
}

/// A sink writing each document as one line of JSON, flushed.
pub struct JsonLines<W: Write>(pub W);

impl<W: Write> Sink for JsonLines<W> {
    fn document(&mut self, doc: &Document) {
        if let Ok(line) = serde_json::to_string(doc) {
            let _ = writeln!(self.0, "{line}");
        }
        let _ = self.0.flush();
    }
}

/// Plan `plans` as a run with `settings` on `store` and, given `workers`,
/// run it; with none, a dry run: planned, never run. Hands every event of
/// the run to `sink`; returns the status the run finished with, none for a
/// run that has not finished (a dry run). A test program that
/// does not prepare is recorded (the program, the run finished as failed)
/// before its error is returned.
pub fn run<S: RunStore>(
    store: S,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    sink: &mut dyn Sink,
) -> Result<Option<RunStatus>, RunLayerError> {
    run_retrying(
        store,
        OnRetryable::default(),
        plans,
        settings,
        workers,
        sink,
    )
}

/// [`run`], the kernel retrying as `retry` says.
pub fn run_retrying<S: RunStore>(
    store: S,
    retry: OnRetryable,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    sink: &mut dyn Sink,
) -> Result<Option<RunStatus>, RunLayerError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| RunLayerError::Runtime {
            reason: e.to_string(),
        })?;
    runtime.block_on(async {
        let core = Arc::new(RunCore::new(store)?.retrying(retry));
        let run = new_run();
        let programs = workers
            .as_ref()
            .map_or(0, |w| w.sets.first().map_or(0, Vec::len));
        core.command(run_cover(&run), &plan_run(plans, settings, programs))
            .await?;
        let mut printer = Printer::new(run.clone(), sink);
        let Some(workers) = workers else {
            printer.catch_up(&core).await?;
            return Ok(printer.status);
        };
        let stopped = Cell::new(false);
        let live = LiveRuns::default();
        let work = async {
            let worked = work(&core, &run, plans, workers, &live).await;
            stopped.set(true);
            worked
        };
        let (worked, followed) = tokio::join!(work, printer.follow(&core, &stopped));
        followed?;
        worked?;
        Ok(printer.status)
    })
}

/// Prepare the run's test programs, recording each on the run, which
/// judges them and, with the last, is ready, handing its mutants on; then
/// serve the bus until the run is `finished`.
async fn work<S: RunStore>(
    core: &Arc<RunCore<S>>,
    run: &Uuid,
    plans: &[MutantPlan],
    workers: Workers,
    live: &LiveRuns,
) -> Result<(), RunLayerError> {
    if plans.is_empty() {
        return Ok(());
    }
    let Workers {
        mut sets,
        jobs,
        explain,
    } = workers;
    if sets.is_empty() {
        return Err(RunLayerError::Workers {
            reason: "no workers".to_string(),
        });
    }
    let first = prepare_programs(core, run, plans, sets.remove(0), jobs, explain).await?;
    sets.insert(0, first);
    let instances = NonZeroUsize::new(sets.len()).unwrap_or(NonZeroUsize::MIN);
    let workers = sets.into_iter().map(Worker::new).collect();
    let runner = Runner::new(core.clone(), explain, workers);
    serve(core, run, runner, instances, live).await
}

/// Prepare the run's test programs on `first`, the first worker's set,
/// recording each on the run as it is ready; the set comes back. A program
/// the run refuses (its unmutated tests fail) or that could not be
/// prepared finishes the run, failed, and is this call's error.
async fn prepare_programs<S: RunStore>(
    core: &Arc<RunCore<S>>,
    run: &Uuid,
    plans: &[MutantPlan],
    mut first: Vec<Box<dyn MutantExecutor + Send>>,
    jobs: usize,
    explain: Explain,
) -> Result<Vec<Box<dyn MutantExecutor + Send>>, RunLayerError> {
    let planned = plans.to_vec();
    let (to, mut told) = unbounded_channel();
    let preparing = tokio::task::spawn_blocking(move || {
        workers::prepare(&planned, &mut first, jobs, explain, &mut |p| {
            let _ = to.send(p);
        });
        first
    });
    while let Some(prepared) = told.recv().await {
        match prepared {
            Ok(preparation) => report_prepared(core, run, plans, preparation).await?,
            Err(Failed { program, reason }) => {
                let fact = PreparationFailed { program, reason };
                core.report(run_cover(run), &fact, "preparation failed")
                    .await?;
            }
        }
    }
    let first = preparing.await.map_err(|_| RunLayerError::Workers {
        reason: "a test program's preparation panicked".to_string(),
    })?;
    refused(core, run).await?;
    Ok(first)
}

/// Tell the run what preparing one program found.
async fn report_prepared<S: RunStore>(
    core: &Arc<RunCore<S>>,
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
async fn refused<S: RunStore>(core: &RunCore<S>, run: &Uuid) -> Result<(), RunLayerError> {
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
            reason: failed.reason,
        })
    })
}

/// Serve the bus until `run` ends. The bus asks whether to go on each time
/// it is idle, which is when nothing is in flight and nothing is owed but
/// what failed: the run has ended, and that is done; or it is served what
/// it can (a failure it holds and retries, a step to come); or it served
/// nothing at all, and nothing will: a mutant the bus gave up on (a dead
/// letter) is owed by the run for good.
async fn serve<S: RunStore>(
    core: &Arc<RunCore<S>>,
    run: &Uuid,
    runner: Runner<S>,
    instances: NonZeroUsize,
    live: &LiveRuns,
) -> Result<(), RunLayerError> {
    let mut failures = Vec::new();
    let ended = live.clone();
    let asking = run.clone();
    host::serve(core, runner, instances, live, |served| {
        failures.extend(served.failures().iter().cloned());
        ended.status(&asking).is_none() && !served.is_empty()
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
fn plan_run(plans: &[MutantPlan], settings: &Settings, programs: usize) -> PlanRun {
    PlanRun {
        programs: u32::try_from(programs).unwrap_or(u32::MAX),
        root: settings.root.clone(),
        language: settings.language.clone(),
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

/// Follows the run's stream and hands each event to the sink as its
/// documents, a mutant's decision as its own stream holds it; keeps the
/// status the run finished with.
struct Printer<'a> {
    run: Uuid,
    follow: Follow,
    sink: &'a mut dyn Sink,
    status: Option<RunStatus>,
    finished: bool,
}

impl<'a> Printer<'a> {
    fn new(run: Uuid, sink: &'a mut dyn Sink) -> Printer<'a> {
        Printer {
            follow: Follow::new(run_cover(&run)),
            run,
            sink,
            status: None,
            finished: false,
        }
    }

    /// Print what the run recorded since the last look, until it finishes
    /// or the work `stopped`. Woken by each commit, and by the poll.
    async fn follow<S: RunStore>(
        &mut self,
        core: &RunCore<S>,
        stopped: &Cell<bool>,
    ) -> Result<(), RunLayerError> {
        loop {
            // Armed before reading: a commit after this resolves it.
            let commit = core.store().next_commit();
            let stop = stopped.get();
            self.catch_up(core).await?;
            if self.finished || stop {
                return Ok(());
            }
            tokio::select! {
                () = commit => {}
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }

    /// Print what the run recorded since the last look.
    async fn catch_up<S: RunStore>(&mut self, core: &RunCore<S>) -> Result<(), RunLayerError> {
        for page in self.follow.next(core).await? {
            match run_event(&page) {
                Some(RunEvent::DecisionRecorded(decided)) => {
                    let detail = self.decision(core, &decided.mutant).await?;
                    self.print(&detail.unwrap_or(page));
                    continue;
                }
                Some(RunEvent::RunFinished(finished)) => {
                    self.status = Some(finished.status);
                    self.finished = true;
                }
                _ => {}
            }
            self.print(&page);
        }
        Ok(())
    }

    /// The page of mutant `id`'s stream that decides it for this run.
    async fn decision<S: RunStore>(
        &self,
        core: &RunCore<S>,
        id: &str,
    ) -> Result<Option<EventPage>, RunLayerError> {
        let pages = core.events(&mutant_cover(id)?, 0).await?;
        Ok(pages.into_iter().find(|page| {
            matches!(mutant_event(page),
                Some(MutantEvent::MutantDecided(d)) if d.run.as_ref().map(|run| &run.0) == Some(&self.run))
        }))
    }

    fn print(&mut self, page: &EventPage) {
        for doc in documents(&self.run, page) {
            self.sink.document(&doc);
        }
    }
}

#[cfg(test)]
#[path = "layer.test.rs"]
mod tests;
