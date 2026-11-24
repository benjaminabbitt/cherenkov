use super::*;

#[test]
fn test_every_error_has_a_message_using_exactly_its_args() {
    let problems = coded_diagnostic::catalog::check(EN_US.0, EN_US.1, &diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn test_an_unknown_language_names_it_in_its_display() {
    let error = SyntaxError::UnknownLanguage {
        lang: LanguageId::new("cobol"),
    };
    assert_eq!(
        error.to_string(),
        "cherenkov-run-ports-unknown-language lang=cobol"
    );
}
