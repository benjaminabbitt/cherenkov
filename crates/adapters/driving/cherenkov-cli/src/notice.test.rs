use super::*;
use cherenkov_mutation::syntax::SkipKind;

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

impl Written {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn catalog() -> Arc<Catalog> {
    let mut sources = vec![crate::error::EN_US];
    sources.extend(cherenkov_run_application::error::CATALOG_SOURCES);
    Arc::new(Catalog::en_us(&sources).unwrap())
}

/// What `format` says of `notice`.
fn said(format: Format, notice: &Notice) -> String {
    let written = Written::default();
    Say::to(format, catalog(), written.clone()).say(notice);
    written.text()
}

fn text(notice: &Notice) -> String {
    said(Format::Text, notice)
}

#[test]
fn test_a_text_notice_is_the_program_s_name_and_its_message_on_a_line() {
    let cases = [
        (
            Notice::Operators {
                label: "breadth".into(),
            },
            "cherenkov: operators: breadth\n",
        ),
        (
            Notice::FileProblem {
                path: "a.rs".into(),
                cause: Report::new("cherenkov-run-application-file-unreadable")
                    .arg("detail", "denied"),
            },
            "cherenkov: problem: a.rs: unreadable: denied\n",
        ),
        (
            Notice::Provenance {
                each: "breadth 10, mine 3".into(),
                shared: 2,
            },
            "cherenkov: provenance: breadth 10, mine 3; in more than one set: 2\n",
        ),
        (
            Notice::Phase {
                phase: "build".into(),
                tenths: 15,
            },
            "cherenkov: phase build: 1.5 s\n",
        ),
        (
            Notice::PhaseStarted {
                phase: "viability check".into(),
            },
            "cherenkov: phase viability check started\n",
        ),
        (
            Notice::ViabilityProgress {
                compile: 2,
                batch: 1,
                batches: 3,
            },
            "cherenkov: viability check: cargo check run 2 (batch 1 of 3)\n",
        ),
        (
            Notice::Phases {
                preparation_tenths: 25,
                mutants_tenths: 75,
            },
            "cherenkov: phase preparation and recording: 2.5 s; phase mutants: 7.5 s\n",
        ),
        (
            Notice::ViabilityOff,
            "cherenkov: viability check off (--no-viability-check): mutants rustc would reject stay unsupported\n",
        ),
        (
            Notice::Viability {
                rejected: 3,
                checked: 10,
                compiles: 2,
                tenths: 12,
            },
            "cherenkov: viability check: 3 of 10 function-body replacements rejected by rustc (unviable when the debugger cannot run them), 2 cargo check runs, 1.2 s\n",
        ),
        (
            Notice::ViabilitySkipped {
                cause: Report::new("cherenkov-run-application-no-workers"),
            },
            "cherenkov: viability check skipped (no workers): mutants rustc would reject stay unsupported; --no-viability-check turns it off\n",
        ),
        (
            Notice::CarriedOn { run: "r1".into() },
            "cherenkov: carried on run r1\n",
        ),
        (
            Notice::Workers { cpus: 4 },
            "cherenkov: 4 workers (cores; available memory unknown)\n",
        ),
        (
            Notice::WorkersSized {
                jobs: 3,
                cpus: 8,
                available_mib: 2048,
                per_worker_mib: 512,
            },
            "cherenkov: 3 workers (8 cores; 2048 MiB available, about 512 MiB per worker)\n",
        ),
        (
            Notice::InDiff { skipped: 3 },
            "cherenkov: --in-diff: 3 not-called mutants not listed (counted above); changed-test mutants are judged by the changed tests alone, so a survivor means those tests do not catch it\n",
        ),
    ];
    for (notice, line) in cases {
        assert_eq!(text(&notice), line, "{notice}");
    }
}

#[test]
fn test_a_compiling_notice_says_how_many_crates_are_done_and_the_latest() {
    let notice = Notice::Compiling {
        done: 12,
        last: "serde".into(),
    };
    assert_eq!(
        text(&notice),
        "cherenkov: compiling: 12 crates done, last serde\n"
    );
}

#[test]
fn test_a_plan_says_each_optional_part_only_when_it_has_something_to_say() {
    let full = Notice::Planned(Box::new(sample_planned()));
    assert_eq!(
        text(&full),
        "cherenkov: python: 12 mutants; concepts mapped 13/16; unmapped: expr, name; \
         unrepresentable: binary 1; skipped (no action for context): 2; \
         skipped regions: test 1; disabled: 3; deduplicated: 4\n"
    );
    let bare = Notice::Planned(Box::new(Planned {
        unmapped: Vec::new(),
        unrepresentable: Vec::new(),
        no_action: 0,
        regions: Vec::new(),
        disabled: 0,
        deduplicated: 0,
        ..sample_planned()
    }));
    assert_eq!(
        text(&bare),
        "cherenkov: python: 12 mutants; concepts mapped 13/16\n"
    );
}

#[test]
fn test_a_plan_is_summarized_from_what_the_planner_counted() {
    let mut d = Diagnostics {
        planned: 5,
        deduplicated: 1,
        ..Diagnostics::default()
    };
    d.unmapped.insert(Concept::Expr);
    d.unrepresentable.insert(Concept::Binary, 2);
    d.disabled.insert("ror".into(), 3);
    d.disabled.insert("aor".into(), 4);
    d.skipped
        .insert(("aor".into(), SkipReason::NoActionForContext), 6);
    d.skipped
        .insert(("aor".into(), SkipReason::Region(SkipKind::Test)), 2);
    d.skipped
        .insert(("ror".into(), SkipReason::Region(SkipKind::Test)), 1);
    let planned = Planned::of("rust", &d);
    assert_eq!(planned.mutants, 5);
    assert_eq!(planned.mapped, Concept::ALL.len() as u64 - 1);
    assert_eq!(planned.unmapped, ["expr"]);
    assert_eq!(planned.unrepresentable, ["binary 2"]);
    assert_eq!(planned.no_action, 6);
    assert_eq!(planned.regions, ["test 3"]);
    assert_eq!(planned.disabled, 7);
    assert_eq!(planned.deduplicated, 1);
}

#[test]
fn test_a_json_notice_is_the_diagnostic_s_envelope_with_its_code_args_and_message() {
    let out = said(
        Format::Json,
        &Notice::Viability {
            rejected: 3,
            checked: 10,
            compiles: 2,
            tenths: 12,
        },
    );
    let envelope: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(envelope["code"], "cherenkov-cli-notice-viability");
    assert_eq!(envelope["args"]["rejected"], 3);
    assert_eq!(envelope["args"]["whole"], 1);
    assert_eq!(envelope["args"]["tenth"], 2);
    assert!(
        envelope["message"]
            .as_str()
            .unwrap()
            .starts_with("viability check: 3 of 10 function-body"),
        "{envelope}"
    );
}

#[test]
fn test_a_json_notice_carrying_a_cause_nests_its_envelope() {
    let out = said(
        Format::Json,
        &Notice::FileProblem {
            path: "a.rs".into(),
            cause: Report::new("cherenkov-run-application-file-syntax-errors"),
        },
    );
    let envelope: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(envelope["args"]["path"], "a.rs");
    assert_eq!(
        envelope["args"]["cause"]["code"],
        "cherenkov-run-application-file-syntax-errors"
    );
}

#[test]
fn test_skipped_tests_are_said_under_text_and_under_json() {
    let notice = Notice::SkippedTests {
        tests: "bus::slow, bus::tail".into(),
    };
    assert_eq!(
        text(&notice),
        "cherenkov: tests left out of judging (--skip-test): bus::slow, bus::tail\n"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(said(Format::Json, &notice).trim()).unwrap();
    assert_eq!(envelope["code"], "cherenkov-cli-notice-skipped-tests");
    assert_eq!(envelope["args"]["tests"], "bus::slow, bus::tail");
}

#[test]
fn test_the_notices_the_stream_or_the_terminal_already_carries_are_left_out_of_json() {
    for notice in [
        Notice::Operators { label: "l".into() },
        Notice::CarriedOn { run: "r".into() },
        Notice::InDiff { skipped: 1 },
    ] {
        assert_eq!(said(Format::Json, &notice), "", "{notice}");
        assert!(!text(&notice).is_empty(), "{notice}");
    }
}

#[test]
fn test_the_tenths_of_a_duration_are_rounded() {
    assert_eq!(tenths(std::time::Duration::from_millis(2_449)), 24);
    assert_eq!(tenths(std::time::Duration::from_millis(2_450)), 25);
    assert_eq!(tenths(std::time::Duration::ZERO), 0);
}
