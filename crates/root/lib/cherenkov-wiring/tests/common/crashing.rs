//! A store that dies at an event write: a process killed inside the call.
//! It forwards every port to the store it wraps, counts the event writes
//! since it was armed, and at the armed one never returns: before the write
//! reaches the store (it does not land), or after it landed (its answer is
//! never given). A store that has died writes no more, as a process that
//! has died does not.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

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

/// What a store and the test that arms it share.
#[derive(Default)]
pub struct Fate {
    crash: Mutex<Option<Crash>>,
    writes: AtomicU64,
    crashed: AtomicBool,
}

impl Fate {
    /// Dies as `crash` says, counting event writes from now.
    pub fn arm(&self, crash: Crash) {
        self.writes.store(0, Ordering::SeqCst);
        self.crashed.store(false, Ordering::SeqCst);
        *self.crash.lock().unwrap_or_else(PoisonError::into_inner) = Some(crash);
    }

    /// Whether a write has died.
    pub fn crashed(&self) -> bool {
        self.crashed.load(Ordering::SeqCst)
    }

    /// The event writes since the store was armed.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::SeqCst)
    }

    /// What happens at the next event write: how it is counted, and
    /// whether, and where, it dies.
    fn next_write(&self) -> Option<Crash> {
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

pub struct Crashing<S> {
    inner: S,
    fate: Arc<Fate>,
}

impl<S> Crashing<S> {
    pub fn new(inner: S, fate: Arc<Fate>) -> Crashing<S> {
        Crashing { inner, fate }
    }
}

impl<S: EventStore> EventStore for Crashing<S> {
    async fn write(&self, write: Write<'_>) -> Result<Written, StoreError> {
        let dying = self.fate.next_write();
        if matches!(dying, Some(Crash::Before(_))) {
            self.fate.die().await;
        }
        let written = self.inner.write(write).await;
        if dying.is_some() {
            self.fate.die().await;
        }
        written
    }

    async fn read(
        &self,
        stream: StreamRef<'_>,
        from: u64,
        to: Option<u64>,
    ) -> Result<Slice, StoreError> {
        self.inner.read(stream, from, to).await
    }

    async fn find_command(
        &self,
        stream: StreamRef<'_>,
        command_id: uuid::Uuid,
    ) -> Result<Vec<EventPage>, StoreError> {
        self.inner.find_command(stream, command_id).await
    }

    async fn covers(&self, domain: &str) -> Result<Vec<Cover>, StoreError> {
        self.inner.covers(domain).await
    }
}

impl<S: SnapshotStore> SnapshotStore for Crashing<S> {
    async fn read_snapshot(&self, stream: StreamRef<'_>) -> Result<Option<Snapshot>, StoreError> {
        self.inner.read_snapshot(stream).await
    }

    async fn write_snapshot(
        &self,
        stream: StreamRef<'_>,
        snapshot: &Snapshot,
    ) -> Result<(), StoreError> {
        self.inner.write_snapshot(stream, snapshot).await
    }
}

impl<S: PositionStore> PositionStore for Crashing<S> {
    async fn position(&self, key: PositionKey<'_>) -> Result<u64, StoreError> {
        self.inner.position(key).await
    }
}

impl<S: ReadStore> ReadStore for Crashing<S> {
    type Reads<'s>
        = S::Reads<'s>
    where
        Self: 's;

    async fn begin_read(&self) -> Result<S::Reads<'_>, StoreError> {
        self.inner.begin_read().await
    }
}

impl<S: ListStreams> ListStreams for Crashing<S> {
    async fn list_streams(&self) -> Result<Vec<Cover>, StoreError> {
        self.inner.list_streams().await
    }
}

impl<S: Processed> Processed for Crashing<S> {
    async fn processed(&self, handler: &str) -> Result<Vec<(Cover, u64)>, StoreError> {
        self.inner.processed(handler).await
    }
}

impl<S: Watch> Watch for Crashing<S> {
    async fn change_token(&self) -> Result<ChangeToken, StoreError> {
        self.inner.change_token().await
    }
}

impl<S: CommitNotify> CommitNotify for Crashing<S> {
    fn next_commit(&self) -> CommitWait {
        self.inner.next_commit()
    }
}

impl<S: Changes> Changes for Crashing<S> {
    async fn changes_cursor(&self) -> Result<ChangeCursor, StoreError> {
        self.inner.changes_cursor().await
    }

    async fn changed_since(
        &self,
        since: &ChangeCursor,
        limit: usize,
    ) -> Result<Changed, StoreError> {
        self.inner.changed_since(since, limit).await
    }
}

impl<S: Outbox> Outbox for Crashing<S> {
    async fn pending(&self, limit: usize) -> Result<Vec<OutboxEntry>, StoreError> {
        self.inner.pending(limit).await
    }

    async fn clear(&self, ids: &[uuid::Uuid]) -> Result<u64, StoreError> {
        self.inner.clear(ids).await
    }
}
