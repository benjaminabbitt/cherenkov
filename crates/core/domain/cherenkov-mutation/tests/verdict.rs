use cherenkov_mutation::verdict::Verdict;

#[test]
fn test_only_killed_survived_and_timeout_count_in_score() {
    let scoring: Vec<_> = Verdict::ALL
        .into_iter()
        .filter(|v| v.counts_in_score())
        .collect();
    assert_eq!(
        scoring,
        [Verdict::Killed, Verdict::Survived, Verdict::Timeout]
    );
}

#[test]
fn test_killed_and_timeout_are_detected_survived_is_not() {
    let detected: Vec<_> = Verdict::ALL.into_iter().filter(|v| v.detected()).collect();
    assert_eq!(detected, [Verdict::Killed, Verdict::Timeout]);
}
