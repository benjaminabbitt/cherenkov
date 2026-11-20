//! Where a run's streams are kept. The run application asks only for
//! angzarr's store ports (`RunStore` in `cherenkov-embedded-edge`), so
//! any store with them serves: today angzarr-ports' in-memory store, which
//! keeps a run for the process's life. A durable store (angzarr-platform's
//! store-jsonl, angzarr's SQLite store) is opened here instead when runs
//! are to survive the process.

use angzarr_ports::memory::MemoryStore;

/// The store a run is kept in: in memory, empty.
pub fn open() -> MemoryStore {
    MemoryStore::new()
}
