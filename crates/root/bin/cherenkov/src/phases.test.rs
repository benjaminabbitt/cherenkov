use std::sync::{Arc, Mutex};

use super::*;
use angzarr_embedded::Timestamp;
use cherenkov_run_wire::output::{Finished, Prepared, Started};
use presentation::Format;
use std::io::Write;

/// Phases said as text into `written`.
fn says(written: &Written) -> Say {
    let catalog = presentation::Catalog::en_us(&[cherenkov_cli::error::EN_US]).unwrap();
    Say::to(Format::Text, Arc::new(catalog), written.clone())
}

#[derive(Clone, Default)]
struct Written(Arc<Mutex<Vec<u8>>>);

impl Write for Written {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default, Clone)]
struct Passed(Arc<Mutex<usize>>);

impl Sink for Passed {
    fn document(&mut self, _: &Document) {
        *self.0.lock().unwrap() += 1;
    }
}

fn doc(event: Event) -> Document {
    Document { event: Some(event) }
}

#[test]
fn test_the_run_layer_s_time_is_split_at_the_last_program_prepared() {
    let notice = summary(
        Some(Duration::from_millis(2_500)),
        Duration::from_millis(10_000),
    );
    assert_eq!(
        notice,
        Notice::Phases {
            preparation_tenths: 25,
            mutants_tenths: 75
        }
    );
}

#[test]
fn test_a_run_that_prepared_nothing_spent_all_of_its_time_in_the_mutants_phase() {
    let notice = summary(None, Duration::from_millis(1_000));
    assert_eq!(
        notice,
        Notice::Phases {
            preparation_tenths: 0,
            mutants_tenths: 10
        }
    );
}

#[test]
fn test_the_phases_are_written_when_the_run_finishes_and_every_document_passes_through() {
    let (written, passed) = (Written::default(), Passed::default());
    let mut sink = Phases::new(Box::new(passed.clone()), says(&written));
    sink.document(&doc(Event::Prepared(Prepared::default())));
    assert!(
        written.0.lock().unwrap().is_empty(),
        "nothing before the end"
    );
    sink.document(&doc(Event::Finished(Finished::default())));
    assert_eq!(*passed.0.lock().unwrap(), 2);
    let text = String::from_utf8(written.0.lock().unwrap().clone()).unwrap();
    assert!(
        text.starts_with("cherenkov: phase preparation and recording: "),
        "{text}"
    );
    assert!(text.contains("; phase mutants: "), "{text}");
}

/// A timestamp `secs` seconds into the run's recorded time.
fn at(secs: i64) -> Option<Timestamp> {
    Some(Timestamp {
        seconds: 1_700_000_000 + secs,
        nanos: 0,
    })
}

fn started_at(secs: i64) -> Document {
    doc(Event::Started(Started {
        at: at(secs),
        ..Default::default()
    }))
}

fn prepared_at(secs: i64) -> Document {
    doc(Event::Prepared(Prepared {
        at: at(secs),
        ..Default::default()
    }))
}

fn finished_at(secs: i64) -> Document {
    doc(Event::Finished(Finished {
        at: at(secs),
        ..Default::default()
    }))
}

fn said(written: &Written) -> String {
    String::from_utf8(written.0.lock().unwrap().clone())
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn test_the_preparation_phase_ends_when_the_last_program_is_prepared() {
    let written = Written::default();
    let mut sink = Phases::new(Box::new(Passed::default()), says(&written));
    for d in [
        started_at(0),
        prepared_at(3),
        prepared_at(5),
        finished_at(10),
    ] {
        sink.document(&d);
    }
    assert_eq!(
        said(&written),
        "cherenkov: phase preparation and recording: 5.0 s; phase mutants: 5.0 s"
    );
}

#[test]
fn test_documents_replayed_instantly_give_the_recorded_split_not_the_printing_pace() {
    // A resumed run reprints its earlier documents at once: the split is the
    // one the documents record, whatever time passes while they are printed.
    let written = Written::default();
    let mut sink = Phases::new(Box::new(Passed::default()), says(&written));
    sink.document(&started_at(0));
    sink.document(&prepared_at(40));
    sink.document(&finished_at(100));
    assert_eq!(
        said(&written),
        "cherenkov: phase preparation and recording: 40.0 s; phase mutants: 60.0 s"
    );
}

#[test]
fn test_a_run_that_prepared_nothing_says_all_its_recorded_time_was_the_mutants() {
    let written = Written::default();
    let mut sink = Phases::new(Box::new(Passed::default()), says(&written));
    sink.document(&started_at(0));
    sink.document(&finished_at(7));
    assert_eq!(
        said(&written),
        "cherenkov: phase preparation and recording: 0.0 s; phase mutants: 7.0 s"
    );
}
