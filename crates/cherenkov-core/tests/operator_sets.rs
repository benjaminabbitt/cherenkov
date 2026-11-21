//! Named operator sets (docs/operator-sets.md): a run uses exactly one, and
//! a set is data, so defining a new one is a table, not new planner code.

mod common;

use cherenkov_core::concept::Concept;
use cherenkov_core::edit::Edit;
use cherenkov_core::operator::{MatchCx, Operator, catalog, operator_inputs};
use cherenkov_core::operator_set::{OperatorSet, mutations, named, registry};
use cherenkov_core::plan::plan_file;
use cherenkov_core::token::Token;
use common::{TOY_SRC, toy_file, toy_profile};

fn swaps(set: &str, operator: &str, from: Token) -> Vec<Token> {
    let set = named(set).unwrap();
    let ops = set.operators();
    let op = ops
        .iter()
        .find(|o| o.id() == operator)
        .unwrap_or_else(|| panic!("{operator} in {}", set.name));
    // One binary site with the `from` token, through the real operator.
    let spell = common::toy_tokens().spell(from).unwrap().to_string();
    let src = format!("a {spell} b");
    let file = common::Toy::new("t", &src)
        .concept(
            Concept::Binary,
            &src,
            0,
            &[(cherenkov_core::concept::Capture::Op, &spell, 0)],
        )
        .build();
    let profile = toy_profile();
    let inputs = operator_inputs(&file);
    let m = inputs
        .iter()
        .find(|m| m.concept == Concept::Binary)
        .unwrap();
    op.mutate(&MatchCx::new(&file, &profile, m))
        .into_iter()
        .map(|e| match e {
            Edit::ReplaceToken { with, .. } => with,
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn test_registry_names_are_unique_and_documented() {
    let sets = registry();
    let mut names: Vec<&str> = sets.iter().map(|s| s.name).collect();
    assert!(
        names.starts_with(&["breadth"]),
        "breadth stays first: the default"
    );
    names.sort();
    names.dedup();
    assert_eq!(names.len(), sets.len());
    for s in &sets {
        assert!(!s.summary.is_empty() && !s.source.is_empty(), "{}", s.name);
    }
    for name in ["breadth", "cargo-mutants", "sufficient"] {
        assert!(named(name).is_some(), "{name}");
    }
    assert!(named("no-such-set").is_none());
}

#[test]
fn test_breadth_set_is_the_prototype_catalog() {
    let ids = |ops: &[Box<dyn Operator>]| ops.iter().map(|o| o.id()).collect::<Vec<_>>();
    assert_eq!(ids(&named("breadth").unwrap().operators()), ids(&catalog()));
    let a = plan_file(&toy_file("toy.src", TOY_SRC), &toy_profile(), &catalog());
    let b = plan_file(
        &toy_file("toy.src", TOY_SRC),
        &toy_profile(),
        &named("breadth").unwrap().operators(),
    );
    assert_eq!(a, b);
}

#[test]
fn test_cargo_mutants_relational_table_matches_its_source() {
    // cargo-mutants 26.2.0 src/visit.rs:576-583
    use Token::*;
    assert_eq!(swaps("cargo-mutants", "ror", Lt), [Eq, Gt, Le]);
    assert_eq!(swaps("cargo-mutants", "ror", Gt), [Eq, Lt, Ge]);
    assert_eq!(swaps("cargo-mutants", "ror", Le), [Gt]);
    assert_eq!(swaps("cargo-mutants", "ror", Ge), [Lt]);
    assert_eq!(swaps("cargo-mutants", "ror", Eq), [Ne]);
    assert_eq!(swaps("cargo-mutants", "ror", Ne), [Eq]);
}

#[test]
fn test_cargo_mutants_arithmetic_bitwise_and_logical_tables_match_its_source() {
    // cargo-mutants 26.2.0 src/visit.rs:578-600
    use Token::*;
    assert_eq!(swaps("cargo-mutants", "aor", Add), [Sub, Mul]);
    assert_eq!(swaps("cargo-mutants", "aor", Sub), [Add, Div]);
    assert_eq!(swaps("cargo-mutants", "aor", Mul), [Add, Div]);
    assert_eq!(swaps("cargo-mutants", "aor", Div), [Rem, Mul]);
    assert_eq!(swaps("cargo-mutants", "aor", Rem), [Div, Add]);
    assert_eq!(swaps("cargo-mutants", "bitwise", BitAnd), [BitOr, BitXor]);
    assert_eq!(swaps("cargo-mutants", "bitwise", BitOr), [BitAnd, BitXor]);
    assert_eq!(swaps("cargo-mutants", "bitwise", BitXor), [BitOr, BitAnd]);
    assert_eq!(swaps("cargo-mutants", "shift", Shl), [Shr]);
    assert_eq!(swaps("cargo-mutants", "shift", Shr), [Shl]);
    assert_eq!(swaps("cargo-mutants", "lcr", And), [Or]);
    assert_eq!(swaps("cargo-mutants", "lcr", Or), [And]);
}

#[test]
fn test_sufficient_token_swaps_are_the_token_part_of_the_sufficient_sets() {
    // Kaminski/Just ROR sets; the `true`/`false` replacements are pending.
    use Token::*;
    assert_eq!(swaps("sufficient", "ror", Lt), [Le, Ne]);
    assert_eq!(swaps("sufficient", "ror", Gt), [Ge, Ne]);
    assert_eq!(swaps("sufficient", "ror", Le), [Lt, Eq]);
    assert_eq!(swaps("sufficient", "ror", Ge), [Gt, Eq]);
    assert_eq!(swaps("sufficient", "ror", Eq), [Le, Ge]);
    assert_eq!(swaps("sufficient", "ror", Ne), [Lt, Gt]);
    // COR's `a == b` / `a != b`; lhs, rhs and constants are pending.
    assert_eq!(swaps("sufficient", "cor", And), [Eq]);
    assert_eq!(swaps("sufficient", "cor", Or), [Ne]);
    // AOR counterparts, `%` -> `/` [choice].
    assert_eq!(swaps("sufficient", "aor", Add), [Sub]);
    assert_eq!(swaps("sufficient", "aor", Mul), [Div]);
    assert_eq!(swaps("sufficient", "aor", Rem), [Div]);
}

#[test]
fn test_every_set_lists_the_spec_rules_it_does_not_implement_yet() {
    for name in ["cargo-mutants", "sufficient"] {
        let set = named(name).unwrap();
        assert!(
            !set.pending.is_empty(),
            "{name} is not complete yet and must say so"
        );
        let mut p = set.pending.to_vec();
        p.sort();
        p.dedup();
        assert_eq!(
            p.len(),
            set.pending.len(),
            "{name}: duplicate pending entries"
        );
    }
    assert!(named("breadth").unwrap().pending.is_empty());
}

#[test]
fn test_a_new_set_is_a_list_of_named_mutations() {
    // A boundary-only ROR set (mutmut / go-mutesting style), from the superset.
    let custom = OperatorSet {
        name: "boundary-only",
        summary: "mutmut/go-mutesting style ROR",
        source: "test",
        mutations: &["ror/lt->le", "ror/gt->ge"],
        pending: &[],
        skips: &[],
    };
    let plans = plan_file(
        &toy_file("toy.src", TOY_SRC),
        &toy_profile(),
        &custom.operators(),
    )
    .plans;
    let listing: Vec<String> = plans.iter().map(|p| p.describe()).collect();
    assert_eq!(
        listing,
        [
            "toy.src:4:8 f ror eval-branch \"a <= b\"",
            "toy.src:5:11 f ror eval-branch \"a >= b\""
        ]
    );
}

#[test]
fn test_superset_names_are_unique_and_every_set_draws_from_it() {
    let mut names: Vec<&str> = mutations().iter().map(|m| m.name).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "duplicate mutation names in the superset");
    for set in registry() {
        for name in set.mutations {
            assert!(
                names.binary_search(name).is_ok(),
                "{}: {name} is not in the superset",
                set.name
            );
        }
    }
}

#[test]
fn test_sets_share_mutations_instead_of_reimplementing_them() {
    // `<` -> `<=` is one named mutation used by three sets.
    for set in ["breadth", "cargo-mutants", "sufficient"] {
        assert!(
            named(set).unwrap().mutations.contains(&"ror/lt->le"),
            "{set}"
        );
    }
}

#[test]
#[should_panic(expected = "no mutation named")]
fn test_a_set_naming_an_unknown_mutation_is_a_programming_error() {
    let bad = OperatorSet {
        name: "bad",
        summary: "",
        source: "",
        mutations: &["ror/lt->zz"],
        pending: &[],
        skips: &[],
    };
    let _ = bad.operators();
}

#[test]
fn test_token_names_cover_the_new_operators() {
    for t in [
        Token::Rem,
        Token::BitAnd,
        Token::BitOr,
        Token::BitXor,
        Token::Shl,
        Token::Shr,
    ] {
        assert_eq!(Token::from_name(t.name()), Some(t));
    }
}

// --- Asking for several sets: `a,b,custom.toml`.

use cherenkov_core::operator_set::{CustomSet, SetError, custom_set, resolve_sets};

fn custom(mutations: &[&str], skips: &[&str]) -> CustomSet {
    CustomSet {
        name: "mine".to_string(),
        summary: None,
        source: None,
        mutations: mutations.iter().map(|m| m.to_string()).collect(),
        skips: skips.iter().map(|m| m.to_string()).collect(),
    }
}

#[test]
fn test_resolve_sets_takes_a_comma_list_in_order_without_repeats() {
    let sets = resolve_sets("sufficient, breadth,sufficient", |_| unreachable!()).unwrap();
    let names: Vec<&str> = sets.iter().map(|s| s.name).collect();
    assert_eq!(names, ["sufficient", "breadth"]);
}

#[test]
fn test_resolve_sets_names_the_unknown_one() {
    let err = resolve_sets("breadth,nope", |_| unreachable!())
        .err()
        .unwrap();
    assert_eq!(err, SetError::Unknown("nope".to_string()));
}

#[test]
fn test_a_toml_item_is_a_custom_set_read_through_the_loader() {
    let sets = resolve_sets("breadth,mine.toml", |path| {
        assert_eq!(path, "mine.toml");
        Ok(custom(&["ror/lt->le"], &["test"]))
    })
    .unwrap();
    assert_eq!(sets[1].name, "mine");
    assert_eq!(sets[1].mutations, ["ror/lt->le"]);
    assert_eq!(sets[1].skips, [cherenkov_core::syntax::SkipKind::Test]);
    assert!(!sets[1].operators().is_empty());
}

#[test]
fn test_a_custom_set_must_name_known_mutations_and_skips() {
    let bad_mutation = custom_set(custom(&["ror/zzz"], &[])).err().unwrap();
    assert_eq!(
        bad_mutation,
        SetError::UnknownMutation("ror/zzz".to_string())
    );
    let bad_skip = custom_set(custom(&[], &["moon"])).err().unwrap();
    assert_eq!(bad_skip, SetError::UnknownSkip("moon".to_string()));
}

#[test]
fn test_an_unreadable_toml_is_reported_with_its_path() {
    let err = resolve_sets("a.toml", |_| Err("denied".to_string()))
        .err()
        .unwrap();
    assert_eq!(
        err,
        SetError::Unreadable {
            path: "a.toml".into(),
            reason: "denied".into()
        }
    );
}

#[test]
fn test_an_invalid_custom_set_is_reported_with_its_path_and_reason() {
    let err = resolve_sets("m.toml", |_| Ok(custom(&["ror/zzz"], &[])))
        .err()
        .unwrap();
    assert_eq!(
        err,
        SetError::Invalid {
            path: "m.toml".into(),
            reason: "no mutation named ror/zzz".into()
        }
    );
}
