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

#[test]
fn test_a_custom_set_does_not_accept_a_tools_alias_as_a_name() {
    let err = custom_set(CustomSet {
        name: "t".into(),
        summary: None,
        source: None,
        mutations: vec!["PIT CONDITIONALS_BOUNDARY".into()],
        skips: vec![],
    })
    .err()
    .unwrap();
    assert_eq!(err.reason(), "no mutation named PIT CONDITIONALS_BOUNDARY");
    assert!(named("breadth").is_some());
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
