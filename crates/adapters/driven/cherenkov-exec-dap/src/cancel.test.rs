use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::*;

fn sleeper() -> Child {
    Command::new("sleep").arg("600").spawn().unwrap()
}

/// Whether the child ended, waiting up to five seconds for it.
fn ended(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    let _ = child.wait();
    false
}

#[test]
fn test_cancelling_kills_the_registered_debugger() {
    let mut child = sleeper();
    let watch = Watch::default();
    watch.debugger(child.id());
    assert!(!watch.cancelled());
    watch.cancel();
    assert!(watch.cancelled());
    assert!(ended(&mut child));
}

#[test]
fn test_cancelling_twice_is_a_no_op_the_second_time() {
    let mut child = sleeper();
    let watch = Watch::default();
    watch.debugger(child.id());
    watch.cancel();
    assert!(ended(&mut child));
    // The child is reaped: its pid is free to be another process's, which a
    // second cancel must not signal. The watch stays cancelled and quiet.
    watch.cancel();
    assert!(watch.cancelled());
}

#[test]
fn test_registering_after_a_cancel_kills_the_process_at_once() {
    let watch = Watch::default();
    watch.cancel();
    let mut child = sleeper();
    watch.debugger(child.id());
    assert!(ended(&mut child));
}

#[test]
fn test_cancelling_after_the_session_released_its_debugger_kills_nothing() {
    let mut child = sleeper();
    let watch = Watch::default();
    watch.debugger(child.id());
    watch.released();
    watch.cancel();
    assert!(child.try_wait().unwrap().is_none());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_cancelling_kills_the_debuggers_grandchildren_too() {
    // sh -> sh -> sleep: the debugger, a shell, the program.
    let mut child = Command::new("sh")
        .args(["-c", "sh -c 'sleep 600; :' ; :"])
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let before = descendants(child.id());
    assert!(before.len() >= 2, "{before:?}");
    let watch = Watch::default();
    watch.debugger(child.id());
    watch.cancel();
    assert!(ended(&mut child));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && before
            .iter()
            .any(|p| std::path::Path::new(&format!("/proc/{p}/exe")).exists())
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let left: Vec<_> = before
        .iter()
        .filter(|p| std::path::Path::new(&format!("/proc/{p}/exe")).exists())
        .collect();
    assert!(left.is_empty(), "still alive: {left:?}");
}
