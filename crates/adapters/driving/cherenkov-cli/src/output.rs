//! A run's events as the command prints them: one JSON document per line
//! (`output-events.md`), or text for a person, each as it is recorded.
//!
//! Text puts what a reader acts on on stdout, one line per mutant
//! (`<id> <file:line:col> <fn> <op> <verdict> [tier]`) with its details
//! indented beneath it, or, for a plan, one line per planned mutant
//! (`<id> <listing>`); progress (each program prepared, the run's counts)
//! goes to stderr.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use crate::notice::{Notice, Say};
use crate::render::documents;
use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use cherenkov_run_application::host::Output;
use cherenkov_run_application::verdicts::VerdictFile;
use cherenkov_run_wire::names;
use cherenkov_run_wire::output::VerdictLine;
use cherenkov_run_wire::output::{
    Decided, Document, Finished, OperatorSet, Prepared, Started, document::Event,
};
use cherenkov_run_wire::{KilledBy, PlannedMutant};
use cherenkov_run_wire::{RunStatus, Selection, Verdict};
use cli_support::Exit;
use coded_diagnostic::Report;
use presentation::{Catalog, Format};

/// Where a run's documents go, each as soon as it is recorded.
pub trait Sink: Send {
    fn document(&mut self, doc: &Document);
}

/// A sink writing each document as one line of JSON, flushed.
pub struct JsonLines<W: Write + Send>(pub W);

impl<W: Write + Send> Sink for JsonLines<W> {
    fn document(&mut self, doc: &Document) {
        if let Ok(line) = serde_json::to_string(doc) {
            let _ = writeln!(self.0, "{line}");
        }
        let _ = self.0.flush();
    }
}

/// The output projector's name.
pub const OUTPUT: &str = "output";

/// The projector that prints a run: each event of the run's stream the bus
/// hands it, as the documents it renders to, to its sink. It reads nothing
/// else: the run records each decision whole.
struct Printing {
    sink: Mutex<Box<dyn Sink>>,
    catalog: Arc<Catalog>,
}

impl ProjectorHandler for Printing {
    fn name(&self) -> &str {
        OUTPUT
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let Some(book) = &request.events else {
            return Ok(ProjectResponse::default());
        };
        let Some(run) = book.cover.as_ref().and_then(|cover| cover.root.as_ref()) else {
            return Ok(ProjectResponse::default());
        };
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        for doc in book
            .pages
            .iter()
            .flat_map(|page| documents(run, page, &self.catalog))
        {
            sink.document(&doc);
        }
        Ok(ProjectResponse::default())
    }
}

/// The verdict file: one JSON line for each mutant (the generated serde of
/// `VerdictLine`), written whole beside the file and renamed over it, so a
/// reader never sees half a file.
pub struct JsonVerdictFile {
    path: PathBuf,
    /// Leave out the mutants no test called (an `--in-diff` run's file).
    without_not_called: bool,
}

impl JsonVerdictFile {
    pub fn new(path: PathBuf) -> JsonVerdictFile {
        JsonVerdictFile {
            path,
            without_not_called: false,
        }
    }

    /// The file of an `--in-diff` run: no line for a mutant that was not
    /// called.
    pub fn without_not_called(mut self) -> JsonVerdictFile {
        self.without_not_called = true;
        self
    }

    fn unwritable(&self, reason: &dyn std::fmt::Display) -> Report {
        Report::new("cherenkov-cli-unreadable")
            .arg("path", self.path.display().to_string())
            .arg("detail", reason.to_string())
    }
}

impl VerdictFile for JsonVerdictFile {
    fn write(&self, lines: &[VerdictLine]) -> Result<(), Report> {
        let mut text = String::new();
        for line in lines
            .iter()
            .filter(|l| !(self.without_not_called && l.verdict() == Verdict::NotCalled))
        {
            text.push_str(&serde_json::to_string(line).map_err(|e| self.unwritable(&e))?);
            text.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| self.unwritable(&e))?;
        }
        let beside = self.path.with_extension("jsonl.tmp");
        std::fs::write(&beside, text).map_err(|e| self.unwritable(&e))?;
        std::fs::rename(&beside, &self.path).map_err(|e| self.unwritable(&e))
    }
}

/// The output of a run that prints to `sink`, a reason in `catalog`'s words.
pub fn printing(sink: Box<dyn Sink>, catalog: Arc<Catalog>) -> Output {
    Box::new(Printing {
        sink: Mutex::new(sink),
        catalog,
    })
}

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
pub fn sink(format: Format, listed: Listed, log: Option<Box<dyn Write + Send>>) -> Box<dyn Sink> {
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
        RunStatus::Failed | RunStatus::Abandoned => Exit::FAILED,
    };
    i32::from(exit.code(false))
}

/// The sink of an `--in-diff` run: the mutants no test called (many, since
/// the changed tests are matched against everything they can reach) are not
/// listed, one `mutant.decided` each; `run.finished` still counts them. A
/// text run says so, and who judged the mutants of changed tests, after its
/// summary.
pub struct InDiff {
    inner: Box<dyn Sink>,
    say: Say,
    skipped: u64,
}

impl InDiff {
    /// The sink of an `--in-diff` run, its note said through `say`.
    pub fn new(inner: Box<dyn Sink>, say: Say) -> Self {
        InDiff {
            inner,
            say,
            skipped: 0,
        }
    }
}

impl Sink for InDiff {
    fn document(&mut self, doc: &Document) {
        match &doc.event {
            Some(Event::Decided(d)) if d.verdict() == Verdict::NotCalled => self.skipped += 1,
            Some(Event::Finished(_)) => {
                self.inner.document(doc);
                self.say.say(&Notice::InDiff {
                    skipped: self.skipped,
                });
            }
            _ => self.inner.document(doc),
        }
    }
}

/// Accepts and logs what could not be run: counts each `unsupported`
/// mutant's `reason_code`, writes it to the log, and adds the counts to the
/// run's `run.finished` as `deficiencies`, with the exit code its status
/// says. Every document goes on unchanged but that one.
pub struct Deficiencies<W: Write + Send> {
    inner: Box<dyn Sink>,
    log: Option<W>,
    counts: std::collections::BTreeMap<String, u32>,
}

impl<W: Write + Send> Deficiencies<W> {
    pub fn new(inner: impl Sink + 'static, log: Option<W>) -> Self {
        Deficiencies {
            inner: Box::new(inner),
            log,
            counts: Default::default(),
        }
    }
}

impl<W: Write + Send> Sink for Deficiencies<W> {
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
pub struct Text<O: Write + Send, E: Write + Send> {
    out: O,
    err: E,
    listed: Listed,
    started: Option<Started>,
}

impl<O: Write + Send, E: Write + Send> Text<O, E> {
    pub fn new(out: O, err: E, listed: Listed) -> Self {
        Text {
            out,
            err,
            listed,
            started: None,
        }
    }
}

impl<O: Write + Send, E: Write + Send> Sink for Text<O, E> {
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
    let (language, mutants) = started.map_or((String::new(), 0), |s| {
        (languages_of(&s.operators), s.mutants)
    });
    format!(
        "{language}: {mutants} mutants; {}{accepted}",
        counts.join(", ")
    )
}

/// The languages of a run's operator sets, each once, comma-joined.
fn languages_of(operators: &[OperatorSet]) -> String {
    let mut languages: Vec<&str> = Vec::new();
    for entry in operators {
        if !languages.contains(&entry.language.as_str()) {
            languages.push(&entry.language);
        }
    }
    languages.join(",")
}

#[cfg(test)]
#[path = "output.test.rs"]
mod tests;
