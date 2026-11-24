//! Named operator sets (docs/operator-sets.md): a run uses exactly one, and
//! a set is data, so defining a new one is a table, not new planner code.

mod common;

use cherenkov_mutation::concept::Concept;
use cherenkov_mutation::edit::Edit;
use cherenkov_mutation::operator::{MatchCx, Operator, catalog, operator_inputs};
use cherenkov_mutation::operator_set::{OperatorSet, mutations, named, registry};
use cherenkov_mutation::plan::plan_file;
use cherenkov_mutation::token::Token;
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
            &[(cherenkov_mutation::concept::Capture::Op, &spell, 0)],
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
    // LCR (Just's COR) `a == b` / `a != b`; lhs, rhs and constants are pending.
    assert_eq!(swaps("sufficient", "lcr", And), [Eq]);
    assert_eq!(swaps("sufficient", "lcr", Or), [Ne]);
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

use cherenkov_mutation::operator_set::{CustomSet, SetError, custom_set, resolve_sets};
use coded_diagnostic::Report;

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
    assert_eq!(
        err,
        SetError::Unknown {
            name: "nope".to_string()
        }
    );
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
    assert_eq!(sets[1].skips, [cherenkov_mutation::syntax::SkipKind::Test]);
    assert!(!sets[1].operators().is_empty());
}

#[test]
fn test_a_custom_set_must_name_known_mutations_and_skips() {
    let bad_mutation = custom_set(custom(&["ror/zzz"], &[])).err().unwrap();
    assert_eq!(
        bad_mutation,
        SetError::UnknownMutation {
            name: "ror/zzz".to_string()
        }
    );
    let bad_skip = custom_set(custom(&[], &["moon"])).err().unwrap();
    assert_eq!(
        bad_skip,
        SetError::UnknownSkip {
            name: "moon".to_string()
        }
    );
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
            detail: "denied".into()
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
            cause: Report::of(&SetError::UnknownMutation {
                name: "ror/zzz".into()
            })
        }
    );
}

// --- Operators are families; a set gives each family a table.

use cherenkov_mutation::operator_set::{Relation, families, mutation_named};

const FAMILIES: [&str; 15] = [
    "ror",
    "aor",
    "bitwise",
    "shift",
    "lcr",
    "compound-assign",
    "unary",
    "match-guard",
    "match-arm",
    "struct-field",
    "fn-value",
    "delete-statement",
    "negate-condition",
    "return-bool-flip",
    "body-default",
];

#[test]
fn test_the_operator_count_is_the_family_count_not_the_replacement_count() {
    let mut got: Vec<&str> = families().iter().map(|f| f.id).collect();
    got.sort();
    let mut want = FAMILIES.to_vec();
    want.sort();
    assert_eq!(got, want);
    assert!(mutations().len() > families().len() * 3);
}

#[test]
fn test_every_replacement_belongs_to_a_family_and_is_named_family_slash_detail() {
    let ids: Vec<&str> = families().iter().map(|f| f.id).collect();
    for m in mutations() {
        assert!(ids.contains(&m.family), "{}", m.name);
        assert!(
            m.name == m.family || m.name.starts_with(&format!("{}/", m.family)),
            "{} is not named after {}",
            m.name,
            m.family
        );
    }
}

#[test]
fn test_compound_assignment_is_one_family_of_its_own() {
    for m in mutations() {
        let assign = matches!(
            m.rule,
            cherenkov_mutation::operator_set::Rule::ReplaceToken { from, .. }
                if from.name().ends_with("_assign")
        );
        assert_eq!(assign, m.family == "compound-assign", "{}", m.name);
    }
    for name in [
        "compound-assign/add->sub",
        "compound-assign/shl->shr",
        "compound-assign/and->or",
    ] {
        assert!(mutation_named(name).is_some(), "{name}");
    }
    let f = families();
    let table = &f.iter().find(|f| f.id == "compound-assign").unwrap();
    assert_eq!(table.replacements.len(), 16);
}

#[test]
fn test_lcr_holds_the_cargo_mutants_and_the_just_replacements_with_their_origins() {
    assert!(mutation_named("cor/and->eq").is_none());
    assert!(mutation_named("cor/or->ne").is_none());
    let derived = |name: &str, source: &str| {
        mutation_named(name)
            .unwrap_or_else(|| panic!("{name}"))
            .origins
            .iter()
            .any(|o| o.relation == Relation::Derived && o.source.contains(source))
    };
    assert!(derived("lcr/and->or", "cargo-mutants"));
    assert!(derived("lcr/or->and", "cargo-mutants"));
    assert!(derived("lcr/and->eq", "Just & Schweiggert 2014"));
    assert!(derived("lcr/or->ne", "Just & Schweiggert 2014"));
}

#[test]
fn test_a_set_lists_its_operators_each_with_its_own_table() {
    let table = |set: &str, family: &str| -> Vec<&'static str> {
        named(set)
            .unwrap()
            .tables()
            .into_iter()
            .find(|t| t.family == family)
            .map(|t| t.replacements.iter().map(|m| m.name).collect())
            .unwrap_or_default()
    };
    assert_eq!(
        table("cargo-mutants", "lcr"),
        ["lcr/and->or", "lcr/or->and"]
    );
    assert_eq!(table("sufficient", "lcr"), ["lcr/and->eq", "lcr/or->ne"]);
    assert_eq!(table("breadth", "lcr"), Vec::<&str>::new());
    assert_eq!(
        table("sufficient", "compound-assign"),
        [
            "compound-assign/add->sub",
            "compound-assign/sub->add",
            "compound-assign/mul->div",
            "compound-assign/div->mul",
            "compound-assign/rem->div",
            "compound-assign/shl->shr",
            "compound-assign/shr->shl",
            "compound-assign/and->or",
            "compound-assign/or->and",
            "compound-assign/xor->and",
        ]
    );
}

#[test]
fn test_set_tables_cover_exactly_the_sets_mutations_once_each() {
    for set in registry() {
        let tables = set.tables();
        let mut ids: Vec<&str> = tables.iter().map(|t| t.family).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "{}: a family twice", set.name);
        let mut listed: Vec<&str> = tables
            .iter()
            .flat_map(|t| t.replacements.iter().map(|m| m.name))
            .collect();
        let mut want: Vec<&str> = set.mutations.to_vec();
        listed.sort();
        want.sort();
        assert_eq!(listed, want, "{}", set.name);
        let ops: Vec<&str> = set.operators().iter().map(|o| o.id()).collect();
        assert_eq!(ops, tables.iter().map(|t| t.family).collect::<Vec<_>>());
    }
}

#[test]
fn test_every_replacement_has_a_derived_origin() {
    for m in mutations() {
        assert!(
            m.origins.iter().any(|o| o.relation == Relation::Derived),
            "{}",
            m.name
        );
    }
}

/// The comment block above `name`'s entry in the source of the superset.
fn comment_above(source: &str, name: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .position(|l| {
            l.trim() == format!("\"{name}\",") || l.trim() == format!("name: \"{name}\",")
        })
        .unwrap_or_else(|| panic!("no entry {name}"));
    // The entry opens one line above its name: `tok(` or `Mutation {`.
    let mut top = at - 1;
    let mut block = Vec::new();
    while top > 0 && lines[top - 1].trim_start().starts_with("//") {
        top -= 1;
        block.push(lines[top].trim());
    }
    block.join(" ")
}

#[test]
fn test_every_replacements_comment_names_each_source_it_is_derived_from() {
    let source = include_str!("../src/operator_set.rs");
    for m in mutations() {
        let comment = comment_above(source, m.name);
        for o in m.origins {
            let relation = match o.relation {
                Relation::Derived => "derived from",
                Relation::Same => "same as",
                Relation::Similar(_) => "similar to",
            };
            let want = format!("{relation} {}", o.source);
            assert!(
                comment.contains(&want),
                "{}: comment lacks `{want}`: {comment}",
                m.name
            );
        }
    }
}

/// The row of a docs table that starts with `family` in backticks.
fn row_of(doc: &str, family: &str) -> String {
    doc.lines()
        .find(|l| l.starts_with(&format!("| `{family}`")))
        .unwrap_or_else(|| panic!("no row for {family}"))
        .to_string()
}

#[test]
fn test_the_docs_name_every_familys_derived_sources() {
    for path in [
        "../../../../docs/mutation-operators.md",
        "../../../../docs/rust-operator-evaluation.md",
    ] {
        let doc =
            std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        for f in families() {
            let row = row_of(&doc, f.id);
            for m in &f.replacements {
                for o in m.origins.iter().filter(|o| o.relation == Relation::Derived) {
                    assert!(
                        row.contains(o.source),
                        "{path}: {} row lacks {}",
                        f.id,
                        o.source
                    );
                }
            }
        }
    }
}

#[test]
fn test_every_family_has_a_definition_and_the_inventory_holds_every_replacement() {
    let all = families();
    assert!(
        all.iter()
            .all(|f| !f.definition.is_empty() && !f.replacements.is_empty())
    );
    let total: usize = all.iter().map(|f| f.replacements.len()).sum();
    assert_eq!(total, mutations().len());
}

#[test]
fn test_the_default_set_is_cargo_mutants_for_rust_and_breadth_for_the_other_languages() {
    use cherenkov_mutation::operator_set::default_set_name;
    assert_eq!(default_set_name("rust"), "cargo-mutants");
    assert_eq!(default_set_name("go"), "breadth");
    assert_eq!(default_set_name("python"), "breadth");
    for lang in ["rust", "go", "python"] {
        assert!(named(default_set_name(lang)).is_some(), "{lang}");
    }
}
