use std::path::Path;

use cherenkov_mutation::operator::Operator;
use cherenkov_mutation::operator_set::registry;
use std::sync::Arc;

use cherenkov_fs::FsSourceReader;
use cherenkov_mutation::concept::Capture as CoreCapture;
use cherenkov_mutation::plan::{MutantPlan as CorePlan, plan_file};
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::session::plan_project;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_mutation::token::Token as CoreToken;
use cherenkov_syntax::TreeSitterSyntax;

use super::*;

/// A program with a site for every kind of action and edit the fixtures do
/// not have: match arms, a guard, a field, a compound assignment, a unary
/// operator, an async function, a statement.
const VARIED: &str = "\
struct P { a: i32, b: i32 }
fn f(a: i32, b: i32, c: bool, d: bool) -> i32 {
    let mut t = 0;
    if a < b { t = 1; }
    if c && d { t = 2; }
    if !c { t = 3; }
    t = -a;
    t = a + b;
    t += a;
    t <<= 1;
    match a { 1 => t = 1, 2 if c => t = 2, _ => t = 3 }
    let p = P { a: 1, b: 2 };
    g(t + 1);
    return t + p.a;
}
async fn h() -> bool { true }
fn g(x: i32) { println!(\"{x}\"); }
";

fn varied_plans(operators: &[Box<dyn Operator>]) -> Vec<CorePlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let file = syntax
        .analyze(&rust, Path::new("src/varied.rs"), Arc::from(VARIED))
        .unwrap();
    plan_file(&file, &profile, operators).plans
}

/// Every plan the planner makes for the fixtures, in every language, and
/// for [`VARIED`], with the operators of every set: the variety the run
/// layer has to carry.
fn fixture_plans() -> Vec<CorePlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let profiles = syntax.profiles();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../fixtures");
    registry()
        .iter()
        .flat_map(|set| {
            let operators = set.operators();
            let fixtures = ["rust", "go", "python"].iter().flat_map(|language| {
                let reader = FsSourceReader::new(root.join(language), &profiles).unwrap();
                plan_project(&reader, &syntax, &profiles, &operators)
                    .unwrap()
                    .plans
            });
            fixtures.chain(varied_plans(&operators)).collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn test_every_fixture_plan_goes_through_proto_and_comes_back_equal() {
    let plans = fixture_plans();
    assert!(
        plans.len() > 150,
        "the fixtures plan {} mutants",
        plans.len()
    );
    for plan in plans {
        let wire: cherenkov_run_wire::mutant::MutantPlan = encode(&plan).into();
        let decoded = decode(MutantPlan::try_from(wire).unwrap()).unwrap();
        assert_eq!(decoded, plan, "{}", plan.describe());
    }
}

#[test]
fn test_the_fixtures_exercise_every_kind_of_action_and_edit() {
    let plans = fixture_plans();
    let actions: std::collections::BTreeSet<_> = plans.iter().map(|p| p.action.kind()).collect();
    let edits: std::collections::BTreeSet<_> = plans
        .iter()
        .map(|p| p.edit.describe().split(' ').next().unwrap().to_string())
        .collect();
    assert_eq!(actions.len(), 7, "{actions:?}");
    assert_eq!(edits.len(), 6, "{edits:?}");
}

#[test]
fn test_the_protos_number_the_planners_tokens_and_captures_in_its_order() {
    for (index, t) in CoreToken::ALL.iter().enumerate() {
        let number = i32::try_from(index + 1).unwrap();
        let named = cherenkov_run_wire::mutant::Token::try_from(number).unwrap();
        assert_eq!(
            named.as_str_name(),
            format!("TOKEN_{}", t.name().to_uppercase()),
            "token {number}"
        );
        assert_eq!(untoken(token(*t)), *t);
    }
    for (index, c) in CoreCapture::ALL.iter().enumerate() {
        let number = i32::try_from(index + 1).unwrap();
        let named = cherenkov_run_wire::mutant::Capture::try_from(number).unwrap();
        assert_eq!(
            named.as_str_name(),
            format!("CAPTURE_{}", c.name().to_uppercase()),
            "capture {number}"
        );
        assert_eq!(uncapture(capture(*c)), *c);
    }
}

#[test]
fn test_a_plan_with_no_action_is_refused_by_the_field() {
    let plan = fixture_plans().remove(0);
    let mut wire = encode(&plan);
    wire.action = None;
    assert_eq!(decode(wire), Err(Unreadable::Missing { field: "action" }));
}

#[test]
fn test_a_plan_of_an_operator_the_catalog_lacks_is_refused_by_the_name() {
    let plan = fixture_plans().remove(0);
    let mut wire = encode(&plan);
    wire.operator = "frobnicate".to_string();
    assert_eq!(
        decode(wire),
        Err(Unreadable::Unknown {
            kind: "operator",
            name: "frobnicate".to_string()
        })
    );
}

#[test]
fn test_a_plan_of_a_mutation_the_catalog_lacks_is_refused_by_the_name() {
    let plan = fixture_plans().remove(0);
    let mut wire = encode(&plan);
    wire.mutation = Some("frobnicate".to_string());
    assert_eq!(
        decode(wire),
        Err(Unreadable::Unknown {
            kind: "mutation",
            name: "frobnicate".to_string()
        })
    );
}
