use cherenkov_mutation::deficiency::{unsupported_code as unsupported, unviable_code};

#[test]
fn test_each_way_a_mutant_cannot_run_has_a_short_machine_readable_code() {
    let cases = [
        (
            "the evaluator cannot apply the mutant: Can not force return from an inlined function. (tier P: no tier P rule)",
            "inlined",
        ),
        (
            "the evaluator cannot apply the mutant: Could not find function named 'x::y' (tier P: expected one conditional jump on line 7, found 0)",
            "function-not-found",
        ),
        (
            "the evaluator cannot apply the mutant: the evaluation did not finish within 10 s with the program's other threads stopped (tier P: x)",
            "evaluation-hung",
        ),
        (
            "the evaluator cannot apply the mutant: the evaluation did not finish within 5 s with the program's other threads stopped (tier P: a return stub cannot produce the value true)",
            "evaluation-hung",
        ),
        (
            "the evaluator cannot apply the mutant: only scalar results are returned (core::time::Duration) (tier P: x)",
            "value-not-buildable",
        ),
        (
            "vec![Default::default()] for Vec<T>: needs more of a mirrored type than its layout: the trait bound",
            "value-not-buildable",
        ),
        (
            "MutexGuard::new() for X: a name the support library does not import: cannot find type `MutexGuard`",
            "value-name-unresolved",
        ),
        (
            "a generic function's copies cannot all be patched: a return stub cannot produce the value true",
            "generic-copies",
        ),
        (
            "the evaluator cannot apply the mutant: No symbol 'key' in current context (tier P: x)",
            "symbol-not-in-scope",
        ),
        (
            "the evaluator cannot apply the mutant: Method calls only supported on struct or enum types (tier P: x)",
            "evaluator-cannot-apply",
        ),
        (
            "expected one conditional jump on line 7, found 2",
            "no-breakpoint-site",
        ),
        (
            "no tier P rule for a comparison made into a value without setcc",
            "no-patch-rule",
        ),
        (
            "the debugger failed on this mutant: evaluate: dap connection closed",
            "debugger-failed",
        ),
        ("something new", "other"),
    ];
    for (text, code) in cases {
        assert_eq!(unsupported(text), code, "{text}");
    }
}

#[test]
fn test_an_unviable_mutant_has_a_code_for_its_reason() {
    assert_eq!(
        unviable_code("Settle has no Default in the source"),
        "no-default"
    );
    assert_eq!(
        unviable_code("Poll::Ready(Ok(())) does not compile as Poll<X>"),
        "does-not-compile"
    );
}
