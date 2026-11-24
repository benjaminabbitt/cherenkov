//! Function body -> value (cargo-mutants' FnValue): one mutant per value the
//! profile's generator gives for the result type.

mod common;

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::operator_set::OperatorSet;
use cherenkov_mutation::plan::{Action, MutantPlan, plan_file, render_source};
use cherenkov_mutation::profile::{Profile, ReturnValues};
use common::{Toy, at, toy_profile};

/// The toy language's generator: booleans, and nothing else.
fn toy_values(ret: Option<&str>, _errs: &[&str]) -> Vec<String> {
    match ret {
        Some("bool") => vec!["no".into(), "yes".into()],
        None => vec!["()".into()],
        _ => vec![],
    }
}

fn profile() -> Profile {
    Profile {
        return_values: Some(ReturnValues(toy_values)),
        body_braces: true,
        ..toy_profile()
    }
}

const FN_VALUE: OperatorSet = OperatorSet {
    name: "fn-value",
    summary: "",
    source: "",
    mutations: &["fn-value"],
    pending: &[],
    skips: &[],
};

fn plans(src: &str, returns: Option<&str>) -> Vec<MutantPlan> {
    let body = &src[src.find('{').unwrap()..src.rfind('}').unwrap() + 1];
    let mut caps = vec![(Capture::Name, "f", 1), (Capture::Body, body, 0)];
    if let Some(r) = returns {
        caps.push((Capture::Returns, r, 0));
    }
    let file = Toy::new("v.src", src)
        .concept(Concept::Function, src.trim_end(), 0, &caps)
        .build();
    plan_file(&file, &profile(), &FN_VALUE.operators()).plans
}

#[test]
fn test_fn_value_plans_one_mutant_per_generated_value_at_the_body_contents() {
    let src = "fn f() bool {\n    x := 1;\n    yes\n}\n";
    let ps = plans(src, Some("bool"));
    let values: Vec<Option<String>> = ps
        .iter()
        .map(|p| match &p.action {
            Action::ReturnDefault { value_text, .. } => value_text.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(values, [Some("no".into()), Some("yes".into())]);
    // The site is the contents between the braces, as cargo-mutants spans it.
    let inner = at(src, "x := 1;\n    yes", 0);
    assert!(ps.iter().all(|p| p.site == inner), "{:?}", ps[0].site);
    assert_ne!(ps[0].id, ps[1].id);
}

#[test]
fn test_fn_value_marks_only_the_profiles_zero_as_zero_bits() {
    let ps = plans("fn f() bool {\n    yes\n}\n", Some("bool"));
    let zero: Vec<bool> = ps
        .iter()
        .map(|p| match &p.action {
            Action::ReturnDefault { zero_bits, .. } => *zero_bits,
            _ => unreachable!(),
        })
        .collect();
    // toy profile: the default for bool is "no".
    assert_eq!(zero, [true]);
}

#[test]
fn test_fn_value_skips_a_value_identical_to_the_body() {
    // The body already is `yes`: only `no` is a mutant.
    let ps = plans("fn f() bool {\n    yes\n}\n", Some("bool"));
    assert_eq!(ps.len(), 1);
}

#[test]
fn test_fn_value_skips_empty_bodies_and_unknown_types() {
    assert!(plans("fn f() bool { }\n", Some("bool")).is_empty());
    assert!(plans("fn f() Widget {\n    w\n}\n", Some("Widget")).is_empty());
}

#[test]
fn test_fn_value_on_a_function_without_result_type_uses_the_generators_unit_value() {
    let ps = plans("fn f() {\n    log(x);\n}\n", None);
    assert_eq!(ps.len(), 1);
    assert!(
        matches!(&ps[0].action, Action::ReturnDefault { value_text: Some(v), .. } if v == "()")
    );
}

#[test]
fn test_fn_value_without_a_generator_plans_nothing() {
    let src = "fn f() bool {\n    yes\n}\n";
    let file = Toy::new("v.src", src)
        .concept(
            Concept::Function,
            src.trim_end(),
            0,
            &[
                (Capture::Name, "f", 1),
                (Capture::Returns, "bool", 0),
                (Capture::Body, &src[12..src.len() - 1], 0),
            ],
        )
        .build();
    assert!(
        plan_file(&file, &toy_profile(), &FN_VALUE.operators())
            .plans
            .is_empty()
    );
}

#[test]
fn test_fn_value_renders_the_value_in_place_of_the_body_contents() {
    let src = "fn f() bool {\n    x := 1;\n    yes\n}\n";
    let p = plans(src, Some("bool")).remove(0);
    assert_eq!(
        render_source(src, &p, &profile()).unwrap(),
        "fn f() bool {\n    no\n}\n"
    );
}
