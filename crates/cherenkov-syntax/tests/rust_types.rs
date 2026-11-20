//! The project's types and their defaults, from Rust source: derived
//! (`#[derive(.., Default, ..)]`), implemented (`impl Default for T`), or
//! absent.

use cherenkov_core::types::SourceDefault;
use cherenkov_syntax::rust_types::default_facts;

#[test]
fn test_derives_impls_and_types_without_default_are_told_apart() {
    let src = r#"
#[derive(Clone, Default, PartialEq)]
pub struct Entry { pub n: i64 }

#[derive(Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Remote { pub url: String }

pub enum Kind { Sha, Tag }

pub struct Lock { entries: Vec<Entry> }

impl Default for Lock {
    fn default() -> Self { Lock { entries: vec![] } }
}

impl<T> std::default::Default for Wrapper<T> {
    fn default() -> Self { todo!() }
}
pub struct Wrapper<T>(T);

// A private type counts as the project's too.
#[derive(Default)]
struct DefaultLooking;
"#;
    let facts = default_facts(src);
    assert_eq!(facts.of("Entry"), SourceDefault::Derived);
    assert_eq!(facts.of("Remote"), SourceDefault::Absent);
    assert_eq!(facts.of("Kind"), SourceDefault::Absent);
    assert_eq!(facts.of("Lock"), SourceDefault::Implemented);
    assert_eq!(facts.of("Wrapper"), SourceDefault::Implemented);
    assert_eq!(facts.of("DefaultLooking"), SourceDefault::Derived);
    assert_eq!(facts.of("Elsewhere"), SourceDefault::Unknown);
}

#[test]
fn test_a_derive_naming_default_in_another_word_may_write_one() {
    // An unknown derive (`DefaultValue`) may write a Default: never taken
    // for none, which would call a compiling mutant unviable.
    let facts = default_facts("#[derive(Clone, DefaultValue)]\npub struct X;\n");
    assert_eq!(facts.of("X"), SourceDefault::Implemented);
}

#[test]
fn test_a_default_a_macro_writes_is_never_taken_for_none() {
    let src = r#"
#[derive(SmartDefault)]
pub struct A { #[default = 3] n: i64 }

#[derive(Derivative)]
#[derivative(Default)]
pub struct B { n: i64 }

pub struct C { n: i64 }
impl_default!(C);

pub struct D { n: i64 }
"#;
    let facts = default_facts(src);
    // Written by a macro: not derived field by field, and not absent.
    assert_eq!(facts.of("A"), SourceDefault::Implemented);
    assert_eq!(facts.of("B"), SourceDefault::Implemented);
    assert_eq!(facts.of("C"), SourceDefault::Implemented);
    assert_eq!(facts.of("D"), SourceDefault::Absent);
}

#[test]
fn test_a_prost_message_derives_a_default() {
    // Generated code (an OUT_DIR file) derives `prost::Message`, whose
    // expansion implements Default; a mutant built with it compiles.
    let facts =
        default_facts("#[derive(Clone, PartialEq, ::prost::Message)]\npub struct EventRef {}\n");
    assert_eq!(facts.of("EventRef"), SourceDefault::Derived);
}
