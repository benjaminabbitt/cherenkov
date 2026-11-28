//! The short machine-readable `reason_code` of a mutant that was not run, as
//! the output documents it (`output-events.md`): a table from the codes of
//! the diagnostics an executor gave to the documented code. A reason is a
//! tree (a refusal wraps its causes), and a mutant has the first documented
//! code, in the table's order, that any diagnostic in the tree has.

use cherenkov_run_domain::Verdict;
use coded_diagnostic::{Arg, Diagnostic, Report};

/// What a mutant Cherenkov cannot run has when no diagnostic in its reason
/// is in [`UNSUPPORTED`].
const OTHER: &str = "other";

/// An unviable mutant has this unless its reason is in [`UNVIABLE`].
const DOES_NOT_COMPILE: &str = "does-not-compile";

/// For an unsupported or errored mutant: the documented code and the codes
/// of the diagnostics that have it, first match first. An earlier row is
/// the more specific cause: a refusal that rides along (the reason tier P
/// gave) never outranks why the evaluator refused.
const UNSUPPORTED: &[(&str, &[&str])] = &[
    ("inlined", &["cherenkov-exec-dap-inlined"]),
    ("generic-copies", &["cherenkov-exec-dap-generic-copies"]),
    ("evaluation-hung", &["cherenkov-exec-dap-evaluation-hung"]),
    (
        "value-name-unresolved",
        &["cherenkov-exec-dap-name-not-imported"],
    ),
    (
        "value-not-buildable",
        &[
            "cherenkov-exec-dap-needs-mirror",
            "cherenkov-exec-dap-not-scalar",
            "cherenkov-exec-dap-unpatchable-non-zero-value",
        ],
    ),
    (
        "function-not-found",
        &["cherenkov-exec-dap-function-not-found"],
    ),
    (
        "symbol-not-in-scope",
        &["cherenkov-exec-dap-symbol-not-in-scope"],
    ),
    (
        "evaluator-cannot-apply",
        &["cherenkov-exec-dap-evaluator-refused"],
    ),
    ("debugger-failed", &["cherenkov-run-domain-debugger-failed"]),
    (
        "no-breakpoint-site",
        &[
            "cherenkov-exec-dap-unpatchable-conditional-jumps",
            "cherenkov-exec-dap-unpatchable-arithmetic-instructions",
            "cherenkov-exec-dap-unpatchable-no-code-at",
        ],
    ),
    (
        "no-patch-rule",
        &[
            "cherenkov-exec-dap-unpatchable-unsupported-edit",
            "cherenkov-exec-dap-unpatchable-condition-mismatch",
        ],
    ),
];

/// For an unviable mutant: the documented code and the codes that have it.
const UNVIABLE: &[(&str, &[&str])] = &[("no-default", &["cherenkov-exec-dap-no-default"])];

/// The `reason_code` of the `why` a mutant of `verdict` was not run for: its
/// own for an unviable mutant or one the debugger cannot run, the verdict's
/// name for a mutant no test calls or reaches.
pub fn reason_code(verdict: Verdict, why: &Report) -> String {
    match verdict {
        Verdict::Unviable => first_of(UNVIABLE, why).unwrap_or(DOES_NOT_COMPILE),
        Verdict::NotCalled => "not-called",
        Verdict::NotReached => "not-reached",
        Verdict::Unsupported | Verdict::Error => first_of(UNSUPPORTED, why).unwrap_or(OTHER),
        _ => "none",
    }
    .to_string()
}

/// The first row of `table` with a code that is in `why`'s tree.
fn first_of(table: &[(&'static str, &[&str])], why: &Report) -> Option<&'static str> {
    let mut codes = Vec::new();
    collect(why, &mut codes);
    table
        .iter()
        .find(|(_, row)| codes.iter().any(|code| row.contains(&code.as_str())))
        .map(|(documented, _)| *documented)
}

/// The code of `d` and of every cause wrapped in its args.
fn collect(d: &dyn Diagnostic, codes: &mut Vec<String>) {
    codes.push(d.code().to_string());
    for (_, arg) in d.args() {
        if let Arg::Nested(cause) = arg {
            collect(cause.as_ref(), codes);
        }
    }
}

#[cfg(test)]
#[path = "code.test.rs"]
mod tests;
