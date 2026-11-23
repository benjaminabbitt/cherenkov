mod common;

use cherenkov_core::concept::{Capture, Concept};
use cherenkov_core::operator::catalog;
use cherenkov_core::plan::{Action, Diagnostics, MutantPlan, Requires, Target, plan_file};
use cherenkov_core::syntax::StmtKind;
use common::{TOY_SRC, Toy, at, toy_file, toy_profile};

fn plans_of(src: &str) -> Vec<MutantPlan> {
    plan_file(&toy_file("toy.src", src), &toy_profile(), &catalog()).plans
}

fn find<'a>(plans: &'a [MutantPlan], operator: &str, line: u32) -> &'a MutantPlan {
    plans
        .iter()
        .find(|p| p.operator == operator && p.stop_at.line == line)
        .unwrap_or_else(|| panic!("no {operator} plan on line {line}"))
}

// --- The whole toy function, as a listing: the planner's contract in one view.

#[test]
fn test_plan_toy_function_lists_every_breadth_mutant_in_source_order() {
    let listing: Vec<String> = plans_of(TOY_SRC).iter().map(|p| p.describe()).collect();
    assert_eq!(
        listing,
        [
            r#"toy.src:1:1 f body-default return-default "no" [force-return]"#,
            r#"toy.src:2:5 f aor eval-then-jump "x := a - b" [assign]"#,
            r#"toy.src:3:5 f delete-statement jump-only"#,
            r#"toy.src:4:8 f negate-condition eval-branch "NOT[a < b]""#,
            r#"toy.src:4:8 f ror eval-branch "a <= b""#,
            r#"toy.src:4:16 f return-bool-flip eval-return "no" [force-return]"#,
            r#"toy.src:4:37 f delete-statement jump-only"#,
            r#"toy.src:5:11 f negate-condition eval-branch "NOT[a > b]""#,
            r#"toy.src:5:11 f ror eval-branch "a >= b""#,
            r#"toy.src:5:19 f aor eval-then-jump "a := a + 1" [assign]"#,
            r#"toy.src:6:5 f delete-statement jump-only"#,
            r#"toy.src:6:7 f aor in-place "a - 1""#,
            r#"toy.src:7:5 f return-bool-flip eval-return "yes" [force-return]"#,
        ]
    );
}

// --- Actions and structural targets per statement kind (design section 3.3).

#[test]
fn test_plan_arith_in_assignment_evaluates_target_assign_mutated_value_then_skips_statement() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "aor", 2);
    assert_eq!(
        p.action,
        Action::EvalThenJump {
            text: "x := a - b".into(),
            target: "x".into(),
            value: "a - b".into(),
            resume: Target::AfterStatement(at(TOY_SRC, "x := a + b;", 0)),
        }
    );
    assert_eq!(p.stop_at, at(TOY_SRC, "x := a + b;", 0).start);
    assert_eq!(p.site, at(TOY_SRC, "+", 0));
    assert_eq!(p.requires, Requires::ASSIGN_IN_EVAL);
}

#[test]
fn test_plan_statement_deletion_jumps_past_statement_without_evaluating() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "delete-statement", 3);
    assert_eq!(
        p.action,
        Action::JumpOnly {
            resume: Target::AfterStatement(at(TOY_SRC, "log(x);", 0))
        }
    );
    assert_eq!(p.requires, Requires::empty());
}

#[test]
fn test_plan_condition_with_else_branches_to_then_or_else_block() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "ror", 4);
    assert_eq!(
        p.action,
        Action::EvalBranch {
            cond_text: "a <= b".into(),
            if_true: Target::BlockStart(at(TOY_SRC, "{ return yes; }", 0)),
            if_false: Target::BlockStart(at(TOY_SRC, "{ g(); }", 0)),
        }
    );
    assert_eq!(p.stop_at, at(TOY_SRC, "a < b", 0).start);
}

#[test]
fn test_plan_condition_without_else_falls_through_past_whole_statement() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "negate-condition", 5);
    assert_eq!(
        p.action,
        Action::EvalBranch {
            cond_text: "NOT[a > b]".into(),
            if_true: Target::BlockStart(at(TOY_SRC, "{ a := a - 1; }", 0)),
            if_false: Target::AfterStatement(at(TOY_SRC, "while a > b { a := a - 1; }", 0)),
        }
    );
}

#[test]
fn test_plan_return_flip_evaluates_literal_and_forces_return() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "return-bool-flip", 7);
    assert_eq!(
        p.action,
        Action::EvalReturn {
            value_text: "yes".into()
        }
    );
    assert_eq!(p.requires, Requires::FORCE_RETURN);
}

#[test]
fn test_plan_body_default_stops_at_function_entry_with_profile_default() {
    let plans = plans_of(TOY_SRC);
    let p = find(&plans, "body-default", 1);
    assert_eq!(
        p.action,
        Action::ReturnDefault {
            value_text: Some("no".into()),
            zero_bits: true,
            async_fn: false,
        }
    );
    assert_eq!(p.stop_at, at(TOY_SRC, "fn f()", 0).start);
    assert_eq!(p.function.as_deref(), Some("f"));
}

#[test]
fn test_plan_body_default_leaves_unknown_result_type_default_to_the_driver() {
    let src = "fn h() Widget { }";
    let file = Toy::new("w.src", src)
        .concept(
            Concept::Function,
            src,
            0,
            &[
                (Capture::Name, "h", 0),
                (Capture::Returns, "Widget", 0),
                (Capture::Body, "{ }", 0),
            ],
        )
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(
        out.plans[0].action,
        Action::ReturnDefault {
            value_text: None,
            zero_bits: false,
            async_fn: false,
        }
    );
    assert_eq!(
        out.plans[0].describe(),
        "w.src:1:1 h body-default return-default <default> [force-return]"
    );
}

// --- Requirements.

#[test]
fn test_plan_call_inside_evaluated_condition_sets_call_requirement() {
    let src = "while i < len(xs) { }";
    let file = Toy::new("c.src", src)
        .stmt(StmtKind::Loop, src, 0)
        .concept(
            Concept::Condition,
            src,
            0,
            &[(Capture::Cond, "i < len(xs)", 0), (Capture::Then, "{ }", 0)],
        )
        .concept(Concept::Binary, "i < len(xs)", 0, &[(Capture::Op, "<", 0)])
        .concept(Concept::Call, "len(xs)", 0, &[(Capture::Callee, "len", 0)])
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert!(!out.plans.is_empty());
    for p in &out.plans {
        assert_eq!(p.requires, Requires::CALL_IN_EVAL, "{}", p.describe());
    }
}

#[test]
fn test_plan_call_inside_assigned_value_sets_assign_and_call_requirements() {
    let src = "x := f(a) + b;";
    let file = Toy::new("a.src", src)
        .stmt(StmtKind::Assign, src, 0)
        .concept(
            Concept::Assign,
            "x := f(a) + b",
            0,
            &[(Capture::Target, "x", 0), (Capture::Value, "f(a) + b", 0)],
        )
        .concept(Concept::Binary, "f(a) + b", 0, &[(Capture::Op, "+", 0)])
        .concept(Concept::Call, "f(a)", 0, &[(Capture::Callee, "f", 0)])
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(out.plans.len(), 1);
    assert_eq!(
        out.plans[0].action,
        Action::EvalThenJump {
            text: "x := f(a) - b".into(),
            target: "x".into(),
            value: "f(a) - b".into(),
            resume: Target::AfterStatement(at(src, src, 0)),
        }
    );
    assert_eq!(
        out.plans[0].requires,
        Requires::ASSIGN_IN_EVAL | Requires::CALL_IN_EVAL
    );
}

#[test]
fn test_plan_call_outside_evaluated_text_does_not_set_call_requirement() {
    // The call is in the statement but not in the evaluated condition.
    let src = "if a < b { g(); }";
    let file = Toy::new("n.src", src)
        .stmt(StmtKind::If, src, 0)
        .concept(
            Concept::Condition,
            src,
            0,
            &[(Capture::Cond, "a < b", 0), (Capture::Then, "{ g(); }", 0)],
        )
        .concept(Concept::Binary, "a < b", 0, &[(Capture::Op, "<", 0)])
        .concept(Concept::Call, "g()", 0, &[(Capture::Callee, "g", 0)])
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(out.plans.len(), 2);
    assert!(out.plans.iter().all(|p| p.requires.is_empty()));
}

#[test]
fn test_requires_names_are_listed_in_fixed_order() {
    let all = Requires::FORCE_RETURN | Requires::CALL_IN_EVAL | Requires::ASSIGN_IN_EVAL;
    assert_eq!(all.names(), ["call", "assign", "force-return"]);
    assert!(Requires::empty().names().is_empty());
}

// --- Things the planner reports instead of planning.

#[test]
fn test_plan_arith_outside_assignment_or_condition_is_planned_in_place() {
    // g(a + 1): no statement-level evaluation exists for a call argument, so
    // the mutant applies at the operator itself.
    let out = plan_file(&toy_file("toy.src", TOY_SRC), &toy_profile(), &catalog());
    let p = out
        .plans
        .iter()
        .find(|p| p.site == at(TOY_SRC, "+", 1))
        .expect("planned");
    assert_eq!(
        p.action,
        Action::InPlace {
            expr_text: "a - 1".into()
        }
    );
    assert_eq!(p.stop_at, at(TOY_SRC, "a + 1", 0).start);
    assert_eq!(p.requires, Requires::empty());
    assert!(out.diagnostics.skipped.is_empty());
}

#[test]
fn test_plan_compound_assignment_swap_evaluates_the_mutated_statement_then_skips_it() {
    let src = "x += y;";
    let file = Toy::new("c.src", src)
        .stmt(StmtKind::Assign, src, 0)
        .concept(
            Concept::CompoundAssign,
            "x += y",
            0,
            &[
                (Capture::Target, "x", 0),
                (Capture::Op, "+=", 0),
                (Capture::Value, "y", 0),
            ],
        )
        .build();
    let set = cherenkov_core::operator_set::named("cargo-mutants").unwrap();
    let out = plan_file(&file, &toy_profile(), &set.operators());
    let listing: Vec<String> = out.plans.iter().map(|p| p.describe()).collect();
    assert_eq!(
        listing,
        [
            r#"c.src:1:1 - compound-assign eval-then-jump "x -= y" [assign]"#,
            r#"c.src:1:1 - compound-assign eval-then-jump "x *= y" [assign]"#
        ]
    );
}

#[test]
fn test_plan_binary_with_repeated_operator_capture_is_unrepresentable() {
    let src = "x := a < b < c;";
    let file = Toy::new("ch.src", src)
        .stmt(StmtKind::Assign, src, 0)
        .concept(
            Concept::Assign,
            "x := a < b < c",
            0,
            &[(Capture::Target, "x", 0), (Capture::Value, "a < b < c", 0)],
        )
        .concept(
            Concept::Binary,
            "a < b < c",
            0,
            &[(Capture::Op, "<", 0), (Capture::Op, "<", 1)],
        )
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert!(out.plans.is_empty());
    assert_eq!(
        out.diagnostics.unrepresentable.get(&Concept::Binary),
        Some(&1)
    );
}

#[test]
fn test_plan_condition_and_assign_with_repeated_roles_are_unrepresentable() {
    use Capture::*;
    let src = "if a { } { } x := 1 2;";
    let file = Toy::new("r.src", src)
        // One cond but two then-blocks.
        .concept(
            Concept::Condition,
            "if a { } { }",
            0,
            &[(Cond, "a", 0), (Then, "{ }", 0), (Then, "{ }", 1)],
        )
        // Two conds.
        .concept(
            Concept::Condition,
            "if a { }",
            0,
            &[(Cond, "a", 0), (Cond, "if", 0), (Then, "{ }", 0)],
        )
        // One target, two values.
        .concept(
            Concept::Assign,
            "x := 1 2",
            0,
            &[(Target, "x", 0), (Value, "1", 0), (Value, "2", 0)],
        )
        // Two targets, one value.
        .concept(
            Concept::Assign,
            "x := 1",
            0,
            &[(Target, "x", 0), (Target, ":=", 0), (Value, "1", 0)],
        )
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert!(out.plans.is_empty());
    assert_eq!(
        out.diagnostics.unrepresentable.get(&Concept::Condition),
        Some(&2)
    );
    assert_eq!(
        out.diagnostics.unrepresentable.get(&Concept::Assign),
        Some(&2)
    );
}

#[test]
fn test_plan_disabled_operator_is_counted_not_planned() {
    let mut profile = toy_profile();
    profile.disabled_operators.insert("ror".into());
    let out = plan_file(&toy_file("toy.src", TOY_SRC), &profile, &catalog());
    assert!(out.plans.iter().all(|p| p.operator != "ror"));
    assert_eq!(out.diagnostics.disabled.get("ror"), Some(&2));
}

#[test]
fn test_plan_reports_unmapped_concepts_and_counts_matches() {
    let out = plan_file(&toy_file("toy.src", TOY_SRC), &toy_profile(), &catalog());
    let unmapped: Vec<Concept> = out.diagnostics.unmapped.iter().copied().collect();
    assert_eq!(
        unmapped,
        [
            Concept::Unary,
            Concept::CompoundAssign,
            Concept::Literal,
            Concept::Expr,
            Concept::Name,
            Concept::Index,
            Concept::Block,
            Concept::Arm,
            Concept::FieldInit,
        ]
    );
    assert_eq!(
        out.diagnostics.concept_counts.get(&Concept::Binary),
        Some(&5)
    );
    assert_eq!(
        out.diagnostics.concept_counts.get(&Concept::Statement),
        Some(&9)
    );
}

#[test]
fn test_plan_dedups_identical_actions_at_the_same_stop() {
    // Synthetic: none of the six prototype operators produce coinciding
    // plans (condition negation and constant-false will, once constant
    // replacement exists), and the adapter merges duplicate matches. Two
    // identical Condition matches stand in for that future case.
    let src = "if a { }";
    let file = Toy::new("d.src", src)
        .stmt(StmtKind::If, src, 0)
        .concept(
            Concept::Condition,
            src,
            0,
            &[(Capture::Cond, "a", 0), (Capture::Then, "{ }", 0)],
        )
        .concept(
            Concept::Condition,
            src,
            0,
            &[(Capture::Cond, "a", 0), (Capture::Then, "{ }", 0)],
        )
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(out.plans.len(), 1);
    assert_eq!(out.diagnostics.deduplicated, 1);
}

#[test]
fn test_plan_file_with_syntax_errors_plans_nothing() {
    let mut file = toy_file("toy.src", TOY_SRC);
    file.has_errors = true;
    assert!(
        plan_file(&file, &toy_profile(), &catalog())
            .plans
            .is_empty()
    );
}

// --- Determinism.

#[test]
fn test_plan_output_does_not_depend_on_match_order() {
    let file = toy_file("toy.src", TOY_SRC);
    let mut shuffled = file.clone();
    shuffled.matches.reverse();
    shuffled.statements.reverse();
    let a = plan_file(&file, &toy_profile(), &catalog());
    let b = plan_file(&shuffled, &toy_profile(), &catalog());
    assert_eq!(a, b);
}

#[test]
fn test_diagnostics_merge_adds_counts() {
    let one = plan_file(&toy_file("toy.src", TOY_SRC), &toy_profile(), &catalog()).diagnostics;
    let mut two = one.clone();
    two.merge(&one);
    assert_eq!(two.concept_counts.get(&Concept::Binary), Some(&10));
    assert!(two.skipped.is_empty() && one.skipped.is_empty());
    assert_eq!(two.unmapped, one.unmapped);
    assert_eq!(two.planned, 2 * one.planned);
}

#[test]
fn test_diagnostics_merge_adds_dedup_and_planned_counts() {
    let a = Diagnostics {
        deduplicated: 2,
        planned: 5,
        ..Diagnostics::default()
    };
    let mut b = Diagnostics {
        deduplicated: 3,
        planned: 7,
        ..Diagnostics::default()
    };
    b.merge(&a);
    assert_eq!((b.deduplicated, b.planned), (5, 12));
}

#[test]
fn test_site_is_the_describe_prefix_up_to_the_operator() {
    for p in plans_of(TOY_SRC) {
        let site = p.site();
        assert!(
            p.describe().starts_with(&format!("{site} ")),
            "{site} / {}",
            p.describe()
        );
        assert!(site.ends_with(p.operator));
    }
}
