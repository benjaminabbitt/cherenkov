//! The bus, hosted: the one place the run layer builds angzarr's bus and
//! says what it serves. Read it top to bottom for how an application is put
//! on the bus.
//!
//! - **The host** ([`serve`]) is the store, the kernel its handlers' failures
//!   and its policies' steps go through, the clock they are stamped by, and
//!   a timer ([`TokioWait`]); the store pushes each commit to wake the bus,
//!   which polls its change token every [`POLL`] for what the push does
//!   not tell it. The bus borrows all of them, so it lives inside the call
//!   that owns them: it cannot be spawned, and dropping the future is a
//!   crash the next serve recovers from, which is what the restart test
//!   does.
//! - **Four subscriptions**, each a name that keys its positions:
//!   - the runners (`runner`), a projector with an instance for each
//!     worker, over the mutants' streams. A stream is served one event at
//!     a time, so a mutant is on one instance at once, and the bus's
//!     instances are the worker pool;
//!   - fan out (`run-mutant`) and fan in (`mutant-run`), policies with one
//!     instance each. A policy's outputs commit with its position through
//!     the store's outbox, so a store must be an [`Outbox`], and a policy
//!     cannot be ephemeral;
//!   - live runs (`live-runs`), an ephemeral projector over the runs'
//!     streams: rebuilt from the first event each serve.
//! - **Stopping**: the bus is asked whether to go on each time it is idle,
//!   with what it served since it was last asked ([`Served`]). A held
//!   failure is retried by the bus after the delay [`RETRY`] says.

use std::num::NonZeroUsize;
use std::time::Duration;

use angzarr_embedded::angzarr_bus::{
    Bus as _, Credit, Following, RetryPolicy, Scope, Served, Subscription,
};
use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::{Bus, BusPolicy, Host, Misconfigured, Wait};
use cherenkov_run_domain::{MUTANT_DOMAIN, RUN_DOMAIN};
use std::future::Future;
use std::pin::Pin;

use crate::core::{RunCore, RunStore, now};
use crate::error::RunLayerError;
use crate::live::LiveRuns;
use crate::policies::{FanIn, FanOut};
use crate::runner::Runner;

/// How often the bus looks at the store when no commit woke it.
pub const POLL: Duration = Duration::from_millis(20);

/// How the bus retries a delivery it holds: a failure a handler reports as
/// retryable (a store that failed, a stream that lost a race too often),
/// five times from 50 ms, doubling, then given up.
pub const RETRY: RetryPolicy = RetryPolicy {
    cap: Some(5),
    initial: Duration::from_millis(50),
    multiplier: 2.0,
    max: Duration::from_secs(1),
    jitter: 0.2,
};

/// The bus's timer on tokio's, and the draw its jitter takes.
///
/// A [`Wait`] is called by many instances at once, so it holds no state of
/// its own: each wait is a fresh sleep. The draw is fixed, so the jitter is
/// a constant fraction of each delay; a retry does not need it spread.
pub struct TokioWait;

impl Wait for TokioWait {
    fn wait(&self, backoff: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(backoff))
    }

    fn draw(&self) -> f64 {
        0.5
    }
}

/// The subscription of a projector over `domain`'s streams, `instances` at
/// once.
fn projector(
    domain: &str,
    instances: NonZeroUsize,
    handler: Box<dyn DynProjectorHandler>,
) -> Subscription<Box<dyn DynProjectorHandler>, BusPolicy> {
    Subscription::projector(Scope::domain(domain), Credit::new(instances), handler)
}

/// Serve `runner` on a bus over `core`'s store until `more` says no: the
/// runners with `instances` at once, both policies, and `live` to fold the
/// runs.
///
/// `more` is asked each time the bus is idle, with what it served since it
/// was last asked. Dropping the future stops serving where it is, as a
/// crash does: what the stored positions do not cover is handed again to
/// whatever serves next.
pub async fn serve<S: RunStore>(
    core: &RunCore<S>,
    runner: Runner<S>,
    instances: NonZeroUsize,
    live: &LiveRuns,
    more: impl FnMut(&Served) -> bool + Send,
) -> Result<(), RunLayerError> {
    let kernel = core.kernel();
    let host = Host::new(
        core.store(),
        &kernel,
        &now,
        &TokioWait,
        POLL,
        Following::Changes(core.store()),
    )
    .pushed_by(core.store())
    .retrying(RETRY);
    let mut bus = Bus::new(host);
    let misconfigured = |e: Misconfigured| RunLayerError::Misconfigured {
        cause: angzarr_diagnostic::Report::of(&e),
    };
    bus.subscribe(projector(MUTANT_DOMAIN, instances, Box::new(runner)))
        .map_err(misconfigured)?;
    bus.subscribe(Subscription::policy(
        NonZeroUsize::MIN,
        BusPolicy::new(FanOut),
    ))
    .map_err(misconfigured)?;
    bus.subscribe(Subscription::policy(
        NonZeroUsize::MIN,
        BusPolicy::new(FanIn),
    ))
    .map_err(misconfigured)?;
    bus.subscribe(projector(RUN_DOMAIN, NonZeroUsize::MIN, Box::new(live.clone())).ephemeral())
        .map_err(misconfigured)?;
    bus.serve(more).await.map_err(|e| RunLayerError::kernel(&e))
}
