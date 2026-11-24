//! The store's exclusive lock: one process at a time keeps a run in a store
//! file (`cherenkov run`) or prunes it (`cherenkov store prune`).
//!
//! The lock is angzarr's advisory file lock (`file-lock`: `flock(2)` on a
//! lock file) taken on `<store>.lock`, never waited for: a second taker is
//! refused at once, naming the holder. The operating system releases the
//! lock when the holder's process ends, however it ends, so **a lock cannot
//! outlive its holder**: there is never a stale lock to break. What can
//! outlive it is the holder's record, `<store>.holder` (its pid, its start
//! and its command line), which the lock's crate has no room for; the next
//! holder overwrites it, and [`unlock`] removes it. [`unlock`] therefore
//! refuses while the lock is held, and otherwise clears what a holder left.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use file_lock::{LockError as Taking, Locked};
use serde::{Deserialize, Serialize};

/// What a holder records of itself, where a refused taker can read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub pid: u32,
    /// When it took the lock, in seconds since the epoch.
    pub started: u64,
    /// Its command line.
    pub command: String,
}

impl Holder {
    /// The calling process.
    fn this() -> Holder {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Holder {
            pid: std::process::id(),
            started,
            command: std::env::args().collect::<Vec<_>>().join(" "),
        }
    }

    /// How long ago it took the lock, at `now` (seconds since the epoch).
    pub fn held_for(&self, now: u64) -> Duration {
        Duration::from_secs(now.saturating_sub(self.started))
    }
}

/// Why the store could not be locked, or unlocked.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds the store, as its record says; none when it
    /// left no record (or one that cannot be read).
    Held {
        lock: PathBuf,
        holder: Option<Holder>,
    },
    /// The lock could not be taken for another reason (a directory that
    /// cannot be written, a lock path that is no file).
    Failed { cause: Taking },
    /// The holder's record could not be read or written.
    Record { path: PathBuf, cause: io::Error },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held { lock, .. } => write!(f, "{} is held", lock.display()),
            LockError::Failed { cause } => write!(f, "{cause}"),
            LockError::Record { path, cause } => write!(f, "{}: {cause}", path.display()),
        }
    }
}

impl std::error::Error for LockError {}

/// The store's lock, held until it is dropped, which removes the holder's
/// record.
#[derive(Debug)]
pub struct StoreLock {
    _locked: Locked,
    record: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.record);
    }
}

/// What [`unlock`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Unlocked {
    /// Nothing held the store and nothing was left.
    Free,
    /// Nothing held the store; the record of a holder that did not end
    /// cleanly (it was killed) was removed.
    Cleared(Option<Holder>),
}

/// The lock file of the store at `store`.
pub fn lock_file(store: &Path) -> PathBuf {
    sibling(store, "lock")
}

/// The holder's record of the store at `store`.
pub fn holder_file(store: &Path) -> PathBuf {
    sibling(store, "holder")
}

fn sibling(store: &Path, extension: &str) -> PathBuf {
    let mut name = store.as_os_str().to_owned();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// The filesystem the lock is taken through, and `path` made absolute.
fn seam(path: &Path) -> io::Result<(file_system::Os, PathBuf)> {
    let absolute = std::path::absolute(path)?;
    let cwd = std::env::current_dir()?;
    Ok((file_system::Os::new(os::host(), &cwd)?, absolute))
}

/// Take the lock of the store at `store`, or be refused at once with the
/// holder.
pub fn acquire(store: &Path) -> Result<StoreLock, LockError> {
    let (fs, store) = seam(store).map_err(|cause| record_error(store, cause))?;
    let lock = lock_file(&store);
    match file_lock::lock_within(&fs, &lock, Duration::ZERO) {
        Ok(locked) => record(locked, &store),
        Err(Taking::Timeout { .. }) => Err(LockError::Held {
            holder: read_holder(&store),
            lock,
        }),
        Err(cause) => Err(LockError::Failed { cause }),
    }
}

fn record_error(path: &Path, cause: io::Error) -> LockError {
    LockError::Record {
        path: path.to_path_buf(),
        cause,
    }
}

/// `locked`, with this process written as its holder.
fn record(locked: Locked, store: &Path) -> Result<StoreLock, LockError> {
    let path = holder_file(store);
    let text = serde_json::to_string(&Holder::this()).map_err(io::Error::other);
    text.and_then(|text| std::fs::write(&path, text))
        .map_err(|cause| record_error(&path, cause))?;
    Ok(StoreLock {
        _locked: locked,
        record: path,
    })
}

/// The holder the store's record names, if there is one.
pub fn read_holder(store: &Path) -> Option<Holder> {
    let text = std::fs::read_to_string(holder_file(store)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Clear what a holder left of the store at `store`: refused while any
/// process holds it, since the operating system gives the lock up with the
/// process and only a live one can be holding it. Makes nothing: a store
/// never locked has nothing to clear.
pub fn unlock(store: &Path) -> Result<Unlocked, LockError> {
    let left = read_holder(store);
    let leftover = holder_file(store).exists();
    if !lock_file(store).exists() && !leftover {
        return Ok(Unlocked::Free);
    }
    // Taking it records this process as the holder, and dropping it removes
    // that record, which is also the one that was left.
    drop(acquire(store)?);
    match (left, leftover) {
        (None, false) => Ok(Unlocked::Free),
        (left, _) => Ok(Unlocked::Cleared(left)),
    }
}

#[cfg(test)]
#[path = "lock.test.rs"]
mod tests;
