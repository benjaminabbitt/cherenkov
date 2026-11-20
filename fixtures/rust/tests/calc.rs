use calc::{count_up, first, is_small, total};

#[test]
fn test_total() {
    assert_eq!(total(2, 3), 5);
}

#[test]
fn test_is_small() {
    assert!(is_small(3));
    assert!(!is_small(12));
}

#[test]
fn test_count_up() {
    assert_eq!(count_up(4), 4);
}

#[test]
fn test_first() {
    assert_eq!(first(&[7, 8]), 7);
}

/// Run on purpose only (`--ignored`): a process killed outright, as the
/// OOM killer would, with no exit of its own.
#[test]
#[ignore = "kills its own process"]
fn test_killed_outright() {
    let _ = std::process::Command::new("kill")
        .args(["-9", &std::process::id().to_string()])
        .status();
    std::thread::sleep(std::time::Duration::from_secs(5));
}

/// Run on purpose only (`--ignored`): a write to a pipe whose reader has
/// exited raises SIGPIPE, which the Rust runtime ignores.
#[test]
#[ignore = "raises SIGPIPE"]
fn test_writes_to_a_closed_pipe() {
    use std::io::Write;
    let mut child = std::process::Command::new("true")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    child.wait().unwrap();
    let mut failed = false;
    for _ in 0..64 {
        if stdin.write_all(&[0; 4096]).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed, "the write to a closed pipe fails");
}
