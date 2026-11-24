//! Planning through a real parse, for semantics the toy language cannot
//! show: they depend on a real grammar's shape.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{Action, MutantPlan, Target, plan_file};
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

fn plans(lang: &str, src: &str) -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    plan_file(&file, &profile, &catalog()).plans
}

fn branch_targets(src: &str, p: &MutantPlan) -> (String, String) {
    let Action::EvalBranch {
        if_true, if_false, ..
    } = &p.action
    else {
        panic!("{}", p.describe())
    };
    let show = |t: &Target| match t {
        Target::BlockStart(s) => format!("block {:?}", s.text(src)),
        Target::AfterStatement(s) => format!("after {:?}", s.text(src)),
    };
    (show(if_true), show(if_false))
}

#[test]
fn test_python_while_else_false_target_is_the_else_clause() {
    // Python runs a loop's else clause when the condition is false.
    let src = "while i < n:\n    step()\nelse:\n    done()\n";
    let ps = plans("python", src);
    let neg = ps
        .iter()
        .find(|p| p.operator == "negate-condition")
        .unwrap();
    let (t, f) = branch_targets(src, neg);
    assert_eq!(t, r#"block "step()""#);
    assert_eq!(f, "block \"else:\\n    done()\"");
}

#[test]
fn test_python_if_elif_false_target_is_the_elif_clause() {
    let src = "if a < b:\n    x()\nelif c:\n    y()\n";
    let ps = plans("python", src);
    let ror = ps.iter().find(|p| p.operator == "ror").unwrap();
    let (_, f) = branch_targets(src, ror);
    assert_eq!(f, "block \"elif c:\\n    y()\"");
}

#[test]
fn test_go_else_if_false_target_is_the_nested_if() {
    let src = "package p\nfunc f() {\n\tif a < b {\n\t\tx()\n\t} else if c {\n\t\ty()\n\t}\n}\n";
    let ps = plans("go", src);
    let ror = ps.iter().find(|p| p.operator == "ror").unwrap();
    let (t, f) = branch_targets(src, ror);
    assert_eq!(t, "block \"{\\n\\t\\tx()\\n\\t}\"");
    assert_eq!(f, "block \"if c {\\n\\t\\ty()\\n\\t}\"");
}

#[test]
fn test_rust_while_false_target_is_after_the_loop() {
    let src = "fn f() {\n    while i < n { i += 1; }\n    g();\n}\n";
    let ps = plans("rust", src);
    let neg = ps
        .iter()
        .find(|p| p.operator == "negate-condition")
        .unwrap();
    let (_, f) = branch_targets(src, neg);
    assert_eq!(f, r#"after "while i < n { i += 1; }""#);
}

// --- Token swaps in compound assignments and return values get statement
// actions (cargo-mutants set), not in-place ones.

fn cargo_plans(src: &str) -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    let ops = cherenkov_mutation::operator_set::named("cargo-mutants")
        .unwrap()
        .operators();
    plan_file(&file, &profile, &ops).plans
}

fn action_of(src: &str, mutation: &str) -> (Action, Vec<&'static str>) {
    let p = cargo_plans(src)
        .into_iter()
        .find(|p| p.mutation == Some(mutation))
        .unwrap_or_else(|| panic!("no {mutation}"));
    (p.action.clone(), p.requires.names())
}

#[test]
fn test_rust_compound_assignment_swap_evaluates_the_mutated_statement() {
    let src = "fn f(mut t: i64, k: i64) -> i64 {\n    t += k;\n    t\n}\n";
    let (action, requires) = action_of(src, "compound-assign/add->sub");
    let Action::EvalThenJump {
        text,
        target,
        value,
        resume,
    } = action
    else {
        panic!("{action:?}")
    };
    assert_eq!(text, "t -= k");
    assert_eq!((target.as_str(), value.as_str()), ("t", "t - k"));
    assert!(matches!(resume, Target::AfterStatement(_)));
    assert_eq!(requires, ["assign"]);
}

#[test]
fn test_rust_tail_expression_swap_evaluates_and_returns_the_mutated_value() {
    let src = "fn f(a: i64, b: i64) -> i64 {\n    let s = a;\n    s * b + 1\n}\n";
    let (action, requires) = action_of(src, "aor/mul->div");
    assert_eq!(
        action,
        Action::EvalReturn {
            value_text: "s / b + 1".to_string()
        }
    );
    assert_eq!(requires, ["force-return"]);
}

#[test]
fn test_rust_return_expression_swap_evaluates_and_returns_the_mutated_value() {
    let src = "fn f(a: i64, b: i64) -> i64 {\n    return a + b;\n}\n";
    let (action, _) = action_of(src, "aor/add->sub");
    assert_eq!(
        action,
        Action::EvalReturn {
            value_text: "a - b".to_string()
        }
    );
}

#[test]
fn test_rust_swap_in_a_call_argument_of_a_tail_returns_the_mutated_call() {
    let src = "fn g(x: i64) -> i64 { x }\nfn f(a: i64, b: i64) -> i64 {\n    g(a + b)\n}\n";
    let (action, requires) = action_of(src, "aor/add->sub");
    assert_eq!(
        action,
        Action::EvalReturn {
            value_text: "g(a - b)".to_string()
        }
    );
    assert!(requires.contains(&"force-return"), "{requires:?}");
}

#[test]
fn test_rust_last_statement_or_comment_is_not_a_return_value() {
    let src = "fn f(v: &mut i64, k: i64) {\n    *v = k + 1;\n    // done\n}\n";
    let (action, _) = action_of(src, "aor/add->sub");
    assert!(
        matches!(action, Action::EvalThenJump { .. } | Action::InPlace { .. }),
        "{action:?}"
    );
    assert!(!matches!(action, Action::EvalReturn { .. }));
}

#[test]
fn test_rust_swap_inside_a_compound_assignments_value_keeps_its_operator() {
    let src = "fn f(mut t: i64, a: i64, b: i64) -> i64 {\n    t += a * b;\n    t\n}\n";
    let (action, _) = action_of(src, "aor/mul->div");
    let Action::EvalThenJump { target, value, .. } = action else {
        panic!("{action:?}")
    };
    assert_eq!((target.as_str(), value.as_str()), ("t", "t + a / b"));
}

#[test]
fn test_rust_swap_inside_a_closure_body_returns_from_the_closure() {
    // The closure is its own function; the statement holding it is not the
    // swap's context.
    let src = "fn f(v: &[i64]) -> usize {\n    let n = v.iter().filter(|x| **x > 1 && **x < 9).count();\n    n\n}\n";
    let (action, _) = action_of(src, "lcr/and->or");
    assert_eq!(
        action,
        Action::EvalReturn {
            value_text: "**x > 1 || **x < 9".to_string()
        }
    );
}

#[test]
fn test_rust_body_replacement_of_an_async_fn_says_so_through_any_attribute() {
    let src = "#[some::wrapper]\npub async fn f() -> u8 { 1 }\nfn g() -> u8 { 2 }\n";
    let async_fn = |name: &str| {
        plans("rust", src)
            .into_iter()
            .filter(|p| p.function.as_deref() == Some(name))
            .find_map(|p| match p.action {
                Action::ReturnDefault { async_fn, .. } => Some(async_fn),
                _ => None,
            })
    };
    assert_eq!(async_fn("f"), Some(true));
    assert_eq!(async_fn("g"), Some(false));
}

#[test]
fn test_a_plan_carries_the_source_it_replaces_and_the_source_it_puts_there() {
    // A reader of a verdict sees the change without the source at hand:
    // the site's text, and what the mutant has there instead.
    let src =
        "fn f(n: i32) -> bool {\n    if n < 10 {\n        return true;\n    }\n    false\n}\n";
    let ps = plans("rust", src);
    let change = |op: &str| {
        let p = ps.iter().find(|p| p.operator == op).unwrap();
        (p.original.clone(), p.mutated.clone())
    };
    assert_eq!(change("ror"), ("<".to_string(), Some("<=".to_string())));
    assert_eq!(
        change("return-bool-flip"),
        ("true".to_string(), Some("false".to_string()))
    );
    let body = ps.iter().find(|p| p.operator == "body-default").unwrap();
    assert!(body.original.contains("if n < 10"), "{}", body.original);
    assert_eq!(
        body.mutated.as_deref(),
        Some("false"),
        "{}",
        body.describe()
    );
}

#[test]
fn test_a_plan_s_listing_is_its_site_then_its_change() {
    let src = "fn f(n: i32) -> bool {\n    n < 10\n}\n";
    for p in plans("rust", src) {
        assert_eq!(p.describe(), format!("{} {}", p.site(), p.change()));
    }
}
