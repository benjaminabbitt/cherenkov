//! What a test of a run needs of the clock and the ids (feature
//! `test-support`): angzarr's `Samples` for the calls, a second apart, each
//! under a command id of its own, and run ids numbered from 1.

use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU8, Ordering};

use angzarr_embedded::testing::Samples;
use angzarr_embedded::v1::Uuid;

use crate::Sampling;

/// A sampling no clock or random source feeds. One value, cloned, is one
/// timeline: a second value starts over, and its command ids and run ids
/// repeat those of the first.
pub fn sampling() -> Sampling {
    let samples = Mutex::new(Samples::starting_at(1_000_000));
    let runs = AtomicU8::new(0);
    Sampling::new(
        move || {
            samples
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .next_sample()
        },
        move || Uuid::from_bytes([runs.fetch_add(1, Ordering::SeqCst) + 1; 16]),
    )
}
