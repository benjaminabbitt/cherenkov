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
        chain(thread_wait(), debugger, debuggee)
    }
}

/// The wait of a run's causal chain: its waiting thread, then the debugger
/// and the debuggee.
fn chain(thread: Duration, debugger: Duration, debuggee: Duration) -> Duration {
    thread + debugger + debuggee
}

/// `read` if there is one, else what was `seen` last.
fn remember(seen: &mut Duration, read: Option<Duration>) -> Duration {
    *seen = read.map_or(*seen, |read| read.max(*seen));
    *seen
}

/// How a bounded wait for an answer ended without one.
#[derive(Debug, PartialEq, Eq)]
pub enum Within<T> {
    /// The answer came.
    Done(T),
    /// The run's own budget ran out first.
    Spent,
    /// The wait had its whole time, net of what the scheduler took from it.
    Unanswered,
}

/// Waits for an answer `attempt` gives within windows of time: `own` at
/// first, and then, each time a window passes unanswered, as much as the
/// scheduler kept the run waiting for a CPU meanwhile (the answer was not
/// late, the machine was busy), never past what the allowance has left.
/// `attempt` returns `Ok(None)` when its window passed without an answer.
pub fn within<T, E>(
    allowance: &mut Allowance,
    own: Duration,
    mut attempt: impl FnMut(Duration) -> Result<Option<T>, E>,
) -> Result<Within<T>, E> {
    let before = allowance.finish();
    let mut granted = Duration::ZERO;
    let mut cut = allowance.left() < own;
    let mut window = own.min(allowance.left());
    loop {
        if window.is_zero() {
            return Ok(Within::Spent);
        }
        if let Some(answer) = attempt(window)? {
            return Ok(Within::Done(answer));
        }
        let lost = allowance.finish().saturating_sub(before);
        let more = lost.saturating_sub(granted);
        let left = allowance.left();
        if left.is_zero() {
            return Ok(Within::Spent);
        }
        if more.is_zero() {
            return Ok(if cut {
                Within::Spent
            } else {
                Within::Unanswered
            });
        }
        granted += more;
        cut |= left < more;
        window = more.min(left);
    }
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
        let before = thread_wait();
        compete_for_one_cpu();
        assert!(thread_wait() > before + Duration::from_millis(20));
    }

    /// Pin two busy threads to this thread's CPU: the one that gets
    /// preempted waits, and the readings show it.
    #[cfg(target_os = "linux")]
    fn compete_for_one_cpu() {
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
    fn test_scheduling_reads_the_waiting_thread_though_the_processes_are_gone() {
        let mut s = Scheduling::new(u32::MAX, u32::MAX);
        let before = s.waited();
        compete_for_one_cpu();
        assert!(s.waited() > before + Duration::from_millis(20));
    }

    #[test]
    fn test_the_chain_waits_for_the_thread_the_debugger_and_the_debuggee() {
        assert_eq!(chain(secs(1), secs(2), secs(4)), secs(7));
    }

    #[test]
    fn test_a_new_allowance_has_all_its_budget_left() {
        let mut a = Allowance::new(secs(60), Box::new(Fake::default()));
        let left = a.left();
        assert!(left > secs(30) && left <= secs(60), "{left:?}");
    }

    #[test]
    fn test_a_process_that_can_no_longer_be_read_keeps_the_wait_it_had() {
        let mut seen = Duration::ZERO;
        assert_eq!(remember(&mut seen, Some(secs(3))), secs(3));
        assert_eq!(remember(&mut seen, None), secs(3));
        assert_eq!(remember(&mut seen, Some(secs(2))), secs(3));
        assert_eq!(remember(&mut seen, Some(secs(5))), secs(5));
    }

    #[test]
    fn test_a_window_the_scheduler_ate_is_given_again_before_the_answer_is_late() {
        let fake = Fake::default();
        let mut a = Allowance::new(secs(60), Box::new(fake.clone()));
        let mut windows = Vec::new();
        let got = within(&mut a, secs(5), |w| -> Result<Option<u8>, ()> {
            windows.push(w);
            if windows.len() == 1 {
                // The first window passed with three seconds spent waiting.
                fake.set(secs(3));
                Ok(None)
            } else {
                Ok(Some(7))
            }
        });
        assert_eq!(got, Ok(Within::Done(7)));
        assert_eq!(windows, vec![secs(5), secs(3)]);
    }

    #[test]
    fn test_an_unanswered_window_on_an_idle_machine_is_unanswered() {
        let mut a = Allowance::new(secs(60), Box::new(Fake::default()));
        let got = within(&mut a, secs(5), |_| -> Result<Option<u8>, ()> { Ok(None) });
        assert_eq!(got, Ok(Within::Unanswered));
    }

    #[test]
    fn test_a_window_cut_short_by_the_budget_is_spent_not_unanswered() {
        let mut a = Allowance::new(secs(2), Box::new(Fake::default()));
        let got = within(&mut a, secs(5), |_| -> Result<Option<u8>, ()> { Ok(None) });
        assert_eq!(got, Ok(Within::Spent));
    }

    // The first window fits the allowance; the scheduler then takes 200 ms,
    // but by the time it is credited the run's own 10 ms budget is already
    // passed, so less than the credit is left. The extra window that follows
    // is cut short by the allowance, and an answer that does not come in it
    // is the run's budget spent, not an evaluation that went unanswered.
    // Elapsed time only grows, so overshooting the sleep still gives Spent.
    #[test]
    fn test_a_later_window_cut_short_by_the_budget_is_spent_not_unanswered() {
        let fake = Fake::default();
        let mut a = Allowance::new(Duration::from_millis(10), Box::new(fake.clone()));
        let mut n = 0;
        let got = within(
            &mut a,
            Duration::from_nanos(1),
            |_| -> Result<Option<u8>, ()> {
                n += 1;
                if n == 1 {
                    std::thread::sleep(Duration::from_millis(20));
                    fake.set(Duration::from_millis(200));
                }
                Ok(None)
            },
        );
        assert_eq!(got, Ok(Within::Spent));
    }

    #[test]
    fn test_waiting_keeps_being_given_until_the_machine_stops_taking_it() {
        let fake = Fake::default();
        let mut a = Allowance::new(secs(60), Box::new(fake.clone()));
        let mut n = 0;
        let got = within(&mut a, secs(5), |_| -> Result<Option<u8>, ()> {
            n += 1;
            // Two windows lose time, the third loses none and stays silent.
            fake.set(secs(if n == 1 { 2 } else { 3 }));
            Ok(None)
        });
        assert_eq!(got, Ok(Within::Unanswered));
        assert_eq!(n, 3);
    }
}
