use angzarr_embedded::v1::Uuid;

use super::*;

fn holder() -> Holder {
    Holder {
        pid: 42,
        started: 1000,
        command: "cherenkov run /p".to_string(),
    }
}

#[test]
fn test_a_holder_is_described_by_its_pid_how_long_it_has_held_and_its_command() {
    assert_eq!(
        describe(Some(&holder()), 1192),
        "pid 42, held for 3m 12s, `cherenkov run /p`"
    );
}

#[test]
fn test_a_holder_that_left_no_record_is_described_as_such() {
    assert_eq!(describe(None, 5), "a process that left no record of itself");
}

#[test]
fn test_a_time_held_is_in_the_largest_two_units() {
    assert_eq!(elapsed(0), "0s");
    assert_eq!(elapsed(59), "59s");
    assert_eq!(elapsed(60), "1m 0s");
    assert_eq!(elapsed(3599), "59m 59s");
    assert_eq!(elapsed(3600), "1h 0m");
    assert_eq!(elapsed(7384), "2h 3m");
}

fn pruned(runs: usize) -> Pruned {
    Pruned {
        runs: (0..runs as u8).map(|n| Uuid::from_bytes([n; 16])).collect(),
        mutants: 3,
        events: 40,
        kept_finished: 1,
        kept_unfinished: 2,
    }
}

#[test]
fn test_a_prune_says_what_it_removed_and_what_stays() {
    let text = report(&pruned(2), false);
    assert!(
        text.starts_with(
            "removed 2 runs, 3 mutant streams, 40 events; kept 1 finished run, 2 unfinished\n"
        ),
        "{text}"
    );
    assert_eq!(text.lines().count(), 3, "a line for each run: {text}");
    assert!(text.contains("  run 00000000"), "{text}");
}

#[test]
fn test_a_dry_prune_says_it_would_remove() {
    let text = report(&pruned(1), true);
    assert!(
        text.starts_with("would remove 1 run, 3 mutant streams"),
        "{text}"
    );
}

#[test]
fn test_a_prune_of_nothing_says_so_and_what_stays() {
    let text = report(&pruned(0), false);
    assert_eq!(
        text,
        "nothing to remove; kept 1 finished run, 2 unfinished\n"
    );
}

#[test]
fn test_a_held_store_is_a_refusal_to_take_it_or_to_unlock_it() {
    let held = || LockError::Held {
        lock: "s.lock".into(),
        holder: Some(holder()),
    };
    assert!(matches!(
        refusal(Path::new("s"), held(), true),
        CliError::StoreLocked { .. }
    ));
    assert!(matches!(
        refusal(Path::new("s"), held(), false),
        CliError::StoreHeld { .. }
    ));
}

#[test]
fn test_a_lock_that_could_not_be_made_is_unreadable_not_held() {
    let broken = LockError::Record {
        path: "s.holder".into(),
        cause: std::io::Error::other("disk full"),
    };
    let error = refusal(Path::new("s"), broken, true);
    assert!(
        matches!(&error, CliError::Unreadable { reason, .. } if reason.contains("disk full")),
        "{error:?}"
    );
}

#[test]
fn test_the_clock_reads_the_present() {
    // After 2023-11-14, before 2100.
    assert!((1_700_000_000..4_100_000_000).contains(&now()));
}
