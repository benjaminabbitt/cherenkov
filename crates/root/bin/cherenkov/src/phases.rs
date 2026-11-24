//! How a run's time splits: the time to the last test program prepared
//! (baseline and recording of each) and the rest, which the mutants take.
//! Said on stderr when the run finishes; the build and the viability
//! check, which precede the run, are timed where they happen.

use std::time::{Duration, Instant};

use cherenkov_run_wire::output::{Document, document::Event};

use cherenkov_cli::notice::{Notice, Say, tenths};
use cherenkov_cli::output::Sink;

/// A sink that passes every document on and, at the run's end, says how
/// long its phases took.
pub struct Phases {
    inner: Box<dyn Sink>,
    say: Say,
    /// Time since the run layer started.
    clock: Box<dyn FnMut() -> Duration + Send>,
    last_prepared: Option<Duration>,
}

impl Phases {
    /// Time from now; the phases are said through `say`.
    pub fn new(inner: Box<dyn Sink>, say: Say) -> Self {
        let started = Instant::now();
        Self::with_clock(inner, say, Box::new(move || started.elapsed()))
    }

    /// [`Phases::new`], timed by `clock`.
    pub fn with_clock(
        inner: Box<dyn Sink>,
        say: Say,
        clock: Box<dyn FnMut() -> Duration + Send>,
    ) -> Self {
        Phases {
            inner,
            say,
            clock,
            last_prepared: None,
        }
    }
}

impl Sink for Phases {
    fn document(&mut self, doc: &Document) {
        self.inner.document(doc);
        match &doc.event {
            Some(Event::Prepared(_)) => self.last_prepared = Some((self.clock)()),
            Some(Event::Finished(_)) => {
                let notice = summary(self.last_prepared, (self.clock)());
                self.say.say(&notice);
            }
            _ => {}
        }
    }
}

/// The phases of a run layer's `total` time, the preparation having ended
/// `prepared_at` into it (never, for a run that prepared nothing).
fn summary(prepared_at: Option<Duration>, total: Duration) -> Notice {
    let prepared = prepared_at.unwrap_or_default();
    Notice::Phases {
        preparation_tenths: tenths(prepared),
        mutants_tenths: tenths(total.saturating_sub(prepared)),
    }
}

#[cfg(test)]
#[path = "phases.test.rs"]
mod tests;
