//! Names are the contract with profile data (capture names, token keys)
//! and with mutant IDs (edit descriptions): they must round-trip and stay put.

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::edit::{Edit, Lit};
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::syntax::{LanguageId, Pos, Span, StmtKind};
use cherenkov_mutation::token::{Token, TokenTableError};

fn assert_round_trip<T: Copy + PartialEq + std::fmt::Debug>(
    all: &[T],
    name: fn(T) -> &'static str,
    from: fn(&str) -> Option<T>,
) {
    let mut names: Vec<&str> = all.iter().map(|&v| name(v)).collect();
    for &v in all {
        assert_eq!(from(name(v)), Some(v), "{v:?}");
    }
    assert_eq!(from("no-such-name"), None);
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all.len(), "names must be unique");
}

#[test]
fn test_concept_names_round_trip_and_are_unique() {
    assert_round_trip(&Concept::ALL, Concept::name, Concept::from_name);
    assert_eq!(Concept::CompoundAssign.name(), "compound_assign");
}

#[test]
fn test_capture_names_round_trip_and_are_unique() {
    assert_round_trip(&Capture::ALL, Capture::name, Capture::from_name);
    assert_eq!(Capture::Else.name(), "else");
}

#[test]
fn test_statement_kind_names_round_trip_and_are_unique() {
    assert_round_trip(&StmtKind::ALL, StmtKind::name, StmtKind::from_name);
    assert_eq!(StmtKind::Loop.name(), "loop");
}

#[test]
fn test_token_names_round_trip_and_are_unique() {
    assert_round_trip(&Token::ALL, Token::name, Token::from_name);
    assert_eq!(Token::Assign.name(), "assign");
}

#[test]
fn test_edit_descriptions_are_fixed_because_they_feed_mutant_ids() {
    // Changing any of these strings changes every affected mutant ID.
    let cases = [
        (
            Edit::ReplaceToken {
                capture: Capture::Op,
                from: Token::Lt,
                with: Token::Le,
            },
            "replace-token op lt -> le",
        ),
        (
            Edit::WrapNot {
                capture: Capture::Cond,
            },
            "wrap-not cond",
        ),
        (
            Edit::ReplaceWithLiteral {
                capture: Capture::Value,
                lit: Lit::Bool(false),
            },
            "replace-with-literal value -> false",
        ),
        (
            Edit::ReplaceWithLiteral {
                capture: Capture::Body,
                lit: Lit::Default,
            },
            "replace-with-literal body -> default",
        ),
        (Edit::DeleteStatement, "delete-statement"),
    ];
    for (edit, want) in cases {
        assert_eq!(edit.describe(), want);
    }
}

#[test]
fn test_display_impls_show_the_value() {
    assert_eq!(
        MutantId("0123456789abcdef".into()).to_string(),
        "0123456789abcdef"
    );
    assert_eq!(LanguageId::new("go").to_string(), "go");
    assert!(
        TokenTableError::BadNotTemplate("not".into())
            .to_string()
            .contains("\"not\"")
    );
    assert!(
        TokenTableError::DuplicateSpelling("&".into())
            .to_string()
            .contains("\"&\"")
    );
}

#[test]
fn test_span_length_and_emptiness() {
    let p = |byte| Pos {
        byte,
        line: 1,
        col: byte as u32 + 1,
    };
    let empty = Span {
        start: p(3),
        end: p(3),
    };
    let three = Span {
        start: p(3),
        end: p(6),
    };
    assert!(empty.is_empty());
    assert!(!three.is_empty());
    assert_eq!(three.len(), 3);
}
