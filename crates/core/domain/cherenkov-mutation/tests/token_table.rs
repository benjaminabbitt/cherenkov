mod common;

use cherenkov_mutation::token::{Token, TokenTable, TokenTableError};

#[test]
fn test_token_table_resolves_spelling_both_ways() {
    let t = common::toy_tokens();
    assert_eq!(t.token("<="), Some(Token::Le));
    assert_eq!(t.spell(Token::Le), Some("<="));
    assert_eq!(t.token(":="), Some(Token::Assign));
    assert_eq!(t.token("<=>"), None);
}

#[test]
fn test_token_table_spell_is_none_for_unmapped_token() {
    let t = TokenTable::new([(Token::Add, "+".to_string())], "!({})").unwrap();
    assert_eq!(t.spell(Token::Sub), None);
}

#[test]
fn test_wrap_not_renders_through_template() {
    let t = common::toy_tokens();
    assert_eq!(t.wrap_not("a < b"), "NOT[a < b]");
}

#[test]
fn test_literal_bool_uses_profile_spellings_only() {
    let t = common::toy_tokens();
    assert_eq!(t.literal_bool("yes"), Some(true));
    assert_eq!(t.literal_bool("no"), Some(false));
    assert_eq!(t.literal_bool("true"), None);
    assert_eq!(t.literal_bool(" yes"), None);
}

#[test]
fn test_token_table_rejects_template_without_exactly_one_placeholder() {
    for bad in ["not", "{} and {}"] {
        let err = TokenTable::new([], bad).unwrap_err();
        assert_eq!(err, TokenTableError::BadNotTemplate(bad.to_string()));
    }
}

#[test]
fn test_token_table_rejects_one_spelling_for_two_tokens() {
    let err = TokenTable::new(
        [(Token::And, "&".to_string()), (Token::Or, "&".to_string())],
        "!({})",
    )
    .unwrap_err();
    assert_eq!(err, TokenTableError::DuplicateSpelling("&".to_string()));
}

#[test]
fn test_unary_spellings_resolve_apart_from_binary_ones() {
    // `-` is both subtraction and negation; which one depends on the concept.
    let t = TokenTable::new(
        [
            (Token::Sub, "-".to_string()),
            (Token::Neg, "-".to_string()),
            (Token::Not, "!".to_string()),
        ],
        "!({})",
    )
    .unwrap();
    assert_eq!(t.token("-"), Some(Token::Sub));
    assert_eq!(t.unary("-"), Some(Token::Neg));
    assert_eq!(t.unary("!"), Some(Token::Not));
    assert_eq!(t.token("!"), None, "`!` is only a unary spelling");
    assert_eq!(t.spell(Token::Neg), Some("-"));
}

#[test]
fn test_two_unary_tokens_with_one_spelling_are_rejected() {
    let err = TokenTable::new(
        [(Token::Neg, "-".to_string()), (Token::Not, "-".to_string())],
        "!({})",
    )
    .unwrap_err();
    assert_eq!(err, TokenTableError::DuplicateSpelling("-".to_string()));
}
