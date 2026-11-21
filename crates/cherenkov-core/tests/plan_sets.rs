//! Several operator sets at once: their mutants are one list, deduplicated by
//! what they mutate (same span, same replacement), each remembering the sets
//! that produced it.

mod common;

use std::collections::HashSet;

use cherenkov_core::operator_set::{OperatorSet, named};
use cherenkov_core::plan::{MutantPlan, plan_file};
use cherenkov_core::plan_sets::plan_file_sets;
use common::{TOY_SRC, toy_file, toy_profile};

fn sets(names: &[&str]) -> Vec<OperatorSet> {
    names.iter().map(|n| named(n).unwrap()).collect()
}

fn planned(names: &[&str]) -> cherenkov_core::plan::PlanOutput {
    plan_file_sets(&toy_file("toy.src", TOY_SRC), &toy_profile(), &sets(names))
}

fn replacement(p: &MutantPlan) -> (cherenkov_core::syntax::Span, String) {
    (
        p.site,
        p.mutated.clone().unwrap_or_else(|| p.edit.describe()),
    )
}

#[test]
fn test_one_set_plans_what_the_set_alone_plans_each_tagged_with_it() {
    let set = named("breadth").unwrap();
    let alone = plan_file(
        &toy_file("toy.src", TOY_SRC),
        &toy_profile(),
        &set.operators(),
    );
    let out = planned(&["breadth"]);
    assert_eq!(out.plans.len(), alone.plans.len());
    assert_eq!(out.diagnostics, alone.diagnostics);
    for (a, b) in out.plans.iter().zip(&alone.plans) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.sets, ["breadth"]);
    }
}

#[test]
fn test_two_sets_plan_each_replacement_once_with_every_set_that_has_it() {
    let both = planned(&["breadth", "sufficient"]);
    let keys: HashSet<_> = both.plans.iter().map(replacement).collect();
    assert_eq!(
        keys.len(),
        both.plans.len(),
        "no span and replacement twice"
    );

    let alone = |name: &str| planned(&[name]).plans;
    let (a, b) = (alone("breadth"), alone("sufficient"));
    let union: HashSet<_> = a.iter().chain(&b).map(replacement).collect();
    assert_eq!(both.plans.len(), union.len());
    assert_eq!(both.diagnostics.planned, union.len());

    let in_a: HashSet<_> = a.iter().map(replacement).collect();
    let in_b: HashSet<_> = b.iter().map(replacement).collect();
    for p in &both.plans {
        let key = replacement(p);
        let mut want = Vec::new();
        if in_a.contains(&key) {
            want.push("breadth");
        }
        if in_b.contains(&key) {
            want.push("sufficient");
        }
        assert_eq!(p.sets, want, "{}", p.describe());
    }
    assert!(
        both.plans.iter().any(|p| p.sets.len() == 2),
        "some mutant is in both"
    );
    assert!(
        both.plans.iter().any(|p| p.sets == ["sufficient"]),
        "some only in the second"
    );
}

#[test]
fn test_set_order_changes_provenance_order_not_the_mutants() {
    let ab: HashSet<_> = planned(&["breadth", "sufficient"])
        .plans
        .iter()
        .map(replacement)
        .collect();
    let ba: HashSet<_> = planned(&["sufficient", "breadth"])
        .plans
        .iter()
        .map(replacement)
        .collect();
    assert_eq!(ab, ba);
}
