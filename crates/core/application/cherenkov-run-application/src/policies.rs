//! The two policies between the run and its mutants, each served by the
//! bus with one instance, none inline in the kernel.
//!
//! - **Fan out** ([`FanOut`], run -> mutant): for the mutants a run hands on
//!   when it is ready (its last program prepared), one `PlanMutant` each,
//!   with its plan, on the mutant's own stream, naming the run. A mutant
//!   appears on its stream only once its programs exist. With one
//!   instance it is the mutant streams' one writer until their runners
//!   start.
//! - **Fan in** ([`FanIn`], mutant -> run): for each mutant's decision, the
//!   fact that reports it to the run that asked for it. With one instance
//!   the run's stream has one writer.
//!
//! The kernel derives the id of what each records from the policy's name
//! and the event it reacted to, so a trigger the bus hands again, after a
//! crash or a held failure, re-derives the same ids, and what it records
//! twice is a duplicate.

use angzarr_embedded::angzarr_router::error::{CodedError, RetryableStatus};
use angzarr_embedded::angzarr_router::hpb::ProjectRequest;
use angzarr_embedded::markup::{Commands, PolicyContext};
use angzarr_embedded::v1::EventPage;
use angzarr_embedded_macros::{on_event, policy};
use coded_diagnostic::Diagnostic;

use crate::error::RunLayerError;
use cherenkov_run_domain::{DecisionReported, MutantDecided, PlanMutant, RunReady};
use cherenkov_run_domain::{Mutant, Run, RunId, UnderivableRoot, mutant_root};

/// The fan-out policy's name.
pub const FAN_OUT: &str = "run-mutant";
/// The fan-in policy's name.
pub const FAN_IN: &str = "mutant-run";

/// Fan out: each mutant a run planned is planned on its own stream.
pub struct FanOut;

#[policy(name = "run-mutant", source = Run, target = Mutant)]
impl FanOut {
    #[on_event(RunReady)]
    fn ready(
        &self,
        event: &RunReady,
        ctx: &PolicyContext,
    ) -> Result<Commands<Mutant>, UnderivableRoot> {
        let run = RunId::from(ctx.source_root().clone());
        event
            .mutants
            .iter()
            .try_fold(Commands::none(), |commands, mutant| {
                let id = mutant.mutant.as_ref().map(|m| m.id.as_str());
                Ok(commands.to(
                    mutant_root(id.unwrap_or_default())?,
                    PlanMutant {
                        run: Some(run.clone()),
                        mutant: Some(mutant.clone()),
                        programs: event.programs,
                    },
                ))
            })
    }
}

/// `error` as a handler's answer: its code and its text, a bug the bus
/// holds and serves again when the core could not take a call for now (a
/// store down), else one it gives up on.
pub fn handler_error(error: &RunLayerError) -> CodedError {
    let details = [("reason".to_string(), error.to_string())];
    match error {
        RunLayerError::Unavailable { .. } => {
            CodedError::retryable_bug(RetryableStatus::Unavailable, error.code(), details)
        }
        _ => CodedError::bug(error.code(), details),
    }
}

/// The one page of a request the bus hands over, with its stream's
/// cover; none for a gone stream's book.
pub fn handed(request: &ProjectRequest) -> Option<(&angzarr_embedded::v1::Cover, &EventPage)> {
    let book = request.events.as_ref()?;
    Some((book.cover.as_ref()?, book.pages.first()?))
}

/// Fan in: each mutant's decision, reported to its run.
pub struct FanIn;

#[policy(name = "mutant-run", source = Mutant, target = Run)]
impl FanIn {
    #[on_event(MutantDecided)]
    fn decided(
        &self,
        event: &MutantDecided,
        ctx: &PolicyContext,
    ) -> Result<Commands<Run>, RunLayerError> {
        let _ = ctx;
        let (Some(run), Some(mutant)) = (&event.run, &event.mutant) else {
            return Ok(Commands::none());
        };
        // A decision with no outcome is a defect of whoever recorded it,
        // and the run would wait for the mutant forever.
        let Some(outcome) = &event.verdict else {
            return Err(RunLayerError::Workers {
                reason: format!("mutant {} was decided with no outcome", mutant.id),
            });
        };
        Ok(Commands::none().fact(
            run.0.clone(),
            DecisionReported {
                mutant: mutant.id.clone(),
                verdict: outcome.verdict,
                decision: Some(event.clone()),
            },
        ))
    }
}

#[cfg(test)]
#[path = "policies.test.rs"]
mod tests;
