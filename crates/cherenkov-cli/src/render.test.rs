use angzarr_embedded::markup::pack_message;
use angzarr_embedded::v1::{EventMeta, event_page};
use angzarr_embedded::{Any, Timestamp};

use serde_json::{Value, json};

use super::*;
use cherenkov_run_domain::{
    DecisionRecorded, KilledBy, MutantsPlanned, ProgramFailed, RunStarted, RunStatus, Selection,
    Tier, VerdictCount,
};

fn page(any: Any) -> EventPage {
    EventPage {
        body: Some(event_page::Body::Event(any)),
        meta: Some(EventMeta {
            recorded_at: Some(Timestamp {
                seconds: 1_763_478_000,
                nanos: 0,
            }),
            ..EventMeta::default()
        }),
        sequence: 0,
    }
}

fn mutant() -> PlannedMutant {
    PlannedMutant {
        id: "0123456789abcdef".to_string(),
        file: "src/lib.rs".to_string(),
        line: 4,
        column: 5,
        end_line: 4,
        end_column: 10,
        function: Some("total".to_string()),
        operator: "aor".to_string(),
        description: "in-place \"a - b\"".to_string(),
        original: "+".to_string(),
        mutated: Some("-".to_string()),
    }
}

/// The mutant with its plan, as a run plans it.
fn executable() -> cherenkov_run_domain::ExecutableMutant {
    cherenkov_run_domain::ExecutableMutant {
        mutant: Some(mutant()),
        plan: Some(cherenkov_run_domain::mutant::MutantPlan::default()),
    }
}

/// The run the documents are of.
fn run() -> Uuid {
    Uuid::from_bytes([7; 16])
}

/// The JSON `docs` print as.
fn json_of(docs: &[Document]) -> Vec<Value> {
    docs.iter()
        .map(|doc| serde_json::to_value(doc).expect("it is JSON"))
        .collect()
}

/// The run's record of `decision`, whole.
fn recorded(decision: MutantDecided) -> Any {
    let (id, verdict) = match (&decision.mutant, &decision.verdict) {
        (Some(mutant), Some(outcome)) => (mutant.id.clone(), outcome.verdict),
        _ => (String::new(), Verdict::Killed),
    };
    pack_message(&DecisionRecorded {
        mutant: id,
        verdict,
        decision: Some(decision),
    })
}

fn one(any: Any) -> Value {
    let docs = json_of(&documents(&run(), &page(any)));
    assert_eq!(docs.len(), 1, "{docs:?}");
    docs.into_iter().next().unwrap()
}

#[test]
fn test_every_document_names_its_event_its_run_and_when_it_was_recorded() {
    let doc = one(pack_message(&RunStarted {
        root: "/p".to_string(),
        language: "rust".to_string(),
        operators: "breadth".to_string(),
        jobs: 4,
        fast: false,
        mutants: 12,
        started_at: None,
        programs: 2,
    }));
    assert_eq!(
        doc,
        json!({
            "event": "run.started", "run": "07070707-0707-0707-0707-070707070707", "at": "2025-11-18T15:00:00Z",
            "root": "/p", "language": "rust", "operators": "breadth", "jobs": 4,
            "fast": false, "mutants": 12,
        })
    );
}

#[test]
fn test_the_mutants_planned_are_one_document_each() {
    let docs = json_of(&documents(
        &run(),
        &page(pack_message(&MutantsPlanned {
            mutants: vec![executable(), executable()],
        })),
    ));
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0]["event"], "mutant.planned");
    assert_eq!(docs[0]["id"], "0123456789abcdef");
    assert_eq!(docs[0]["end_column"], 10);
    assert_eq!(docs[0]["function"], "total");
    assert_eq!(docs[0]["original"], "+");
    assert_eq!(docs[0]["mutated"], "-");
    assert_eq!(docs[0]["description"], "in-place \"a - b\"");
}

#[test]
fn test_a_killed_mutant_s_document_holds_the_mutant_and_every_test_that_killed_it() {
    let doc = one(recorded(MutantDecided {
        mutant: Some(mutant()),
        verdict: Some(Outcome {
            verdict: Verdict::Killed,
            tier: Some(Tier::P),
            took_ms: 31,
            tests_run: vec!["test_total".to_string()],
            killed_by: vec![KilledBy {
                test: "test_total".to_string(),
                location: Some("tests/calc.rs:5:5".to_string()),
                message: "assertion `left == right` failed".to_string(),
                got: Some("-1".to_string()),
                expected: Some("5".to_string()),
            }],
            reason: None,
        }),
        ..MutantDecided::default()
    }));
    assert_eq!(doc["event"], "mutant.decided");
    assert_eq!(doc["id"], "0123456789abcdef");
    assert_eq!(doc["file"], "src/lib.rs");
    assert_eq!(doc["line"], 4);
    assert_eq!(doc["operator"], "aor");
    assert_eq!(doc["verdict"], "killed");
    assert_eq!(doc["tier"], "p");
    assert_eq!(doc["took_ms"], 31);
    assert_eq!(doc["tests_run"], json!(["test_total"]));
    assert_eq!(doc["complete"], true);
    assert_eq!(
        doc["killed_by"],
        json!([{
            "test": "test_total", "location": "tests/calc.rs:5:5",
            "message": "assertion `left == right` failed", "got": "-1", "expected": "5",
        }])
    );
    assert!(doc.get("reason").is_none());
}

#[test]
fn test_a_survivor_names_the_tests_that_executed_it_and_a_refusal_its_reason() {
    let decided = |verdict, reason: Option<&str>| MutantDecided {
        mutant: Some(mutant()),
        verdict: Some(Outcome {
            verdict,
            tier: None,
            took_ms: 0,
            tests_run: vec!["test_total".to_string()],
            killed_by: Vec::new(),
            reason: reason.map(String::from),
        }),
        ..MutantDecided::default()
    };
    let survived = one(recorded(decided(Verdict::Survived, None)));
    assert_eq!(survived["executed_by"], json!(["test_total"]));
    assert!(survived.get("killed_by").is_none());
    assert!(survived.get("tier").is_none(), "no tier when none is said");
    let unsupported = one(recorded(decided(Verdict::Unsupported, Some("no rule"))));
    assert_eq!(unsupported["reason"], "no rule");
    assert_eq!(unsupported["reason_code"], "other");
    let inlined = one(recorded(decided(
        Verdict::Unsupported,
        Some(
            "the evaluator cannot apply the mutant: Can not force return from an inlined function.",
        ),
    )));
    assert_eq!(inlined["reason_code"], "inlined");
    assert!(
        survived.get("reason_code").is_none(),
        "none without a reason"
    );
}

#[test]
fn test_a_record_of_a_decision_holding_none_prints_nothing() {
    let record = DecisionRecorded {
        mutant: "0123456789abcdef".to_string(),
        verdict: Verdict::Killed,
        decision: None,
    };
    assert!(documents(&run(), &page(pack_message(&record))).is_empty());
    // A decision with no outcome is no document either.
    let bare = recorded(MutantDecided::default());
    assert!(documents(&run(), &page(bare)).is_empty());
}

#[test]
fn test_a_mutant_s_own_event_prints_nothing() {
    let decided = MutantDecided {
        mutant: Some(mutant()),
        ..MutantDecided::default()
    };
    assert!(documents(&run(), &page(pack_message(&decided))).is_empty());
}

#[test]
fn test_a_prepared_program_and_the_run_s_finish_are_documents_of_their_own() {
    let prepared = one(pack_message(&cherenkov_run_domain::ProgramPrepared {
        program: "target/debug/deps/calc-1".to_string(),
        baseline_ms: 40,
        prepared_ms: 90,
        tests: Some(3),
        selection: Selection::RecordedWithFailures,
        failed_tests: vec!["test_flaky".to_string()],
        selection_reason: None,
        index: 0,
        budget_ms: 200,
        drives: None,
    }));
    assert_eq!(prepared["event"], "program.prepared");
    assert_eq!(prepared["program"], "target/debug/deps/calc-1");
    assert_eq!(prepared["baseline_ms"], 40);
    assert_eq!(prepared["prepared_ms"], 90);
    assert_eq!(prepared["tests"], 3);
    assert_eq!(prepared["selection"], "recorded-with-failures");
    assert_eq!(prepared["failed_tests"], json!(["test_flaky"]));
    assert!(prepared.get("reason").is_none());
    let blind = one(pack_message(&cherenkov_run_domain::ProgramPrepared {
        program: "calc-2".to_string(),
        baseline_ms: 0,
        prepared_ms: 0,
        tests: None,
        selection: Selection::EveryTest,
        failed_tests: Vec::new(),
        selection_reason: Some("no list".to_string()),
        index: 1,
        budget_ms: 2_000,
        drives: None,
    }));
    assert_eq!(blind["tests"], Value::Null, "not counted");
    assert_eq!(blind["reason"], "no list");
    assert!(blind.get("failed_tests").is_none());
    let failed = one(pack_message(&ProgramFailed {
        program: Some("calc-1".to_string()),
        reason: "unmutated tests do not pass".to_string(),
    }));
    assert_eq!(failed["event"], "program.failed");
    assert_eq!(failed["program"], "calc-1");
    assert_eq!(failed["reason"], "unmutated tests do not pass");
    let finished = one(pack_message(&RunFinished {
        counts: vec![
            VerdictCount {
                verdict: Verdict::Killed,
                count: 2,
            },
            VerdictCount {
                verdict: Verdict::Survived,
                count: 1,
            },
        ],
        took_ms: 1200,
        status: RunStatus::Complete,
    }));
    assert_eq!(finished["event"], "run.finished");
    assert_eq!(finished["counts"], json!({"killed": 2, "survived": 1}));
    assert_eq!(finished["took_ms"], 1200);
    assert_eq!(finished["exit"], 0, "the program's to say");
    assert_eq!(finished["status"], "complete");
    assert_eq!(finished["deficiencies"], json!({}));
}

#[test]
fn test_a_document_reads_its_event_first() {
    // A person scanning the stream, or `grep '^{"event":"mutant.decided"'`,
    // sees what each line is before its fields.
    let doc = one(pack_message(&RunFinished {
        counts: Vec::new(),
        took_ms: 0,
        status: RunStatus::Complete,
    }));
    assert!(
        doc.to_string().starts_with(
            r#"{"event":"run.finished","run":"07070707-0707-0707-0707-070707070707","at":"#
        ),
        "{doc}"
    );
}

#[test]
fn test_each_reason_has_the_code_of_its_verdict() {
    let coded = |verdict, reason: &str| reason_code(verdict, reason);
    assert_eq!(
        coded(Verdict::Unviable, "Settle has no Default in the source"),
        "no-default"
    );
    assert_eq!(
        coded(
            Verdict::Unviable,
            "Poll::Ready(Ok(())) does not compile as Poll<X>"
        ),
        "does-not-compile"
    );
    assert_eq!(
        coded(Verdict::NotCalled, "no selected test calls it"),
        "not-called"
    );
    assert_eq!(
        coded(
            Verdict::NotReached,
            "the selected tests never execute the mutated code"
        ),
        "not-reached"
    );
    assert_eq!(
        coded(Verdict::Error, "the debugger failed"),
        "debugger-failed"
    );
    assert_eq!(coded(Verdict::Timeout, "anything"), "none");
}
