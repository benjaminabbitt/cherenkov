//! The core the application's tests run on: the edge over a store, the
//! domains registered, every call sampled from angzarr's `Samples` (a
//! second apart, each under its own command id), and a run id from a
//! counter. No test here reads a clock or mints an id of its own.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::testing::Samples;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{OnRetryable, Registry};
use cherenkov_embedded_edge::{EmbeddedEdge, RunStore, TokioWait};

/// The core over the in-memory store.
pub type Core = EmbeddedEdge<MemoryStore>;

/// The core over `store`, its kernel retrying as `retry` says.
pub async fn new_core_on<S: RunStore>(store: S, retry: OnRetryable) -> EmbeddedEdge<S> {
    let mut registry = Registry::new();
    cherenkov_run_domain::register(&mut registry).expect("the run layer registers");
    let samples = Mutex::new(Samples::starting_at(1_000_000));
    EmbeddedEdge::start(
        store,
        registry,
        retry,
        move || {
            samples
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .next_sample()
        },
        Arc::new(TokioWait),
    )
    .await
}

/// The core over an empty in-memory store.
pub async fn new_core() -> Core {
    new_core_on(MemoryStore::new(), OnRetryable::default()).await
}

/// A run id no other call of this process has been given.
pub fn new_run() -> Uuid {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0x5a; 16];
    bytes[12..].copy_from_slice(&n.to_be_bytes());
    Uuid::from_bytes(bytes)
}
