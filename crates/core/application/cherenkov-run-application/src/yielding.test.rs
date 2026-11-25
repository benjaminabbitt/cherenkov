//! A store for the tests that lets other work run inside every call, as a
//! store doing real I/O does: the in-memory store, yielding to the runtime
//! before each call, and counting the writes it refused for losing a race
//! on their stream (the expected length was not the stream's). It can also
//! die at an event write: the write never lands, or lands and its answer is
//! never given, and the call never returns, as a process killed in the call
//! does not.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use angzarr_embedded::angzarr_ports::memory::{MemoryRead, MemoryStore};
use angzarr_embedded::angzarr_ports::{ChangeCursor, Changed, Changes};
use angzarr_embedded::v1::{Cover, EventPage, Snapshot};
use angzarr_embedded::{
    ChangeToken, CommitNotify, CommitWait, EventStore, ListStreams, Outbox, OutboxEntry,
    PositionKey, PositionStore, Processed, ReadStore, Slice, SnapshotStore, StoreError, StreamRef,
    Watch, Write, Written,
};

/// Where an armed store dies: at its `n`th event write (from 1) since it
/// was armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crash {
    /// Before write `n` reaches the store: it does not land.
    Before(u64),
    /// After write `n` landed: its answer is never given.
    After(u64),
    /// Never: the writes are only counted.
    Never,
}

#[derive(Clone, Default)]
pub struct Yielding {
    inner: MemoryStore,
    conflicts: Arc<AtomicU64>,
    crash: Arc<Mutex<Option<Crash>>>,
    writes: Arc<AtomicU64>,
    crashed: Arc<AtomicBool>,
}

impl Yielding {
    /// The writes refused so far for losing a race on their stream.
    pub fn conflicts(&self) -> u64 {
        self.conflicts.load(Ordering::SeqCst)
    }
}

impl Yielding {
    /// Dies as `crash` says, counting event writes from now.
    pub fn arm(&self, crash: Crash) {
        self.writes.store(0, Ordering::SeqCst);
        self.crashed.store(false, Ordering::SeqCst);
        *self.crash.lock().unwrap_or_else(PoisonError::into_inner) = Some(crash);
    }

    /// Dies no more.
    pub fn disarm(&self) {
        *self.crash.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether a write has died.
    pub fn crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }

    /// The event writes since the store was armed.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::SeqCst)
    }

    /// What the store does at its next event write: counted, and, when it
    /// is the armed one (or a store that has died is written to), died at.
    fn dying(&self) -> Option<Crash> {
        let armed = *self.crash.lock().unwrap_or_else(PoisonError::into_inner);
        armed?;
        let nth = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        if self.crashed() {
            return Some(Crash::Before(nth));
        }
        armed.filter(|crash| matches!(crash, Crash::Before(n) | Crash::After(n) if *n == nth))
    }

    /// Never returns: the process died in the call.
    async fn die(&self) {
        self.crashed.store(true, Ordering::SeqCst);
        std::future::pending::<()>().await;
    }
}

async fn yielded() {
    tokio::task::yield_now().await;
}

impl EventStore for Yielding {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        yielded().await;
        let dying = self.dying();
        if dying.is_some_and(|crash| matches!(crash, Crash::Before(_))) {
            self.die().await;
        }
        let written = self.inner.write(write).await;
        if let Err(StoreError::Conflict { .. }) = &written {
            self.conflicts.fetch_add(1, Ordering::SeqCst);
        }
        if dying.is_some() {
            self.die().await;
        }
        written
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        yielded().await;
        self.inner.read(stream, from, to).await
    }

    async fn find_command(
        &self,
        stream: StreamRef<'_>,
        command_id: uuid::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        yielded().await;
        self.inner.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        yielded().await;
        self.inner.covers(domain).await
    }
}

impl SnapshotStore for Yielding {
    async fn read_snapshot(&self, stream: StreamRef<'_>) -> Result<Option<Snapshot>, StoreError> {
        yielded().await;
        self.inner.read_snapshot(stream).await
    }

    async fn write_snapshot(
        &self,
        stream: StreamRef<'_>,
        snapshot: &Snapshot,
    ) -> Result<(), StoreError> {
        yielded().await;
        self.inner.write_snapshot(stream, snapshot).await
    }
}

impl PositionStore for Yielding {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        yielded().await;
        self.inner.position(key).await
    }
}

impl ReadStore for Yielding {
    type Reads<'s> = MemoryRead;

    async fn begin_read(&self) -> Result<MemoryRead, StoreError> {
        yielded().await;
        self.inner.begin_read().await
    }
}

impl ListStreams for Yielding {
    async fn list_streams(&self) -> Result<Vec<Cover>, StoreError> {
        yielded().await;
        self.inner.list_streams().await
    }
}

impl Processed for Yielding {
    async fn processed(&self, handler: &str) -> Result<Vec<(Cover, u64)>, StoreError> {
        yielded().await;
        self.inner.processed(handler).await
    }
}

impl Watch for Yielding {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        yielded().await;
        self.inner.change_token().await
    }
}

impl CommitNotify for Yielding {
    fn next_commit(&self) -> CommitWait {
        self.inner.next_commit()
    }
}

impl Changes for Yielding {
    async fn changes_cursor(&self) -> Result<ChangeCursor, StoreError> {
        yielded().await;
        self.inner.changes_cursor().await
    }

    async fn changed_since(
        &self,
        since: &ChangeCursor,
        limit: usize,
    ) -> Result<Changed, StoreError> {
        yielded().await;
        self.inner.changed_since(since, limit).await
    }
}

impl Outbox for Yielding {
    async fn pending(&self, limit: usize) -> Result<Vec<OutboxEntry>, StoreError> {
        yielded().await;
        self.inner.pending(limit).await
    }

    async fn clear(&self, ids: &[uuid::Uuid]) -> Result<u64, StoreError> {
        yielded().await;
        self.inner.clear(ids).await
    }
}
