//! Which names of a replacement value the source file could not resolve at
//! the mutated place, so the mutant would not compile (cargo-mutants'
//! `unviable`).

use cherenkov_syntax::rust_scope::unresolved_names;

const SRC: &str = r#"
use std::sync::{Arc, Mutex as Lock};
use crate::model::{self, Thing};

struct Local;
enum Mode { A }
type Alias = u8;

fn plain() -> u32 { 1 }

impl Local {
    fn make<T>(&self) -> T { todo!() }
}

mod inner {
    use std::collections::HashMap;
    pub fn f() -> u8 { 1 }
}
"#;

fn at(needle: &str) -> usize {
    SRC.find(needle).unwrap()
}

fn unresolved(needle: &str, value: &str) -> Vec<String> {
    unresolved_names(SRC, at(needle), value)
}

#[test]
fn test_a_name_neither_imported_nor_defined_nor_in_the_prelude_is_unresolved() {
    assert_eq!(unresolved("fn plain", "MutexGuard::new()"), ["MutexGuard"]);
    assert_eq!(unresolved("fn plain", "Ok(Next::Idle)"), ["Next"]);
}

#[test]
fn test_imports_definitions_and_the_prelude_resolve() {
    for v in [
        "Arc::new(Default::default())",
        "Lock::new(1)",
        "Thing::default()",
        "model::make()",
        "Local",
        "Mode::A",
        "Alias::MAX",
        "Some(String::new())",
        "Ok(Vec::new())",
        "Box::new(Default::default())",
        "std::sync::MutexGuard::new()",
        "crate::Foo::new()",
        "\"Zed\".into()",
    ] {
        assert!(unresolved("fn plain", v).is_empty(), "{v}");
    }
}

#[test]
fn test_a_name_imported_by_the_original_name_of_an_alias_is_not_in_scope() {
    assert_eq!(unresolved("fn plain", "Mutex::new(1)"), ["Mutex"]);
}

#[test]
fn test_a_generic_parameter_resolves_within_its_item_only() {
    assert!(unresolved("todo!()", "T::default()").is_empty());
    assert_eq!(unresolved("fn plain", "T::default()"), ["T"]);
}

#[test]
fn test_a_module_sees_its_own_imports_not_the_files() {
    assert!(unresolved("pub fn f", "HashMap::new()").is_empty());
    assert_eq!(unresolved("fn plain", "HashMap::new()"), ["HashMap"]);
    assert_eq!(unresolved("pub fn f", "Arc::new(1)"), ["Arc"]);
}

#[test]
fn test_a_glob_import_in_scope_decides_nothing() {
    let src = "use crate::prelude::*;\nfn f() -> u8 { 1 }\n";
    assert!(unresolved_names(src, src.find("fn f").unwrap(), "Whatever::new()").is_empty());
}

#[test]
fn test_names_in_a_macro_or_after_a_dot_or_in_a_string_are_read_properly() {
    assert_eq!(unresolved("fn plain", "vec![Foo::new()]"), ["Foo"]);
    assert!(unresolved("fn plain", "x.Foo()").is_empty());
    assert!(unresolved("fn plain", "\"Foo::new\".to_string()").is_empty());
}

#[test]
fn test_an_item_level_macro_may_define_any_name() {
    let src = "make_types!(A, B);\nfn f() -> u8 { 1 }\n";
    assert!(unresolved_names(src, src.find("fn f").unwrap(), "Whatever::new()").is_empty());
}

#[test]
fn test_a_local_item_in_the_function_body_resolves() {
    let src = "fn f() -> u8 { struct Inner; use std::rc::Rc; 1 }\n";
    let at = src.find("1 }").unwrap();
    assert!(unresolved_names(src, at, "Inner").is_empty());
    assert!(unresolved_names(src, at, "Rc::new(1)").is_empty());
}
