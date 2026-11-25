//! Why the executor did not run a mutant, or did not reach it: coded, with
//! named data, never English (the messages are `locales/en-US/messages.ftl`'s).
//! What the debugger said is carried as `detail`, as it was said. A reason
//! wraps its causes (a refusal at tier B holds why tier P had no patch), and
//! the output's `reason_code` is a table over the codes in that tree.

use coded_diagnostic::{Arg, Diagnostic, Report, code};

/// Why a mutant was not run, or not reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The debugger cannot force a return from an inlined function.
    Inlined { detail: String },
    /// The debugger finds no function to call.
    FunctionNotFound { detail: String },
    /// A name the expression needs is not visible to the debugger.
    SymbolNotInScope { detail: String },
    /// The evaluator returns only scalar results.
    NotScalar { detail: String },
    /// Another limit of the debugger's evaluator, as it said it.
    EvaluatorSaid { detail: String },
    /// An evaluation had all the time it is given and did not finish.
    EvaluationHung { secs: u64 },
    /// The evaluator cannot apply the mutant, for `cause`.
    EvaluatorRefused { cause: Report },
    /// `cause`, though the swap of `&&` or `||` was not steered, for
    /// `unsteered`.
    Unsteered { cause: Report, unsteered: Report },
    /// `cause`, with no tier P patch to apply instead, for `tier_p`.
    Unappliable { cause: Report, tier_p: Report },
    /// The value is not built from its layout, for `built`, and no
    /// constructor makes it, for `constructor`.
    NoConstructor { built: Report, constructor: Report },
    /// A generic function's copies cannot all be patched, for `cause`.
    GenericCopies { cause: Report },
    /// A negated boolean that no call computes.
    NegatedNotFromCall,
    /// A small result holding a float, which may come back in registers
    /// this builder does not write.
    FloatResult { bytes: u64 },
    /// No instruction comes from the mutant's line.
    NoCodeForLine { line: u32 },
    /// The debugger has no tier B evaluator.
    NoTierBEvaluator,
    /// The mutated function has no code.
    NoCode,
    /// The mutated line is not in the program's debug info.
    NotInProgram { file: String, line: u32 },
    /// The selected tests never execute the mutated code.
    NeverExecuted,
    /// The mutant's value names something the source cannot resolve.
    UnresolvedInSource { value: String, names: String },
    /// rustc rejects the mutant, saying `detail`.
    RustcRejects { detail: String },
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Reason {}

impl Reason {
    /// A reason that wraps `cause` as its one arg.
    fn wrapping(cause: &Report) -> Vec<(&str, Arg)> {
        vec![("cause", Arg::nested(cause))]
    }
}

impl Diagnostic for Reason {
    fn code(&self) -> &str {
        match self {
            Reason::Inlined { .. } => code!("inlined"),
            Reason::FunctionNotFound { .. } => code!("function-not-found"),
            Reason::SymbolNotInScope { .. } => code!("symbol-not-in-scope"),
            Reason::NotScalar { .. } => code!("not-scalar"),
            Reason::EvaluatorSaid { .. } => code!("evaluator-said"),
            Reason::EvaluationHung { .. } => code!("evaluation-hung"),
            Reason::EvaluatorRefused { .. } => code!("evaluator-refused"),
            Reason::Unsteered { .. } => code!("unsteered"),
            Reason::Unappliable { .. } => code!("unappliable"),
            Reason::NoConstructor { .. } => code!("no-constructor"),
            Reason::GenericCopies { .. } => code!("generic-copies"),
            Reason::NegatedNotFromCall => code!("negated-not-from-call"),
            Reason::FloatResult { .. } => code!("float-result"),
            Reason::NoCodeForLine { .. } => code!("no-code-for-line"),
            Reason::NoTierBEvaluator => code!("no-tier-b-evaluator"),
            Reason::NoCode => code!("no-code"),
            Reason::NotInProgram { .. } => code!("not-in-program"),
            Reason::NeverExecuted => code!("never-executed"),
            Reason::UnresolvedInSource { .. } => code!("unresolved-in-source"),
            Reason::RustcRejects { .. } => code!("rustc-rejects"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Reason::Inlined { detail }
            | Reason::FunctionNotFound { detail }
            | Reason::SymbolNotInScope { detail }
            | Reason::NotScalar { detail }
            | Reason::EvaluatorSaid { detail }
            | Reason::RustcRejects { detail } => vec![("detail", detail.as_str().into())],
            Reason::EvaluationHung { secs } => vec![("secs", (*secs).into())],
            Reason::EvaluatorRefused { cause } | Reason::GenericCopies { cause } => {
                Reason::wrapping(cause)
            }
            Reason::Unsteered { cause, unsteered } => vec![
                ("cause", Arg::nested(cause)),
                ("unsteered", Arg::nested(unsteered)),
            ],
            Reason::Unappliable { cause, tier_p } => vec![
                ("cause", Arg::nested(cause)),
                ("tier_p", Arg::nested(tier_p)),
            ],
            Reason::NoConstructor { built, constructor } => vec![
                ("built", Arg::nested(built)),
                ("constructor", Arg::nested(constructor)),
            ],
            Reason::FloatResult { bytes } => vec![("bytes", (*bytes).into())],
            Reason::NoCodeForLine { line } => vec![("line", (*line).into())],
            Reason::NotInProgram { file, line } => {
                vec![("file", file.as_str().into()), ("line", (*line).into())]
            }
            Reason::UnresolvedInSource { value, names } => {
                vec![
                    ("value", value.as_str().into()),
                    ("names", names.as_str().into()),
                ]
            }
            Reason::NegatedNotFromCall
            | Reason::NoTierBEvaluator
            | Reason::NoCode
            | Reason::NeverExecuted => Vec::new(),
        }
    }
}

/// The reason for an evaluator's refusal, as the debugger said it: the one
/// place that reads the debugger's own words, which are not coded.
pub fn evaluator_said(said: &str) -> Reason {
    let has = |needle: &str| said.contains(needle);
    let detail = said.to_string();
    if has("inlined function") {
        Reason::Inlined { detail }
    } else if has("only scalar results are returned") {
        Reason::NotScalar { detail }
    } else if has("Could not find function named") {
        Reason::FunctionNotFound { detail }
    } else if has("No symbol") || has("There is no member named") || has("no local ") {
        Reason::SymbolNotInScope { detail }
    } else {
        Reason::EvaluatorSaid { detail }
    }
}

/// One value of every reason, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let cause = || Report::new("cherenkov-exec-dap-no-code");
    let text = |s: &str| s.to_string();
    let reasons = vec![
        Reason::Inlined { detail: text("d") },
        Reason::FunctionNotFound { detail: text("d") },
        Reason::SymbolNotInScope { detail: text("d") },
        Reason::NotScalar { detail: text("d") },
        Reason::EvaluatorSaid { detail: text("d") },
        Reason::EvaluationHung { secs: 5 },
        Reason::EvaluatorRefused { cause: cause() },
        Reason::Unsteered {
            cause: cause(),
            unsteered: cause(),
        },
        Reason::Unappliable {
            cause: cause(),
            tier_p: cause(),
        },
        Reason::NoConstructor {
            built: cause(),
            constructor: cause(),
        },
        Reason::GenericCopies { cause: cause() },
        Reason::NegatedNotFromCall,
        Reason::FloatResult { bytes: 8 },
        Reason::NoCodeForLine { line: 7 },
        Reason::NoTierBEvaluator,
        Reason::NoCode,
        Reason::NotInProgram {
            file: text("src/lib.rs"),
            line: 7,
        },
        Reason::NeverExecuted,
        Reason::UnresolvedInSource {
            value: text("v"),
            names: text("`n`"),
        },
        Reason::RustcRejects { detail: text("d") },
    ];
    reasons
        .into_iter()
        .map(|r| Box::new(r) as Box<dyn Diagnostic>)
        .collect()
}
