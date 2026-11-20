//! A function body's contents begin at its first code, not at a comment
//! before it: cargo-mutants' body span starts at the first token, and the
//! replaced text should not swallow the comments.

use std::path::Path;
use std::sync::Arc;

use cherenkov_core::operator_set::named;
use cherenkov_core::plan::{plan_file, render_source};
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

/// (line, col) of the first fn-value mutant's site, and the rendered source.
fn first_fn_value(lang: &str, src: &str) -> ((u32, u32), String) {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
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
    let p = plans.iter().find(|p| p.operator == "fn-value").unwrap();
    (
        (p.site.start.line, p.site.start.col),
        render_source(src, p, &profile).unwrap(),
    )
}

#[test]
fn test_rust_leading_line_and_block_comments_stay_outside_the_body_contents() {
    let src = "fn f() -> bool {\n    // why\n    /* and how */\n    g()\n}\n";
    let (at, out) = first_fn_value("rust", src);
    assert_eq!(at, (4, 5));
    assert_eq!(
        out,
        "fn f() -> bool {\n    // why\n    /* and how */\n    false\n}\n"
    );
}

#[test]
fn test_rust_comment_only_lines_between_code_are_part_of_the_contents() {
    let src = "fn f() -> bool {\n    a();\n    // between\n    g()\n}\n";
    let (at, out) = first_fn_value("rust", src);
    assert_eq!(at, (2, 5));
    assert_eq!(out, "fn f() -> bool {\n    false\n}\n");
}

/// A fn-value mutant's site, (line, col) to (line, col), and its value.
type Site = ((u32, u32), (u32, u32), String);

/// Every fn-value mutant's site and value.
fn fn_values(src: &str) -> Vec<Site> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
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
    .into_iter()
    .filter(|p| p.operator == "fn-value")
    .map(|p| {
        let value = match &p.action {
            cherenkov_core::plan::Action::ReturnDefault { value_text, .. } => {
                value_text.clone().unwrap_or_default()
            }
            _ => String::new(),
        };
        (
            (p.site.start.line, p.site.start.col),
            (p.site.end.line, p.site.end.col),
            value,
        )
    })
    .collect()
}

#[test]
fn test_rust_a_trailing_line_comment_stays_outside_the_body_contents() {
    // cargo-mutants' body span ends at the last token: `None`, not the
    // comment after it.
    let src = "fn f() -> Option<usize> {\n    Some(1) // why\n}\n";
    let sites = fn_values(src);
    assert!(!sites.is_empty());
    assert!(
        sites
            .iter()
            .all(|(start, end, _)| *start == (2, 5) && *end == (2, 12)),
        "{sites:?}"
    );
}

#[test]
fn test_rust_a_trailing_block_comment_stays_outside_the_body_contents() {
    let src = "fn f() -> bool {\n    g() /* why */\n}\n";
    let sites = fn_values(src);
    assert!(sites.iter().all(|(_, end, _)| *end == (2, 8)), "{sites:?}");
}

#[test]
fn test_rust_a_value_the_body_already_is_is_no_mutant() {
    // Replacing `None` with `None` changes nothing: cargo-mutants does not
    // generate it.
    let src = "fn f() -> Option<usize> {\n    None // no limit\n}\n";
    let values: Vec<String> = fn_values(src).into_iter().map(|(_, _, v)| v).collect();
    assert!(!values.iter().any(|v| v == "None"), "{values:?}");
    assert!(!values.is_empty(), "the other values are still mutants");
}

#[test]
fn test_rust_a_comment_marker_inside_a_string_is_part_of_the_body() {
    let src = "fn f() -> String {\n    \"http://x\".to_string() // why\n}\n";
    let sites = fn_values(src);
    // `"http://x".to_string()` is 22 bytes from column 5.
    assert!(sites.iter().all(|(_, end, _)| *end == (2, 27)), "{sites:?}");
}
