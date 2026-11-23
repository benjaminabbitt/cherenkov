//! The operator inventory: what is exported for documentation (a dump of
//! the operators, their replacements, sources, names and spellings).

mod common;

use cherenkov_core::inventory::{Inventory, Spellings};
use cherenkov_core::operator_set::{families, mutations, named, registry};
use cherenkov_core::set_spec::{CustomSet, custom_set};

fn without_spellings() -> Spellings<'static> {
    Spellings::none()
}

#[test]
fn test_the_full_inventory_has_every_operator_and_every_replacement_once() {
    let inv = Inventory::of_all(&without_spellings());
    assert_eq!(inv.operators.len(), families().len());
    let n: usize = inv.operators.iter().map(|o| o.replacements.len()).sum();
    assert_eq!(n, mutations().len());
    assert!(inv.set.is_none());
}

#[test]
fn test_a_sets_view_has_only_its_families_and_its_tables() {
    for set in registry() {
        let inv = Inventory::of_set(&set, &without_spellings());
        let want: Vec<(&str, Vec<&str>)> = set
            .tables()
            .into_iter()
            .map(|t| (t.family, t.replacements.iter().map(|m| m.name).collect()))
            .collect();
        let got: Vec<(&str, Vec<&str>)> = inv
            .operators
            .iter()
            .map(|o| {
                (
                    o.name,
                    o.replacements.iter().map(|r| r.name).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(got, want, "{}", set.name);
        assert_eq!(inv.set.as_ref().unwrap().name, set.name);
    }
}

#[test]
fn test_each_replacement_says_which_sets_use_it() {
    let inv = Inventory::of_all(&without_spellings());
    let lt_le = inv
        .operators
        .iter()
        .flat_map(|o| &o.replacements)
        .find(|r| r.name == "ror/lt->le")
        .unwrap();
    assert_eq!(lt_le.sets, ["breadth", "cargo-mutants", "sufficient"]);
    assert_eq!(lt_le.detail, "lt->le");
    let and_eq = inv
        .operators
        .iter()
        .flat_map(|o| &o.replacements)
        .find(|r| r.name == "lcr/and->eq")
        .unwrap();
    assert_eq!(and_eq.sets, ["sufficient"]);
}

#[test]
fn test_definitions_are_language_neutral_and_present() {
    let inv = Inventory::of_all(&without_spellings());
    for o in &inv.operators {
        assert!(!o.definition.is_empty(), "{}", o.name);
        for r in &o.replacements {
            assert!(!r.definition.is_empty(), "{}", r.name);
            assert!(
                !r.definition.contains('<') && !r.definition.contains("&&"),
                "{} spells a language's token: {}",
                r.name,
                r.definition
            );
        }
    }
    let lt_le = &inv.operators[0].replacements[0];
    assert_eq!(lt_le.definition, "Replace less-than with less-or-equal.");
}

#[test]
fn test_aliases_are_the_names_other_tools_use_for_it() {
    let inv = Inventory::of_all(&without_spellings());
    let ror = inv.operators.iter().find(|o| o.name == "ror").unwrap();
    assert!(ror.aliases.contains(&"Mothra ROR".to_string()));
    let lt_le = ror
        .replacements
        .iter()
        .find(|r| r.name == "ror/lt->le")
        .unwrap();
    assert!(
        lt_le
            .aliases
            .contains(&"PIT CONDITIONALS_BOUNDARY".to_string())
    );
    let arm = inv
        .operators
        .iter()
        .find(|o| o.name == "match-arm")
        .unwrap();
    assert!(arm.aliases.contains(&"cargo-mutants MatchArm".to_string()));
}

#[test]
fn test_spellings_are_per_language_and_optional() {
    let tokens = common::toy_tokens();
    let with = Spellings::of(&[("toy", &tokens)]);
    let inv = Inventory::of_all(&with);
    let lt_le = &inv.operators[0].replacements[0];
    let s = lt_le.spellings.get("toy").expect("toy spelling");
    assert_eq!(
        (s.from.as_deref(), s.with.as_deref()),
        (Some("<"), Some("<="))
    );
    assert!(
        Inventory::of_all(&without_spellings()).operators[0].replacements[0]
            .spellings
            .is_empty()
    );
}

#[test]
fn test_every_canonical_name_in_the_dump_is_accepted_by_a_custom_set() {
    let inv = Inventory::of_all(&without_spellings());
    for o in &inv.operators {
        let set = |name: &str| {
            custom_set(CustomSet {
                name: "t".into(),
                summary: None,
                source: None,
                mutations: vec![name.to_string()],
                skips: vec![],
            })
            .unwrap_or_else(|e| panic!("{name}: {}", e.reason()))
        };
        let whole = set(o.name);
        let got: Vec<&str> = whole.mutations.to_vec();
        let want: Vec<&str> = o.replacements.iter().map(|r| r.name).collect();
        assert_eq!(got, want, "family {} names its whole table", o.name);
        for r in &o.replacements {
            assert_eq!(set(r.name).mutations, [r.name], "{}", r.name);
        }
    }
}

fn alias_error(alias: &str) -> String {
    custom_set(CustomSet {
        name: "t".into(),
        summary: None,
        source: None,
        mutations: vec![alias.into()],
        skips: vec![],
    })
    .err()
    .unwrap_or_else(|| panic!("{alias} was accepted"))
    .reason()
}

#[test]
fn test_a_custom_set_does_not_accept_a_tools_alias_and_names_what_it_maps_to() {
    let reason = alias_error("PIT CONDITIONALS_BOUNDARY");
    assert!(
        reason.starts_with("PIT CONDITIONALS_BOUNDARY is another tool's name for "),
        "{reason}"
    );
    for canonical in ["ror/lt->le", "ror/le->lt", "ror/gt->ge", "ror/ge->gt"] {
        assert!(reason.contains(canonical), "{reason}");
    }
    assert!(!reason.contains("ror/lt->eq"), "{reason}");
    // An operator-level alias names the operator, not its replacements.
    assert_eq!(
        alias_error("Mothra ROR"),
        "Mothra ROR is another tool's name for ror"
    );
    // A name nobody uses is still just unknown.
    assert_eq!(alias_error("nonsense"), "no mutation named nonsense");
    assert!(named("breadth").is_some());
}

#[test]
fn test_canonical_names_of_an_alias_are_empty_for_a_canonical_name() {
    assert!(cherenkov_core::inventory::canonical_names("ror/lt->le").is_empty());
    assert_eq!(
        cherenkov_core::inventory::canonical_names("cargo-mutants MatchArm"),
        ["match-arm"]
    );
}

#[test]
fn test_markdown_lists_a_replacements_own_aliases_beside_its_name() {
    let md = Inventory::of_all(&without_spellings()).markdown();
    let row = md
        .lines()
        .find(|l| l.starts_with("| `ror/lt->le`"))
        .unwrap();
    let also = row.split('|').nth(4).unwrap();
    assert!(also.contains("PIT CONDITIONALS_BOUNDARY"), "{row}");
    // The operator's own alias is not repeated on each of its replacements.
    assert!(!also.contains("Mothra ROR"), "{row}");
}

#[test]
fn test_markdown_has_a_table_row_for_each_family_naming_its_sources_and_tables() {
    let md = Inventory::of_all(&without_spellings()).markdown();
    for f in families() {
        let row = md
            .lines()
            .find(|l| l.starts_with(&format!("| `{}`", f.id)))
            .unwrap_or_else(|| panic!("no row for {}", f.id));
        for m in &f.replacements {
            for o in m.origins {
                if o.relation == cherenkov_core::operator_set::Relation::Derived {
                    assert!(row.contains(o.source), "{}: {}", f.id, o.source);
                }
            }
        }
    }
    assert!(md.contains("sufficient"));
}

fn replacement<'a>(
    inv: &'a Inventory,
    name: &str,
) -> &'a cherenkov_core::inventory::ReplacementEntry {
    inv.operators
        .iter()
        .flat_map(|o| &o.replacements)
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name}"))
}

#[test]
fn test_a_similar_origin_carries_its_note_and_a_derived_one_has_none() {
    let inv = Inventory::of_all(&without_spellings());
    let call = replacement(&inv, "delete-statement/call");
    let similar = call
        .origins
        .iter()
        .find(|o| o.relation == "similar")
        .unwrap();
    assert_eq!(similar.note, Some("removes calls to void methods only"));
    let derived = call
        .origins
        .iter()
        .find(|o| o.relation == "derived")
        .unwrap();
    assert_eq!(derived.note, None);
}

#[test]
fn test_deletions_and_guards_are_defined_by_what_they_delete_or_set() {
    let inv = Inventory::of_all(&without_spellings());
    assert_eq!(
        replacement(&inv, "unary/delete-not").definition,
        "Delete the logical not operator, keeping its operand."
    );
    assert_eq!(
        replacement(&inv, "unary/delete-neg").definition,
        "Delete the negation operator, keeping its operand."
    );
    assert_eq!(
        replacement(&inv, "match-guard/false").definition,
        "Replace a match arm's guard with the constant false."
    );
    // A family with one replacement is defined as the family is.
    let single = replacement(&inv, "negate-condition");
    assert_eq!(single.definition, "Negate the condition of an if or loop.");
    assert_eq!(
        cherenkov_core::operator_set::family_definition("lcr"),
        families()
            .iter()
            .find(|f| f.id == "lcr")
            .map(|f| f.definition)
    );
    assert_eq!(
        cherenkov_core::operator_set::family_definition("nope"),
        None
    );
}

/// Tokens spelled as Rust spells them, including the unary ones.
fn rust_like() -> cherenkov_core::token::TokenTable {
    use cherenkov_core::token::Token::*;
    let spell = [
        (Lt, "<"),
        (Le, "<="),
        (Shl, "<<"),
        (Shr, ">>"),
        (Not, "!"),
        (Neg, "-"),
    ];
    cherenkov_core::token::TokenTable::new(spell.map(|(t, s)| (t, s.to_string())), "!{}").unwrap()
}

#[test]
fn test_spellings_of_deletions_and_cargo_mutants_names_exist_for_rust_only() {
    let tokens = rust_like();
    let both = Spellings::of(&[("rust", &tokens), ("toy", &tokens)]);
    let inv = Inventory::of_all(&both);
    let lt_le = replacement(&inv, "ror/lt->le");
    assert_eq!(
        lt_le.spellings["rust"].cargo_mutants.as_deref(),
        Some("replace < with <=")
    );
    assert_eq!(lt_le.spellings["toy"].cargo_mutants, None);
    let not = replacement(&inv, "unary/delete-not");
    let s = &not.spellings["rust"];
    assert_eq!((s.from.as_deref(), s.with.as_deref()), (Some("!"), None));
    assert_eq!(s.cargo_mutants.as_deref(), Some("delete !"));
    // No spelling for what is not a token.
    assert!(replacement(&inv, "match-arm/delete").spellings.is_empty());
}

#[test]
fn test_markdown_shows_spellings_grouped_by_language_only_when_asked_for() {
    let tokens = rust_like();
    let with = Inventory::of_all(&Spellings::of(&[("rust", &tokens), ("toy", &tokens)])).markdown();
    assert!(with.contains("| Spelling |"), "{with}");
    assert!(with.contains("`<` -> `<=` (rust, toy)"), "{with}");
    assert!(with.contains("`<<` -> `>>` (rust, toy)"), "{with}");
    assert!(with.contains("delete `!` (rust, toy)"), "{with}");
    let without = Inventory::of_all(&without_spellings()).markdown();
    assert!(!without.contains("| Spelling |"), "{without}");
}
