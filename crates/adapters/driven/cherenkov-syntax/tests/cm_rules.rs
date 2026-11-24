//! Rules of cargo-mutants 26.2.0 that are options or quirks: which calls
//! are skipped with their arguments (`skip_calls`), and which attributes
//! count (`cfg_attr(test, mutants::skip)` does, `cfg_attr(not(test), ..)`
//! does not; attributes on a `let` or a `const` are ignored).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::operator_set::named;
use cherenkov_mutation::plan::plan_file;
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

fn mutated_lines(syntax: &TreeSitterSyntax, src: &str) -> BTreeSet<u32> {
    let id = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans
    .iter()
    .filter(|p| p.operator == "aor")
    .map(|p| p.site.start.line)
    .collect()
}

const CALLS: &str = "fn f(n: usize) -> usize {\n\
    let v: Vec<u8> = Vec::with_capacity(1 + 2);\n\
    let w = keep(3 + 4);\n\
    let x = n.keep(5 + 6);\n\
    v.len() + w + x\n\
}\n\
fn keep(a: usize) -> usize { a }\n";

#[test]
fn skip_calls_default_to_with_capacity() {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    assert_eq!(mutated_lines(&syntax, CALLS), BTreeSet::from([3, 4, 5]));
}

#[test]
fn skip_calls_name_functions_and_methods_by_their_last_segment() {
    let syntax = TreeSitterSyntax::builtin()
        .unwrap()
        .with_skip_calls(vec!["keep".into()]);
    // `keep(..)` and `n.keep(..)` go; `with_capacity` is no longer listed.
    assert_eq!(mutated_lines(&syntax, CALLS), BTreeSet::from([2, 5]));
    let none = TreeSitterSyntax::builtin().unwrap().with_skip_calls(vec![]);
    assert_eq!(mutated_lines(&none, CALLS), BTreeSet::from([2, 3, 4, 5]));
    // An entry that is not a bare name never matches.
    let odd = TreeSitterSyntax::builtin()
        .unwrap()
        .with_skip_calls(vec!["Vec::with_capacity".into(), "keep!".into()]);
    assert_eq!(mutated_lines(&odd, CALLS), BTreeSet::from([2, 3, 4, 5]));
}

const ATTRS: &str = "#[cfg_attr(test, mutants::skip)]\nfn a(x: i32) -> i32 { x + 1 }\n\
#[cfg_attr(not(test), mutants::skip)]\nfn b(x: i32) -> i32 { x + 1 }\n\
#[mutants::skip]\nfn c(x: i32) -> i32 { x + 1 }\n\
fn d(x: i32) -> i32 {\n    #[mutants::skip]\n    let y = x + 1;\n    y\n}\n\
#[mutants::skip]\nconst E: i32 = 1 + 1;\n\
#[cfg(test)]\nstatic F: i32 = 2 * 2;\n";

#[test]
fn only_the_attributes_cargo_mutants_reads_count() {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    // a and c are skipped; b is not (cm cannot read `not(test)`); the
    // attributes on the `let`, the const and the static are ignored.
    assert_eq!(
        mutated_lines(&syntax, ATTRS),
        BTreeSet::from([4, 9, 13, 15])
    );
}

#[test]
fn every_and_of_a_let_chain_is_a_binary_operation() {
    let src = "fn f(a: Option<u8>, b: bool, c: bool) -> u8 {\n    if let Some(x) = a && b && c { x } else { 0 }\n}\n\
               fn g(a: Option<u8>, b: bool) -> u8 {\n    if b && let Some(x) = a { x } else { 0 }\n}\n";
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    let plans = plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans;
    let ands: Vec<(u32, u32, Option<&str>)> = plans
        .iter()
        .filter(|p| p.original == "&&")
        .map(|p| (p.site.start.line, p.site.start.col, p.mutated.as_deref()))
        .collect();
    assert_eq!(
        ands,
        [
            (2, 24, Some("||")),
            (2, 29, Some("||")),
            (5, 10, Some("||"))
        ]
    );
}
