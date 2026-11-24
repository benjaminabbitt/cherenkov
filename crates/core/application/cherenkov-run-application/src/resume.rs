//! Resuming: a run that begins on a store looks first for the runs an
//! earlier process left unfinished. One whose plan is the plan about to be
//! run is carried on (the newest, if several are); every other unfinished
//! run is told it is abandoned, so a store never holds a live run nothing
//! will finish. A finished run is never touched.
//!
//! Which runs there are, and how far each got, comes from the streams
//! alone and costs a read of each run's last event and, for an unfinished
//! one, its first two (the start and the plan): not its history.

use angzarr_embedded::v1::{Cover, Uuid};
use cherenkov_run_domain::events::{RunEvent, run_event};
use cherenkov_run_domain::{Abandoned, MutantsPlanned, PlanRun, RUN_DOMAIN, RunStarted};
use cherenkov_run_ports::{Edge, Listing};

use crate::covers::run_cover;
use crate::error::RunLayerError;

/// An unfinished run, as its first events record it.
pub(crate) struct Unfinished {
    pub(crate) run: Uuid,
    pub(crate) started: Option<RunStarted>,
    pub(crate) planned: Option<MutantsPlanned>,
}

impl Unfinished {
    /// Whether it was planned as `wanted` is: the same settings and the same
    /// mutants, each with its plan, in the same order. The number of workers
    /// is no part of it, nor are the run's id or its time.
    fn is_plan_of(&self, wanted: &PlanRun) -> bool {
        let (Some(started), Some(planned)) = (&self.started, &self.planned) else {
            return false;
        };
        started.root == wanted.root
            && started.operators == wanted.operators
            && started.fast == wanted.fast
            && started.programs == wanted.programs
            && planned.mutants == wanted.mutants
    }
}

/// Whether the events of a run's last page say it ended.
pub(crate) fn ended(last: &[angzarr_embedded::v1::EventPage]) -> bool {
    last.iter()
        .any(|page| matches!(run_event(page), Some(RunEvent::RunFinished(_))))
}

/// The run's start and its plan, the first two events of its stream.
pub(crate) async fn beginning<C: Listing>(
    core: &C,
    cover: &Cover,
) -> Result<Unfinished, RunLayerError> {
    let mut found = Unfinished {
        run: cover.root.clone().unwrap_or_default(),
        started: None,
        planned: None,
    };
    for event in core.window(cover, 0, 2).await?.iter().filter_map(run_event) {
        match event {
            RunEvent::RunStarted(started) => found.started = Some(started),
            RunEvent::MutantsPlanned(planned) => found.planned = Some(planned),
            _ => {}
        }
    }
    Ok(found)
}

/// The runs the store holds that have not ended, oldest first (a run's id is
/// ordered by when it was made).
async fn unfinished<C: Listing>(core: &C) -> Result<Vec<Unfinished>, RunLayerError> {
    let mut runs = Vec::new();
    for listed in core.streams(RUN_DOMAIN).await? {
        if listed.head == 0 {
            continue;
        }
        let last = core
            .window(&listed.cover, listed.head - 1, listed.head)
            .await?;
        if !ended(&last) {
            runs.push(beginning(core, &listed.cover).await?);
        }
    }
    runs.sort_by_key(|found| found.run.to_string());
    Ok(runs)
}

/// Tell `run` it is abandoned, for `reason`.
async fn abandon<C: Edge>(core: &C, run: &Uuid, reason: &str) -> Result<(), RunLayerError> {
    let fact = Abandoned {
        reason: reason.to_string(),
    };
    core.report(run_cover(run), &fact, "abandoned").await?;
    Ok(())
}

/// The run to carry on, if the store holds an unfinished one planned as
/// `wanted`; every other unfinished run is abandoned.
pub async fn settle<C: Listing>(core: &C, wanted: &PlanRun) -> Result<Option<Uuid>, RunLayerError> {
    let mut runs = unfinished(core).await?;
    let resumed = runs
        .iter()
        .rposition(|found| found.is_plan_of(wanted))
        .map(|at| runs.remove(at).run);
    for found in runs {
        let reason = match found.is_plan_of(wanted) {
            true => "a later run of the same plan was resumed",
            false => "the plan changed",
        };
        abandon(core, &found.run, reason).await?;
    }
    Ok(resumed)
}
