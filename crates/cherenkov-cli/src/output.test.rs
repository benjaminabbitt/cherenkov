use std::collections::BTreeMap;

use cherenkov_run_wire::output::{Executors, FailedTests, Killers};
use cherenkov_run_wire::{RunStatus, Tier};

use super::*;

/// A planned mutant, as `mutant.decided` documents hold it.
fn planned(original: &str, mutated: &str) -> PlannedMutant {
    PlannedMutant {
        id: "0123456789abcdef".to_string(),
        file: "src/lib.rs".to_string(),
        line: 4,
        column: 5,
        end_line: 4,
        end_column: 6,
        function: Some("total".to_string()),
        operator: "aor".to_string(),
        description: "in-place \"a - b\"".to_string(),
        original: original.to_string(),
        mutated: Some(mutated.to_string()),
    }
}

/// A decided mutant of `verdict`, the rest as `fields` says.
fn mutant(verdict: Verdict, fields: impl FnOnce(&mut Decided)) -> Document {
    let mut decided = Decided {
        run: None,
        at: None,
        mutant: Some(planned("+", "-")),
        verdict: verdict.into(),
        tier: None,
        took_ms: 31,
        tests_run: vec!["test_total".to_string()],
        killers: None,
        executors: None,
        reason: None,
        reason_code: None,
    };
    fields(&mut decided);
    Document {
        event: Some(Event::Decided(decided)),
    }
}

fn out(doc: &Document) -> Vec<String> {
    render(doc, Listed::Decided, None).0
}

#[test]
fn test_a_killed_mutant_is_its_line_then_what_it_changed_who_killed_it_and_its_time() {
    let doc = mutant(Verdict::Killed, |d| {
        d.tier = Some(Tier::P.into());
        d.killers = Some(Killers {
            killed_by: vec![KilledBy {
                test: "test_total".to_string(),
                location: Some("tests/calc.rs:5:5".to_string()),
                message: "assertion failed".to_string(),
                got: Some("-1".to_string()),
                expected: Some("5".to_string()),
            }],
            complete: true,
        });
    });
    assert_eq!(
        out(&doc),
        [
            "0123456789abcdef src/lib.rs:4:5 total aor killed p",
            "  `+` becomes `-`",
            "  killed by test_total at tests/calc.rs:5:5: got -1, expected 5",
            "  took 31 ms",
        ]
    );
}

#[test]
fn test_a_survivor_names_the_tests_that_ran_it_unnoticed() {
    let doc = mutant(Verdict::Survived, |d| {
        d.tier = Some(Tier::P.into());
        d.executors = Some(Executors {
            executed_by: vec!["test_total".to_string()],
        });
    });
    assert_eq!(out(&doc)[2], "  executed by test_total, unnoticed");
    let every = mutant(Verdict::Survived, |d| {
        d.executors = Some(Executors::default())
    });
    assert_eq!(out(&every)[2], "  executed by every test, unnoticed");
}

#[test]
fn test_a_deletion_reads_as_one() {
    let doc = mutant(Verdict::Survived, |d| {
        d.mutant = Some(planned("log(s);", ""))
    });
    assert_eq!(out(&doc)[1], "  `log(s);` deleted");
}

#[test]
fn test_a_mutant_not_run_says_why_and_a_multi_line_change_is_left_to_json() {
    let doc = mutant(Verdict::Unsupported, |d| {
        d.reason = Some("no rule\nfor this".to_string());
        d.mutant = Some(planned("{\n    a + b\n}", "0"));
    });
    assert_eq!(
        out(&doc),
        [
            "0123456789abcdef src/lib.rs:4:5 total aor unsupported",
            "  reason: no rule\n    for this",
            "  took 31 ms",
        ]
    );
}

#[test]
fn test_a_plan_lists_each_planned_mutant_and_a_run_does_not() {
    let doc = Document {
        event: Some(Event::Planned(cherenkov_run_wire::output::Planned {
            run: None,
            at: None,
            mutant: Some(planned("+", "-")),
        })),
    };
    assert_eq!(
        render(&doc, Listed::Planned, None).0,
        ["0123456789abcdef src/lib.rs:4:5 total aor in-place \"a - b\""]
    );
    assert!(render(&doc, Listed::Decided, None).0.is_empty());
}

fn started(language: &str, mutants: u32) -> Started {
    Started {
        run: None,
        at: None,
        root: "/p".to_string(),
        language: language.to_string(),
        operators: "breadth".to_string(),
        jobs: 1,
        fast: false,
        mutants,
    }
}

fn finished(counts: &[(&str, u32)], deficiencies: &[(&str, u32)]) -> Finished {
    let map = |pairs: &[(&str, u32)]| -> BTreeMap<String, u32> {
        pairs.iter().map(|&(k, n)| (k.to_string(), n)).collect()
    };
    Finished {
        run: None,
        at: None,
        counts: map(counts).into_iter().collect(),
        took_ms: 1,
        exit: 0,
        status: RunStatus::Complete.into(),
        deficiencies: map(deficiencies).into_iter().collect(),
    }
}

#[test]
fn test_progress_goes_to_stderr() {
    let prepared = Document {
        event: Some(Event::Prepared(Prepared {
            run: None,
            at: None,
            program: "target/debug/deps/calc-1".to_string(),
            baseline_ms: 40,
            prepared_ms: 90,
            tests: Some(3),
            selection: Selection::RecordedWithFailures.into(),
            failures: Some(FailedTests {
                failed_tests: vec!["test_flaky".to_string()],
            }),
            reason: None,
        })),
    };
    let (out, err) = render(&prepared, Listed::Decided, None);
    assert!(out.is_empty());
    assert_eq!(
        err,
        [
            "cherenkov: target/debug/deps/calc-1: prepared in 90 ms, baseline 40 ms; 3 tests; \
          each mutant runs the tests that call it; these failed recording and run every mutant: test_flaky"
        ]
    );
    let done = Document {
        event: Some(Event::Finished(finished(
            &[("killed", 2), ("not-called", 1)],
            &[],
        ))),
    };
    assert_eq!(
        render(&done, Listed::Decided, Some(&started("rust", 3))).1,
        ["rust: 3 mutants; killed 2, not-called 1"]
    );
}

#[test]
fn test_text_writes_each_document_as_it_comes() {
    let mut text = Text::new(Vec::new(), Vec::new(), Listed::Decided);
    text.document(&mutant(Verdict::Killed, |d| d.tier = Some(Tier::P.into())));
    let written = String::from_utf8(text.out.clone()).unwrap();
    assert!(written.starts_with("0123456789abcdef "), "{written}");
}

/// Documents recorded by a sink.
#[derive(Default, Clone)]
struct Kept(std::sync::Arc<std::sync::Mutex<Vec<Document>>>);

impl Sink for Kept {
    fn document(&mut self, doc: &Document) {
        self.0.lock().unwrap().push(doc.clone());
    }
}

/// A log that can be read after the sink is dropped.
#[derive(Default, Clone)]
struct Log(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A mutant of `verdict` not run, for `reason` and its code.
fn refused(verdict: Verdict, reason: &str, code: &str) -> Document {
    mutant(verdict, |d| {
        d.reason = Some(reason.to_string());
        d.reason_code = Some(code.to_string());
    })
}

#[test]
fn test_the_run_s_finish_counts_the_mutants_it_could_not_run_by_reason() {
    let log = Log::default();
    let kept = Kept::default();
    {
        let mut sink = Deficiencies::new(kept.clone(), Some(log.clone()));
        let reason =
            "the evaluator cannot apply the mutant: Can not force return from an inlined function.";
        sink.document(&refused(Verdict::Unsupported, reason, "inlined"));
        sink.document(&refused(Verdict::Unsupported, "x", "inlined"));
        sink.document(&refused(Verdict::Unsupported, "y", "other"));
        sink.document(&mutant(Verdict::Killed, |_| {}));
        sink.document(&refused(Verdict::Unviable, "z", "no-default"));
        sink.document(&Document {
            event: Some(Event::Finished(finished(&[("unsupported", 3)], &[]))),
        });
    }
    let docs = kept.0.lock().unwrap();
    let Some(Event::Finished(finished)) = &docs.last().unwrap().event else {
        panic!("{docs:?}");
    };
    assert_eq!(
        finished.deficiencies,
        [("inlined".to_string(), 2), ("other".to_string(), 1)].into()
    );
    assert_eq!(docs.len(), 6, "every document goes on");
    // The log has one line for each mutant not run, with its reason.
    let lines: Vec<serde_json::Value> = String::from_utf8(log.0.lock().unwrap().clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["reason_code"], "inlined");
    assert_eq!(lines[0]["file"], "src/lib.rs");
    assert_eq!(lines[0]["line"], 4);
    assert_eq!(lines[0]["column"], 5);
    assert_eq!(lines[0]["mutated"], "-");
    assert!(lines[0]["reason"].as_str().unwrap().contains("inlined"));
}

#[test]
fn test_the_finish_says_the_exit_code_its_status_maps_to() {
    for (status, exit) in [(RunStatus::Complete, 0), (RunStatus::Failed, 32)] {
        let kept = Kept::default();
        let mut sink = Deficiencies::new(kept.clone(), None::<Log>);
        let mut done = finished(&[], &[]);
        done.status = status.into();
        sink.document(&Document {
            event: Some(Event::Finished(done)),
        });
        let docs = kept.0.lock().unwrap();
        let Some(Event::Finished(told)) = &docs[0].event else {
            panic!("{docs:?}");
        };
        assert_eq!(told.exit, exit, "{status:?}");
        assert_eq!(exit_code(status), exit);
    }
}

#[test]
fn test_the_text_summary_names_the_reasons_accepted() {
    let doc = Document {
        event: Some(Event::Finished(finished(
            &[("killed", 4), ("unsupported", 3)],
            &[("inlined", 2), ("other", 1)],
        ))),
    };
    let (_, err) = render(&doc, Listed::Decided, Some(&started("rust", 7)));
    assert_eq!(
        err,
        ["rust: 7 mutants; killed 4, unsupported 3; not run, accepted: inlined 2, other 1"]
    );
}

mod printing {
    use std::sync::{Arc, Mutex};

    use angzarr_embedded::angzarr_router::hpb::ProjectRequest;
    use angzarr_embedded::markup::pack_message;
    use angzarr_embedded::v1::{Cover, EventBook, EventPage, Uuid, event_page};
    use cherenkov_run_domain::{ProgramFailed, RUN_DOMAIN, RunStarted};

    use super::*;

    fn page(sequence: u64, event: angzarr_embedded::Any) -> EventPage {
        EventPage {
            sequence,
            body: Some(event_page::Body::Event(event)),
            meta: None,
        }
    }

    fn request(root: Option<Uuid>, pages: Vec<EventPage>) -> ProjectRequest {
        ProjectRequest {
            events: Some(EventBook {
                cover: Some(Cover {
                    domain: RUN_DOMAIN.to_string(),
                    root,
                    edition: None,
                }),
                pages,
                next_sequence: 0,
                snapshot: None,
            }),
        }
    }

    fn started() -> angzarr_embedded::Any {
        pack_message(&RunStarted::default())
    }

    fn failed() -> angzarr_embedded::Any {
        pack_message(&ProgramFailed {
            program: None,
            reason: "no debugger".to_string(),
        })
    }

    #[tokio::test]
    async fn test_each_page_handed_is_printed_as_its_documents_in_order() {
        let kept = Kept::default();
        let output = printing(Box::new(kept.clone()));
        let run = Uuid::from_bytes([3; 16]);
        output
            .dispatch(&request(
                Some(run.clone()),
                vec![page(0, started()), page(1, failed())],
            ))
            .await
            .unwrap();
        let docs = kept.0.lock().unwrap();
        let events: Vec<&str> = docs
            .iter()
            .map(|d| match d.event {
                Some(Event::Started(_)) => "started",
                Some(Event::Failed(_)) => "failed",
                _ => "other",
            })
            .collect();
        assert_eq!(events, ["started", "failed"]);
        // Each names the run it was handed under.
        let Some(Event::Started(first)) = &docs[0].event else {
            panic!("{docs:?}");
        };
        assert_eq!(first.run, Some(run));
    }

    #[tokio::test]
    async fn test_a_stream_with_no_root_or_a_book_with_no_cover_prints_nothing() {
        let kept = Kept::default();
        let output = printing(Box::new(kept.clone()));
        output
            .dispatch(&request(None, vec![page(0, started())]))
            .await
            .unwrap();
        output
            .dispatch(&ProjectRequest { events: None })
            .await
            .unwrap();
        assert!(kept.0.lock().unwrap().is_empty());
    }

    #[test]
    fn test_the_output_is_named() {
        let output = printing(Box::new(Kept::default()));
        assert_eq!(output.name(), OUTPUT);
    }

    /// Documents kept by a sink that can be shared.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<Document>>>);

    impl Sink for Kept {
        fn document(&mut self, doc: &Document) {
            self.0.lock().unwrap().push(doc.clone());
        }
    }
}

mod verdict_file {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use angzarr_diagnostic::Diagnostic;
    use cherenkov_run_application::verdicts::VerdictFile;
    use cherenkov_run_wire::output::VerdictLine;

    use super::*;

    /// A directory of this test's own, gone with it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("cherenkov-verdicts-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn line(id: &str, verdict: Verdict) -> VerdictLine {
        VerdictLine {
            id: id.to_string(),
            verdict: verdict.into(),
            file: "src/lib.rs".to_string(),
            line: 4,
            column: 5,
            function: None,
            operator: "aor".to_string(),
            original: "+".to_string(),
            mutated: Some("-".to_string()),
            killed_by: vec!["t".to_string()],
        }
    }

    #[test]
    fn test_the_file_holds_one_json_line_for_each_mutant_and_replaces_what_was_there() {
        let scratch = Scratch::new();
        let path = scratch.0.join(".cherenkov").join("verdicts.jsonl");
        let file = JsonVerdictFile::new(path.clone());
        file.write(&[line("a", Verdict::Killed), line("b", Verdict::Survived)])
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(r#"{"id":"a","verdict":"killed","file":"src/lib.rs""#));
        assert!(lines[1].contains(r#""verdict":"survived""#));
        assert!(text.ends_with('\n'));
        // Written again, it is replaced, and the same lines are the same bytes.
        file.write(&[line("a", Verdict::Killed)]).unwrap();
        let again = std::fs::read_to_string(&path).unwrap();
        assert_eq!(again.lines().count(), 1);
        file.write(&[line("a", Verdict::Killed)]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), again);
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "no temporary file is left behind");
    }

    #[test]
    fn test_a_file_that_cannot_be_written_is_a_report_naming_the_path() {
        let scratch = Scratch::new();
        let blocker = scratch.0.join("file");
        std::fs::write(&blocker, "x").unwrap();
        // Its parent is a file.
        let file = JsonVerdictFile::new(blocker.join("verdicts.jsonl"));
        let error = file.write(&[]).unwrap_err();
        assert_eq!(error.code(), "cherenkov-cli-unreadable");
        assert!(error.to_string().contains("verdicts.jsonl"), "{error}");
    }
}
