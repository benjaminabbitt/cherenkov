//! Where a run's streams are kept. The run application asks only for
//! angzarr's store ports (`RunStore` in `cherenkov-embedded-edge`), so any
//! store with them serves: angzarr's SQLite store ([`open`]) in a file under
//! the project, which keeps a run past the process (a killed run is resumed
//! by the next, `layer::start`), or angzarr-ports' in-memory store
//! ([`memory`]), which keeps one for the process's life.

use std::io;
use std::path::{Path, PathBuf};

use angzarr_ports::StoreError;
use angzarr_ports::memory::MemoryStore;

pub use angzarr_store_sqlite::SqliteStore;

/// The en-US messages of the errors a store can fail with, for the program
/// that renders them.
pub const CATALOG_SOURCES: [(&str, &str); 1] = [angzarr_store_sqlite::EN_US];

/// The store a run is kept in when it need not outlive the process: in
/// memory, empty.
pub fn memory() -> MemoryStore {
    MemoryStore::new()
}

/// The durable store in the file at `path`: made if it is not there (its
/// directory is the caller's to make), migrated if an earlier build made it.
/// A file that is not a store is refused and left as it is. One process
/// writes a store at a time.
pub async fn open(path: &Path) -> Result<SqliteStore, StoreError> {
    SqliteStore::open(path).await
}

/// Where a project's store is: `.cherenkov/store/runs.sqlite` under its root.
/// The directory is made, holding a `.gitignore` of everything, so the store
/// is never committed whatever the project's own ignore rules say (the
/// project's verdict file, beside it, is).
pub fn in_project(root: &Path) -> io::Result<PathBuf> {
    let directory = root.join(".cherenkov").join("store");
    std::fs::create_dir_all(&directory)?;
    let ignore = directory.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n")?;
    }
    Ok(directory.join("runs.sqlite"))
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
