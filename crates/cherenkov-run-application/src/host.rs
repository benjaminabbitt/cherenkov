//! The bus, as the run layer uses it: what is served and by how many. Read
//! it top to bottom for how an application is put on the bus. The root
//! hosts the bus ([`Serving`]: the store, the kernel its handlers' failures
//! and its policies' steps go through, the clock they are stamped by, the
//! timer, and the push and poll that wake it); this says what it serves.
//!
//! - **Four subscriptions**, each a name that keys its positions:
//!   - the runners (`runner`), a projector with an instance for each
//!     worker, over the mutants' streams. A stream is served one event at
//!     a time, so a mutant is on one instance at once, and the bus's
//!     instances are the worker pool;
//!   - fan out (`run-mutant`) and fan in (`mutant-run`), policies with one
//!     instance each. A policy's outputs commit with its position through
//!     the store's outbox, so a store must be an outbox, and a policy
//!     cannot be ephemeral;
//!   - live runs (`live-runs`), an ephemeral projector over the runs'
//!     streams: rebuilt from the first event each serve.
//! - **Stopping**: the bus is asked whether to go on each time it is idle,
//!   with what it served since it was last asked ([`Served`]). A held
//!   failure is retried by the bus after the delay the root says.
//!
//! The bus borrows the core it is hosted on, so it lives inside the call
//! that owns them: it cannot be spawned, and dropping the future is a
//! crash the next serve recovers from, which is what the restart test
//! does.

use std::num::NonZeroUsize;

use angzarr_embedded::BusPolicy;
use angzarr_embedded::angzarr_bus::{Credit, Scope, Served, Subscription};
use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use cherenkov_run_domain::{MUTANT_DOMAIN, RUN_DOMAIN};
use cherenkov_run_ports::{Serving, Subscriptions};

use crate::error::RunLayerError;
use crate::live::LiveRuns;
use crate::policies::{FanIn, FanOut};
use crate::runner::Runner;

/// The subscription of a projector over `domain`'s streams, `instances` at
/// once.
fn projector(
    domain: &str,
    instances: NonZeroUsize,
    handler: Box<dyn DynProjectorHandler>,
) -> Subscription<Box<dyn DynProjectorHandler>, BusPolicy> {
    Subscription::projector(Scope::domain(domain), Credit::new(instances), handler)
}

/// What the run layer serves: the runners with `instances` at once, both
/// policies, and `live` to fold the runs.
pub fn subscriptions(
    runner: impl DynProjectorHandler + 'static,
    instances: NonZeroUsize,
    live: &LiveRuns,
) -> Subscriptions {
    vec![
        projector(MUTANT_DOMAIN, instances, Box::new(runner)),
        Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanOut)),
        Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanIn)),
        projector(RUN_DOMAIN, NonZeroUsize::MIN, Box::new(live.clone())).ephemeral(),
    ]
}

/// Serve `runner` on the bus `core` hosts until `more` says no: the
/// runners with `instances` at once, both policies, and `live` to fold the
/// runs.
///
/// `more` is asked each time the bus is idle, with what it served since it
/// was last asked. Dropping the future stops serving where it is, as a
/// crash does: what the stored positions do not cover is handed again to
/// whatever serves next.
pub async fn serve<C: Serving + 'static>(
    core: &C,
    runner: Runner<C>,
    instances: NonZeroUsize,
    live: &LiveRuns,
    mut more: impl FnMut(&Served) -> bool + Send,
) -> Result<(), RunLayerError> {
    core.serve(subscriptions(runner, instances, live), &mut more)
        .await
        .map_err(RunLayerError::from)
}
