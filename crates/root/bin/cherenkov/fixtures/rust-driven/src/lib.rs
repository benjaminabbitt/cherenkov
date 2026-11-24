//! Code the tests drive, code they do not, and tests in a file of their own.

pub fn called(a: i64, b: i64) -> i64 {
    a + b
}

pub fn never_called(a: i64, b: i64) -> i64 {
    a - b
}

/// Called by a test that never takes the branch: its mutants there are
/// never executed.
pub fn bump(mut total: i64, n: i64) -> i64 {
    if n > 100 {
        total += n;
    }
    total
}

/// Computed once, kept for every later caller.
fn compute() -> i64 {
    40 + 2
}

pub fn table() -> i64 {
    static TABLE: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *TABLE.get_or_init(compute)
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
