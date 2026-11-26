use super::*;

fn scratch(name: &str) -> test_dir::TestDir {
    test_dir::TestDir::new(&format!("cherenkov-sweep-{name}"))
}

#[test]
fn test_the_scratch_of_a_process_that_is_gone_is_swept_and_a_living_one_is_not() {
    let temp = scratch("sweep");
    for name in [
        "cherenkov-support-100",
        "cherenkov-support-200",
        "cherenkov-run-100",
        "cherenkov-run-200",
        "cherenkov-support-notapid",
        "other-100",
    ] {
        std::fs::create_dir_all(temp.join(name).join("deep")).unwrap();
    }
    let swept = sweep_stale_in(&temp, |pid| pid == 200);
    let mut left: Vec<String> = std::fs::read_dir(&temp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "cherenkov-run-200",
            "cherenkov-support-200",
            "cherenkov-support-notapid",
            "other-100"
        ]
    );
    assert_eq!(swept, 2);
}

#[test]
fn test_a_directory_in_a_missing_temp_directory_sweeps_nothing() {
    assert_eq!(
        sweep_stale_in(Path::new("/nonexistent-cherenkov-dir"), |_| false),
        0
    );
}

#[test]
fn test_the_sweep_of_the_temp_directory_removes_what_no_process_owns() {
    // No process has a pid this large.
    let stale = std::env::temp_dir().join("cherenkov-support-4294967290");
    std::fs::create_dir_all(stale.join("deep")).unwrap();
    assert!(sweep_stale_scratch() >= 1);
    assert!(!stale.exists());
}
