//! The run layer's ports served in process: angzarr-embedded's `App` over
//! one store, with the bus hosted on it. A driven adapter: it knows no
//! domain (the registry is the caller's) and no clock (the caller's sample
//! stamps each call), so the composition root owns the clock and the ids,
//! and a test owns them as `Samples`.
//!
//! The bus polls the store every [`POLL`] for what a commit's push does
//! not tell it, and holds a failed delivery by [`RETRY`].

use std::sync::Arc;
use std::time::Duration;

use angzarr_embedded::angzarr_bus::{Bus as _, Following, RetryPolicy, Served};
use angzarr_embedded::angzarr_ports::CommitWait;
use angzarr_embedded::v1::{CommandBook, Cover, EventPage};
use angzarr_embedded::{Bus, Host, OnRetryable, Registry, Sampled, StreamRef, Wait};
use angzarr_embedded_host::{App, OneStore};
use cherenkov_run_ports::{
    Delivery, Edge, EdgeError, Listed, Listing, Recovery, Report, Reported, Serving, Subscriptions,
};

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
    fn wait(
        &self,
        backoff: Duration,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(backoff))
    }

    fn draw(&self) -> f64 {
        0.5
    }
}

/// What the run layer needs of a store: angzarr's ports for the kernel, and
/// for the bus the positions, the listings, the change token, the change
/// feed the bus follows, the outbox a policy's outputs commit through, and
/// a push on each commit that wakes it; shared across the bus's instances.
pub trait RunStore:
    angzarr_embedded::Store
    + angzarr_embedded::ReadStore
    + angzarr_embedded::Processed
    + angzarr_embedded::ListStreams
    + angzarr_embedded::Watch
    + angzarr_embedded::Outbox
    + angzarr_embedded::angzarr_ports::CommitNotify
    + angzarr_embedded::angzarr_ports::Changes
    + Send
    + Sync
    + 'static
{
}

impl<S> RunStore for S where
    S: angzarr_embedded::Store
        + angzarr_embedded::ReadStore
        + angzarr_embedded::Processed
        + angzarr_embedded::ListStreams
        + angzarr_embedded::Watch
        + angzarr_embedded::Outbox
        + angzarr_embedded::angzarr_ports::CommitNotify
        + angzarr_embedded::angzarr_ports::Changes
        + Send
        + Sync
        + 'static
{
}

/// `error`, captured, with who must act on it.
fn failed(
    error: &dyn angzarr_embedded::angzarr_diagnostic::Diagnostic,
    recovery: Recovery,
) -> EdgeError {
    EdgeError {
        cause: Report::of(error),
        recovery,
    }
}

/// The core served in process: the `App` over one store, the root's
/// sample, and the timer the bus waits on.
pub struct EmbeddedEdge<S: RunStore> {
    app: App<OneStore<S>>,
    sample: Arc<dyn Fn() -> Sampled + Send + Sync>,
    wait: Arc<dyn Wait>,
}

impl<S: RunStore> EmbeddedEdge<S> {
    /// The application over `store` with `registry`'s domains, its kernel
    /// retrying as `retry` says and waiting on `wait`; `sample` gives each
    /// call its instant and its command id (the root's clock and ids).
    pub async fn start(
        store: S,
        registry: Registry,
        retry: OnRetryable,
        sample: impl Fn() -> Sampled + Send + Sync + 'static,
        wait: Arc<dyn Wait>,
    ) -> EmbeddedEdge<S> {
        let app = App::builder(OneStore::new(store), registry)
            .on_retryable(retry)
            .wait_with(Arc::clone(&wait))
            .start()
            .await;
        EmbeddedEdge {
            app,
            sample: Arc::new(sample),
            wait,
        }
    }

    /// The store the streams are kept in.
    pub fn store(&self) -> &S {
        self.app.store()
    }
}

impl<S: RunStore> Edge for EmbeddedEdge<S> {
    async fn handle(&self, book: CommandBook) -> Result<Reported, EdgeError> {
        let handled = self
            .app
            .handle(book, &(self.sample)())
            .await
            .map_err(|e| failed(&e, e.recovery()))?;
        let delivery = match handled.delivery() {
            angzarr_embedded::v1::DeliveryOutcome::Duplicate => Delivery::Duplicate,
            _ => Delivery::Applied,
        };
        let events = handled
            .books()
            .into_iter()
            .flat_map(|book| book.pages)
            .collect();
        Ok(Reported { delivery, events })
    }

    async fn events(&self, stream: &Cover, from: u64) -> Result<Vec<EventPage>, EdgeError> {
        let at = StreamRef::try_from(stream).map_err(|e| failed(&e, Recovery::Invalid))?;
        let slice = self
            .store()
            .read(at, from, None)
            .await
            .map_err(|e| failed(&e, e.recovery()))?;
        Ok(slice.pages)
    }

    fn next_commit(&self) -> CommitWait {
        angzarr_embedded::angzarr_ports::CommitNotify::next_commit(self.store())
    }
}

impl<S: RunStore> Listing for EmbeddedEdge<S> {
    async fn streams(&self, domain: &str) -> Result<Vec<Listed>, EdgeError> {
        let covers = self
            .store()
            .covers(domain)
            .await
            .map_err(|e| failed(&e, e.recovery()))?;
        let mut listed = Vec::with_capacity(covers.len());
        for cover in covers {
            let at = StreamRef::try_from(&cover).map_err(|e| failed(&e, Recovery::Invalid))?;
            let head = self
                .store()
                .head(at)
                .await
                .map_err(|e| failed(&e, e.recovery()))?;
            listed.push(Listed {
                cover,
                head: head.next_sequence,
            });
        }
        Ok(listed)
    }

    async fn window(
        &self,
        stream: &Cover,
        from: u64,
        to: u64,
    ) -> Result<Vec<EventPage>, EdgeError> {
        let at = StreamRef::try_from(stream).map_err(|e| failed(&e, Recovery::Invalid))?;
        let slice = self
            .store()
            .read(at, from, Some(to))
            .await
            .map_err(|e| failed(&e, e.recovery()))?;
        Ok(slice.pages)
    }
}

impl<S: RunStore> Serving for EmbeddedEdge<S> {
    async fn serve<'a>(
        &'a self,
        subscriptions: Subscriptions,
        more: &'a mut (dyn FnMut(&Served) -> bool + Send),
    ) -> Result<(), EdgeError> {
        let kernel = self.app.kernel();
        let store = self.store();
        let sample = Arc::clone(&self.sample);
        let clock = move || sample().now;
        let host = Host::new(
            store,
            &kernel,
            &clock,
            &*self.wait,
            POLL,
            Following::Changes(store),
        )
        .pushed_by(store)
        .retrying(RETRY);
        let mut bus = Bus::new(host);
        for subscription in subscriptions {
            bus.subscribe(subscription)
                .map_err(|e| failed(&e, Recovery::Bug))?;
        }
        bus.serve(more).await.map_err(|e| failed(&e, e.recovery()))
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
