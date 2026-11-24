//! How a run's time splits: the time to the last test program prepared
//! (baseline and recording of each) and the rest, which the mutants take.
//! Written to stderr when the run finishes; the build and the viability
//! check, which precede the run, are timed where they happen.

use std::io::Write;
use std::time::{Duration, Instant};

use cherenkov_run_wire::output::{Document, document::Event};

use cherenkov_cli::output::Sink;

/// A sink that passes every document on and, at the run's end, writes how
/// long its phases took.
pub struct Phases {
    inner: Box<dyn Sink>,
    to: Box<dyn Write + Send>,
    /// Time since the run layer started.
    clock: Box<dyn FnMut() -> Duration + Send>,
    last_prepared: Option<Duration>,
}

impl Phases {
    /// Time from now; the phases are written to `to`.
    pub fn new(inner: Box<dyn Sink>, to: Box<dyn Write + Send>) -> Self {
        let started = Instant::now();
        Self::with_clock(inner, to, Box::new(move || started.elapsed()))
    }

    /// [`Phases::new`], timed by `clock`.
    pub fn with_clock(
        inner: Box<dyn Sink>,
        to: Box<dyn Write + Send>,
        clock: Box<dyn FnMut() -> Duration + Send>,
    ) -> Self {
        Phases {
            inner,
            to,
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
                let line = summary(self.last_prepared, (self.clock)());
                let _ = writeln!(self.to, "{line}");
            }
            _ => {}
        }
    }
}

/// The phases of a run layer's `total` time, the preparation having ended
/// `prepared_at` into it (never, for a run that prepared nothing).
fn summary(prepared_at: Option<Duration>, total: Duration) -> String {
    let prepared = prepared_at.unwrap_or_default();
    format!(
        "cherenkov: phase preparation and recording: {:.1} s; phase mutants: {:.1} s",
        prepared.as_secs_f64(),
        total.saturating_sub(prepared).as_secs_f64()
    )
}

#[cfg(test)]
#[path = "phases.test.rs"]
mod tests;
