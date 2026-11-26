//! Ending a debugger session from another thread. A call into gdb or Delve
//! cannot be interrupted, and the test process under the debugger outlives
//! a Cherenkov that gives up on it: so the debugger's process tree is killed
//! from the outside, and the call blocked on its pipe returns an error.

use std::sync::{Mutex, MutexGuard};

use cherenkov_run_ports::executor::Cancel;
use rustix::process::{Pid, Signal, kill_process};

/// What a session's owner and its canceller share: whether the session was
/// cancelled, and the debugger process that cancelling kills.
#[derive(Default)]
pub struct Watch {
    state: Mutex<Watched>,
}

#[derive(Default)]
struct Watched {
    cancelled: bool,
    debugger: Option<u32>,
}

impl Watch {
    /// The session's debugger process, just spawned. When the watch is
    /// already cancelled, it is killed at once.
    pub fn debugger(&self, pid: u32) {
        let mut state = self.locked();
        state.debugger = Some(pid);
        if state.cancelled {
            kill_tree(pid);
        }
    }

    /// The session ends its own debugger process, which it reaps next: a
    /// cancel after this signals nothing (the pid may be another process's).
    /// Call it before `Child::wait`.
    pub fn released(&self) {
        self.locked().debugger = None;
    }

    /// Whether the session was cancelled.
    pub fn cancelled(&self) -> bool {
        self.locked().cancelled
    }

    /// The state, held across a poisoned lock: a panic elsewhere must not
    /// stop a cancel.
    fn locked(&self) -> MutexGuard<'_, Watched> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Cancel for Watch {
    fn cancel(&self) {
        let mut state = self.locked();
        if state.cancelled {
            return;
        }
        state.cancelled = true;
        if let Some(pid) = state.debugger {
            kill_tree(pid);
        }
    }
}

/// How many levels of descendants below the debugger are killed: the
/// inferior, and what a shell between the debugger and it started.
const LEVELS: usize = 2;

/// Stop the debugger (so it cannot start anything or reap anything while
/// its tree is read and killed), kill its live descendants, then it.
fn kill_tree(debugger: u32) {
    signal(debugger, Signal::STOP);
    for pid in descendants(debugger) {
        signal(pid, Signal::KILL);
    }
    signal(debugger, Signal::KILL);
}

fn signal(pid: u32, signal: Signal) {
    let pid = i32::try_from(pid).ok().and_then(Pid::from_raw);
    if let Some(pid) = pid {
        let _ = kill_process(pid, signal);
    }
}

/// The live processes up to [`LEVELS`] below `pid`, as the OS lists them.
fn descendants(pid: u32) -> Vec<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let Ok(mut system) = System::new() else {
        return Vec::new();
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let live =
        |p: &sysinfo::Process| !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead);
    let mut level = vec![pid];
    let mut found = Vec::new();
    for _ in 0..LEVELS {
        level = system
            .processes()
            .values()
            .filter(|p| {
                live(p)
                    && p.parent()
                        .is_some_and(|parent| level.contains(&parent.as_u32()))
            })
            .map(|p| p.pid().as_u32())
            .collect();
        found.extend(level.iter().copied());
    }
    found
}

#[cfg(test)]
#[path = "cancel.test.rs"]
mod tests;
