use coded_diagnostic::{Arg, Diagnostic, Report};

use super::*;
use crate::angzarr::DiagnosticArgValue;

fn session() -> Report {
    let cause = Report::new("cherenkov-exec-dap-rpc").arg("detail", "connection closed");
    Report::new("cherenkov-exec-dap-session")
        .arg("program", "calc-0123")
        .arg("cause", Arg::nested(&cause))
}

/// The value of the arg `name` of `wire`.
fn value<'a>(wire: &'a angzarr::Diagnostic, name: &str) -> &'a DiagnosticArgValue {
    &wire
        .typed_args
        .iter()
        .find(|arg| arg.name == name)
        .unwrap_or_else(|| panic!("no {name} in {wire:?}"))
        .value
}

#[test]
fn test_encoding_a_report_keeps_its_code_and_types_its_plain_args() {
    let wire = encode(
        &Report::new("c-x")
            .arg("detail", "d")
            .arg("count", 3_u32)
            .arg("delta", -3_i32),
    );
    assert_eq!(wire.code, "c-x");
    assert_eq!(
        value(&wire, "detail"),
        &DiagnosticArgValue::Text("d".into())
    );
    assert_eq!(value(&wire, "count"), &DiagnosticArgValue::Uint(3));
    assert_eq!(value(&wire, "delta"), &DiagnosticArgValue::Int(-3));
}

#[test]
fn test_encoding_keeps_the_args_in_the_reports_order() {
    let wire = encode(&Report::new("c").arg("zeta", "1").arg("alpha", "2"));
    let names: Vec<&str> = wire.typed_args.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["zeta", "alpha"]);
}

#[test]
fn test_encoding_holds_a_nested_cause_as_a_nested_arg() {
    let wire = encode(&session());
    let DiagnosticArgValue::Nested(cause) = value(&wire, "cause") else {
        panic!("a nested cause: {wire:?}");
    };
    assert_eq!(cause.code, "cherenkov-exec-dap-rpc");
    assert_eq!(
        value(cause, "detail"),
        &DiagnosticArgValue::Text("connection closed".into())
    );
    assert_eq!(
        value(&wire, "program"),
        &DiagnosticArgValue::Text("calc-0123".into())
    );
}

#[test]
fn test_decoding_reads_a_nested_cause_back_as_nested() {
    let decoded = decode(Some(&encode(&session())), "");
    assert_eq!(decoded.code(), "cherenkov-exec-dap-session");
    let args = decoded.args();
    let cause = args
        .into_iter()
        .find_map(|(name, arg)| match (name, arg) {
            ("cause", Arg::Nested(cause)) => Some(Report::of(cause.as_ref())),
            _ => None,
        })
        .expect("a nested cause");
    assert_eq!(cause.code(), "cherenkov-exec-dap-rpc");
    assert_eq!(encode(&decoded), encode(&session()));
}

#[test]
fn test_a_cause_with_no_args_is_still_read_back_as_a_cause() {
    let outer = Report::new("o").arg("cause", Arg::nested(&Report::new("c-closed")));
    let wire = encode(&outer);
    assert!(
        matches!(value(&wire, "cause"), DiagnosticArgValue::Nested(cause) if cause.typed_args.is_empty()),
        "{wire:?}"
    );
    let decoded = decode(Some(&wire), "");
    assert!(
        matches!(&decoded.args()[..], [("cause", Arg::Nested(cause))] if cause.code() == "c-closed"),
        "{decoded:?}"
    );
    assert_eq!(encode(&decoded), wire);
}

#[test]
fn test_a_text_arg_that_holds_a_code_stays_text() {
    let report = Report::new("o").arg("detail", "c-closed");
    let decoded = decode(Some(&encode(&report)), "");
    assert!(matches!(&decoded.args()[..], [("detail", Arg::Str(s))] if s == "c-closed"));
}

#[test]
fn test_decoding_two_levels_of_cause_keeps_both() {
    let inner = Report::new("i").arg("detail", "x");
    let middle = Report::new("m").arg("cause", Arg::nested(&inner));
    let outer = Report::new("o").arg("cause", Arg::nested(&middle));
    assert_eq!(encode(&decode(Some(&encode(&outer)), "")), encode(&outer));
}

#[test]
fn test_decoding_without_a_why_holds_the_old_text_as_a_recorded_reason() {
    let decoded = decode(None, "no debug info");
    assert_eq!(
        decoded,
        Report::of(&Failure::Recorded {
            reason: "no debug info".to_string(),
        })
    );
}

#[test]
fn test_decoding_prefers_the_why_to_the_text() {
    let decoded = decode(Some(&encode(&Report::new("c-x"))), "old text");
    assert_eq!(decoded.code(), "c-x");
}
