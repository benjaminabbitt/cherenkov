//! The runner: a projector on the bus, subscribed to the mutant domain
//! with one instance per worker. It is handed one planned mutant at a time
//! per instance, runs it program by program on a worker of its own off the
//! async thread, and records what each program made of it as a fact on the
//! mutant's stream, the one stream it writes. It decides nothing: the
//! mutant does, and its reply to each fact carries the events it recorded,
//! so the runner stops running programs when the mutant is decided.
//!
//! It needs the streams alone: the mutant's plan is in the event it is
//! handed, what the run prepared (the time a mutant may run, which tests
//! call it) in the run's own stream, read once for each run it serves, and
//! what is already done of this mutant in the mutant's. A mutant handed
//! again after a crash is run in the programs that have no fact, and not
//! at all if it is decided; what a crash lost is run again, and anything
//! recorded twice is a duplicate by the fact's id, derived from the event
//! that planned the mutant and the program.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::v1::{Cover, EventPage, Uuid};
use cherenkov_core::plan::MutantPlan;
use cherenkov_run_ports::Edge;

use crate::covers::run_cover;
use crate::error::RunLayerError;
use crate::live::LiveRuns;
use crate::policies::{handed, handler_error};
use crate::pool::Pool;
use crate::programs;
use crate::workers::{Explain, Programs, Ran, Seen};
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::observation;
use cherenkov_run_domain::plan;
use cherenkov_run_domain::{
    DebuggerFailed, KilledBy, ProgramRun, ProgramRunReported, ProgramRunResult, Ran as Observed,
    RunId, Unit,
};

/// The runner's name, under which its positions are kept.
pub const RUNNER: &str = "runner";

/// The runner: its workers, each held by one instance while it runs a
/// mutant in one program.
pub struct Runner<E> {
    core: Arc<E>,
    explain: Explain,
    /// The workers no instance holds.
    pool: Arc<Pool>,
    /// What the live runs read model knows of the runs' preparation, read
    /// before a run's stream is.
    live: Option<LiveRuns>,
    /// What each run prepared, by the run's id: read from its stream when
    /// one of its mutants is first handed.
    programs: Mutex<BTreeMap<String, Arc<Programs>>>,
}

/// What is already recorded of a mutant for one run.
#[derive(Default)]
struct Done {
    /// The mutant is decided: there is nothing to run.
    decided: bool,
    /// The programs it has a fact of.
    programs: BTreeSet<u32>,
}

impl<E> Runner<E> {
    /// The runner over `core`'s streams, on the workers of `pool`:
    /// subscribe it with one instance per worker.
    pub fn new(core: Arc<E>, explain: Explain, pool: Arc<Pool>) -> Runner<E> {
        Runner {
            core,
            explain,
            pool,
            live: None,
            programs: Mutex::new(BTreeMap::new()),
        }
    }

    /// The same runner, reading what a run prepared from `live` when it
    /// knows it, before reading the run's stream.
    pub fn knowing(self, live: LiveRuns) -> Runner<E> {
        Runner {
            live: Some(live),
            ..self
        }
    }

    /// Run `plan` in test program `program` on a worker, on a thread where
    /// blocking is allowed.
    async fn run_in(
        &self,
        program: usize,
        plan: MutantPlan,
        programs: Arc<Programs>,
    ) -> Result<Ran, RunLayerError> {
        let mut worker = self.pool.take().await;
        let explain = self.explain;
        let ran = tokio::task::spawn_blocking(move || {
            let ran = worker.run_in(program, &plan, &programs, explain);
            (worker, ran)
        })
        .await;
        let (worker, ran) = ran.map_err(|_| RunLayerError::Workers {
            reason: "a worker panicked".to_string(),
        })?;
        self.pool.give(worker);
        Ok(ran)
    }
}

impl<E: Edge> Runner<E> {
    /// What `run` prepared: as the live runs model knows it, else read from
    /// the run's stream.
    async fn prepared(&self, run: &Uuid) -> Result<Programs, RunLayerError> {
        if let Some(known) = self.live.as_ref().and_then(|live| live.programs(run)) {
            return Ok(known);
        }
        let prepared: Vec<_> = self
            .core
            .events(&run_cover(run), 0)
            .await?
            .iter()
            .filter_map(|page| match run_event(page) {
                Some(RunEvent::ProgramPrepared(prepared)) => Some(prepared),
                _ => None,
            })
            .collect();
        Ok(programs::programs(prepared))
    }

    /// What `run` prepared: read the first time, then kept.
    async fn programs_of(&self, run: &Uuid) -> Result<Arc<Programs>, RunLayerError> {
        let key = run.to_string();
        let kept = self
            .programs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned();
        if let Some(kept) = kept {
            return Ok(kept);
        }
        let programs = Arc::new(self.prepared(run).await?);
        if programs.is_empty() {
            return Err(RunLayerError::Workers {
                reason: format!("run {run} prepared no test program"),
            });
        }
        self.programs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, programs.clone());
        Ok(programs)
    }
}

impl<E: Edge> Runner<E> {
    /// What the mutant's stream already says of `run`'s asking.
    async fn done(&self, cover: &Cover, run: &Uuid) -> Result<Done, RunLayerError> {
        let mut done = Done::default();
        for page in self.core.events(cover, 0).await? {
            match mutant_event(&page) {
                Some(MutantEvent::ProgramRan(ran)) if is_run(ran.run.as_ref(), run) => {
                    done.programs.extend(ran.ran.map(|ran| ran.program));
                }
                Some(MutantEvent::MutantDecided(decided)) if is_run(decided.run.as_ref(), run) => {
                    done.decided = true;
                }
                _ => {}
            }
        }
        Ok(done)
    }

    /// Run the mutant in each program of `programs` that has no fact yet,
    /// recording each as a fact on its stream, until it is decided.
    /// `planned` is the sequence of the event that planned it, which the
    /// facts' ids derive from.
    async fn run_programs(
        &self,
        cover: &Cover,
        run: &Uuid,
        plan: &MutantPlan,
        asked: (u32, u64),
    ) -> Result<(), RunLayerError> {
        let (count, planned) = asked;
        let done = self.done(cover, run).await?;
        if done.decided {
            return Ok(());
        }
        let programs = self.programs_of(run).await?;
        for program in (0..count).filter(|program| !done.programs.contains(program)) {
            let ran = self
                .run_in(program as usize, plan.clone(), programs.clone())
                .await?;
            let fact = ProgramRunReported {
                run: Some(RunId::from(run.clone())),
                ran: Some(program_run(ran, &programs)),
            };
            let root = cover
                .root
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let external_id = format!("{RUNNER} {root} {planned} {program}");
            let reported = self.core.report(cover.clone(), &fact, &external_id).await?;
            // The mutant decides, and says so in what it recorded: no
            // further program is run for a mutant that is decided.
            if decides(&reported.events, run) {
                break;
            }
        }
        Ok(())
    }
}

/// Whether `run` is the run `candidate` names.
fn is_run(candidate: Option<&RunId>, run: &Uuid) -> bool {
    candidate.map(|id| &id.0) == Some(run)
}

/// Whether `events` hold the decision of the mutant for `run`.
fn decides(events: &[EventPage], run: &Uuid) -> bool {
    events.iter().any(|page| {
        matches!(mutant_event(page),
            Some(MutantEvent::MutantDecided(decided)) if is_run(decided.run.as_ref(), run))
    })
}

/// What a worker made of a mutant in a program, as the fact tells it.
fn program_run(ran: Ran, programs: &Programs) -> ProgramRun {
    let Ran {
        program,
        took,
        tests,
        seen,
    } = ran;
    ProgramRun {
        program: u32::try_from(program).unwrap_or(u32::MAX),
        program_name: programs.name(program).unwrap_or_default().to_string(),
        took_ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        tests,
        result: match seen {
            Seen::NotCalled => ProgramRunResult::NotCalled(Unit {}),
            Seen::Failed { reason } => ProgramRunResult::Failed(DebuggerFailed { reason }),
            Seen::Observed { raw, killed_by } => ProgramRunResult::Ran(Observed {
                observation: Some(observation::encode(&raw)),
                killed_by: killed_by.into_iter().map(killed).collect(),
            }),
        },
    }
}

/// A failing test as the fact tells it.
fn killed(killer: crate::workers::Killer) -> KilledBy {
    KilledBy {
        test: killer.test,
        location: killer.location,
        message: killer.message,
        got: killer.got,
        expected: killer.expected,
    }
}

impl<E: Edge + 'static> ProjectorHandler for Runner<E> {
    fn name(&self) -> &str {
        RUNNER
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let Some((cover, page)) = handed(request) else {
            return Ok(ProjectResponse::default());
        };
        let Some(MutantEvent::MutantPlanned(planned)) = mutant_event(page) else {
            return Ok(ProjectResponse::default());
        };
        let (Some(run), Some(executable)) = (planned.run, planned.mutant) else {
            return Ok(ProjectResponse::default());
        };
        let plan = executable.plan.ok_or_else(|| {
            handler_error(&RunLayerError::Workers {
                reason: "a mutant was planned with no plan".to_string(),
            })
        })?;
        let plan = plan::decode(plan).map_err(|e| {
            handler_error(&RunLayerError::Workers {
                reason: e.to_string(),
            })
        })?;
        self.run_programs(cover, &run.0, &plan, (planned.programs, page.sequence))
            .await
            .map_err(|e| handler_error(&e))?;
        Ok(ProjectResponse::default())
    }
}

#[cfg(test)]
#[path = "runner.test.rs"]
mod tests;
