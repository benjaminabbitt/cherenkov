//! Regions no mutation is planned in. Profiles mark them with
//! `@skip.<kind>` captures; a set names the kinds it honours. The
//! cargo-mutants set honours cargo-mutants 26.2.0's rules (visit.rs:
//! attrs_excluded, fn_sig_excluded, `fn new` in impls and traits,
//! `impl Default`, calls to `with_capacity`).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use cherenkov_core::operator_set::named;
use cherenkov_core::plan::{PlanOutput, SkipReason, plan_file};
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::{LanguageId, SkipKind};
use cherenkov_syntax::TreeSitterSyntax;

fn plan(src: &str, set: &str) -> PlanOutput {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    assert!(!file.has_errors, "sample does not parse");
    plan_file(&file, &profile, &named(set).unwrap().operators())
}

/// Functions with at least one mutant.
fn mutated(src: &str, set: &str) -> BTreeSet<String> {
    plan(src, set)
        .plans
        .into_iter()
        .filter_map(|p| p.function)
        .collect()
}

const SAMPLE: &str = r#"
pub fn kept(a: i32) -> i32 { a + 1 }

#[test]
fn test_fn() { let _ = 1 + 1; }

#[tokio::test(flavor = "multi_thread")]
async fn async_test() { let _ = 1 + 1; }

#[test]
#[ignore]
fn ignored_test() { let _ = 1 + 1; }

#[cfg(test)]
mod tests {
    fn helper(a: i32) -> i32 { a + 1 }
}

#[cfg(not(test))]
fn not_test(a: i32) -> i32 { a + 1 }

#[mutants::skip]
fn opted_out(a: i32) -> i32 { a + 1 }

#[cfg_attr(test, mutants::skip)]
fn opted_out_in_tests(a: i32) -> i32 { a + 1 }

unsafe fn raw(a: i32) -> i32 { a + 1 }

pub fn new(a: i32) -> i32 { a + 1 }

pub struct S { pub a: i32, pub b: i32 }

impl S {
    pub fn new(a: i32) -> S { S { a: a + 1, b: 0 } }
    pub fn get(&self) -> i32 { self.a + 1 }
}

trait T {
    fn new() -> i32 { 1 + 1 }
}

impl Default for S {
    fn default() -> S { S { a: 1 + 1, b: 0 } }
}

pub fn capacity(n: usize) -> Vec<u8> {
    let v = Vec::with_capacity(n + 1);
    let w: Vec<u8> = Vec::new();
    w.len() + 1;
    v
}
"#;

#[test]
fn test_cargo_mutants_set_skips_what_cargo_mutants_skips() {
    assert_eq!(
        mutated(SAMPLE, "cargo-mutants"),
        ["capacity", "get", "kept", "new", "not_test"]
            .map(String::from)
            .into(),
        "only the top-level `fn new` is mutated; `new` in impls and traits is not"
    );
    let top_level_new = SAMPLE
        .lines()
        .position(|l| l.starts_with("pub fn new"))
        .unwrap() as u32
        + 1;
    for p in plan(SAMPLE, "cargo-mutants").plans {
        if p.function.as_deref() == Some("new") {
            assert_eq!(p.site.start.line, top_level_new, "{}", p.describe());
        }
    }
}

#[test]
fn test_with_capacity_arguments_are_not_mutated_but_the_rest_of_the_function_is() {
    let out = plan(SAMPLE, "cargo-mutants");
    let lines: BTreeSet<u32> = out
        .plans
        .iter()
        .filter(|p| p.function.as_deref() == Some("capacity") && p.operator == "aor")
        .map(|p| p.site.start.line)
        .collect();
    let with_capacity = SAMPLE
        .lines()
        .position(|l| l.contains("with_capacity"))
        .unwrap() as u32
        + 1;
    assert!(!lines.contains(&with_capacity), "{lines:?}");
    assert!(lines.contains(&(with_capacity + 2)), "{lines:?}");
}

#[test]
fn test_skipped_mutants_are_counted_by_kind() {
    let skipped = plan(SAMPLE, "cargo-mutants").diagnostics.skipped;
    let count = |kind| {
        skipped
            .iter()
            .filter(|((_, r), _)| *r == SkipReason::Region(kind))
            .map(|(_, n)| n)
            .sum::<usize>()
    };
    for kind in [
        SkipKind::Test,
        SkipKind::Annotated,
        SkipKind::Unsafe,
        SkipKind::Constructor,
        SkipKind::DefaultImpl,
        SkipKind::CapacityCall,
    ] {
        assert!(count(kind) > 0, "{kind:?}: {skipped:?}");
    }
}

#[test]
fn test_cfg_test_on_a_field_initializer_skips_only_that_field() {
    let src = "fn f(a: i32) -> S {\n    S {\n        #[cfg(test)]\n        a: a + 1,\n        b: a - 1,\n        ..Default::default()\n    }\n}\n";
    let out = plan(src, "cargo-mutants");
    let aor: Vec<u32> = out
        .plans
        .iter()
        .filter(|p| p.operator == "aor")
        .map(|p| p.site.start.line)
        .collect();
    assert!(aor.iter().all(|&l| l == 5), "{aor:?}");
    assert!(!aor.is_empty());
}

#[test]
fn test_inner_cfg_test_attribute_skips_the_whole_file() {
    let src = "#![cfg(test)]\nfn f(a: i32) -> i32 { a + 1 }\n";
    assert_eq!(plan(src, "cargo-mutants").plans, []);
}

#[test]
fn test_breadth_set_honours_no_skips() {
    // The prototype set is unchanged: it plans inside test functions too.
    assert!(mutated(SAMPLE, "breadth").contains("test_fn"));
}
