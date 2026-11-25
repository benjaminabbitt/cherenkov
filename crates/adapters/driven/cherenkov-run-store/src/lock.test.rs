use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use super::*;
use test_dir::TestDir;

/// One test at a time: a test that starts a process hands it, for the
/// instant between fork and exec, every file this process has open, the
/// locks another test holds among them.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn held_by(error: LockError) -> Option<Holder> {
    match error {
        LockError::Held { holder, .. } => holder,
        other => panic!("not refused as held: {other:?}"),
    }
}

#[test]
fn test_a_second_take_of_a_held_store_is_refused_at_once_naming_this_process() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-second");
    let store = dir.join("runs.sqlite");
    let _first = acquire(&store).unwrap();
    let began = Instant::now();
    let holder = held_by(acquire(&store).unwrap_err()).expect("the holder left its record");
    assert!(began.elapsed() < Duration::from_secs(2), "not waited for");
    assert_eq!(holder.pid, std::process::id());
    assert!(!holder.command.is_empty());
}

#[test]
fn test_a_store_dropped_is_free_to_take_again_and_its_record_is_gone() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-drop");
    let store = dir.join("runs.sqlite");
    drop(acquire(&store).unwrap());
    assert!(!holder_file(&store).exists());
    drop(acquire(&store).unwrap());
}

#[test]
fn test_the_lock_of_one_store_does_not_hold_another() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-two");
    let _one = acquire(&dir.join("one.sqlite")).unwrap();
    let _two = acquire(&dir.join("two.sqlite")).unwrap();
}

#[test]
fn test_a_relative_store_path_is_locked_as_the_file_it_names() {
    let _serial = serial();
    let relative = PathBuf::from("target-relative-lock-test.sqlite");
    let _first = acquire(&relative).unwrap();
    assert!(held_by(acquire(&std::path::absolute(&relative).unwrap()).unwrap_err()).is_some());
    drop(_first);
    let _ = std::fs::remove_file(lock_file(&std::path::absolute(&relative).unwrap()));
}

/// A process that holds the lock file with `flock(1)` for a while, and
/// leaves no record: another taker of the lock, as any other program.
fn foreign_holder(lock: &Path) -> Child {
    let child = Command::new("flock")
        .arg("--exclusive")
        .arg(lock)
        .args(["sleep", "60"])
        .process_group(0)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !lock.exists() {
        assert!(Instant::now() < deadline, "flock did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The file exists before the lock is taken on it.
    std::thread::sleep(Duration::from_millis(200));
    child
}

/// `holder`, and the command `flock` ran, which holds the lock too.
fn stop(holder: &mut Child) {
    let group = format!("-{}", holder.id());
    Command::new("kill")
        .args(["-KILL", "--", &group])
        .status()
        .unwrap();
    holder.wait().unwrap();
}

#[test]
fn test_a_store_held_by_a_process_that_left_no_record_is_refused_without_a_holder() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-foreign");
    let store = dir.join("runs.sqlite");
    let mut other = foreign_holder(&lock_file(&store));
    assert_eq!(held_by(acquire(&store).unwrap_err()), None);
    stop(&mut other);
}

#[test]
fn test_a_lock_does_not_outlive_its_holder_killed() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-killed");
    let store = dir.join("runs.sqlite");
    let mut other = foreign_holder(&lock_file(&store));
    assert!(acquire(&store).is_err());
    stop(&mut other);
    // The kill is delivered, and the process's files closed, a moment later.
    let deadline = Instant::now() + Duration::from_secs(10);
    let taken = loop {
        match acquire(&store) {
            Err(LockError::Held { .. }) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            taken => break taken,
        }
    };
    drop(taken.expect("the operating system gave the lock up with the process"));
}

/// A record of a holder that was killed: its process is long gone.
fn leave_record(store: &Path) -> Holder {
    let left = Holder {
        pid: 4_000_000,
        started: 1,
        command: "cherenkov run /project".to_string(),
    };
    std::fs::write(holder_file(store), serde_json::to_string(&left).unwrap()).unwrap();
    left
}

#[test]
fn test_a_record_a_killed_holder_left_does_not_refuse_the_next_taker_and_is_replaced() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-stale");
    let store = dir.join("runs.sqlite");
    leave_record(&store);
    let lock = acquire(&store).unwrap();
    assert_eq!(read_holder(&store).unwrap().pid, std::process::id());
    drop(lock);
}

#[test]
fn test_unlock_of_a_held_store_is_refused_and_leaves_the_holder_holding() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-unlock-held");
    let store = dir.join("runs.sqlite");
    let lock = acquire(&store).unwrap();
    let holder = held_by(unlock(&store).unwrap_err()).unwrap();
    assert_eq!(holder.pid, std::process::id());
    assert!(acquire(&store).is_err(), "still held");
    assert!(holder_file(&store).exists(), "its record is not touched");
    drop(lock);
}

#[test]
fn test_unlock_of_a_free_store_clears_what_a_killed_holder_left() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-unlock-left");
    let store = dir.join("runs.sqlite");
    drop(acquire(&store).unwrap());
    let left = leave_record(&store);
    assert_eq!(unlock(&store).unwrap(), Unlocked::Cleared(Some(left)));
    assert!(!holder_file(&store).exists());
    drop(acquire(&store).unwrap());
}

#[test]
fn test_unlock_of_a_free_store_with_nothing_left_says_it_was_free() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-unlock-free");
    let store = dir.join("runs.sqlite");
    drop(acquire(&store).unwrap());
    assert_eq!(unlock(&store).unwrap(), Unlocked::Free);
}

#[test]
fn test_unlock_of_a_store_never_locked_makes_no_file() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-unlock-never");
    let store = dir.join("sub").join("runs.sqlite");
    assert_eq!(unlock(&store).unwrap(), Unlocked::Free);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn test_a_holder_says_how_long_it_has_held_and_never_a_negative_time() {
    let _serial = serial();
    let holder = Holder {
        pid: 1,
        started: 100,
        command: String::new(),
    };
    assert_eq!(holder.held_for(160), Duration::from_secs(60));
    assert_eq!(holder.held_for(50), Duration::ZERO);
}

#[test]
fn test_a_refusal_says_which_lock_is_held() {
    let _serial = serial();
    let held = LockError::Held {
        lock: PathBuf::from("/s/runs.sqlite.lock"),
        holder: None,
    };
    assert_eq!(held.to_string(), "/s/runs.sqlite.lock is held");
}

#[test]
fn test_unlock_of_a_store_a_process_holds_that_left_no_record_is_refused() {
    let _serial = serial();
    let dir = TestDir::new("cherenkov-lock-unlock-foreign");
    let store = dir.join("runs.sqlite");
    let mut other = foreign_holder(&lock_file(&store));
    assert_eq!(held_by(unlock(&store).unwrap_err()), None);
    stop(&mut other);
}
