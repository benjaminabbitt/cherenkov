//! The composition root of the run layer. It owns what the application
//! must not: the clock and the ids (every call the core handles is
//! stamped at a [`sample`], and a run's id comes from [`new_run`]), the
//! async runtime, the store, and the core itself, angzarr's `App` with the
//! bus hosted on it (`cherenkov-embedded-edge`). The application sees only
//! the ports (`cherenkov-run-ports`).

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::angzarr_diagnostic::Report;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{OnRetryable, Registry, Sampled, Timestamp};
use cherenkov_embedded_edge::{EmbeddedEdge, RunStore, TokioWait};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_application::error::RunLayerError;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::{self, Settings, Started, Workers};
use cherenkov_run_domain::RunStatus;

/// The instant and the fresh command id a call is handled at: this is the
/// composition root, where the clock and the ids live.
pub fn sample() -> Sampled {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Sampled {
        now: Timestamp {
            seconds: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
            nanos: i32::try_from(since.subsec_nanos()).unwrap_or(0),
        },
        command_id: fresh_id(),
    }
}

/// A fresh run's id.
pub fn new_run() -> Uuid {
    fresh_id()
}

fn fresh_id() -> Uuid {
    Uuid::from_bytes(*uuid::Uuid::now_v7().as_bytes())
}

/// The core over `store`: the run and mutant aggregates registered, the
/// kernel retrying as `retry` says, every call sampled at the clock.
pub async fn core<S: RunStore>(
    store: S,
    retry: OnRetryable,
) -> Result<EmbeddedEdge<S>, RunLayerError> {
    let mut registry = Registry::new();
    cherenkov_run_domain::register(&mut registry).map_err(|e| RunLayerError::Misconfigured {
        cause: angzarr_embedded::angzarr_diagnostic::Report::of(&e),
    })?;
    Ok(EmbeddedEdge::start(store, registry, retry, sample, Arc::new(TokioWait)).await)
}

/// Run `plans` with `settings` on `store` and, given `workers`, run them;
/// with none, a dry run: planned, never run. An unfinished run of the same
/// plan the store holds is carried on (an earlier process's), any other
/// unfinished run is abandoned, and a new run is made otherwise. The
/// `readers` print every event of the run as it is recorded and write its
/// verdict file when it finishes. See [`layer::start`].
pub fn start<S: RunStore>(
    store: S,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Started, RunLayerError> {
    start_on(async { Ok(store) }, plans, settings, workers, readers)
}

/// [`start`] on the store `opening` opens, which is opened on the runtime
/// the run is served on.
pub fn start_on<S: RunStore>(
    opening: impl Future<Output = Result<S, RunLayerError>>,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Started, RunLayerError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| RunLayerError::Runtime {
            reason: e.to_string(),
        })?;
    runtime.block_on(async {
        let core = Arc::new(core(opening.await?, OnRetryable::default()).await?);
        layer::start(&core, new_run(), plans, settings, workers, readers).await
    })
}

/// [`start`] on the durable store in the file at `path`, created if it is
/// not there: the run survives the process.
pub fn start_in_file(
    path: &Path,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Started, RunLayerError> {
    let opening = async {
        cherenkov_run_store::open(path)
            .await
            .map_err(|cause| RunLayerError::Store {
                cause: Report::of(&cause),
            })
    };
    start_on(opening, plans, settings, workers, readers)
}

/// [`start`], and the status the run finished with: none for a run that has
/// not finished (a dry run).
pub fn run<S: RunStore>(
    store: S,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Option<RunStatus>, RunLayerError> {
    start(store, plans, settings, workers, readers).map(|started| started.status)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
