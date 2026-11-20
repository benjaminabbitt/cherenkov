use super::*;

fn helper(a: i64) -> i64 {
    a * 2
}

#[test]
fn test_called_adds() {
    assert_eq!(called(helper(1), 3), 5);
}

#[test]
fn test_bump_leaves_small_totals() {
    assert_eq!(bump(1, 2), 1);
}

// The first caller makes the value and checks nothing; the second only
// reads what was kept, never calling `compute` itself: it waits until the
// first has made it (forced, not left to thread timing).
static MADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[test]
fn test_a_touches_the_table() {
    let _ = table();
    MADE.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn test_b_checks_the_table() {
    while !MADE.load(std::sync::atomic::Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(table(), 42);
}
