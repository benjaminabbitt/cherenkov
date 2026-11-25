use super::*;
use coded_diagnostic::Arg;

const EXEC: &str = "cherenkov-exec-dap";

fn leaf(code: &str) -> Report {
    Report::new(format!("{EXEC}-{code}"))
}

fn wrapping(code: &str, cause: Report) -> Report {
    Report::new(format!("{EXEC}-{code}")).arg("cause", Arg::nested(&cause))
}

/// Why a mutant could not be applied at tier B, with `tier_p` beside it.
fn at_tier_b(cause: Report, tier_p: Report) -> Report {
    Report::new(format!("{EXEC}-unappliable"))
        .arg("cause", Arg::nested(&cause))
        .arg("tier_p", Arg::nested(&tier_p))
}

fn no_rule() -> Report {
    leaf("unpatchable-unsupported-edit").arg("what", "x")
}

/// Every `reason_code` of an unsupported mutant the output documents, each
/// with a reason that has it.
fn unsupported() -> Vec<(&'static str, Report)> {
    vec![
        (
            "inlined",
            at_tier_b(wrapping("evaluator-refused", leaf("inlined")), no_rule()),
        ),
        (
            "generic-copies",
            wrapping("generic-copies", leaf("unpatchable-non-zero-value")),
        ),
        (
            "evaluation-hung",
            at_tier_b(
                wrapping("evaluator-refused", leaf("evaluation-hung")),
                leaf("unpatchable-non-zero-value"),
            ),
        ),
        (
            "value-not-buildable",
            at_tier_b(wrapping("evaluator-refused", leaf("not-scalar")), no_rule()),
        ),
        ("value-not-buildable", leaf("needs-mirror")),
        ("value-not-buildable", leaf("unpatchable-non-zero-value")),
        ("value-name-unresolved", leaf("name-not-imported")),
        (
            "function-not-found",
            at_tier_b(
                wrapping("evaluator-refused", leaf("function-not-found")),
                leaf("unpatchable-conditional-jumps"),
            ),
        ),
        (
            "symbol-not-in-scope",
            at_tier_b(
                wrapping("evaluator-refused", leaf("symbol-not-in-scope")),
                no_rule(),
            ),
        ),
        (
            "evaluator-cannot-apply",
            at_tier_b(
                wrapping("evaluator-refused", leaf("evaluator-said")),
                no_rule(),
            ),
        ),
        (
            "debugger-failed",
            Report::new("cherenkov-run-domain-debugger-failed")
                .arg("cause", Arg::nested(&leaf("session-no-restart"))),
        ),
        ("no-breakpoint-site", leaf("unpatchable-conditional-jumps")),
        (
            "no-breakpoint-site",
            leaf("unpatchable-arithmetic-instructions"),
        ),
        ("no-patch-rule", no_rule()),
        ("no-patch-rule", leaf("unpatchable-condition-mismatch")),
        ("other", leaf("something-new")),
        ("other", leaf("unpatchable-no-code-at")),
    ]
}

#[test]
fn test_each_documented_code_of_an_unsupported_mutant_has_a_reason() {
    for (code, why) in unsupported() {
        assert_eq!(reason_code(Verdict::Unsupported, &why), code, "{why}");
        assert_eq!(reason_code(Verdict::Error, &why), code, "{why}");
    }
}

#[test]
fn test_every_documented_code_of_an_unsupported_mutant_is_pinned() {
    let documented = [
        "inlined",
        "generic-copies",
        "value-not-buildable",
        "value-name-unresolved",
        "evaluation-hung",
        "function-not-found",
        "symbol-not-in-scope",
        "evaluator-cannot-apply",
        "debugger-failed",
        "no-breakpoint-site",
        "no-patch-rule",
        "other",
    ];
    let pinned: Vec<&str> = unsupported().iter().map(|(code, _)| *code).collect();
    for code in documented {
        assert!(pinned.contains(&code), "{code} is not pinned");
    }
}

#[test]
fn test_each_documented_code_of_an_unviable_mutant_has_a_reason() {
    assert_eq!(
        reason_code(Verdict::Unviable, &leaf("no-default")),
        "no-default"
    );
    for why in [
        leaf("does-not-compile"),
        leaf("rustc-rejects"),
        leaf("unresolved-in-source"),
    ] {
        assert_eq!(reason_code(Verdict::Unviable, &why), "does-not-compile");
    }
}

#[test]
fn test_a_verdict_that_is_no_deficiency_has_its_own_code() {
    let why = leaf("something-new");
    assert_eq!(reason_code(Verdict::NotCalled, &why), "not-called");
    assert_eq!(reason_code(Verdict::NotReached, &why), "not-reached");
    for verdict in [Verdict::Killed, Verdict::Survived, Verdict::Timeout] {
        assert_eq!(reason_code(verdict, &why), "none");
    }
}
