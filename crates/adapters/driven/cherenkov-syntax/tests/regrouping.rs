//! A swapped operator's operands are those of the mutated source as it
//! parses, not of the original tree: replacing the first `&&` of
//! `a && b && c` with `||` gives `a || b && c`, which is `a || (b && c)`,
//! because `&&` binds tighter. Its right operand is `b && c`. The same in
//! every language whose logical and binds tighter than its logical or.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::operator_set::named;
use cherenkov_mutation::plan::plan_file;
use cherenkov_mutation::syntax::{LanguageId, Span};
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

/// For each logical swap in `src`, (mutation, left operand, right operand).
fn swaps(lang: &str, src: &str) -> Vec<(&'static str, String, String)> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    assert!(!file.has_errors, "{lang} sample does not parse");
    let text = |s: &Span| src[s.start.byte..s.end.byte].to_string();
    let mut got: Vec<_> = plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans
    .into_iter()
    .filter(|p| p.operator == "lcr")
    .filter_map(|p| match &p.operands[..] {
        [l, r] => Some((p.mutation.unwrap(), text(l), text(r))),
        _ => None,
    })
    .collect();
    got.sort();
    got
}

fn owned(rows: &[(&'static str, &str, &str)]) -> Vec<(&'static str, String, String)> {
    rows.iter()
        .map(|&(m, l, r)| (m, l.to_string(), r.to_string()))
        .collect()
}

#[test]
fn test_rust_and_to_or_inside_an_and_chain_takes_the_rest_of_the_chain() {
    let src = "fn f(a: bool, b: bool, c: bool) -> bool {\n    a && b && c\n}\n";
    assert_eq!(
        swaps("rust", src),
        owned(&[
            // `a || b && c` is `a || (b && c)`.
            ("lcr/and->or", "a", "b && c"),
            // `a && b || c` is `(a && b) || c`: as the tree has it.
            ("lcr/and->or", "a && b", "c"),
        ])
    );
}

#[test]
fn test_rust_or_to_and_atop_an_or_chain_takes_only_the_last_left_operand() {
    let src = "fn f(a: bool, b: bool, c: bool) -> bool {\n    a || b || c\n}\n";
    assert_eq!(
        swaps("rust", src),
        owned(&[
            // `a && b || c` is `(a && b) || c`: as the tree has it.
            ("lcr/or->and", "a", "b"),
            // `a || b && c` is `a || (b && c)`, not `(a || b) && c`.
            ("lcr/or->and", "b", "c"),
        ])
    );
}

#[test]
fn test_go_and_python_regroup_the_same_way() {
    let go = "package p\n\nfunc f(a, b, c bool) bool {\n\treturn a && b && c\n}\n";
    let py = "def f(a, b, c):\n    return a and b and c\n";
    let and_first = |rows: Vec<(&'static str, String, String)>| {
        rows.into_iter()
            .find(|(_, l, _)| l == "a")
            .map(|(_, _, r)| r)
    };
    assert_eq!(and_first(swaps("go", go)).as_deref(), Some("b && c"));
    assert_eq!(and_first(swaps("python", py)).as_deref(), Some("b and c"));
}
