//! Unary operator deletion (cargo-mutants' UnaryOperator genre): `!x` and
//! `-x` lose their operator; other unary operators (deref, address-of,
//! bitwise complement, unary plus) are left alone. The same mutation in
//! every language, through each profile's spellings.

use std::path::Path;
use std::sync::Arc;

use cherenkov_core::operator_set::named;
use cherenkov_core::plan::{MutantPlan, plan_file, render_source};
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

/// The unary mutants of the cargo-mutants set, as (mutation, mutated source line).
fn unary(lang: &str, src: &str) -> Vec<(&'static str, String)> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    assert!(!file.has_errors, "{lang} sample does not parse");
    let plans: Vec<MutantPlan> = plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans;
    plans
        .iter()
        .filter(|p| p.operator == "unary")
        .map(|p| {
            let out = render_source(src, p, &profile).expect("renders");
            let line = out
                .lines()
                .nth(p.site.start.line as usize - 1)
                .unwrap()
                .trim();
            (p.mutation.unwrap(), line.to_string())
        })
        .collect()
}

#[test]
fn test_rust_not_and_negation_are_deleted() {
    let src = "fn f(a: i32, b: bool) -> i32 {\n    if !b {\n        return -a;\n    }\n    a\n}\n";
    assert_eq!(
        unary("rust", src),
        [
            ("unary/delete-not", "if b {".to_string()),
            ("unary/delete-neg", "return a;".to_string()),
        ]
    );
}

#[test]
fn test_rust_deref_is_not_a_unary_mutation() {
    let src = "fn f(p: &i32) -> i32 {\n    let v = *p;\n    v\n}\n";
    assert_eq!(unary("rust", src), []);
}

#[test]
fn test_go_not_and_negation_are_deleted_but_complement_is_not() {
    let src =
        "package p\n\nfunc f(a int, b bool) int {\n\tif !b {\n\t\treturn -a\n\t}\n\treturn ^a\n}\n";
    assert_eq!(
        unary("go", src),
        [
            ("unary/delete-not", "if b {".to_string()),
            ("unary/delete-neg", "return a".to_string()),
        ]
    );
}

#[test]
fn test_python_not_and_negation_are_deleted_but_complement_is_not() {
    let src = "def f(a, b):\n    if not b:\n        return -a\n    return ~a\n";
    assert_eq!(
        unary("python", src),
        [
            ("unary/delete-not", "if  b:".to_string()),
            ("unary/delete-neg", "return a".to_string()),
        ]
    );
}
