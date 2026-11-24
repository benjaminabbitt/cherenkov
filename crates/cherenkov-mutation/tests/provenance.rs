//! Every named mutation says where its definition came from, and which peer
//! tools have the same or a similar mutation, under their own names.

mod common;

use cherenkov_mutation::operator_set::{Relation, mutation_named, mutations, named};
use cherenkov_mutation::plan::plan_file;
use common::{TOY_SRC, toy_file, toy_profile};

#[test]
fn test_every_mutation_is_derived_from_a_cited_source() {
    for m in mutations() {
        let derived: Vec<_> = m
            .origins
            .iter()
            .filter(|o| o.relation == Relation::Derived)
            .collect();
        assert!(!derived.is_empty(), "{} has no Derived origin", m.name);
        for o in m.origins {
            assert!(!o.source.is_empty(), "{}: empty origin", m.name);
        }
    }
}

#[test]
fn test_every_cargo_mutants_mutation_is_marked_as_derived_from_cargo_mutants() {
    let set = named("cargo-mutants").unwrap();
    for name in set.mutations {
        let m = mutation_named(name).unwrap();
        let cited = m
            .origins
            .iter()
            .any(|o| o.relation == Relation::Derived && o.source == "cargo-mutants");
        assert!(
            cited,
            "{name} is in the cargo-mutants set without a cargo-mutants origin"
        );
    }
}

#[test]
fn test_peers_with_the_same_mutation_are_recorded_under_their_own_names() {
    let m = mutation_named("ror/lt->le").unwrap();
    let pit = m
        .origins
        .iter()
        .find(|o| o.source == "PIT")
        .expect("PIT peer");
    assert_eq!(pit.relation, Relation::Same);
    assert_eq!(pit.name, Some("CONDITIONALS_BOUNDARY"));
}

#[test]
fn test_plans_name_the_mutation_that_made_them() {
    let set = named("breadth").unwrap();
    let plans = plan_file(
        &toy_file("toy.src", TOY_SRC),
        &toy_profile(),
        &set.operators(),
    )
    .plans;
    for p in &plans {
        let name = p
            .mutation
            .unwrap_or_else(|| panic!("{} has no mutation name", p.describe()));
        assert!(set.mutations.contains(&name), "{name} not in the set");
    }
    let ror = plans
        .iter()
        .find(|p| p.operator == "ror" && p.stop_at.line == 4)
        .unwrap();
    assert_eq!(ror.mutation, Some("ror/lt->le"));
}
