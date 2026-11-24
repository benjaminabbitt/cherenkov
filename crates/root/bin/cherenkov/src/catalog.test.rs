use cherenkov_cli::error::CliError;
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
