//! What a running cargo says of itself: each crate it finishes compiling,
//! reported as it happens so that a long cold compile is visibly alive.

/// Crates cargo has finished compiling: how many, and the latest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiling {
    pub done: usize,
    pub last: String,
}
