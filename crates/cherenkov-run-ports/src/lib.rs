//! What the run layer's application asks of the core it is hosted on, and
//! nothing of how the core is hosted: the root supplies a clock, ids, a
//! runtime and a store, and an adapter (`cherenkov-embedded-edge`) serves
//! these ports over angzarr-embedded's `App`.
//!
//! - **The edge** ([`Edge`]): handle a command or a fact on a stream, read
//!   a stream, wait for the next commit.
//! - **Serving** ([`Serving`]): serve a set of bus subscriptions until the
//!   caller says to stop.
//!
//! Errors are angzarr's diagnostics, captured as a [`Report`] with their
//! recovery ([`EdgeError`]).

use std::future::Future;

use angzarr_embedded::BusPolicy;
use angzarr_embedded::angzarr_bus::{Served, Subscription};
use angzarr_embedded::angzarr_ports::CommitWait;
use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::markup::{DomainMessage, pack_message};
use angzarr_embedded::v1::{
    Command, CommandBook, CommandPage, Cover, EventPage, Fact, FactOrigin, command_page,
};

pub use angzarr_embedded::angzarr_diagnostic::{Recovery, Report};

/// Why the core did not take a call: what failed, captured, and who must
/// act on it. A call that failed with [`Recovery::Retry`] (a store down for
/// now, a write that lost a race) is worth making again; the others are not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeError {
    pub cause: Report,
    pub recovery: Recovery,
}

impl EdgeError {
    /// Whether making the call again may succeed.
    pub fn retryable(&self) -> bool {
        self.recovery == Recovery::Retry
    }
}

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

/// The subscriptions the bus serves: projectors (boxed) and policies.
pub type Subscriptions = Vec<Subscription<Box<dyn DynProjectorHandler>, BusPolicy>>;

/// The book that sends `command` to `stream`.
pub fn command_book<C: DomainMessage>(stream: Cover, command: &C) -> CommandBook {
    CommandBook {
        cover: Some(stream),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(pack_message(command)),
                handling: None,
            })),
            ..CommandPage::default()
        }),
    }
}

/// The book that reports `fact` to `stream` under `external_id`.
pub fn fact_book<F: DomainMessage>(stream: Cover, fact: &F, external_id: &str) -> CommandBook {
    CommandBook {
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
    }
}

/// The edge to the core: how the application sends it commands and facts
/// and reads its streams.
pub trait Edge: Send + Sync {
    /// Handle one book (a command or a fact) on its stream. The core owns
    /// the clock and the ids the call is stamped with.
    fn handle(&self, book: CommandBook)
    -> impl Future<Output = Result<Reported, EdgeError>> + Send;

    /// `stream`'s events from sequence `from` on.
    fn events(
        &self,
        stream: &Cover,
        from: u64,
    ) -> impl Future<Output = Result<Vec<EventPage>, EdgeError>> + Send;

    /// A wait armed now, resolved by the first commit after this call: arm
    /// it before reading, so a commit between the read and the wait is not
    /// missed.
    fn next_commit(&self) -> CommitWait;

    /// Send `command` to `stream`.
    fn command<C: DomainMessage + Sync>(
        &self,
        stream: Cover,
        command: &C,
    ) -> impl Future<Output = Result<(), EdgeError>> + Send {
        let book = command_book(stream, command);
        async move { self.handle(book).await.map(drop) }
    }

    /// Report `fact` to `stream` under `external_id`: a second report under
    /// the same id is a duplicate, and records nothing. Answered with the
    /// events the report recorded, or, for a duplicate, the ones it had.
    fn report<F: DomainMessage + Sync>(
        &self,
        stream: Cover,
        fact: &F,
        external_id: &str,
    ) -> impl Future<Output = Result<Reported, EdgeError>> + Send {
        let book = fact_book(stream, fact, external_id);
        async move { self.handle(book).await }
    }
}

/// The core serving the bus: the root builds the bus over the core's store
/// with the clock, timer and retry it owns.
pub trait Serving: Edge {
    /// Serve `subscriptions` until `more` says no. `more` is asked each
    /// time the bus is idle, with what it served since it was last asked.
    /// Dropping the future stops serving where it is, as a crash does: what
    /// the stored positions do not cover is handed again to whatever serves
    /// next.
    fn serve<'a>(
        &'a self,
        subscriptions: Subscriptions,
        more: &'a mut (dyn FnMut(&Served) -> bool + Send),
    ) -> impl Future<Output = Result<(), EdgeError>> + 'a;
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
