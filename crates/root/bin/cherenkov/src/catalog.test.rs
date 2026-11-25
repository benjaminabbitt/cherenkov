use cherenkov_cli::code::reason_code;
use cherenkov_cli::error::CliError;
use cherenkov_exec_dap::reason::{Reason, evaluator_said};
use cherenkov_exec_dap::tierp::Unpatchable;
use cherenkov_exec_dap::unbuildable::{Unbuildable, Unconstructed};
use cherenkov_run_domain::Verdict;
use coded_diagnostic::{Arg, Report};

use super::catalog;

#[test]
fn test_the_program_s_catalog_renders_the_cli_s_and_the_run_layer_s_messages() {
    let catalog = catalog().unwrap();
    let run = CliError::Run {
        cause: Report::new("cherenkov-run-application-workers").arg(
            "cause",
            Arg::nested(&Report::new("cherenkov-run-application-no-workers")),
        ),
    };
    assert_eq!(
        catalog.render(&run).unwrap(),
        "the run could not run: the mutants could not run: no workers"
    );
}

#[test]
fn test_a_debugger_session_failure_renders_its_nested_causes_through_the_whole_catalog() {
    let closed = cherenkov_exec_dap::dap::DapError::Closed;
    let step = cherenkov_exec_dap::error::Session::Step {
        step: "continue".into(),
        cause: Report::of(&closed),
    };
    let failure = cherenkov_run_ports::executor::ExecError::of(&step);
    let error = CliError::toolchain(&failure);
    assert_eq!(
        catalog().unwrap().render(&error).unwrap(),
        "continue: dap connection closed"
    );
}

#[test]
fn test_a_programs_failure_the_run_recorded_renders_through_the_catalog_as_it_was_read_back() {
    let recorded =
        cherenkov_run_domain::why::encode(&Report::of(&cherenkov_exec_dap::error::Session::Step {
            step: "continue".into(),
            cause: Report::of(&cherenkov_exec_dap::dap::DapError::Closed),
        }));
    let read = cherenkov_run_domain::why::decode(Some(&recorded), "");
    let error = cherenkov_run_application::error::RunLayerError::Workers {
        program: Some("calc-1".into()),
        cause: read,
    };
    assert_eq!(
        catalog().unwrap().render(&error).unwrap(),
        "the mutants could not run (calc-1): continue: dap connection closed"
    );
}

#[test]
fn test_a_parsers_failure_renders_through_the_profile_and_the_token_table_it_names() {
    let duplicate = cherenkov_mutation::token::TokenTableError::DuplicateSpelling {
        spelling: "&".into(),
    };
    let profile = cherenkov_syntax::ProfileError::Tokens {
        lang: "go".into(),
        cause: Report::of(&duplicate),
    };
    let error = CliError::syntax(&profile);
    assert_eq!(
        catalog().unwrap().render(&error).unwrap(),
        "the parsers did not load: go: tokens.toml: the spelling \"&\" is mapped to more than one token"
    );
}

#[test]
fn test_a_file_the_planner_could_not_analyze_renders_its_parsers_failure() {
    let problem = cherenkov_run_application::session::FileProblem::Analysis {
        cause: Report::of(&cherenkov_run_ports::syntax::SyntaxError::Parser {
            detail: "boom".into(),
        }),
    };
    assert_eq!(
        catalog().unwrap().render(&problem).unwrap(),
        "analysis failed: parser failure: boom"
    );
}

#[test]
fn test_an_instruction_table_error_renders_through_the_program_s_catalog() {
    let error = cherenkov_exec_dap::isa::IsaError::NoStub { abi: "vax".into() };
    assert_eq!(
        catalog().unwrap().render(&error).unwrap(),
        "isa table: no return stub for ABI \"vax\""
    );
}

/// Why a mutant was not run, as the executor gives it, its words as an
/// earlier release printed them (the catalog says them the same), and the
/// documented `reason_code` of its verdict.
type Refusal = (Verdict, Report, &'static str, &'static str);

/// A refusal at tier B for what the debugger `said`, with no tier P patch
/// for `tier_p`.
fn at_tier_b(said: Reason, tier_p: Unpatchable) -> Report {
    Report::of(&Reason::Unappliable {
        cause: Report::of(&Reason::EvaluatorRefused {
            cause: Report::of(&said),
        }),
        tier_p: Report::of(&tier_p),
    })
}

fn no_rule() -> Unpatchable {
    Unpatchable::UnsupportedEdit("x".into())
}

/// What the debugger's evaluator refused.
fn evaluator_refusals() -> Vec<Refusal> {
    let hung = |secs, tier_p| at_tier_b(Reason::EvaluationHung { secs }, tier_p);
    let unsupported = Verdict::Unsupported;
    vec![
        (
            unsupported,
            at_tier_b(
                evaluator_said("Can not force return from an inlined function."),
                no_rule(),
            ),
            "the evaluator cannot apply the mutant: Can not force return from an inlined function. (tier P: no tier P rule for x)",
            "inlined",
        ),
        (
            unsupported,
            at_tier_b(
                evaluator_said("Could not find function named 'x::y'"),
                Unpatchable::ConditionalJumps { line: 7, found: 0 },
            ),
            "the evaluator cannot apply the mutant: Could not find function named 'x::y' (tier P: expected one conditional jump on line 7, found 0)",
            "function-not-found",
        ),
        (
            unsupported,
            hung(10, no_rule()),
            "the evaluator cannot apply the mutant: the evaluation did not finish within 10 s with the program's other threads stopped (tier P: no tier P rule for x)",
            "evaluation-hung",
        ),
        (
            unsupported,
            hung(5, Unpatchable::NonZeroValue("true".into())),
            "the evaluator cannot apply the mutant: the evaluation did not finish within 5 s with the program's other threads stopped (tier P: a return stub cannot produce the value true)",
            "evaluation-hung",
        ),
        (
            unsupported,
            at_tier_b(
                evaluator_said("only scalar results are returned (core::time::Duration)"),
                no_rule(),
            ),
            "the evaluator cannot apply the mutant: only scalar results are returned (core::time::Duration) (tier P: no tier P rule for x)",
            "value-not-buildable",
        ),
        (
            unsupported,
            at_tier_b(
                evaluator_said("No symbol 'key' in current context."),
                no_rule(),
            ),
            "the evaluator cannot apply the mutant: No symbol 'key' in current context. (tier P: no tier P rule for x)",
            "symbol-not-in-scope",
        ),
        (
            unsupported,
            at_tier_b(
                evaluator_said("Method calls only supported on struct or enum types"),
                no_rule(),
            ),
            "the evaluator cannot apply the mutant: Method calls only supported on struct or enum types (tier P: no tier P rule for x)",
            "evaluator-cannot-apply",
        ),
    ]
}

/// What a value is not built from, or made by the support library.
fn value_refusals() -> Vec<Refusal> {
    let unsupported = Verdict::Unsupported;
    let generic = |cause: Unpatchable| {
        Report::of(&Reason::GenericCopies {
            cause: Report::of(&cause),
        })
    };
    vec![
        (
            unsupported,
            Report::of(&Unconstructed::NeedsMirror {
                value: "vec![Default::default()]".into(),
                ty: "Vec<T>".into(),
                detail: "the trait bound".into(),
            }),
            "vec![Default::default()] for Vec<T>: needs more of a mirrored type than its layout: the trait bound",
            "value-not-buildable",
        ),
        (
            unsupported,
            Report::of(&Unconstructed::NameNotImported {
                value: "MutexGuard::new()".into(),
                ty: "X".into(),
                detail: "cannot find type `MutexGuard`".into(),
            }),
            "MutexGuard::new() for X: a name the support library does not import: cannot find type `MutexGuard`",
            "value-name-unresolved",
        ),
        (
            unsupported,
            generic(Unpatchable::NonZeroValue("true".into())),
            "a generic function's copies cannot all be patched: a return stub cannot produce the value true",
            "generic-copies",
        ),
        (
            unsupported,
            Report::of(&Unbuildable::DefaultNotInBinary { name: "X".into() }),
            "X's Default, if it has one, is not in the test binary",
            "other",
        ),
        (
            Verdict::Unviable,
            Report::of(&Unbuildable::NoDefault {
                name: "Settle".into(),
            }),
            "Settle has no Default in the source",
            "no-default",
        ),
        (
            Verdict::Unviable,
            Report::of(&Unconstructed::DoesNotCompile {
                value: "Poll::Ready(Ok(()))".into(),
                ty: "Poll<X>".into(),
                detail: "mismatched types".into(),
            }),
            "Poll::Ready(Ok(())) does not compile as Poll<X>: mismatched types",
            "does-not-compile",
        ),
    ]
}

/// What no patch exists for, and the debugger that failed.
fn patch_refusals() -> Vec<Refusal> {
    let unsupported = Verdict::Unsupported;
    vec![
        (
            unsupported,
            Report::of(&Unpatchable::ConditionalJumps { line: 7, found: 2 }),
            "expected one conditional jump on line 7, found 2",
            "no-breakpoint-site",
        ),
        (
            unsupported,
            Report::of(&Unpatchable::UnsupportedEdit(
                "a comparison made into a value without setcc".into(),
            )),
            "no tier P rule for a comparison made into a value without setcc",
            "no-patch-rule",
        ),
        (
            unsupported,
            Report::new("cherenkov-run-domain-debugger-failed").arg(
                "cause",
                Arg::nested(
                    &Report::new("cherenkov-run-domain-recorded-reason")
                        .arg("reason", "evaluate: dap connection closed"),
                ),
            ),
            "the debugger failed on this mutant: evaluate: dap connection closed",
            "debugger-failed",
        ),
    ]
}

fn refusals() -> Vec<Refusal> {
    [evaluator_refusals(), value_refusals(), patch_refusals()].concat()
}

#[test]
fn test_every_refusal_is_said_as_it_was_and_has_its_documented_reason_code() {
    let catalog = catalog().unwrap();
    for (verdict, why, said, code) in refusals() {
        assert_eq!(catalog.render(&why).unwrap(), said);
        assert_eq!(reason_code(verdict, &why), code, "{said}");
    }
}

#[test]
fn test_a_line_over_a_thousand_is_said_in_digits() {
    let why = Reason::NoCodeForLine { line: 12_345 };
    assert_eq!(
        catalog().unwrap().render(&why).unwrap(),
        "no code for line 12345"
    );
}

/// How many `eprintln!` calls `source` holds, its tests included: the
/// program's tests are in files of their own, and run.rs's print nothing.
fn eprintlns(source: &str) -> usize {
    source.matches(concat!("eprint", "ln!")).count()
}

#[test]
fn test_the_program_says_nothing_on_stderr_but_through_the_catalog() {
    // The one that stays is the failure reporter's fallback, for a catalog
    // that did not load: it has no catalog to say it with.
    assert_eq!(eprintlns(include_str!("main.rs")), 1);
    assert_eq!(eprintlns(include_str!("run.rs")), 0);
    assert_eq!(eprintlns(include_str!("selection.rs")), 0);
    assert_eq!(eprintlns(include_str!("phases.rs")), 0);
}
