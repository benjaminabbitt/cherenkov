//! The composition root of the run layer. It owns what the application
//! must not: the clock and the ids (every call the core handles is
//! stamped at a [`sample`], and a run's id comes from [`new_run`]), the
//! async runtime, the store, and the core itself, angzarr's `App` with the
//! bus hosted on it (`cherenkov-embedded-edge`). The application sees only
//! the ports (`cherenkov-run-ports`).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{OnRetryable, Registry, Sampled, Timestamp};
use cherenkov_core::plan::MutantPlan;
use cherenkov_embedded_edge::{EmbeddedEdge, RunStore, TokioWait};
use cherenkov_run_application::error::RunLayerError;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::{self, Settings, Workers};
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

/// Plan `plans` as a new run with `settings` on `store` and, given
/// `workers`, run it; with none, a dry run: planned, never run. the `readers`
/// print every event of the run as it is recorded and write its verdict file
/// when it finishes; returns the status the run finished with,
/// none for a run that has not finished (a dry run). See [`layer::run`].
pub fn run<S: RunStore>(
    store: S,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    readers: Readers,
) -> Result<Option<RunStatus>, RunLayerError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| RunLayerError::Runtime {
            reason: e.to_string(),
        })?;
    runtime.block_on(async {
        let core = Arc::new(core(store, OnRetryable::default()).await?);
        layer::run(&core, new_run(), plans, settings, workers, readers).await
    })
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
