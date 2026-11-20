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
struct Kept(std::rc::Rc<std::cell::RefCell<Vec<Document>>>);

impl Sink for Kept {
    fn document(&mut self, doc: &Document) {
        self.0.borrow_mut().push(doc.clone());
    }
}

/// A log that can be read after the sink is dropped.
#[derive(Default, Clone)]
struct Log(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl Write for Log {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
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
    let docs = kept.0.borrow();
    let Some(Event::Finished(finished)) = &docs.last().unwrap().event else {
        panic!("{docs:?}");
    };
    assert_eq!(
        finished.deficiencies,
        [("inlined".to_string(), 2), ("other".to_string(), 1)].into()
    );
    assert_eq!(docs.len(), 6, "every document goes on");
    // The log has one line for each mutant not run, with its reason.
    let lines: Vec<serde_json::Value> = String::from_utf8(log.0.borrow().clone())
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
    for (status, exit) in [(RunStatus::Complete, 0), (RunStatus::Failed, 10)] {
        let kept = Kept::default();
        let mut sink = Deficiencies::new(kept.clone(), None::<Log>);
        let mut done = finished(&[], &[]);
        done.status = status.into();
        sink.document(&Document {
            event: Some(Event::Finished(done)),
        });
        let docs = kept.0.borrow();
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
