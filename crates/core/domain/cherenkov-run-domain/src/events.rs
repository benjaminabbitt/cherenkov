//! The run's and the mutants' events as read back from their streams: the
//! generated domain layer's event enums, one for the run's stream and one
//! for a mutant's.

use angzarr_embedded::Payload;
use angzarr_embedded::Timestamp;
use angzarr_embedded::v1::{EventPage, event_page};

pub use crate::mutant::MutantSnapshotEvent as MutantEvent;
pub use crate::run::RunSnapshotEvent as RunEvent;

/// The event `page` holds as a `T`, if it holds one.
fn event_of<T: Payload>(page: &EventPage) -> Option<T> {
    match &page.body {
        Some(event_page::Body::Event(any)) => T::from_any(any).ok(),
        _ => None,
    }
}

/// The run event `page` holds; none for a page that holds none.
pub fn run_event(page: &EventPage) -> Option<RunEvent> {
    event_of(page)
}

/// The mutant event `page` holds; none for a page that holds none.
pub fn mutant_event(page: &EventPage) -> Option<MutantEvent> {
    event_of(page)
}

/// When `page` was recorded.
pub fn recorded_at(page: &EventPage) -> Timestamp {
    page.meta
        .as_ref()
        .and_then(|meta| meta.recorded_at)
        .unwrap_or_default()
}
