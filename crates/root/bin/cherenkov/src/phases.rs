//! How a run's time splits: the time to the last test program prepared
//! (baseline and recording of each) and the rest, which the mutants take.
//! Said on stderr when the run finishes; the build and the viability
//! check, which precede the run, are timed where they happen.

use std::time::Duration;

use angzarr_embedded::Timestamp;
use cherenkov_run_wire::output::{Document, document::Event};

use cherenkov_cli::notice::{Notice, Say, tenths};
use cherenkov_cli::output::Sink;

/// A sink that passes every document on and, at the run's end, says how
/// long its phases took, by the times the documents record (`at` of
/// `run.started`, of the last `program.prepared` and of `run.finished`),
/// never the clock of the process that prints them: a resumed run reprints
/// its earlier documents at once.
pub struct Phases {
    inner: Box<dyn Sink>,
    say: Say,
    started: Option<Timestamp>,
    last_prepared: Option<Timestamp>,
}

impl Phases {
    /// The phases are said through `say`.
    pub fn new(inner: Box<dyn Sink>, say: Say) -> Self {
        Phases {
            inner,
            say,
            started: None,
            last_prepared: None,
        }
    }
}

impl Sink for Phases {
    fn document(&mut self, doc: &Document) {
        self.inner.document(doc);
        match &doc.event {
            Some(Event::Started(started)) => self.started = started.at,
            Some(Event::Prepared(prepared)) => self.last_prepared = prepared.at,
            Some(Event::Finished(finished)) => {
                let notice = self.notice(finished.at);
                self.say.say(&notice);
            }
            _ => {}
        }
    }
}

impl Phases {
    /// The phases of a run recorded as finished at `finished`.
    fn notice(&self, finished: Option<Timestamp>) -> Notice {
        let started = self.started;
        let prepared = elapsed(started, self.last_prepared.or(started));
        let total = elapsed(started, finished);
        summary(Some(prepared), total)
    }
}

/// Time from `from` to `to`; nothing when either is not recorded or `to`
/// is the earlier.
fn elapsed(from: Option<Timestamp>, to: Option<Timestamp>) -> Duration {
    let (Some(from), Some(to)) = (from, to) else {
        return Duration::ZERO;
    };
    let nanos = |t: Timestamp| i128::from(t.seconds) * 1_000_000_000 + i128::from(t.nanos);
    let diff = nanos(to).saturating_sub(nanos(from));
    Duration::from_nanos(u64::try_from(diff).unwrap_or(0))
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
