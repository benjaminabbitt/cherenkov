//! Every message encodes to the bytes `encodings.txt` holds: its full name
//! and its encoding as hex, one message per line. Representative values set
//! every field, and an `optional` field set to its type's zero value is
//! still encoded, so a stream written by one build reads back the same in
//! another.

use angzarr_embedded::Timestamp;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::v1::diagnostic_arg::Value;
use prost::{Message, Name};

use super::*;

/// `message`'s full name and its encoding as lowercase hex.
fn line<M: Message + Name>(message: &M) -> String {
    let hex: String = message
        .encode_to_vec()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{} {hex}", M::full_name())
}

fn uuid() -> Uuid {
    Uuid {
        value: (1..=16).collect::<Vec<u8>>().into(),
    }
}

fn at() -> Timestamp {
    Timestamp {
        seconds: 1_763_400_000,
        nanos: 123_456_789,
    }
}

fn counts() -> Vec<VerdictCount> {
    [
        (Verdict::Killed, 3),
        (Verdict::Survived, 1),
        (Verdict::Timeout, 0),
    ]
    .map(|(verdict, count)| VerdictCount {
        verdict: verdict.into(),
        count,
    })
    .to_vec()
}

fn planned() -> PlannedMutant {
    PlannedMutant {
        id: "f2ba84283a346cc0".to_string(),
        file: "src/lib.rs".to_string(),
        line: 4,
        column: 5,
        end_line: 4,
        end_column: 6,
        function: Some("total".to_string()),
        operator: "aor".to_string(),
        description: "`+` becomes `-`".to_string(),
        original: "+".to_string(),
        mutated: Some("-".to_string()),
    }
}

/// A planned mutant whose `optional` fields are set to the empty string.
fn planned_empty_options() -> PlannedMutant {
    PlannedMutant {
        function: Some(String::new()),
        mutated: Some(String::new()),
        ..planned()
    }
}

fn killed_by() -> KilledBy {
    KilledBy {
        test: "calc::test_total".to_string(),
        location: Some("tests/calc.rs:5:5".to_string()),
        message: "assertion failed".to_string(),
        got: Some("-1".to_string()),
        expected: Some("5".to_string()),
    }
}

fn outcome() -> Outcome {
    Outcome {
        verdict: Verdict::Killed.into(),
        tier: Some(Tier::B.into()),
        took_ms: 492,
        tests_run: vec!["calc::test_total".to_string(), "calc::test_sum".to_string()],
        killed_by: vec![
            killed_by(),
            KilledBy {
                location: None,
                got: None,
                expected: None,
                ..killed_by()
            },
        ],
        reason: Some("exit 101".to_string()),
        why: None,
    }
}

/// A mutant decided for a run.
fn decided() -> MutantDecided {
    MutantDecided {
        mutant: Some(planned()),
        verdict: Some(outcome()),
        run: Some(uuid()),
        programs: vec!["calc-0123".to_string()],
    }
}

fn duration(seconds: i64, nanos: i32) -> Option<prost_types::Duration> {
    Some(prost_types::Duration { seconds, nanos })
}

/// An unmutated run that passed.
fn exited() -> Observation {
    Observation {
        kind: Some(observation::Kind::Exited(Exited {
            code: 0,
            tier: Tier::P.into(),
            wall: duration(1, 200_000_000),
        })),
    }
}

/// One of each kind of observation but the passing run, with every field
/// set.
fn observations() -> Vec<Observation> {
    use observation::Kind;
    [
        Kind::Exited(Exited {
            code: 101,
            tier: Tier::B.into(),
            wall: duration(0, 0),
        }),
        Kind::Crashed(Crashed {
            kind: "panic".to_string(),
            message: "index out of range".to_string(),
            tier: Tier::A.into(),
            wall: duration(3, 123_456_789),
        }),
        Kind::TimedOut(TimedOut {
            tier: Tier::P.into(),
            wall: duration(2, 0),
        }),
        Kind::NotReached(NotReached {
            reason: "not linked".to_string(),
            why: None,
        }),
        Kind::Unsupported(Unsupported {
            reason: "inlined".to_string(),
            why: None,
        }),
        Kind::Unviable(Unviable {
            reason: "no default".to_string(),
            why: None,
        }),
    ]
    .into_iter()
    .map(|kind| Observation { kind: Some(kind) })
    .collect()
}

fn drives() -> Drives {
    let driven = |kind| Some(Driven { kind: Some(kind) });
    Drives {
        drives: vec![
            Drive {
                mutant: "f2ba84283a346cc0".to_string(),
                driven: driven(driven::Kind::Tests(TestList {
                    tests: vec!["calc::test_total".to_string()],
                })),
            },
            Drive {
                mutant: "0123456789abcdef".to_string(),
                driven: driven(driven::Kind::NotCalled(Unit {})),
            },
            Drive {
                mutant: "fedcba9876543210".to_string(),
                driven: driven(driven::Kind::Every(Unit {})),
            },
            Drive {
                mutant: String::new(),
                driven: None,
            },
        ],
    }
}

/// A text arg.
fn text(name: &str, value: &str) -> angzarr_embedded::v1::DiagnosticArg {
    angzarr_embedded::v1::DiagnosticArg::text(name, value)
}

/// An arg of any type.
fn typed(name: &str, value: Value) -> angzarr_embedded::v1::DiagnosticArg {
    angzarr_embedded::v1::DiagnosticArg {
        name: name.to_string(),
        value: Some(value),
    }
}

/// A coded reason with one arg.
fn why() -> angzarr_embedded::v1::Diagnostic {
    angzarr_embedded::v1::Diagnostic {
        code: "cherenkov-exec-dap-session".to_string(),
        typed_args: vec![text("detail", "connection closed")],
    }
}

/// A coded reason with several args of every type and a nested cause, in
/// the reason's own order (not name order): a list encodes in the order
/// it is given.
fn why_many() -> angzarr_embedded::v1::Diagnostic {
    angzarr_embedded::v1::Diagnostic {
        code: "cherenkov-exec-dap-session".to_string(),
        typed_args: vec![
            text("program", "calc-0123"),
            typed("count", Value::Uint(3)),
            typed("delta", Value::Int(-3)),
            typed(
                "cause",
                Value::Nested(angzarr_embedded::v1::Diagnostic {
                    code: "cherenkov-exec-dap-rpc".to_string(),
                    typed_args: vec![text("detail", "connection closed")],
                }),
            ),
        ],
    }
}

/// A coded reason whose cause has no args of its own: still a nested
/// cause on the wire, not its code as text.
fn why_bare() -> angzarr_embedded::v1::Diagnostic {
    angzarr_embedded::v1::Diagnostic {
        code: "cherenkov-exec-dap-session".to_string(),
        typed_args: vec![typed(
            "cause",
            Value::Nested(angzarr_embedded::v1::Diagnostic {
                code: "cherenkov-exec-dap-closed".to_string(),
                typed_args: vec![],
            }),
        )],
    }
}

fn preparation() -> PreparationReported {
    PreparationReported {
        program: "target/debug/deps/calc-0123".to_string(),
        prepared_ms: 340,
        tests: Some(7),
        selection: Selection::RecordedWithFailures.into(),
        failed_tests: vec!["calc::flaky".to_string()],
        selection_reason: Some("debug info".to_string()),
        index: 2,
        baseline: Some(exited()),
        baseline_failures: vec!["calc::test_total at tests/calc.rs:5:5: boom".to_string()],
        drives: Some(drives()),
        selection_why: None,
        baseline_output_tail: None,
    }
}

fn prepared() -> ProgramPrepared {
    let p = preparation();
    ProgramPrepared {
        program: p.program,
        baseline_ms: 1_200,
        prepared_ms: p.prepared_ms,
        tests: p.tests,
        selection: p.selection,
        failed_tests: p.failed_tests,
        selection_reason: p.selection_reason,
        index: p.index,
        budget_ms: 6_000,
        drives: p.drives,
        selection_why: None,
    }
}

fn pos(byte: u64, line: u32, col: u32) -> Pos {
    Pos { byte, line, col }
}

fn span(from: u64, to: u64) -> Span {
    Span {
        start: Some(pos(from, 4, from as u32)),
        end: Some(pos(to, 4, to as u32)),
    }
}

fn after() -> Target {
    Target {
        kind: Some(target::Kind::AfterStatement(span(30, 41))),
    }
}

fn block() -> Target {
    Target {
        kind: Some(target::Kind::BlockStart(span(50, 61))),
    }
}

fn action(kind: action::Kind) -> Action {
    Action { kind: Some(kind) }
}

fn edit(kind: edit::Kind) -> Edit {
    Edit { kind: Some(kind) }
}

/// A plan with every field set.
fn plan() -> MutantPlan {
    MutantPlan {
        id: "f2ba84283a346cc0".to_string(),
        operator: "aor".to_string(),
        mutation: Some("aor/add->sub".to_string()),
        file: "src/lib.rs".to_string(),
        function: Some("total".to_string()),
        stop_at: Some(pos(12, 4, 5)),
        action: Some(action(action::Kind::EvalThenJump(EvalThenJump {
            text: "x = a - b".to_string(),
            target: "x".to_string(),
            value: "a - b".to_string(),
            resume: Some(after()),
        }))),
        edit: Some(edit(edit::Kind::ReplaceToken(ReplaceToken {
            capture: Capture::Op.into(),
            original: Token::Add.into(),
            replacement: Token::Sub.into(),
        }))),
        site: Some(span(14, 15)),
        requires: vec![
            Requirement::CallInEval.into(),
            Requirement::ForceReturn.into(),
        ],
        operands: vec![span(12, 13), span(16, 17)],
        operand_texts: vec!["a".to_string(), "b".to_string()],
        original: "+".to_string(),
        mutated: Some("-".to_string()),
    }
}

fn executable() -> ExecutableMutant {
    ExecutableMutant {
        mutant: Some(planned()),
        plan: Some(plan()),
    }
}

/// An executable mutant with nothing optional set.
fn executable_bare() -> ExecutableMutant {
    ExecutableMutant {
        mutant: Some(PlannedMutant::default()),
        plan: Some(MutantPlan::default()),
    }
}

/// The lines of the plan, with each arm of every choice in it.
fn plan_lines() -> Vec<String> {
    let lit = |kind| Lit { kind: Some(kind) };
    let replace = |capture: Capture, kind| {
        edit(edit::Kind::ReplaceWithLiteral(ReplaceWithLiteral {
            capture: capture.into(),
            lit: Some(lit(kind)),
        }))
    };
    let bare = MutantPlan {
        id: "a".to_string(),
        ..MutantPlan::default()
    };
    let mut lines = vec![line(&plan()), line(&bare)];
    lines.extend(observations().iter().map(line));
    lines.extend(
        [
            action::Kind::JumpOnly(JumpOnly {
                resume: Some(after()),
            }),
            action::Kind::EvalBranch(EvalBranch {
                cond_text: "a < b".to_string(),
                if_true: Some(block()),
                if_false: Some(after()),
            }),
            action::Kind::EvalReturn(EvalReturn {
                value_text: "0".to_string(),
            }),
            action::Kind::InPlace(InPlace {
                expr_text: "a - b".to_string(),
            }),
            action::Kind::Redirect(Redirect {
                from: Some(span(1, 9)),
                to: Some(span(11, 19)),
            }),
            action::Kind::ReturnDefault(ReturnDefault {
                value_text: Some("0".to_string()),
                zero_bits: true,
                async_fn: true,
            }),
        ]
        .into_iter()
        .map(|kind| line(&action(kind))),
    );
    lines.extend(
        [
            edit(edit::Kind::DeleteToken(DeleteToken {
                capture: Capture::Op.into(),
                token: Token::Not.into(),
            })),
            edit(edit::Kind::WrapNot(WrapNot {
                capture: Capture::Cond.into(),
            })),
            replace(Capture::Body, lit::Kind::Flag(true)),
            replace(Capture::Body, lit::Kind::Default(Unit {})),
            replace(Capture::Value, lit::Kind::Text("0".to_string())),
            edit(edit::Kind::DeleteStatement(Unit {})),
            edit(edit::Kind::DeleteMatch(Unit {})),
        ]
        .iter()
        .map(line),
    );
    lines
}

/// One line for each sample of every message.
fn lines() -> Vec<String> {
    plan_lines()
        .into_iter()
        .chain(run_lines())
        .chain(coded_lines())
        .chain(decision_lines())
        .chain(snapshot_lines())
        .chain(documents().iter().map(line))
        .chain(verdict_lines().iter().map(line))
        .chain(nested_lines())
        .chain(output_tail_lines())
        .collect()
}

/// The messages that carry a failing program's output tail.
fn output_tail_lines() -> Vec<String> {
    let tail = Some("thread 'a' panicked\n".to_string());
    vec![
        line(&PreparationReported {
            baseline_output_tail: tail.clone(),
            ..preparation()
        }),
        line(&ProgramFailed {
            program: Some("calc-1".to_string()),
            reason: String::new(),
            why: None,
            output_tail: tail.clone(),
        }),
        line(&output::Failed {
            run: Some(uuid()),
            at: recorded(),
            program: None,
            reason: "build failed".to_string(),
            output_tail: tail,
        }),
    ]
}

/// The lines of the plan, the run's start and the preparation messages.
fn run_lines() -> Vec<String> {
    vec![
        line(&planned()),
        line(&planned_empty_options()),
        line(&PlanRun {
            root: "/work/calc".to_string(),
            operators: vec![OperatorSet {
                language: "rust".to_string(),
                set: "cargo-mutants".to_string(),
            }],
            jobs: 8,
            fast: true,
            mutants: vec![executable(), executable_bare()],
            programs: 2,
        }),
        line(&RunStarted {
            root: "/work/calc".to_string(),
            operators: vec![
                OperatorSet {
                    language: "go".to_string(),
                    set: "breadth".to_string(),
                },
                OperatorSet {
                    language: "rust".to_string(),
                    set: "cargo-mutants".to_string(),
                },
            ],
            jobs: 4,
            fast: true,
            mutants: 2,
            started_at: Some(at()),
            programs: 2,
        }),
        line(&MutantsPlanned {
            mutants: vec![executable(), executable_bare()],
        }),
        line(&preparation()),
        line(&PreparationReported {
            tests: Some(0),
            selection_reason: Some(String::new()),
            baseline: None,
            baseline_failures: Vec::new(),
            drives: None,
            ..preparation()
        }),
        line(&prepared()),
        line(&ProgramPrepared {
            tests: Some(0),
            selection_reason: None,
            drives: None,
            ..prepared()
        }),
        line(&RunReady {
            programs: 2,
            mutants: vec![executable(), executable_bare()],
        }),
        line(&PreparationFailed {
            program: Some("target/debug/deps/calc-0123".to_string()),
            reason: "no debug info".to_string(),
            why: None,
        }),
        line(&PreparationFailed {
            program: Some(String::new()),
            reason: "build failed".to_string(),
            why: None,
        }),
        line(&ProgramFailed {
            program: Some("target/debug/deps/calc-0123".to_string()),
            reason: "no debug info".to_string(),
            why: None,
            output_tail: None,
        }),
        line(&ProgramFailed {
            program: None,
            reason: "build failed".to_string(),
            why: None,
            output_tail: None,
        }),
        line(&Abandoned {
            reason: "the plan changed".to_string(),
            why: None,
        }),
        line(&RunAbandoned {
            reason: "the plan changed".to_string(),
            why: None,
        }),
    ]
}

/// The lines of the preparation messages that carry a coded reason.
fn coded_lines() -> Vec<String> {
    vec![
        line(&PreparationReported {
            selection_reason: None,
            selection_why: Some(why()),
            ..preparation()
        }),
        line(&ProgramPrepared {
            selection_reason: None,
            selection_why: Some(why()),
            ..prepared()
        }),
        line(&PreparationFailed {
            program: Some("target/debug/deps/calc-0123".to_string()),
            reason: String::new(),
            why: Some(why()),
        }),
        line(&ProgramFailed {
            program: None,
            reason: String::new(),
            why: Some(why()),
            output_tail: None,
        }),
        line(&Abandoned {
            reason: String::new(),
            why: Some(why()),
        }),
        line(&RunAbandoned {
            reason: String::new(),
            why: Some(why()),
        }),
        line(&PreparationFailed {
            program: Some("target/debug/deps/calc-0123".to_string()),
            reason: String::new(),
            why: Some(why_many()),
        }),
        line(&ProgramFailed {
            program: None,
            reason: String::new(),
            why: Some(why_many()),
            output_tail: None,
        }),
    ]
}

/// The lines that carry a reason whose cause has no args of its own, and
/// those of an outcome's reasons, coded.
fn nested_lines() -> Vec<String> {
    use observation::Kind;
    let coded = |kind| line(&Observation { kind: Some(kind) });
    vec![
        line(&ProgramFailed {
            program: None,
            reason: String::new(),
            why: Some(why_bare()),
            output_tail: None,
        }),
        coded(Kind::NotReached(NotReached {
            reason: String::new(),
            why: Some(why()),
        })),
        coded(Kind::Unsupported(Unsupported {
            reason: String::new(),
            why: Some(why_many()),
        })),
        coded(Kind::Unviable(Unviable {
            reason: String::new(),
            why: Some(why_bare()),
        })),
        line(&Outcome {
            reason: None,
            why: Some(why_many()),
            ..outcome()
        }),
    ]
}

/// A test program's run of a mutant that observed a failing test.
fn program_run() -> ProgramRun {
    ProgramRun {
        program: 0,
        program_name: "calc-0123".to_string(),
        took_ms: 31,
        tests: vec!["calc::test_total".to_string()],
        result: Some(program_run::Result::Ran(Ran {
            observation: observations().into_iter().next(),
            killed_by: vec![killed_by()],
        })),
    }
}

/// The lines of the outcomes, decisions and the commands that plan a mutant.
fn decision_lines() -> Vec<String> {
    vec![
        line(&killed_by()),
        line(&outcome()),
        line(&RunFinished {
            counts: counts(),
            took_ms: 9_876,
            status: RunStatus::Failed.into(),
        }),
        line(&RunFinished {
            counts: Vec::new(),
            took_ms: 5,
            status: RunStatus::Abandoned.into(),
        }),
        line(&DecisionReported {
            mutant: "f2ba84283a346cc0".to_string(),
            verdict: Verdict::Survived.into(),
            decision: None,
        }),
        line(&DecisionRecorded {
            mutant: "f2ba84283a346cc0".to_string(),
            verdict: Verdict::Killed.into(),
            decision: None,
        }),
        line(&DecisionReported {
            mutant: "f2ba84283a346cc0".to_string(),
            verdict: Verdict::Killed.into(),
            decision: Some(decided()),
        }),
        line(&DecisionRecorded {
            mutant: "f2ba84283a346cc0".to_string(),
            verdict: Verdict::Killed.into(),
            decision: Some(decided()),
        }),
        line(&PlanMutant {
            run: Some(uuid()),
            mutant: Some(executable()),
            programs: 2,
        }),
        line(&PlanMutant {
            run: Some(Uuid::default()),
            mutant: Some(executable_bare()),
            programs: 0,
        }),
        line(&MutantPlanned {
            run: Some(uuid()),
            mutant: Some(executable()),
            programs: 2,
        }),
        line(&ProgramRunReported {
            run: Some(uuid()),
            ran: Some(program_run()),
        }),
        line(&ProgramRunReported {
            run: Some(Uuid::default()),
            ran: Some(ProgramRun::default()),
        }),
        line(&ProgramRan {
            run: Some(uuid()),
            ran: Some(program_run()),
        }),
        line(&ProgramRun {
            program: 1,
            program_name: String::new(),
            took_ms: 0,
            tests: Vec::new(),
            result: Some(program_run::Result::NotCalled(Unit {})),
        }),
        line(&ProgramRun {
            program: 2,
            program_name: "it-4567".to_string(),
            took_ms: 12,
            tests: vec!["it::test_b".to_string()],
            result: Some(program_run::Result::Failed(DebuggerFailed {
                reason: "evaluate: dap connection closed".to_string(),
                why: None,
            })),
        }),
        line(&decided()),
        line(&ProgramRun {
            program: 2,
            program_name: "it-4567".to_string(),
            took_ms: 12,
            tests: vec!["it::test_b".to_string()],
            result: Some(program_run::Result::Failed(DebuggerFailed {
                reason: String::new(),
                why: Some(why()),
            })),
        }),
    ]
}

/// The lines of the run and mutant snapshots.
fn snapshot_lines() -> Vec<String> {
    vec![
        line(&RunSnapshot {
            started_at: Some(at()),
            planned: 5,
            counts: counts(),
            finished: true,
            programs: 2,
            prepared: vec![0, 1],
            plans: vec![executable()],
            ready: true,
        }),
        line(&RunSnapshot {
            started_at: Some(Timestamp::default()),
            planned: 0,
            counts: Vec::new(),
            finished: false,
            programs: 0,
            prepared: Vec::new(),
            plans: Vec::new(),
            ready: false,
        }),
        line(&MutantRun {
            run: "0102".to_string(),
            mutant: Some(planned()),
            decided: true,
            programs: 2,
            ran: vec![program_run()],
        }),
        line(&MutantSnapshot {
            runs: vec![
                MutantRun {
                    run: "0102".to_string(),
                    mutant: Some(planned()),
                    decided: true,
                    programs: 2,
                    ran: vec![program_run()],
                },
                MutantRun {
                    run: String::new(),
                    mutant: Some(PlannedMutant::default()),
                    decided: false,
                    programs: 0,
                    ran: Vec::new(),
                },
            ],
        }),
    ]
}

/// The `at` every document is recorded at.
fn recorded() -> Option<Timestamp> {
    Some(Timestamp {
        seconds: 1_763_478_000,
        nanos: 500_000_000,
    })
}

/// The operator sets of the printed run: two languages, one set each.
fn output_operators() -> Vec<output::OperatorSet> {
    vec![
        output::OperatorSet {
            language: "go".to_string(),
            set: "breadth".to_string(),
        },
        output::OperatorSet {
            language: "rust".to_string(),
            set: "cargo-mutants".to_string(),
        },
    ]
}

/// One document of every kind, the run's documents of a killed and a
/// surviving mutant among them.
/// A `run.started` that left two tests out of judging.
fn started_skipping(run: Option<Uuid>, at: Option<Timestamp>) -> output::document::Event {
    output::document::Event::Started(output::Started {
        run,
        at,
        root: "/work/calc".to_string(),
        operators: output_operators(),
        jobs: 4,
        fast: false,
        mutants: 2,
        skipped_tests: vec!["bus::slow".to_string(), "bus::tail".to_string()],
    })
}

fn documents() -> Vec<output::Document> {
    use output::document::Event;
    let (run, at) = (Some(uuid()), recorded());
    let killers = |complete| output::Killers {
        killed_by: vec![killed_by()],
        complete,
    };
    let decided = |verdict: Verdict| output::Decided {
        run: run.clone(),
        at,
        mutant: Some(planned()),
        verdict: verdict.into(),
        tier: Some(Tier::P.into()),
        took_ms: 31,
        tests_run: vec!["calc::test_total".to_string()],
        killers: None,
        executors: None,
        reason: None,
        reason_code: None,
    };
    [
        Event::Started(output::Started {
            run: run.clone(),
            at,
            root: "/work/calc".to_string(),
            operators: output_operators(),
            jobs: 4,
            fast: false,
            mutants: 2,
            skipped_tests: Vec::new(),
        }),
        Event::Planned(output::Planned {
            run: run.clone(),
            at,
            mutant: Some(planned()),
        }),
        Event::Prepared(output::Prepared {
            run: run.clone(),
            at,
            program: "target/debug/deps/calc-0123".to_string(),
            baseline_ms: 40,
            prepared_ms: 90,
            tests: Some(3),
            selection: Selection::RecordedWithFailures.into(),
            failures: Some(output::FailedTests {
                failed_tests: vec!["calc::flaky".to_string()],
            }),
            reason: None,
        }),
        Event::Prepared(output::Prepared {
            run: run.clone(),
            at,
            program: "target/debug/deps/it-4567".to_string(),
            baseline_ms: 0,
            prepared_ms: 0,
            tests: None,
            selection: Selection::EveryTest.into(),
            failures: None,
            reason: Some("no debug info".to_string()),
        }),
        Event::Failed(output::Failed {
            run: run.clone(),
            at,
            program: None,
            reason: "build failed".to_string(),
            output_tail: None,
        }),
        Event::Decided(output::Decided {
            killers: Some(killers(true)),
            ..decided(Verdict::Killed)
        }),
        Event::Decided(output::Decided {
            executors: Some(output::Executors {
                executed_by: vec!["calc::test_total".to_string()],
            }),
            ..decided(Verdict::Survived)
        }),
        Event::Decided(output::Decided {
            tier: None,
            reason: Some("inlined function".to_string()),
            reason_code: Some("inlined".to_string()),
            ..decided(Verdict::Unsupported)
        }),
        Event::Finished(output::Finished {
            run: run.clone(),
            at,
            counts: [("killed".to_string(), 3), ("not-called".to_string(), 1)].into(),
            took_ms: 9_876,
            exit: 10,
            status: RunStatus::Failed.into(),
            deficiencies: [("inlined".to_string(), 1)].into(),
        }),
        started_skipping(run.clone(), at),
    ]
    .into_iter()
    .map(|event| output::Document { event: Some(event) })
    .collect()
}

#[test]
fn test_a_reason_with_several_args_encodes_to_the_same_bytes_every_time() {
    let first = coded_lines();
    for _ in 0..16 {
        assert_eq!(coded_lines(), first);
    }
    let once = why_many().encode_to_vec();
    assert_eq!(once, why_many().encode_to_vec());
}

#[test]
fn test_every_message_encodes_to_the_bytes_it_always_has() {
    let expected: Vec<&str> = include_str!("encodings.txt").lines().collect();
    assert_eq!(lines(), expected);
}

/// The documents as JSON text, one per line: the schema of
/// `docs/output-events.md`. A date and a run's id are as the canonical
/// mapping writes them, a duration is a number, an unset field its
/// document's choice of `null` or left out, and the enums their kebab-case
/// names.
const DOCUMENTS: &str = r#"{"event":"run.started","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","root":"/work/calc","jobs":4,"fast":false,"mutants":2,"operators":[{"language":"go","set":"breadth"},{"language":"rust","set":"cargo-mutants"}],"skipped_tests":[]}
{"event":"mutant.planned","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","id":"f2ba84283a346cc0","file":"src/lib.rs","line":4,"column":5,"end_line":4,"end_column":6,"function":"total","operator":"aor","description":"`+` becomes `-`","original":"+","mutated":"-"}
{"event":"program.prepared","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","program":"target/debug/deps/calc-0123","baseline_ms":40,"prepared_ms":90,"tests":3,"selection":"recorded-with-failures","failed_tests":["calc::flaky"]}
{"event":"program.prepared","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","program":"target/debug/deps/it-4567","baseline_ms":0,"prepared_ms":0,"tests":null,"selection":"every-test","reason":"no debug info"}
{"event":"program.failed","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","program":null,"reason":"build failed","output_tail":null}
{"event":"mutant.decided","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","id":"f2ba84283a346cc0","file":"src/lib.rs","line":4,"column":5,"end_line":4,"end_column":6,"function":"total","operator":"aor","description":"`+` becomes `-`","original":"+","mutated":"-","verdict":"killed","tier":"p","took_ms":31,"tests_run":["calc::test_total"],"killed_by":[{"test":"calc::test_total","location":"tests/calc.rs:5:5","message":"assertion failed","got":"-1","expected":"5"}],"complete":true}
{"event":"mutant.decided","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","id":"f2ba84283a346cc0","file":"src/lib.rs","line":4,"column":5,"end_line":4,"end_column":6,"function":"total","operator":"aor","description":"`+` becomes `-`","original":"+","mutated":"-","verdict":"survived","tier":"p","took_ms":31,"tests_run":["calc::test_total"],"executed_by":["calc::test_total"]}
{"event":"mutant.decided","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","id":"f2ba84283a346cc0","file":"src/lib.rs","line":4,"column":5,"end_line":4,"end_column":6,"function":"total","operator":"aor","description":"`+` becomes `-`","original":"+","mutated":"-","verdict":"unsupported","took_ms":31,"tests_run":["calc::test_total"],"reason":"inlined function","reason_code":"inlined"}
{"event":"run.finished","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","counts":{"killed":3,"not-called":1},"took_ms":9876,"exit":10,"status":"failed","deficiencies":{"inlined":1}}
{"event":"run.started","run":"01020304-0506-0708-090a-0b0c0d0e0f10","at":"2025-11-18T15:00:00.500Z","root":"/work/calc","jobs":4,"fast":false,"mutants":2,"operators":[{"language":"go","set":"breadth"},{"language":"rust","set":"cargo-mutants"}],"skipped_tests":["bus::slow","bus::tail"]}
"#;

#[test]
fn test_every_document_is_the_json_the_output_schema_says() {
    let written: Vec<String> = documents()
        .iter()
        .map(|doc| serde_json::to_string(doc).unwrap())
        .collect();
    let expected: Vec<&str> = DOCUMENTS.lines().collect();
    assert_eq!(written, expected);
}

#[test]
fn test_every_document_reads_back_as_it_was_written() {
    for doc in documents() {
        let text = serde_json::to_string(&doc).unwrap();
        let read: output::Document = serde_json::from_str(&text).unwrap();
        assert_eq!(read, doc, "{text}");
    }
}

#[test]
fn test_the_vocabulary_is_named_in_kebab_case() {
    let verdicts: Vec<String> = [
        Verdict::Killed,
        Verdict::Survived,
        Verdict::Timeout,
        Verdict::Unviable,
        Verdict::Unsupported,
        Verdict::NotCalled,
        Verdict::NotReached,
        Verdict::Error,
        Verdict::Unevaluable,
        Verdict::EvaluatorRaised,
    ]
    .map(|v| names::verdict(v.into()))
    .to_vec();
    assert_eq!(
        verdicts,
        [
            "killed",
            "survived",
            "timeout",
            "unviable",
            "unsupported",
            "not-called",
            "not-reached",
            "error",
            "unevaluable",
            "evaluator-raised"
        ]
    );
    assert_eq!(
        [Tier::A, Tier::B, Tier::P].map(|t| names::tier(t.into())),
        ["a", "b", "p"]
    );
    assert_eq!(
        [
            Selection::Recorded,
            Selection::RecordedWithFailures,
            Selection::EveryTest
        ]
        .map(|s| names::selection(s.into())),
        ["recorded", "recorded-with-failures", "every-test"]
    );
    assert_eq!(
        [RunStatus::Complete, RunStatus::Failed].map(|s| names::run_status(s.into())),
        ["complete", "failed"]
    );
}

#[test]
fn test_a_number_with_no_value_is_named_by_its_number() {
    assert_eq!(names::verdict(99), "99");
}

#[test]
fn test_no_value_is_named_unspecified_by_the_run() {
    for verdict in 1..=10 {
        assert_ne!(names::verdict(verdict), "unspecified");
    }
}

/// The verdict file's lines: a killed mutant with every field, and a
/// survivor with the optional ones unset.
fn verdict_lines() -> Vec<output::VerdictLine> {
    vec![
        output::VerdictLine {
            id: "f2ba84283a346cc0".to_string(),
            verdict: Verdict::Killed.into(),
            file: "src/lib.rs".to_string(),
            line: 4,
            column: 5,
            function: Some("total".to_string()),
            operator: "aor".to_string(),
            original: "+".to_string(),
            mutated: Some("-".to_string()),
            killed_by: vec!["calc::test_a".to_string(), "calc::test_b".to_string()],
        },
        output::VerdictLine {
            id: "0123456789abcdef".to_string(),
            verdict: Verdict::Survived.into(),
            file: "src/lib.rs".to_string(),
            line: 9,
            column: 1,
            function: None,
            operator: "sdl".to_string(),
            original: "x += 1;".to_string(),
            mutated: None,
            killed_by: Vec::new(),
        },
    ]
}

/// The verdict file's lines as JSON text: the field order of the message,
/// an unset `function` or `mutated` as `null`, and the verdict by name.
const VERDICT_LINES: &str = r#"{"id":"f2ba84283a346cc0","verdict":"killed","file":"src/lib.rs","line":4,"column":5,"function":"total","operator":"aor","original":"+","mutated":"-","killed_by":["calc::test_a","calc::test_b"]}
{"id":"0123456789abcdef","verdict":"survived","file":"src/lib.rs","line":9,"column":1,"function":null,"operator":"sdl","original":"x += 1;","mutated":null,"killed_by":[]}
"#;

#[test]
fn test_a_verdict_line_is_the_json_the_verdict_file_holds() {
    let written: Vec<String> = verdict_lines()
        .iter()
        .map(|line| serde_json::to_string(line).unwrap())
        .collect();
    let expected: Vec<&str> = VERDICT_LINES.lines().collect();
    assert_eq!(written, expected);
    for (text, line) in expected.iter().zip(verdict_lines()) {
        let read: output::VerdictLine = serde_json::from_str(text).unwrap();
        assert_eq!(read, line);
    }
}

/// A cause with no args of its own must come back nested, not as its code
/// held as text: the two differ on the wire.
#[test]
fn test_an_argless_nested_cause_decodes_as_nested_not_text() {
    let bytes = why_bare().encode_to_vec();
    let back = angzarr_embedded::v1::Diagnostic::decode(&*bytes).expect("decodes");
    assert_eq!(back, why_bare());
    assert!(matches!(
        &back.typed_args[0].value,
        Some(Value::Nested(cause)) if cause.typed_args.is_empty()
    ));
}

/// The reason's args are typed on the wire: a count is a number, not its
/// decimal text.
#[test]
fn test_a_reasons_count_is_a_number_on_the_wire() {
    let back = angzarr_embedded::v1::Diagnostic::decode(&*why_many().encode_to_vec()).unwrap();
    assert_eq!(back.typed_args[1].value, Some(Value::Uint(3)));
    assert_eq!(back.typed_args[2].value, Some(Value::Int(-3)));
}
