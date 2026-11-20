//! The verdict projection: a read model of the run's stream that, when a
//! run has finished complete, hands the file its verdicts, one line for each
//! mutant sorted by id. A projector on the bus whose positions are in
//! memory, as the live runs' are: handed every run's stream from its first
//! event each serve, it rebuilds a run's lines from the decisions the run
//! recorded whole, so a line is the same for however often an event is
//! handed, and two runs of the same project write the same file.
//!
//! The file is the caller's ([`VerdictFile`]): where it is and how its lines
//! are written (JSON, by the generated serde of [`VerdictLine`]) are the
//! program's to say. A run that did not complete writes nothing, so the last
//! good file stays.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use cherenkov_run_domain::events::{RunEvent, run_event};
use cherenkov_run_domain::{MutantDecided, RunStatus};
use cherenkov_run_ports::Report;
use cherenkov_run_wire::output::VerdictLine;

use crate::error::RunLayerError;
use crate::policies::handler_error;

/// The projector's name. Its positions are not stored.
pub const VERDICTS: &str = "verdicts";

/// Where a finished run's verdict lines are written, whole, replacing what
/// was there.
pub trait VerdictFile: Send + Sync {
    /// Write `lines`, sorted by mutant id.
    fn write(&self, lines: &[VerdictLine]) -> Result<(), Report>;
}

/// The verdict projection over the runs' streams.
pub struct VerdictProjection {
    file: Arc<dyn VerdictFile>,
    /// Each run's lines by mutant id, which sorts them.
    runs: Mutex<BTreeMap<String, BTreeMap<String, VerdictLine>>>,
}

impl VerdictProjection {
    /// A projection writing to `file`.
    pub fn new(file: Arc<dyn VerdictFile>) -> VerdictProjection {
        VerdictProjection {
            file,
            runs: Mutex::new(BTreeMap::new()),
        }
    }
}

/// The line of a decided mutant: none for a decision that holds no mutant or
/// no outcome.
fn line_of(decided: MutantDecided) -> Option<VerdictLine> {
    let planned = decided.mutant?;
    let outcome = decided.verdict?;
    let killers: BTreeSet<String> = outcome.killed_by.into_iter().map(|k| k.test).collect();
    Some(VerdictLine {
        id: planned.id,
        verdict: outcome.verdict.into(),
        file: planned.file,
        line: planned.line,
        column: planned.column,
        function: planned.function,
        operator: planned.operator,
        original: planned.original,
        mutated: planned.mutated,
        killed_by: killers.into_iter().collect(),
    })
}

impl ProjectorHandler for VerdictProjection {
    fn name(&self) -> &str {
        VERDICTS
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let Some(book) = request.events.as_ref() else {
            return Ok(ProjectResponse::default());
        };
        let Some(root) = book.cover.as_ref().and_then(|cover| cover.root.as_ref()) else {
            return Ok(ProjectResponse::default());
        };
        let mut runs = self.runs.lock().unwrap_or_else(PoisonError::into_inner);
        let lines = runs.entry(root.to_string()).or_default();
        for event in book.pages.iter().filter_map(run_event) {
            match event {
                RunEvent::DecisionRecorded(record) => {
                    if let Some(line) = record.decision.and_then(line_of) {
                        lines.insert(line.id.clone(), line);
                    }
                }
                RunEvent::RunFinished(finished) if finished.status == RunStatus::Complete => {
                    let sorted: Vec<VerdictLine> = lines.values().cloned().collect();
                    self.file
                        .write(&sorted)
                        .map_err(|cause| handler_error(&RunLayerError::Unavailable { cause }))?;
                }
                _ => {}
            }
        }
        Ok(ProjectResponse::default())
    }
}

#[cfg(test)]
#[path = "verdicts.test.rs"]
mod tests;
