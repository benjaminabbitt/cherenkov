//! The run's core served in process: the kernel over one store, with the
//! run and mutant aggregates registered. Both policies are on the bus, not
//! in the registry, so a handled call runs no policy: it writes its one
//! stream. Commands and facts are handled by a call, and a stream is read
//! from the store. The store is any of angzarr's (`S`): the core needs its
//! ports only.

use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::markup::{DomainMessage, pack_message};
use angzarr_embedded::v1::{
    Command, CommandBook, CommandPage, Cover, EventPage, Fact, FactOrigin, Uuid, command_page,
};
use angzarr_embedded::{
    Handled, Kernel, OnRetryable, Registry, Sampled, Store, StreamRef, Timestamp,
};

use crate::error::RunLayerError;
use cherenkov_run_domain::{MUTANT_DOMAIN, Mutant, RUN_DOMAIN, Run, mutant_root};

/// Whether a reported fact was recorded, or was a redelivery of one
/// already on the stream (the same external id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Applied,
    Duplicate,
}

/// What a handled call came to: whether it recorded, and the events of the
/// stream it left, which a redelivery answers with the ones it had
/// recorded. A caller that records on behalf of a decision reads from them
/// whether it was made.
#[derive(Clone, Debug)]
pub struct Reported {
    pub delivery: Delivery,
    pub events: Vec<EventPage>,
}

/// The kernel's registry, its store, and how it retries.
pub struct RunCore<S> {
    registry: Registry,
    store: S,
    retry: OnRetryable,
}

/// The instant, as the bus stamps what it records.
pub fn now() -> Timestamp {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(since.subsec_nanos()).unwrap_or(0),
    }
}

/// The instant and the fresh command id a call is handled at.
fn sample() -> Sampled {
    Sampled {
        now: now(),
        command_id: Uuid::from_bytes(*uuid::Uuid::now_v7().as_bytes()),
    }
}

/// A fresh run's id.
pub fn new_run() -> Uuid {
    Uuid::from_bytes(*uuid::Uuid::now_v7().as_bytes())
}

/// The stream of run `run`.
pub fn run_cover(run: &Uuid) -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: Some(run.clone()),
        edition: None,
    }
}

/// The stream of mutant `id`.
pub fn mutant_cover(id: &str) -> Result<Cover, RunLayerError> {
    Ok(Cover {
        domain: MUTANT_DOMAIN.to_string(),
        root: Some(mutant_root(id).map_err(|e| RunLayerError::kernel(&e))?),
        edition: None,
    })
}

fn misconfigured(e: &dyn angzarr_diagnostic::Diagnostic) -> RunLayerError {
    RunLayerError::Misconfigured {
        cause: angzarr_diagnostic::Report::of(e),
    }
}

impl<S: Store> RunCore<S> {
    /// A core over `store`, retrying as the kernel does by default.
    pub fn new(store: S) -> Result<RunCore<S>, RunLayerError> {
        let mut registry = Registry::new();
        registry
            .aggregate::<Run>(Run::default)
            .map_err(|e| misconfigured(&e))?;
        registry
            .aggregate::<Mutant>(Mutant::default)
            .map_err(|e| misconfigured(&e))?;
        Ok(RunCore {
            registry,
            store,
            retry: OnRetryable::default(),
        })
    }

    /// Retrying as `retry` says: with one race run and no retry, a write
    /// that loses a race on its stream fails instead of running again.
    pub fn retrying(mut self, retry: OnRetryable) -> RunCore<S> {
        self.retry = retry;
        self
    }

    /// The kernel every call goes through, and the bus serves its policies
    /// and projectors on: the registry's, retrying as the core says.
    pub fn kernel(&self) -> Kernel<'_> {
        Kernel::new(&self.registry).on_retryable(self.retry)
    }

    /// The store the streams are kept in.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Send `command` to `stream`, and every policy step it sets off.
    pub async fn command<C: DomainMessage>(
        &self,
        stream: Cover,
        command: &C,
    ) -> Result<(), RunLayerError> {
        let book = CommandBook {
            cover: Some(stream),
            page: Some(CommandPage {
                body: Some(command_page::Body::Command(Command {
                    payload: Some(pack_message(command)),
                    handling: None,
                })),
                ..CommandPage::default()
            }),
        };
        self.handle(book).await.map(drop)
    }

    /// Report `fact` to `stream` under `external_id`: a second report under
    /// the same id is a duplicate, and records nothing. Answered with the
    /// events the report recorded, or, for a duplicate, the ones it had.
    pub async fn report<F: DomainMessage>(
        &self,
        stream: Cover,
        fact: &F,
        external_id: &str,
    ) -> Result<Reported, RunLayerError> {
        let book = CommandBook {
            cover: Some(stream),
            page: Some(CommandPage {
                body: Some(command_page::Body::Fact(Fact {
                    payload: Some(pack_message(fact)),
                    origin: Some(FactOrigin {
                        external_id: external_id.to_string(),
                        description: "cherenkov".to_string(),
                    }),
                })),
                ..CommandPage::default()
            }),
        };
        self.handle(book).await
    }

    async fn handle(&self, book: CommandBook) -> Result<Reported, RunLayerError> {
        let handled = self
            .kernel()
            .handle_any(&self.store, book, &sample())
            .await
            .map_err(|e| RunLayerError::kernel(&e))?;
        // A step of the chain that was dead-lettered was not recorded: the
        // run would wait for it forever.
        let letters = handled.dead_letters().len();
        if letters > 0 {
            return Err(RunLayerError::Undelivered {
                letters: u64::try_from(letters).unwrap_or(u64::MAX),
            });
        }
        let delivery = match handled {
            Handled::Applied(_) => Delivery::Applied,
            Handled::Duplicate(..) => Delivery::Duplicate,
        };
        let events = handled
            .books()
            .into_iter()
            .flat_map(|book| book.pages)
            .collect();
        Ok(Reported { delivery, events })
    }

    /// `stream`'s events from sequence `from` on.
    pub async fn events(&self, stream: &Cover, from: u64) -> Result<Vec<EventPage>, RunLayerError> {
        let at = StreamRef::try_from(stream).map_err(|e| RunLayerError::kernel(&e))?;
        let slice = self
            .store
            .read(at, from, None)
            .await
            .map_err(|e| RunLayerError::kernel(&e))?;
        Ok(slice.pages)
    }
}

/// What the run layer needs of a store: angzarr's ports for the kernel, and
/// for the bus the positions, the listings, the change token, the change feed the bus follows, the outbox a
/// policy's outputs commit through, and a push on each commit that wakes
/// it; shared across the bus's instances.
pub trait RunStore:
    Store
    + angzarr_embedded::ReadStore
    + angzarr_embedded::Processed
    + angzarr_embedded::Watch
    + angzarr_embedded::Outbox
    + angzarr_embedded::CommitNotify
    + angzarr_embedded::angzarr_ports::Changes
    + Send
    + Sync
    + 'static
{
}

impl<S> RunStore for S where
    S: Store
        + angzarr_embedded::ReadStore
        + angzarr_embedded::Processed
        + angzarr_embedded::Watch
        + angzarr_embedded::Outbox
        + angzarr_embedded::CommitNotify
        + angzarr_embedded::angzarr_ports::Changes
        + Send
        + Sync
        + 'static
{
}
