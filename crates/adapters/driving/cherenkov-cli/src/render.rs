//! A run's events, each with its decisions whole, as the documents a reader
//! prints, one per line (`output-events.md`): the messages of
//! `cherenkov.output.v1`, whose JSON the wire crate's generated code writes.
//! Each has the event's name, the run and when it was recorded, and is
//! usable on its own.

use crate::code::reason_code;
use angzarr_embedded::v1::{EventPage, Uuid};
use cherenkov_run_domain::events::{RunEvent, recorded_at, run_event};
use cherenkov_run_domain::why;
use cherenkov_run_domain::{
    MutantDecided, Outcome, PlannedMutant, ProgramPrepared, RunFinished, Selection, Verdict,
};
use cherenkov_run_wire::output::{
    Decided, Document, Executors, Failed, FailedTests, Finished, Killers, OperatorSet, Planned,
    Prepared, Started, document::Event,
};
use cherenkov_run_wire::{self as wire, names};
use coded_diagnostic::Report;
use presentation::Catalog;

/// The documents `page` of run `run`'s stream prints as: one per mutant for
/// the mutants planned, one for a mutant's decision (the run records it
/// whole), one for each other run event, and none for anything else. A
/// reason the run recorded coded is said in `catalog`'s words.
pub fn documents(run: &Uuid, page: &EventPage, catalog: &Catalog) -> Vec<Document> {
    let at = recorded_at(page);
    let document = |event| Document { event: Some(event) };
    let Some(event) = run_event(page) else {
        return Vec::new();
    };
    match event {
        RunEvent::RunStarted(s) => vec![document(Event::Started(Started {
            run: Some(run.clone()),
            at: Some(at),
            root: s.root,
            operators: s
                .operators
                .into_iter()
                .map(|o| OperatorSet {
                    language: o.language,
                    set: o.set,
                })
                .collect(),
            jobs: s.jobs,
            fast: s.fast,
            mutants: s.mutants,
        }))],
        RunEvent::MutantsPlanned(planned) => planned
            .mutants
            .into_iter()
            .filter_map(|executable| executable.mutant)
            .map(|m| {
                document(Event::Planned(Planned {
                    run: Some(run.clone()),
                    at: Some(at),
                    mutant: Some(m.into()),
                }))
            })
            .collect(),
        RunEvent::ProgramPrepared(p) => {
            vec![document(Event::Prepared(prepared(run, at, p, catalog)))]
        }
        RunEvent::ProgramFailed(f) => vec![document(Event::Failed(Failed {
            run: Some(run.clone()),
            at: Some(at),
            program: f.program,
            reason: said(catalog, &why::decode(f.why.as_ref(), &f.reason)),
            output_tail: f.output_tail,
        }))],
        RunEvent::DecisionRecorded(record) => record
            .decision
            .and_then(|decision| decided(run, at, decision, catalog))
            .map(Event::Decided)
            .map(document)
            .into_iter()
            .collect(),
        // Readiness is no event a reader needs, and a run that was
        // superseded is not one that is printed: its `run.finished` says so.
        RunEvent::RunReady(_) | RunEvent::RunAbandoned(_) => Vec::new(),
        RunEvent::RunFinished(f) => vec![document(Event::Finished(finished(run, at, f)))],
    }
}

/// A duration in the milliseconds a document holds.
fn millis(ms: u64) -> u32 {
    u32::try_from(ms).unwrap_or(u32::MAX)
}

/// A prepared program's document: its timings, its tests, and how its
/// mutants choose their tests (with the tests that failed recording, or
/// why every test runs).
fn prepared(
    run: &Uuid,
    at: angzarr_embedded::Timestamp,
    p: ProgramPrepared,
    catalog: &Catalog,
) -> Prepared {
    Prepared {
        run: Some(run.clone()),
        at: Some(at),
        program: p.program,
        baseline_ms: millis(p.baseline_ms),
        prepared_ms: millis(p.prepared_ms),
        tests: p.tests,
        selection: p.selection.into(),
        failures: (p.selection == Selection::RecordedWithFailures).then_some(FailedTests {
            failed_tests: p.failed_tests,
        }),
        reason: match (&p.selection_why, p.selection_reason) {
            (None, None) => None,
            (why_, text) => Some(said(
                catalog,
                &why::decode(why_.as_ref(), text.as_deref().unwrap_or_default()),
            )),
        },
    }
}

/// `report` in words: the catalog's, or its keyed form where the catalog
/// holds no message for it.
fn said(catalog: &Catalog, report: &Report) -> String {
    catalog
        .render(report)
        .unwrap_or_else(|_| report.to_string())
}

/// A decided mutant's document: the mutant, its verdict, and what the
/// verdict says (who killed it, who executed it, why it was not run). None
/// for a decision that holds no outcome.
fn decided(
    run: &Uuid,
    at: angzarr_embedded::Timestamp,
    d: MutantDecided,
    catalog: &Catalog,
) -> Option<Decided> {
    let Outcome {
        verdict,
        tier,
        took_ms,
        tests_run,
        killed_by,
        reason,
        why,
    } = d.verdict?;
    // A reason recorded as text (before reasons were coded) reads as a
    // recorded one.
    let recorded = match (why.as_ref(), reason) {
        (None, None) => None,
        (why, text) => Some(why::decode(why, &text.unwrap_or_default())),
    };
    let (reason, reason_code) = match recorded {
        Some(why) => (Some(said(catalog, &why)), Some(reason_code(verdict, &why))),
        None => (None, None),
    };
    Some(Decided {
        run: Some(run.clone()),
        at: Some(at),
        mutant: d.mutant.map(wire_mutant),
        verdict: verdict.into(),
        tier: tier.map(Into::into),
        took_ms: millis(took_ms),
        killers: (verdict == Verdict::Killed).then(|| Killers {
            killed_by: killed_by.into_iter().map(Into::into).collect(),
            complete: true,
        }),
        executors: (verdict == Verdict::Survived).then(|| Executors {
            executed_by: tests_run.clone(),
        }),
        tests_run,
        reason,
        reason_code,
    })
}

fn wire_mutant(m: PlannedMutant) -> wire::PlannedMutant {
    m.into()
}

/// A finished run's document: its tallies by the verdicts' names, its time
/// and its status. Its exit code and its deficiencies are the program's to
/// say.
fn finished(run: &Uuid, at: angzarr_embedded::Timestamp, f: RunFinished) -> Finished {
    Finished {
        run: Some(run.clone()),
        at: Some(at),
        counts: f
            .counts
            .into_iter()
            .map(|tally| (names::verdict(tally.verdict.into()), tally.count))
            .collect(),
        took_ms: millis(f.took_ms),
        exit: 0,
        status: f.status.into(),
        deficiencies: Default::default(),
    }
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
