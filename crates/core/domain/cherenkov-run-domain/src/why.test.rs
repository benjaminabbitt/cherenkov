use coded_diagnostic::{Arg, Diagnostic, Report};

use super::*;

fn session() -> Report {
    let cause = Report::new("cherenkov-exec-dap-rpc").arg("detail", "connection closed");
    Report::new("cherenkov-exec-dap-session")
        .arg("program", "calc-0123")
        .arg("cause", Arg::nested(&cause))
}

#[test]
fn test_encoding_a_report_keeps_its_code_and_plain_args() {
    let wire = encode(&Report::new("c-x").arg("detail", "d").arg("count", 3_u32));
    assert_eq!(wire.code, "c-x");
    assert_eq!(wire.args["detail"], "d");
    assert_eq!(wire.args["count"], "3");
}

#[test]
fn test_encoding_flattens_a_nested_cause_under_its_arg_name() {
    let wire = encode(&session());
    assert_eq!(wire.args["cause"], "cherenkov-exec-dap-rpc");
    assert_eq!(wire.args["cause.detail"], "connection closed");
    assert_eq!(wire.args["program"], "calc-0123");
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
