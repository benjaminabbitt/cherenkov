//! What Cherenkov cannot run, accepted and logged: a short machine-readable
//! code for the free-text reason an executor gives, so the run's events and
//! summary can count them and the owner can see exactly what was accepted.
//! The text stays as the detail; the code is for counting and filtering.

/// The code for the `reason` of a mutant decided as unviable:
/// - `no-default`: the type has no default in the source to build the
///   mutant's value from;
/// - `does-not-compile`: the mutated source would not compile.
pub fn unviable_code(reason: &str) -> &'static str {
    if reason.contains("has no Default in the source") {
        "no-default"
    } else {
        "does-not-compile"
    }
}

/// The code for the `reason` of a mutant Cherenkov cannot run (decided as
/// unsupported, or as an error), in the order of what blocked it first:
/// - `inlined`: the function has no out-of-line copy to return from;
/// - `generic-copies`: a generic function's copies cannot all be patched;
/// - `evaluation-hung`: the debugger's evaluation never returned;
/// - `value-not-buildable`: the replacement value cannot be built in the
///   debugger (a type with no layout, a name the support library cannot
///   import, a result that is not a scalar);
/// - `value-name-unresolved`: the replacement names something the support
///   library does not import (often a name the source would not resolve
///   either, so the mutant may be unviable);
/// - `function-not-found`: the debugger cannot find a function to call;
/// - `symbol-not-in-scope`: a name the expression needs is not visible;
/// - `evaluator-cannot-apply`: another limit of the debugger's evaluator;
/// - `debugger-failed`: the debugger session failed on this mutant;
/// - `no-breakpoint-site`: the compiled code has no site to match the source;
/// - `no-patch-rule`: no instruction patch exists for this shape;
/// - `other`.
pub fn unsupported_code(reason: &str) -> &'static str {
    let has = |needle: &str| reason.contains(needle);
    if has("inlined function") {
        "inlined"
    } else if reason.starts_with("a generic function's copies") {
        "generic-copies"
    } else if has("evaluation did not finish") {
        // Before the value's reasons: the tier P refusal that rides along
        // ("a return stub cannot produce the value") is not why it hung.
        "evaluation-hung"
    } else if has("a name the support library does not import") {
        "value-name-unresolved"
    } else if has("needs more of a mirrored type")
        || has("only scalar results are returned")
        || has("cannot produce the value")
    {
        "value-not-buildable"
    } else if has("Could not find function named") {
        "function-not-found"
    } else if has("No symbol") || has("There is no member named") || has("no local ") {
        "symbol-not-in-scope"
    } else if has("Method calls only supported")
        || has("Cannot access memory")
        || has("the evaluator cannot apply")
    {
        "evaluator-cannot-apply"
    } else if has("the debugger failed") {
        "debugger-failed"
    } else if has("expected one") {
        "no-breakpoint-site"
    } else if has("no tier P rule") || has("condition code") {
        "no-patch-rule"
    } else {
        "other"
    }
}
