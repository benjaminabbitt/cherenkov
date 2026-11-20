//! Each operator in isolation: which edits it proposes for a concept match.
//! Whether an edit becomes a plan is the planner's job (tests/planner.rs).

mod common;

use cherenkov_core::concept::{Capture, Concept};
use cherenkov_core::edit::{Edit, Lit};
use cherenkov_core::operator::{self, MatchCx, Operator};
use cherenkov_core::syntax::{FileSyntax, StmtKind};
use cherenkov_core::token::Token;
use common::{Toy, toy_profile};

/// Edits `op` proposes for each `concept` match, given the same inputs and
/// context the planner builds.
fn edits(op: &dyn Operator, file: &FileSyntax, concept: Concept) -> Vec<Vec<Edit>> {
    let profile = toy_profile();
    operator::operator_inputs(file)
        .iter()
        .filter(|m| m.concept == concept)
        .map(|m| op.mutate(&MatchCx::new(file, &profile, m)))
        .collect()
}

fn binary(op_text: &str) -> FileSyntax {
    let src = format!("a {op_text} b");
    Toy::new("t", &src)
        .concept(Concept::Binary, &src, 0, &[(Capture::Op, op_text, 0)])
        .build()
}

fn swap(from: Token, with: Token) -> Edit {
    Edit::ReplaceToken {
        capture: Capture::Op,
        from,
        with,
    }
}

#[test]
fn test_catalog_has_six_breadth_operators_with_unique_ids() {
    let ids: Vec<_> = operator::catalog().iter().map(|o| o.id()).collect();
    assert_eq!(
        ids,
        [
            "delete-statement",
            "aor",
            "ror",
            "negate-condition",
            "return-bool-flip",
            "body-default"
        ]
    );
}

#[test]
fn test_ror_swaps_to_boundary_neighbour_only() {
    let cases = [
        ("<", Token::Le),
        ("<=", Token::Lt),
        (">", Token::Ge),
        (">=", Token::Gt),
    ];
    for (text, to) in cases {
        assert_eq!(
            edits(&operator::Ror, &binary(text), Concept::Binary),
            [vec![swap(common::toy_tokens().token(text).unwrap(), to)]],
            "{text}"
        );
    }
}

#[test]
fn test_ror_ignores_equality_arithmetic_and_unknown_tokens() {
    for text in ["==", "!=", "+", "&&", "<=>"] {
        assert_eq!(
            edits(&operator::Ror, &binary(text), Concept::Binary),
            [vec![]],
            "{text}"
        );
    }
}

#[test]
fn test_aor_swaps_add_sub_and_mul_div() {
    let cases = [
        ("+", Token::Sub),
        ("-", Token::Add),
        ("*", Token::Div),
        ("/", Token::Mul),
    ];
    for (text, to) in cases {
        assert_eq!(
            edits(&operator::Aor, &binary(text), Concept::Binary),
            [vec![swap(common::toy_tokens().token(text).unwrap(), to)]],
            "{text}"
        );
    }
    assert_eq!(
        edits(&operator::Aor, &binary("<"), Concept::Binary),
        [vec![]]
    );
}

#[test]
fn test_binary_operators_skip_match_with_repeated_op_capture() {
    // a < b < c: one Binary node, two @op captures.
    let src = "a < b < c";
    let file = Toy::new("t", src)
        .concept(
            Concept::Binary,
            src,
            0,
            &[(Capture::Op, "<", 0), (Capture::Op, "<", 1)],
        )
        .build();
    assert_eq!(edits(&operator::Ror, &file, Concept::Binary), [vec![]]);
}

#[test]
fn test_negate_condition_wraps_cond_capture() {
    let src = "if a { }";
    let file = Toy::new("t", src)
        .concept(
            Concept::Condition,
            src,
            0,
            &[(Capture::Cond, "a", 0), (Capture::Then, "{ }", 0)],
        )
        .build();
    assert_eq!(
        edits(&operator::NegateCondition, &file, Concept::Condition),
        [vec![Edit::WrapNot {
            capture: Capture::Cond
        }]]
    );
}

#[test]
fn test_negate_condition_needs_a_cond_capture() {
    let src = "loop { }";
    let file = Toy::new("t", src)
        .concept(Concept::Condition, src, 0, &[(Capture::Then, "{ }", 0)])
        .build();
    assert_eq!(
        edits(&operator::NegateCondition, &file, Concept::Condition),
        [vec![]]
    );
}

#[test]
fn test_return_bool_flip_flips_profile_bool_literals() {
    let src = "return yes; return no;";
    let file = Toy::new("t", src)
        .concept(
            Concept::Return,
            "return yes",
            0,
            &[(Capture::Value, "yes", 0)],
        )
        .concept(
            Concept::Return,
            "return no",
            0,
            &[(Capture::Value, "no", 0)],
        )
        .build();
    let lit = |b| Edit::ReplaceWithLiteral {
        capture: Capture::Value,
        lit: Lit::Bool(b),
    };
    assert_eq!(
        edits(&operator::ReturnBoolFlip, &file, Concept::Return),
        [vec![lit(false)], vec![lit(true)]]
    );
}

#[test]
fn test_return_bool_flip_ignores_non_literal_and_bare_returns() {
    let src = "return yes_please; return;";
    let file = Toy::new("t", src)
        .concept(
            Concept::Return,
            "return yes_please",
            0,
            &[(Capture::Value, "yes_please", 0)],
        )
        .concept(Concept::Return, "return;", 0, &[])
        .build();
    assert_eq!(
        edits(&operator::ReturnBoolFlip, &file, Concept::Return),
        [vec![], vec![]]
    );
}

#[test]
fn test_body_default_replaces_body_of_function_with_result_type() {
    let src = "fn f() bool { }";
    let file = Toy::new("t", src)
        .concept(
            Concept::Function,
            src,
            0,
            &[(Capture::Returns, "bool", 0), (Capture::Body, "{ }", 0)],
        )
        .build();
    assert_eq!(
        edits(&operator::BodyDefault, &file, Concept::Function),
        [vec![Edit::ReplaceWithLiteral {
            capture: Capture::Body,
            lit: Lit::Default
        }]]
    );
}

#[test]
fn test_body_default_skips_unit_and_undeclared_result_types() {
    let src = "fn f() void { } fn g() { }";
    let file = Toy::new("t", src)
        .concept(
            Concept::Function,
            "fn f() void { }",
            0,
            &[(Capture::Returns, "void", 0), (Capture::Body, "{ }", 0)],
        )
        .concept(
            Concept::Function,
            "fn g() { }",
            0,
            &[(Capture::Body, "{ }", 1)],
        )
        .build();
    assert_eq!(
        edits(&operator::BodyDefault, &file, Concept::Function),
        [vec![], vec![]]
    );
}

#[test]
fn test_delete_statement_only_deletes_call_statements() {
    let src = "log(x); x := 1;";
    let file = Toy::new("t", src)
        .stmt(StmtKind::Call, "log(x);", 0)
        .stmt(StmtKind::Assign, "x := 1;", 0)
        .build();
    assert_eq!(
        edits(&operator::DeleteStatement, &file, Concept::Statement),
        [vec![Edit::DeleteStatement], vec![]]
    );
}

#[test]
fn test_operator_inputs_add_one_statement_match_per_statement_in_position_order() {
    let src = "x := 1; log(x);";
    let file = Toy::new("t", src)
        .stmt(StmtKind::Call, "log(x);", 0)
        .stmt(StmtKind::Assign, "x := 1;", 0)
        .concept(
            Concept::Assign,
            "x := 1",
            0,
            &[(Capture::Target, "x", 0), (Capture::Value, "1", 0)],
        )
        .build();
    let got: Vec<(Concept, &str)> = operator::operator_inputs(&file)
        .iter()
        .map(|m| (m.concept, m.span.text(src)))
        .collect();
    assert_eq!(
        got,
        [
            (Concept::Assign, "x := 1"),
            (Concept::Statement, "x := 1;"),
            (Concept::Statement, "log(x);")
        ]
    );
}

#[test]
fn test_match_context_is_the_innermost_containing_statement() {
    let src = "if a { x := b + 1; }";
    let file = Toy::new("t", src)
        .stmt(StmtKind::If, src, 0)
        .stmt(StmtKind::Assign, "x := b + 1;", 0)
        .concept(Concept::Binary, "b + 1", 0, &[(Capture::Op, "+", 0)])
        .build();
    let profile = toy_profile();
    let m = &file.matches[0];
    let cx = MatchCx::new(&file, &profile, m);
    assert_eq!(cx.stmt.map(|s| s.kind), Some(StmtKind::Assign));
    assert_eq!(cx.text(Capture::Op), Some("+"));
    assert_eq!(cx.token(Capture::Op), Some(Token::Add));
}
