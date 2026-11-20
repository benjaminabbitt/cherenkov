use super::*;

#[test]
fn test_every_error_has_a_message_using_exactly_its_args() {
    let problems = angzarr_diagnostic::catalog::check(EN_US.0, EN_US.1, &diagnostic_samples());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn test_a_cores_failure_is_a_kernel_error_holding_it() {
    let cause = Report::new("angzarr-embedded-x");
    assert_eq!(
        RunLayerError::from(cause.clone()),
        RunLayerError::Kernel { cause }
    );
}
