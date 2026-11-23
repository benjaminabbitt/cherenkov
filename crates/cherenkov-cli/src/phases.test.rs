use std::sync::{Arc, Mutex};

use super::*;
use cherenkov_run_wire::output::{Finished, Prepared};

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
    let line = summary(
        Some(Duration::from_millis(2_500)),
        Duration::from_millis(10_000),
    );
    assert_eq!(
        line,
        "cherenkov: phase preparation and recording: 2.5 s; phase mutants: 7.5 s"
    );
}

#[test]
fn test_a_run_that_prepared_nothing_spent_all_of_its_time_in_the_mutants_phase() {
    let line = summary(None, Duration::from_millis(1_000));
    assert_eq!(
        line,
        "cherenkov: phase preparation and recording: 0.0 s; phase mutants: 1.0 s"
    );
}

#[test]
fn test_the_phases_are_written_when_the_run_finishes_and_every_document_passes_through() {
    let (written, passed) = (Written::default(), Passed::default());
    let mut sink = Phases::new(Box::new(passed.clone()), Box::new(written.clone()));
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

#[test]
fn test_the_preparation_phase_ends_when_the_last_program_is_prepared() {
    let written = Written::default();
    let mut ticks = [3, 5, 10].map(Duration::from_secs).into_iter();
    let mut sink = Phases::with_clock(
        Box::new(Passed::default()),
        Box::new(written.clone()),
        Box::new(move || ticks.next().unwrap()),
    );
    sink.document(&doc(Event::Prepared(Prepared::default())));
    sink.document(&doc(Event::Prepared(Prepared::default())));
    sink.document(&doc(Event::Finished(Finished::default())));
    let text = String::from_utf8(written.0.lock().unwrap().clone()).unwrap();
    assert_eq!(
        text.trim(),
        "cherenkov: phase preparation and recording: 5.0 s; phase mutants: 5.0 s"
    );
}
