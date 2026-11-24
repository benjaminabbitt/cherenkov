use super::*;

#[test]
fn test_every_error_has_a_message_using_exactly_its_args() {
    let problems = coded_diagnostic::catalog::check(EN_US.0, EN_US.1, &diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn test_a_token_table_failure_is_held_as_the_cause_of_the_tokens_error() {
    let duplicate = cherenkov_mutation::token::TokenTableError::DuplicateSpelling {
        spelling: "&".into(),
    };
    let error = ProfileError::Tokens {
        lang: "go".into(),
        cause: Report::of(&duplicate),
    };
    assert_eq!(
        error.to_string(),
        "cherenkov-syntax-tokens lang=go cause=cherenkov-mutation-duplicate-spelling cause.spelling=&"
    );
}
