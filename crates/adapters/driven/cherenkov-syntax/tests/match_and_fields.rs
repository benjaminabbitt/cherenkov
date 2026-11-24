//! Match arms, match guards and struct literal fields (cargo-mutants'
//! MatchArm, MatchArmGuard and StructField genres), as language-neutral
//! mutations:
//!
//! - an arm is deleted only when its match has a catch-all arm, which is
//!   never deleted itself, nor is a guarded arm (its guard -> false already
//!   removes it);
//! - a guard is replaced with true and with false;
//! - a field is deleted only from a literal whose omitted fields take a
//!   default, with its separator, so the result still parses.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::operator_set::named;
use cherenkov_mutation::plan::{plan_file, render_source};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

/// (mutation, mutated source) for every mutant of `families`, in plan order.
fn mutants(lang: &str, src: &str, families: &[&str]) -> Vec<(&'static str, String)> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    assert!(!file.has_errors, "{lang} sample does not parse");
    let plans = plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans;
    plans
        .iter()
        .filter(|p| families.contains(&p.operator))
        .map(|p| {
            let out = render_source(src, p, &profile).expect("renders");
            let reparsed = syntax
                .analyze(&id, Path::new("t"), Arc::from(out.as_str()))
                .unwrap();
            assert!(
                !reparsed.has_errors,
                "{} does not parse:\n{out}",
                p.describe()
            );
            (p.mutation.unwrap(), out)
        })
        .collect()
}

fn names(ms: &[(&'static str, String)]) -> Vec<&'static str> {
    ms.iter().map(|(n, _)| *n).collect()
}

// --- Match arms.

#[test]
fn test_rust_arms_are_deleted_when_the_match_has_a_wildcard_arm() {
    let src = "fn f(k: u8) -> u8 {\n    match k {\n        0 => 10,\n        1 | 2 => 11,\n        _ => 0,\n    }\n}\n";
    let ms = mutants("rust", src, &["match-arm"]);
    assert_eq!(names(&ms), ["match-arm/delete", "match-arm/delete"]);
    assert!(
        ms[0]
            .1
            .contains("match k {\n        \n        1 | 2 => 11,")
    );
    assert!(ms[1].1.contains("0 => 10,\n        \n        _ => 0,"));
}

#[test]
fn test_rust_arms_are_kept_when_the_match_has_no_wildcard_arm() {
    let src = "fn f(x: Option<i64>) -> i64 {\n    match x {\n        Some(_) => 2,\n        None => 3,\n    }\n}\n";
    assert_eq!(mutants("rust", src, &["match-arm"]), []);
}

#[test]
fn test_rust_guarded_arms_are_not_deleted_and_a_guarded_wildcard_still_counts() {
    // cargo-mutants treats `_ if c` as a wildcard arm (visit.rs).
    let src = "fn f(k: u8, c: bool) -> u8 {\n    match k {\n        0 if c => 1,\n        1 => 2,\n        _ if c => 3,\n        _ => 0,\n    }\n}\n";
    let ms = mutants("rust", src, &["match-arm"]);
    assert_eq!(names(&ms), ["match-arm/delete"]);
    assert!(!ms[0].1.contains("1 => 2"));
}

#[test]
fn test_go_cases_are_deleted_when_the_switch_has_a_default() {
    let src = "package p\n\nfunc f(k int) int {\n\tswitch k {\n\tcase 0:\n\t\treturn 10\n\tcase 1, 2:\n\t\treturn 11\n\tdefault:\n\t\treturn 0\n\t}\n}\n";
    let ms = mutants("go", src, &["match-arm"]);
    assert_eq!(names(&ms), ["match-arm/delete", "match-arm/delete"]);
    assert!(!ms[0].1.contains("case 0:"));
    assert!(!ms[1].1.contains("case 1, 2:"));
}

#[test]
fn test_go_cases_are_kept_when_the_switch_has_no_default() {
    let src = "package p\n\nfunc f(k int) int {\n\tswitch k {\n\tcase 0:\n\t\treturn 10\n\t}\n\treturn 0\n}\n";
    assert_eq!(mutants("go", src, &["match-arm"]), []);
}

#[test]
fn test_python_cases_are_deleted_when_the_match_has_a_wildcard_case() {
    let src = "def f(k):\n    match k:\n        case 0:\n            return 10\n        case _:\n            return 0\n";
    let ms = mutants("python", src, &["match-arm"]);
    assert_eq!(names(&ms), ["match-arm/delete"]);
    assert!(!ms[0].1.contains("case 0:"));
}

// --- Match guards.

#[test]
fn test_rust_guard_is_replaced_with_true_and_false() {
    let src = "fn f(x: Option<i64>) -> i64 {\n    match x {\n        Some(n) if n > 10 => 1,\n        Some(_) => 2,\n        None => 3,\n    }\n}\n";
    let ms = mutants("rust", src, &["match-guard"]);
    assert_eq!(names(&ms), ["match-guard/false", "match-guard/true"]);
    assert!(ms[0].1.contains("Some(n) if false => 1,"));
    assert!(ms[1].1.contains("Some(n) if true => 1,"));
}

#[test]
fn test_python_guard_is_replaced_with_its_spelling_of_true_and_false() {
    let src = "def f(k, c):\n    match k:\n        case 0 if c > 1:\n            return 10\n        case _:\n            return 0\n";
    let ms = mutants("python", src, &["match-guard"]);
    assert_eq!(names(&ms), ["match-guard/false", "match-guard/true"]);
    assert!(ms[0].1.contains("case 0 if False:"));
    assert!(ms[1].1.contains("case 0 if True:"));
}

// --- Struct literal fields.

#[test]
fn test_rust_fields_are_deleted_from_a_literal_with_a_base() {
    let src = "fn f(a: i64) -> S {\n    S { a, b: 2, ..Default::default() }\n}\n";
    let ms = mutants("rust", src, &["struct-field"]);
    assert_eq!(names(&ms), ["struct-field/delete", "struct-field/delete"]);
    assert!(ms[0].1.contains("S {  b: 2, ..Default::default() }"));
    assert!(ms[1].1.contains("S { a,  ..Default::default() }"));
}

#[test]
fn test_rust_fields_are_kept_in_a_literal_without_a_base() {
    let src = "fn f(a: i64) -> S {\n    S { a, b: 2 }\n}\n";
    assert_eq!(mutants("rust", src, &["struct-field"]), []);
}

#[test]
fn test_go_keyed_struct_fields_are_deleted_since_omitted_fields_are_zero() {
    let src = "package p\n\nfunc f() S {\n\treturn S{A: 1, B: 2}\n}\n";
    let ms = mutants("go", src, &["struct-field"]);
    assert_eq!(names(&ms), ["struct-field/delete", "struct-field/delete"]);
    assert!(ms[0].1.contains("S{ B: 2}"));
    assert!(ms[1].1.contains("S{A: 1, }"));
}

#[test]
fn test_go_map_and_positional_literals_have_no_field_mutants() {
    let src = "package p\n\nfunc f() (map[string]int, P) {\n\treturn map[string]int{\"a\": 1}, P{1, 2}\n}\n";
    assert_eq!(mutants("go", src, &["struct-field"]), []);
}

#[test]
fn test_rust_guard_mutants_are_branches_to_the_arm_body() {
    use cherenkov_mutation::plan::{Action, Target};
    let src = "fn f(x: Option<i64>) -> i64 {\n    match x {\n        Some(n) if n > 10 => 1,\n        Some(_) => 2,\n        None => 3,\n    }\n}\n";
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    let plans = plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans;
    for name in ["match-guard/true", "match-guard/false", "ror/gt->ge"] {
        let p = plans.iter().find(|p| p.mutation == Some(name)).unwrap();
        let Action::EvalBranch {
            if_true: Target::BlockStart(body),
            ..
        } = &p.action
        else {
            panic!("{name}: {:?}", p.action)
        };
        assert_eq!(body.text(src), "1", "{name}");
        assert_eq!((p.stop_at.line, p.stop_at.col), (3, 20), "{name}");
    }
}
