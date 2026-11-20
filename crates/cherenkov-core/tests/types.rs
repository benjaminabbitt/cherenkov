//! A type's default by the project's source: derived, written by hand,
//! absent, or unknown (not the project's, or ambiguous).

use cherenkov_core::types::{DefaultFacts, SourceDefault};

#[test]
fn test_a_type_s_default_is_what_its_one_definition_says() {
    let mut facts = DefaultFacts::default();
    facts.defined("Entry", true);
    facts.defined("Remote", false);
    facts.defined("Lock", false);
    facts.implemented("Lock");
    assert_eq!(facts.of("Entry"), SourceDefault::Derived);
    assert_eq!(facts.of("Remote"), SourceDefault::Absent);
    assert_eq!(facts.of("Lock"), SourceDefault::Implemented);
    // DWARF names carry generics.
    assert_eq!(
        facts.of("Entry<alloc::string::String>"),
        SourceDefault::Derived
    );
}

#[test]
fn test_a_type_not_defined_once_in_the_project_is_unknown() {
    let mut facts = DefaultFacts::default();
    facts.defined("Kind", false);
    let mut other = DefaultFacts::default();
    other.defined("Kind", true);
    facts.merge(other);
    assert_eq!(facts.of("Kind"), SourceDefault::Unknown, "two Kinds");
    assert_eq!(facts.of("VersionReq"), SourceDefault::Unknown, "semver's");
}
