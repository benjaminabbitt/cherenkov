//! Following one stream: a cursor of the next sequence wanted, read from
//! the store when polled. Nothing here is a position across streams.

use angzarr_embedded::v1::{Cover, EventPage};

use crate::error::RunLayerError;
use cherenkov_run_ports::Edge;

/// A reader of one stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    stream: Cover,
    next_sequence: u64,
}

impl Follow {
    /// From the start of `stream`.
    pub fn new(stream: Cover) -> Follow {
        Follow {
            stream,
            next_sequence: 0,
        }
    }

    /// The pages at or above the cursor, which then moves past them: no
    /// gap and no duplicate. Empty when nothing is new.
    pub async fn next<E: Edge>(&mut self, core: &E) -> Result<Vec<EventPage>, RunLayerError> {
        let pages = core.events(&self.stream, self.next_sequence).await?;
        if let Some(last) = pages.last() {
            self.next_sequence = last.sequence + 1;
        }
        Ok(pages)
    }

    /// The next sequence this reader wants.
    pub fn cursor(&self) -> u64 {
        self.next_sequence
    }
}

#[cfg(test)]
#[path = "follow.test.rs"]
mod tests;
