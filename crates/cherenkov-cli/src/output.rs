//! A run's events as the command prints them: one JSON document per line
//! (`output-events.md`), or text for a person, each as it is recorded.
//!
//! Text puts what a reader acts on on stdout, one line per mutant
//! (`<id> <file:line:col> <fn> <op> <verdict> [tier]`) with its details
//! indented beneath it, or, for a plan, one line per planned mutant
//! (`<id> <listing>`); progress (each program prepared, the run's counts)
//! goes to stderr.

use std::io::Write;

use angzarr_presentation::Format;
use cherenkov_run_application::layer::{JsonLines, Sink};
use cherenkov_run_wire::names;
use cherenkov_run_wire::output::{Decided, Document, Finished, Prepared, Started, document::Event};
use cherenkov_run_wire::{KilledBy, PlannedMutant};
use cherenkov_run_wire::{RunStatus, Selection, Verdict};
use cli_support::Exit;

/// Which mutants text lists on stdout: the planned ones (a plan) or the
/// decided ones (a run).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listed {
    Planned,
    Decided,
}

/// The sink a command prints its run with, on the process's streams. With a
/// `log`, every mutant that could not be run is also written to it, one
/// JSON document a line.
pub fn sink(format: Format, listed: Listed, log: Option<Box<dyn Write>>) -> Box<dyn Sink> {
    let printer: Box<dyn Sink> = match format {
        Format::Json => Box::new(JsonLines(std::io::stdout())),
        Format::Text => Box::new(Text::new(std::io::stdout(), std::io::stderr(), listed)),
    };
    Box::new(Deficiencies::new(BoxedSink(printer), log))
}

/// A boxed sink, which is itself a sink.
struct BoxedSink(Box<dyn Sink>);

impl Sink for BoxedSink {
    fn document(&mut self, doc: &Document) {
        self.0.document(doc);
    }
}

/// The exit code a run that finished with `status` ends the process with:
/// the exit-code convention's, success or "tried, and something broke".
pub fn exit_code(status: RunStatus) -> i32 {
    let exit = match status {
        RunStatus::Unspecified | RunStatus::Complete => Exit::OK,
        RunStatus::Failed => Exit::FAILED,
    };
    i32::from(exit.code(false))
}

/// Accepts and logs what could not be run: counts each `unsupported`
/// mutant's `reason_code`, writes it to the log, and adds the counts to the
/// run's `run.finished` as `deficiencies`, with the exit code its status
/// says. Every document goes on unchanged but that one.
pub struct Deficiencies<W: Write> {
    inner: Box<dyn Sink>,
    log: Option<W>,
    counts: std::collections::BTreeMap<String, u32>,
}

impl<W: Write> Deficiencies<W> {
    pub fn new(inner: impl Sink + 'static, log: Option<W>) -> Self {
        Deficiencies {
            inner: Box::new(inner),
            log,
            counts: Default::default(),
        }
    }
}

impl<W: Write> Sink for Deficiencies<W> {
    fn document(&mut self, doc: &Document) {
        match &doc.event {
            Some(Event::Decided(d)) if d.verdict() == Verdict::Unsupported => {
                let code = d.reason_code.as_deref().unwrap_or("other");
                *self.counts.entry(code.to_string()).or_default() += 1;
                if let (Some(log), Ok(line)) = (&mut self.log, serde_json::to_string(doc)) {
                    let _ = writeln!(log, "{line}");
                    let _ = log.flush();
                }
                self.inner.document(doc);
            }
            Some(Event::Finished(f)) => {
                let finished = Finished {
                    exit: exit_code(f.status()),
                    deficiencies: self.counts.clone().into_iter().collect(),
                    ..f.clone()
                };
                self.inner.document(&Document {
                    event: Some(Event::Finished(finished)),
                });
            }
            _ => self.inner.document(doc),
        }
    }
}

/// Text for a person: [`render`] each document, written and flushed at once.
pub struct Text<O: Write, E: Write> {
    out: O,
    err: E,
    listed: Listed,
    started: Option<Started>,
}

impl<O: Write, E: Write> Text<O, E> {
    pub fn new(out: O, err: E, listed: Listed) -> Self {
        Text {
            out,
            err,
            listed,
            started: None,
        }
    }
}

impl<O: Write, E: Write> Sink for Text<O, E> {
    fn document(&mut self, doc: &Document) {
        if let Some(Event::Started(started)) = &doc.event {
            self.started = Some(started.clone());
        }
        let (out, err) = render(doc, self.listed, self.started.as_ref());
        for line in out {
            let _ = writeln!(self.out, "{line}");
        }
        for line in err {
            let _ = writeln!(self.err, "{line}");
        }
        let _ = self.out.flush();
        let _ = self.err.flush();
    }
}

/// `doc`'s text: the lines for stdout and the lines for stderr. `started`
/// is the run's `run.started`, for the summary at its finish.
pub fn render(
    doc: &Document,
    listed: Listed,
    started: Option<&Started>,
) -> (Vec<String>, Vec<String>) {
    match &doc.event {
        Some(Event::Planned(p)) if listed == Listed::Planned => {
            let mutant = p.mutant.clone().unwrap_or_default();
            (
                vec![format!("{} {}", site(&mutant), mutant.description)],
                Vec::new(),
            )
        }
        Some(Event::Decided(d)) => (decided(d), Vec::new()),
        Some(Event::Prepared(p)) => (Vec::new(), vec![prepared(p)]),
        Some(Event::Finished(f)) => (Vec::new(), vec![finished(f, started)]),
        _ => (Vec::new(), Vec::new()),
    }
}

/// `<id> <file:line:col> <fn> <op>`: where a mutant is and what it is.
fn site(m: &PlannedMutant) -> String {
    format!(
        "{} {}:{}:{} {} {}",
        m.id,
        m.file,
        m.line,
        m.column,
        m.function.as_deref().unwrap_or("-"),
        m.operator,
    )
}

/// A decided mutant's line and its details: what it changed, who killed
/// it or ran it unnoticed, why it was not run, and how long it took.
fn decided(d: &Decided) -> Vec<String> {
    let mutant = d.mutant.clone().unwrap_or_default();
    let tier = d
        .tier
        .map(|t| format!(" {}", names::tier(t)))
        .unwrap_or_default();
    let mut lines = vec![format!(
        "{} {}{tier}",
        site(&mutant),
        names::verdict(d.verdict)
    )];
    if let (Some(original), Some(mutated)) = (
        Some(mutant.original.as_str()).filter(|s| !s.contains('\n')),
        mutant.mutated.as_deref().filter(|s| !s.contains('\n')),
    ) {
        lines.push(match mutated.is_empty() {
            true => format!("  `{original}` deleted"),
            false => format!("  `{original}` becomes `{mutated}`"),
        });
    }
    for killer in d.killers.iter().flat_map(|k| &k.killed_by) {
        lines.push(format!("  killed by {}", killer_line(killer)));
    }
    if let Some(executors) = &d.executors {
        let tests = &executors.executed_by;
        lines.push(match tests.is_empty() {
            true => "  executed by every test, unnoticed".to_string(),
            false => format!("  executed by {}, unnoticed", tests.join(", ")),
        });
    }
    if let Some(reason) = &d.reason {
        lines.push(format!("  reason: {}", reason.replace('\n', "\n    ")));
    }
    lines.push(format!("  took {} ms", d.took_ms));
    lines
}

/// `test at location: got X, expected Y`, or `test at location: message`
/// (its first line).
fn killer_line(k: &KilledBy) -> String {
    let at = k
        .location
        .as_ref()
        .map(|l| format!(" at {l}"))
        .unwrap_or_default();
    match (&k.got, &k.expected) {
        (Some(got), Some(expected)) => format!("{}{at}: got {got}, expected {expected}", k.test),
        _ => format!(
            "{}{at}: {}",
            k.test,
            k.message.lines().next().unwrap_or_default()
        ),
    }
}

/// `cherenkov: <program>: prepared in N ms, baseline M ms; T tests, <selection>`.
fn prepared(p: &Prepared) -> String {
    let tests = match p.tests {
        Some(n) => format!("{n} tests"),
        None => "tests not listed".to_string(),
    };
    let selection = match p.selection() {
        Selection::Recorded => "each mutant runs the tests that call it".to_string(),
        Selection::RecordedWithFailures => {
            let failed = p.failures.iter().flat_map(|f| &f.failed_tests);
            format!(
                "each mutant runs the tests that call it; these failed recording and run every mutant: {}",
                failed.cloned().collect::<Vec<_>>().join(", ")
            )
        }
        _ => format!(
            "every mutant runs every test: {}",
            p.reason.as_deref().unwrap_or_default()
        ),
    };
    format!(
        "cherenkov: {}: prepared in {} ms, baseline {} ms; {tests}; {selection}",
        p.program, p.prepared_ms, p.baseline_ms,
    )
}

/// `<language>: N mutants; <verdict> n, ...`.
fn finished(f: &Finished, started: Option<&Started>) -> String {
    let counts: Vec<String> = f
        .counts
        .iter()
        .map(|(verdict, n)| format!("{verdict} {n}"))
        .collect();
    let accepted: Vec<String> = f
        .deficiencies
        .iter()
        .map(|(code, n)| format!("{code} {n}"))
        .collect();
    let accepted = match accepted.is_empty() {
        true => String::new(),
        false => format!("; not run, accepted: {}", accepted.join(", ")),
    };
    let (language, mutants) =
        started.map_or((String::new(), 0), |s| (s.language.clone(), s.mutants));
    format!(
        "{language}: {mutants} mutants; {}{accepted}",
        counts.join(", ")
    )
}

#[cfg(test)]
#[path = "output.test.rs"]
mod tests;
