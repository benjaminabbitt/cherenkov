//! Live runs: the read model of which runs are in flight, what each owes
//! and how each ended. A projector on the bus whose positions are in memory
//! (`Subscription::ephemeral`): it is handed every run's stream from its
//! first event each time the bus is served, and rebuilds the model from
//! them, as a read model kept in memory has to. The shared durable mark
//! it would otherwise have is what a second process starves on (it would
//! mark an event handled before the first saw it), and a mark under a name
//! of its own for each process would litter the store.
//!
//! What it holds follows from the run's events alone, each folded as a set
//! or a flag, so being handed an event twice (delivery is at least once,
//! and the whole history again after a restart) changes nothing. The
//! application asks it whether a run has ended, and what a run still owes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::v1::Uuid;
use cherenkov_run_domain::RunStatus;
use cherenkov_run_domain::events::{RunEvent, run_event};

/// The projector's name. Its positions are not stored, so nothing is kept
/// under it.
pub const LIVE_RUNS: &str = "live-runs";

/// What the run's events say of one run.
#[derive(Clone, Debug, Default)]
struct Live {
    /// The ids of the mutants it planned.
    planned: BTreeSet<String>,
    /// Whether every test program is prepared: the mutants can run.
    ready: bool,
    /// The ids of the mutants decided.
    decided: BTreeSet<String>,
    /// How it ended; none while it is live.
    status: Option<RunStatus>,
}

impl Live {
    /// Fold `event` in.
    fn fold(&mut self, event: RunEvent) {
        match event {
            RunEvent::MutantsPlanned(planned) => {
                self.planned = planned
                    .mutants
                    .into_iter()
                    .filter_map(|executable| executable.mutant)
                    .map(|mutant| mutant.id)
                    .collect();
            }
            RunEvent::RunReady(_) => self.ready = true,
            RunEvent::DecisionRecorded(decision) => {
                self.decided.insert(decision.mutant);
            }
            RunEvent::RunFinished(finished) => self.status = Some(finished.status),
            RunEvent::RunStarted(_) | RunEvent::ProgramPrepared(_) | RunEvent::ProgramFailed(_) => {
            }
        }
    }
}

/// The live runs, shared by the projector that folds them and whoever
/// asks.
#[derive(Clone, Debug, Default)]
pub struct LiveRuns {
    runs: Arc<Mutex<BTreeMap<String, Live>>>,
}

impl LiveRuns {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Live>> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How `run` ended; none while it is live, and for a run not seen.
    pub fn status(&self, run: &Uuid) -> Option<RunStatus> {
        self.lock().get(&run.to_string())?.status
    }

    /// How many of `run`'s mutants are planned and not decided: what it
    /// still owes. Nothing for a run not seen.
    pub fn owed(&self, run: &Uuid) -> usize {
        self.lock()
            .get(&run.to_string())
            .map_or(0, |live| live.planned.difference(&live.decided).count())
    }

    /// The runs seen that have not ended, by their ids as text.
    pub fn unfinished(&self) -> Vec<String> {
        self.lock()
            .iter()
            .filter(|(_, live)| live.status.is_none())
            .map(|(run, _)| run.clone())
            .collect()
    }

    /// Whether `run`'s programs are all prepared.
    pub fn ready(&self, run: &Uuid) -> bool {
        self.lock()
            .get(&run.to_string())
            .is_some_and(|live| live.ready)
    }
}

impl ProjectorHandler for LiveRuns {
    fn name(&self) -> &str {
        LIVE_RUNS
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let Some(book) = request.events.as_ref() else {
            return Ok(ProjectResponse::default());
        };
        let Some(root) = book.cover.as_ref().and_then(|cover| cover.root.as_ref()) else {
            return Ok(ProjectResponse::default());
        };
        let key = root.to_string();
        let mut runs = self.lock();
        // A stream handed with no page is one the store no longer holds.
        if book.pages.is_empty() {
            runs.remove(&key);
            return Ok(ProjectResponse::default());
        }
        let live = runs.entry(key).or_default();
        for event in book.pages.iter().filter_map(run_event) {
            live.fold(event);
        }
        Ok(ProjectResponse::default())
    }
}

#[cfg(test)]
#[path = "live.test.rs"]
mod tests;
