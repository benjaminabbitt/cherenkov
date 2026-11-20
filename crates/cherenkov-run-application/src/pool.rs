//! The workers no runner instance holds. There are as many workers as
//! instances once the first set is prepared, so an instance handed a mutant
//! finds one; until then (the first set is preparing the run's test
//! programs) an instance waits for it.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::Notify;

use crate::workers::Worker;

/// The idle workers, shared by the runner's instances and whoever gives a
/// worker back.
pub struct Pool {
    idle: Mutex<Vec<Worker>>,
    freed: Notify,
}

impl Pool {
    /// A pool holding `workers`.
    pub fn new(workers: Vec<Worker>) -> Arc<Pool> {
        Arc::new(Pool {
            idle: Mutex::new(workers),
            freed: Notify::new(),
        })
    }

    /// An idle worker, if there is one.
    pub fn try_take(&self) -> Option<Worker> {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
    }

    /// An idle worker; waits for one to be given when there is none.
    pub async fn take(&self) -> Worker {
        loop {
            let freed = self.freed.notified();
            tokio::pin!(freed);
            // Armed before looking: a worker given after the look wakes it.
            freed.as_mut().enable();
            if let Some(worker) = self.try_take() {
                return worker;
            }
            freed.await;
        }
    }

    /// `worker` is idle: one waiting take has it.
    pub fn give(&self, worker: Worker) {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(worker);
        self.freed.notify_one();
    }
}

#[cfg(test)]
#[path = "pool.test.rs"]
mod tests;
