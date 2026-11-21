//! A run's time allowance, stretched by what machine load took from it.
//!
//! A mutant is timed out when it runs past a budget derived from the
//! unmutated run. Wall time alone makes the verdict depend on how busy the
//! machine is: a mutant that waits in the scheduler's queue for the CPU is
//! slow, not hung. The kernel counts that wait per thread (`schedstat`), so
//! an [`Allowance`] adds the time its run spent runnable but not running
//! (the test program's, the debugger's and the waiting thread's own) to the
//! budget. A program that is hung stays hung: it burns the stretched budget
//! too, only later, and never past [`BACKSTOP`] times the budget.

use std::time::{Duration, Instant};

/// The most the budget stretches to, in budgets: a run past it is timed
/// out however long the machine kept it waiting.
pub const BACKSTOP: u32 = 6;

/// Cumulative time what a run depends on has spent runnable and waiting
/// for a CPU.
pub trait Waiting {
    fn waited(&mut self) -> Duration;
}

/// The time one run may take: the budget plus the scheduler wait it
/// suffered, up to the backstop.
pub struct Allowance {
    timeout: Duration,
    started: Instant,
    waiting: Box<dyn Waiting + Send>,
    base: Duration,
    lost: Duration,
}

impl Allowance {
    /// An allowance of `timeout` starting now, measuring wait from here.
    pub fn new(timeout: Duration, mut waiting: Box<dyn Waiting + Send>) -> Allowance {
        let base = waiting.waited();
        Allowance {
            timeout,
            started: Instant::now(),
            waiting,
            base,
            lost: Duration::ZERO,
        }
    }

    /// What is left of the allowance now.
    pub fn left(&mut self) -> Duration {
        self.left_at(self.started.elapsed())
    }

    /// What is left of the allowance `elapsed` into the run. The wait is
    /// read only once the allowance is spent as it stands.
    pub fn left_at(&mut self, elapsed: Duration) -> Duration {
        if elapsed >= self.timeout + self.lost {
            self.read();
        }
        (self.timeout + self.lost).saturating_sub(elapsed)
    }

    /// The time the run lost to waiting for a CPU, as of the last reading.
    pub fn lost(&self) -> Duration {
        self.lost
    }

    /// The run is over: the time it lost to waiting for a CPU, read afresh.
    pub fn finish(&mut self) -> Duration {
        self.read();
        self.lost
    }

    /// Read the wait now, up to what the backstop allows.
    fn read(&mut self) {
        let waited = self.waiting.waited().saturating_sub(self.base);
        self.lost = waited.min(self.timeout * (BACKSTOP - 1));
    }
}

/// The scheduler wait in a `schedstat` file (its second field, ns).
pub fn parse_schedstat(text: &str) -> Option<Duration> {
    let mut fields = text.split_whitespace();
    let waited = fields.nth(1)?.parse().ok()?;
    fields.next()?;
    Some(Duration::from_nanos(waited))
}

/// The wait of the busiest thread of process `pid`, if it can be read.
pub fn process_wait(pid: u32) -> Option<Duration> {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .ok()?
        .filter_map(|task| {
            let path = task.ok()?.path().join("schedstat");
            parse_schedstat(&std::fs::read_to_string(path).ok()?)
        })
        .max()
}

/// The wait of the calling thread.
pub fn thread_wait() -> Duration {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|text| parse_schedstat(&text))
        .unwrap_or_default()
}

/// The wait of the waiting thread plus the debugger and the debuggee: the
/// causal chain of one run.
pub struct Scheduling {
    debugger: u32,
    debuggee: u32,
    /// What each process had waited when last read: a process that has
    /// exited cannot be read, and its wait is not undone by that.
    seen: [Duration; 2],
}

impl Scheduling {
    pub fn new(debugger: u32, debuggee: u32) -> Scheduling {
        Scheduling {
            debugger,
            debuggee,
            seen: [Duration::ZERO; 2],
        }
    }
}

impl Waiting for Scheduling {
    fn waited(&mut self) -> Duration {
        let debugger = remember(&mut self.seen[0], process_wait(self.debugger));
        let debuggee = remember(&mut self.seen[1], process_wait(self.debuggee));
        thread_wait() + debugger + debuggee
    }
}

/// `read` if there is one, else what was `seen` last.
fn remember(seen: &mut Duration, read: Option<Duration>) -> Duration {
    *seen = read.map_or(*seen, |read| read.max(*seen));
    *seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const BUDGET: Duration = Duration::from_secs(2);

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// A wait that reads whatever the test sets, counting readings.
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<(Duration, u32)>>);

    impl Fake {
        fn set(&self, wait: Duration) {
            self.0.lock().unwrap().0 = wait;
        }
        fn readings(&self) -> u32 {
            self.0.lock().unwrap().1
        }
    }

    impl Waiting for Fake {
        fn waited(&mut self) -> Duration {
            let mut state = self.0.lock().unwrap();
            state.1 += 1;
            state.0
        }
    }

    fn allowance(fake: &Fake) -> Allowance {
        Allowance::new(BUDGET, Box::new(fake.clone()))
    }

    #[test]
    fn test_an_idle_machine_leaves_the_budget_as_it_is() {
        let fake = Fake::default();
        let mut a = allowance(&fake);
        assert_eq!(a.left_at(secs(1)), secs(1));
        assert_eq!(a.left_at(secs(2)), Duration::ZERO);
        assert_eq!(a.left_at(secs(3)), Duration::ZERO);
    }

    #[test]
    fn test_time_spent_waiting_for_a_cpu_is_added_to_the_budget() {
        let fake = Fake::default();
        let mut a = allowance(&fake);
        fake.set(secs(3));
        // At the budget's end, three of the seconds were waiting.
        assert_eq!(a.left_at(secs(2)), secs(3));
        assert_eq!(a.lost(), secs(3));
        assert_eq!(a.left_at(secs(4)), secs(1));
    }

    #[test]
    fn test_only_the_wait_since_the_run_began_counts() {
        let fake = Fake::default();
        fake.set(secs(100));
        let mut a = allowance(&fake);
        fake.set(secs(101));
        assert_eq!(a.left_at(secs(2)), secs(1));
    }

    #[test]
    fn test_a_budget_not_yet_spent_does_not_read_the_wait() {
        let fake = Fake::default();
        let mut a = allowance(&fake);
        let before = fake.readings();
        assert_eq!(a.left_at(secs(1)), secs(1));
        assert_eq!(fake.readings(), before);
    }

    #[test]
    fn test_a_run_the_machine_starves_is_still_timed_out_at_the_backstop() {
        let fake = Fake::default();
        let mut a = allowance(&fake);
        let mut elapsed = secs(0);
        // Always runnable, never running: every second is lost.
        while a.left_at(elapsed) > Duration::ZERO {
            elapsed += secs(1);
            fake.set(elapsed);
            assert!(elapsed <= BUDGET * BACKSTOP, "ran past the backstop");
        }
        assert_eq!(elapsed, BUDGET * BACKSTOP);
    }

    #[test]
    fn test_a_finished_run_counts_the_wait_up_to_its_end() {
        let fake = Fake::default();
        let mut a = allowance(&fake);
        fake.set(secs(1));
        assert_eq!(a.left_at(secs(2)), secs(1));
        // It ended 1.5 s later, having waited two seconds more meanwhile.
        fake.set(secs(3));
        assert_eq!(a.finish(), secs(3));
        assert_eq!(a.lost(), secs(3));
    }

    #[test]
    fn test_a_wait_that_shrinks_loses_nothing() {
        let fake = Fake::default();
        fake.set(secs(10));
        let mut a = allowance(&fake);
        fake.set(secs(4));
        assert_eq!(a.left_at(secs(2)), Duration::ZERO);
        assert_eq!(a.lost(), Duration::ZERO);
    }

    #[test]
    fn test_a_schedstat_file_gives_its_run_queue_wait() {
        assert_eq!(
            parse_schedstat("483828 142194 4\n"),
            Some(Duration::from_nanos(142_194))
        );
        assert_eq!(parse_schedstat("1 2"), None);
        assert_eq!(parse_schedstat("1 x 3"), None);
        assert_eq!(parse_schedstat(""), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_this_process_has_a_wait_to_read_and_a_dead_one_has_none() {
        assert!(process_wait(std::process::id()).is_some());
        assert!(process_wait(u32::MAX).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_a_thread_competing_for_one_cpu_waits() {
        // Pin two busy threads to this thread's CPU: the one that gets
        // preempted waits, and the reading shows it.
        let before = thread_wait();
        let busy = |until: Instant| while Instant::now() < until {};
        let until = Instant::now() + Duration::from_millis(300);
        let pinned = pin_to_one_cpu();
        let rival = std::thread::spawn(move || {
            pin_to(pinned);
            busy(until);
        });
        pin_to(pinned);
        busy(until);
        rival.join().unwrap();
        assert!(thread_wait() > before + Duration::from_millis(20));
    }

    #[cfg(target_os = "linux")]
    fn pin_to_one_cpu() -> usize {
        rustix::thread::sched_getcpu()
    }

    #[cfg(target_os = "linux")]
    fn pin_to(cpu: usize) {
        let mut set = rustix::thread::CpuSet::new();
        set.set(cpu);
        rustix::thread::sched_setaffinity(None, &set).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_scheduling_reads_the_thread_though_the_processes_are_gone() {
        let before = thread_wait();
        let mut s = Scheduling::new(u32::MAX, u32::MAX);
        assert!(s.waited() >= before);
    }

    #[test]
    fn test_a_process_that_can_no_longer_be_read_keeps_the_wait_it_had() {
        let mut seen = Duration::ZERO;
        assert_eq!(remember(&mut seen, Some(secs(3))), secs(3));
        assert_eq!(remember(&mut seen, None), secs(3));
        assert_eq!(remember(&mut seen, Some(secs(2))), secs(3));
        assert_eq!(remember(&mut seen, Some(secs(5))), secs(5));
    }
}
