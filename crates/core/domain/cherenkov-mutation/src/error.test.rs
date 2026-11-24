use super::*;

#[test]
fn test_every_error_has_a_message_using_exactly_its_args() {
    let problems = angzarr_diagnostic::catalog::check(EN_US.0, EN_US.1, &diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}
