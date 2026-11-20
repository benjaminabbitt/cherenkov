//! Rendering a plan as mutated source text: the ground truth a run-time
//! mutant is judged against (docs/mutation-operators.md, "How mutants are judged").

mod common;

use cherenkov_core::operator::catalog;
use cherenkov_core::plan::{Action, MutantPlan, plan_file, render_source};
use common::{TOY_SRC, toy_file, toy_profile};

fn plans() -> Vec<MutantPlan> {
    plan_file(&toy_file("toy.src", TOY_SRC), &toy_profile(), &catalog()).plans
}

fn rendered(operator: &str, line: u32) -> String {
    let p = plans()
        .into_iter()
        .find(|p| p.operator == operator && p.stop_at.line == line)
        .unwrap();
    render_source(TOY_SRC, &p, &toy_profile()).expect("renderable")
}

/// The one line that differs from the original, or all lines if more do.
fn changed(mutated: &str) -> Vec<(usize, String)> {
    let orig: Vec<&str> = TOY_SRC.lines().collect();
    mutated
        .lines()
        .enumerate()
        .filter(|(i, l)| orig.get(*i) != Some(l))
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect()
}

#[test]
fn test_render_token_swap_changes_only_the_operator() {
    assert_eq!(
        changed(&rendered("aor", 2)),
        [(2, "    x := a - b;".to_string())]
    );
    assert_eq!(
        changed(&rendered("ror", 4)),
        [(4, "    if a <= b { return yes; } else { g(); }".to_string())]
    );
}

#[test]
fn test_render_negation_wraps_the_condition_with_the_profile_template() {
    assert_eq!(
        changed(&rendered("negate-condition", 5)),
        [(5, "    while NOT[a > b] { a := a - 1; }".to_string())]
    );
}

#[test]
fn test_render_return_flip_replaces_the_literal() {
    assert_eq!(
        changed(&rendered("return-bool-flip", 7)),
        [(7, "    return yes;".to_string())]
    );
}

#[test]
fn test_render_statement_deletion_removes_the_statement() {
    assert_eq!(
        changed(&rendered("delete-statement", 3)),
        [(3, "    ".to_string())]
    );
}

#[test]
fn test_render_body_default_fills_the_profile_body_template() {
    // The body's contents (between the braces) become the template, `{}`.
    let got = rendered("body-default", 1);
    assert_eq!(got, "fn f() bool {\n    no\n}\n");
}

#[test]
fn test_render_body_default_without_a_known_value_is_none() {
    let mut p = plans()
        .into_iter()
        .find(|p| p.operator == "body-default")
        .unwrap();
    p.action = Action::ReturnDefault {
        value_text: None,
        zero_bits: false,
        async_fn: false,
    };
    assert_eq!(render_source(TOY_SRC, &p, &toy_profile()), None);
}

#[test]
fn test_render_every_planned_mutant_differs_from_the_original() {
    for p in plans() {
        let got = render_source(TOY_SRC, &p, &toy_profile()).expect("renderable");
        assert_ne!(got, TOY_SRC, "{}", p.describe());
    }
}
