use super::*;

#[test]
fn test_every_error_has_a_message_using_exactly_its_args() {
    let problems = angzarr_diagnostic::catalog::check(EN_US.0, EN_US.1, &diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn test_a_request_the_state_forbids_is_declined_and_a_breakage_failed() {
    // Refused (48) tells a caller to change the request; failed (32), to look at what broke.
    let missing = CliError::ToolMissing { tool: "gdb".into() };
    assert_eq!(missing.exit(), Exit::REFUSED);
    let broke = CliError::Toolchain { reason: "r".into() };
    assert_eq!(broke.exit(), Exit::FAILED);
}

#[test]
fn test_the_program_s_catalog_renders_every_error_and_the_run_layer_s_within_it() {
    let catalog = crate::catalog().unwrap();
    for sample in diagnostic_samples() {
        let message = catalog.render(&*sample);
        assert!(message.is_ok(), "{sample}: {message:?}");
    }
    let run = CliError::Run {
        cause: Report::new("cherenkov-run-application-workers").arg("reason", "no gdb"),
    };
    assert_eq!(
        catalog.render(&run).unwrap(),
        "the run could not run: the mutants could not run: no gdb"
    );
}

/// A cause with a number, a count, text and a cause of its own.
#[derive(Debug)]
struct Cause;

#[derive(Debug)]
struct Inner;

impl std::fmt::Display for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::fmt::Display for Cause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl angzarr_diagnostic::Diagnostic for Inner {
    fn code(&self) -> &str {
        "inner-code"
    }

    fn args(&self) -> Vec<(&str, angzarr_diagnostic::Arg)> {
        vec![("why", "because".into())]
    }
}

impl angzarr_diagnostic::Diagnostic for Cause {
    fn code(&self) -> &str {
        "outer-code"
    }

    fn args(&self) -> Vec<(&str, angzarr_diagnostic::Arg)> {
        vec![
            ("signed", angzarr_diagnostic::Arg::Int(-3)),
            ("count", angzarr_diagnostic::Arg::Uint(7)),
            ("text", "a b".into()),
            ("cause", angzarr_diagnostic::Arg::Nested(Box::new(Inner))),
        ]
    }
}

#[test]
fn test_a_run_layer_cause_is_held_with_its_code_its_args_and_its_nested_cause() {
    let carried = match CliError::run(&Cause) {
        CliError::Run { cause } => cause,
        other => panic!("{other:?}"),
    };
    let expected = Report::new("outer-code")
        .arg("signed", -3i64)
        .arg("count", 7u64)
        .arg("text", "a b")
        .arg(
            "cause",
            Arg::nested(&Report::new("inner-code").arg("why", "because")),
        );
    assert_eq!(carried, expected);
    assert_eq!(
        carried.to_string(),
        r#"outer-code signed=-3 count=7 text="a b" cause=inner-code cause.why=because"#
    );
}
