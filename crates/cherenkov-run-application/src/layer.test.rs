use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use angzarr_embedded::OnRetryable;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use cherenkov_core::execution::{Budget, ExecError, RawOutcome, Tier};
use cherenkov_embedded_edge::RunStore;
use cherenkov_run_domain::Verdict;
use cherenkov_run_domain::events::run_event;
use cherenkov_run_ports::Edge;

use super::*;
use crate::host::Readers;
use crate::test_plans::{many, plans};
use crate::testing::{new_core, new_core_on, new_run, recording, recording_to};
use crate::workers::Killer;

/// What a run came to: how it ended, what it printed (the names of the run's
/// events in the order the output was handed them), and what its stream
/// holds.
struct Ran {
    status: Result<Option<RunStatus>, RunLayerError>,
    printed: Vec<&'static str>,
    events: Vec<RunEvent>,
}

/// [`layer::run`] on `store`, the kernel retrying as `retry` says, with an
/// output that records the names of the events it is handed; on a runtime of
/// its own, as a process starts one.
fn run_retrying<S: RunStore>(
    store: S,
    retry: OnRetryable,
    plans: &[MutantPlan],
    workers: Option<Workers>,
) -> Ran {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let core = Arc::new(new_core_on(store, retry).await);
        let (output, printed) = recording();
        let run = new_run();
        let status = super::run(
            &core,
            run.clone(),
            plans,
            &settings(),
            workers,
            Readers::printing(output),
        )
        .await;
        let events = core
            .events(&run_cover(&run), 0)
            .await
            .unwrap()
            .iter()
            .filter_map(run_event)
            .collect();
        let printed = printed.lock().unwrap().clone();
        Ran {
            status,
            printed,
            events,
        }
    })
}

/// [`run_retrying`] in memory, the kernel retrying as it does.
fn run(plans: &[MutantPlan], workers: Option<Workers>) -> Ran {
    run_retrying(MemoryStore::new(), OnRetryable::default(), plans, workers)
}

/// Kills `a`'s mutant (`test_a` fails); `b`'s survives. Counts its runs.
struct Fake {
    runs: Arc<AtomicUsize>,
}

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(1),
    }
}

impl MutantExecutor for Fake {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101),
            _ => exited(0),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }

    fn describe(&self) -> Option<String> {
        Some("calc-test".to_string())
    }
}

fn explain(_: &str) -> Vec<Killer> {
    Vec::new()
}

fn settings() -> Settings {
    Settings {
        root: "/project".to_string(),
        operators: vec![OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string(),
        }],
        jobs: 2,
    }
}

fn workers(runs: &Arc<AtomicUsize>) -> Workers {
    let set =
        || -> Vec<Box<dyn MutantExecutor + Send>> { vec![Box::new(Fake { runs: runs.clone() })] };
    Workers {
        sets: vec![set(), set()],
        jobs: 2,
        explain,
    }
}

/// The mutant ids of the decisions a run recorded, with their verdicts.
fn decisions(events: &[RunEvent]) -> Vec<(String, Verdict)> {
    events
        .iter()
        .filter_map(|event| match event {
            RunEvent::DecisionRecorded(d) => Some((d.mutant.clone(), d.verdict)),
            _ => None,
        })
        .collect()
}

#[test]
fn test_a_run_prints_its_events_from_its_start_to_its_finish() {
    let runs = Arc::new(AtomicUsize::new(0));
    let ran = run(&plans(), Some(workers(&runs)));
    assert_eq!(ran.status, Ok(Some(RunStatus::Complete)));
    assert_eq!(
        ran.printed,
        [
            "run.started",
            "mutant.planned",
            "program.prepared",
            "run.ready",
            "mutant.decided",
            "mutant.decided",
            "run.finished",
        ]
    );
    let verdicts: Vec<Verdict> = decisions(&ran.events).into_iter().map(|d| d.1).collect();
    assert_eq!(verdicts.len(), 2);
    assert!(verdicts.contains(&Verdict::Killed) && verdicts.contains(&Verdict::Survived));
    assert_eq!(runs.load(Ordering::SeqCst), 2, "each mutant ran once");
}

#[test]
fn test_each_decision_the_run_records_holds_the_mutant_and_what_decided_it_whole() {
    let runs = Arc::new(AtomicUsize::new(0));
    let ran = run(&plans(), Some(workers(&runs)));
    let decided: Vec<_> = ran
        .events
        .iter()
        .filter_map(|event| match event {
            RunEvent::DecisionRecorded(d) => d.decision.as_ref(),
            _ => None,
        })
        .collect();
    assert_eq!(decided.len(), 2);
    let a = decided
        .iter()
        .find(|d| {
            d.mutant
                .as_ref()
                .is_some_and(|m| m.function.as_deref() == Some("a"))
        })
        .unwrap();
    assert_eq!(
        a.verdict.as_ref().map(|outcome| outcome.verdict),
        Some(Verdict::Killed)
    );
    assert_eq!(a.programs, ["calc-test"]);
}

#[test]
fn test_a_dry_run_plans_the_run_and_runs_nothing() {
    let ran = run(&plans(), None);
    assert_eq!(ran.status, Ok(None), "it did not finish");
    assert_eq!(ran.printed, ["run.started", "mutant.planned"]);
}

#[test]
fn test_a_run_of_no_mutants_finishes_at_once_and_prints_it() {
    let runs = Arc::new(AtomicUsize::new(0));
    let ran = run(&[], Some(workers(&runs)));
    assert_eq!(ran.status, Ok(Some(RunStatus::Complete)));
    assert_eq!(ran.printed.last(), Some(&"run.finished"));
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[test]
fn test_a_run_with_no_workers_or_no_test_programs_is_refused_saying_so() {
    let none = Workers {
        sets: Vec::new(),
        jobs: 1,
        explain,
    };
    let Err(RunLayerError::Workers { reason }) = run(&plans(), Some(none)).status else {
        panic!("a run with no workers");
    };
    assert_eq!(reason, "no workers");
    let empty = Workers {
        sets: vec![Vec::new()],
        jobs: 1,
        explain,
    };
    let Err(RunLayerError::Workers { reason }) = run(&plans(), Some(empty)).status else {
        panic!("a run with no test programs");
    };
    assert_eq!(reason, "no test programs");
}

/// Fails its baseline.
struct Broken;

impl MutantExecutor for Broken {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(101))
    }
}

#[test]
fn test_a_run_whose_workers_cannot_run_fails_saying_why() {
    let broken = Workers {
        sets: vec![vec![Box::new(Broken)]],
        jobs: 1,
        explain,
    };
    let ran = run(&plans(), Some(broken));
    let Err(RunLayerError::Workers { reason }) = ran.status else {
        panic!("{:?}", ran.status);
    };
    assert!(reason.contains("unmutated tests do not pass"), "{reason}");
    // The stream says so too, and ends, and the output printed both.
    assert!(ran.events.iter().any(|event| matches!(event,
        RunEvent::ProgramFailed(f) if f.reason.contains("unmutated tests do not pass"))));
    assert!(matches!(ran.events.last(),
        Some(RunEvent::RunFinished(f)) if f.status == RunStatus::Failed));
    assert_eq!(
        &ran.printed[ran.printed.len() - 2..],
        ["program.failed", "run.finished"]
    );
}

/// Panics preparing.
struct Panicking;

impl MutantExecutor for Panicking {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        panic!("the debugger fell over");
    }
}

#[test]
fn test_a_preparation_that_panics_finishes_the_run_failed_and_says_so() {
    let panicking = Workers {
        sets: vec![vec![Box::new(Panicking)]],
        jobs: 1,
        explain,
    };
    let ran = run(&plans(), Some(panicking));
    let Err(RunLayerError::Workers { reason }) = ran.status else {
        panic!("{:?}", ran.status);
    };
    assert_eq!(reason, "a test program's preparation panicked");
    assert!(matches!(ran.events.last(),
        Some(RunEvent::RunFinished(f)) if f.status == RunStatus::Failed));
}

/// A gate the test opens: whoever waits on it blocks until then.
#[derive(Clone, Default)]
struct Gate(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

impl Gate {
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    /// Wait until the gate opens, up to a minute.
    fn wait(&self) {
        self.wait_for(Duration::from_secs(60));
    }

    /// Wait until the gate opens, up to `most`.
    fn wait_for(&self, most: Duration) {
        let guard = self.0.0.lock().unwrap();
        let _ = self
            .0
            .1
            .wait_timeout_while(guard, most, |open| !*open)
            .unwrap();
    }
}

/// Program `slow` waits at its baseline until `prepared` opens, and its
/// mutant `b` waits until `decided` opens; the other program and mutant
/// finish at once.
struct Slow {
    program: &'static str,
    prepared: Gate,
    decided: Gate,
}

impl MutantExecutor for Slow {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        if plan.function.as_deref() == Some("b") {
            self.decided.wait();
        }
        Ok(exited(101))
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        if self.program == "slow" {
            self.prepared.wait();
        }
        Ok(exited(0))
    }

    fn describe(&self) -> Option<String> {
        Some(self.program.to_string())
    }
}

/// Mutants `a` and `b` of a program that prepares at once and one that
/// waits for `prepared`; `b` waits for `decided`.
#[test]
fn test_each_event_is_printed_as_it_happens_while_later_work_is_still_running() {
    // An agent acts on the first prepared program and the first decided
    // mutant while the run goes on: neither waits for slower work.
    let (prepared, decided) = (Gate::default(), Gate::default());
    let set = |program| -> Box<dyn MutantExecutor + Send> {
        Box::new(Slow {
            program,
            prepared: prepared.clone(),
            decided: decided.clone(),
        })
    };
    let workers = Workers {
        sets: vec![
            vec![set("quick"), set("slow")],
            vec![set("quick"), set("slow")],
        ],
        jobs: 2,
        explain,
    };
    let (to, names) = std::sync::mpsc::channel();
    let running = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let core = Arc::new(new_core().await);
            super::run(
                &core,
                new_run(),
                &plans(),
                &settings(),
                Some(workers),
                Readers::printing(recording_to(to)),
            )
            .await
        })
    });
    // The first program prepared is printed while the slow one is not.
    assert_eq!(next(&names, "program.prepared"), Some("program.prepared"));
    prepared.open();
    // The first mutant decided is printed while the other is not.
    assert_eq!(next(&names, "mutant.decided"), Some("mutant.decided"));
    decided.open();
    assert_eq!(running.join().unwrap().unwrap(), Some(RunStatus::Complete));
}

/// The next name from `names` that is `wanted`, within ten seconds: none
/// when the run holds it back.
fn next(names: &std::sync::mpsc::Receiver<&'static str>, wanted: &str) -> Option<&'static str> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let name = names.recv_timeout(left).ok()?;
        if name == wanted {
            return Some(name);
        }
    }
}

/// Plan `a`'s run waits for plan `b`'s to finish (up to 5 s): only
/// workers running at once let it end without waiting out.
struct AWaitsForB {
    b_done: Gate,
    waited_out: Arc<std::sync::atomic::AtomicBool>,
}

impl MutantExecutor for AWaitsForB {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        if plan.function.as_deref() == Some("a") {
            let started = std::time::Instant::now();
            self.b_done.wait_for(Duration::from_secs(5));
            if started.elapsed() >= Duration::from_secs(5) {
                self.waited_out.store(true, Ordering::SeqCst);
            }
        } else {
            self.b_done.open();
        }
        Ok(exited(101))
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }
}

#[test]
fn test_mutants_run_on_the_workers_at_once() {
    let b_done = Gate::default();
    let waited_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let set = || -> Vec<Box<dyn MutantExecutor + Send>> {
        vec![Box::new(AWaitsForB {
            b_done: b_done.clone(),
            waited_out: waited_out.clone(),
        })]
    };
    let workers = Workers {
        sets: vec![set(), set()],
        jobs: 2,
        explain,
    };
    assert_eq!(
        run(&plans(), Some(workers)).status,
        Ok(Some(RunStatus::Complete))
    );
    assert!(
        !waited_out.load(Ordering::SeqCst),
        "the two mutants did not run at once"
    );
}

/// The kernel retrying nothing: a write that loses a race on its stream,
/// or fails at all, fails the run instead of running again.
fn strict() -> OnRetryable {
    OnRetryable {
        max_retries: 0,
        max_race_runs: std::num::NonZeroU32::MIN,
        ..OnRetryable::default()
    }
}

#[test]
fn test_many_workers_decide_every_mutant_once_with_no_write_racing_on_any_stream() {
    // One writer per stream: each mutant's stream is written by the worker
    // that ran it, the run's by the fan-in alone. The store lets other work
    // run inside every call, so two writers of one stream would race; a
    // write that lost would be counted, and fail the run, since nothing is
    // retried.
    let plans = many(64);
    let runs = Arc::new(AtomicUsize::new(0));
    let set =
        || -> Vec<Box<dyn MutantExecutor + Send>> { vec![Box::new(Fake { runs: runs.clone() })] };
    let workers = Workers {
        sets: (0..8).map(|_| set()).collect(),
        jobs: 2,
        explain,
    };
    let store = crate::yielding::Yielding::default();
    let ran = run_retrying(store.clone(), strict(), &plans, Some(workers));
    assert_eq!(ran.status, Ok(Some(RunStatus::Complete)));
    assert_eq!(store.conflicts(), 0, "a write lost a race on its stream");
    let mut decided: Vec<String> = decisions(&ran.events).into_iter().map(|d| d.0).collect();
    decided.sort_unstable();
    let ids: Vec<String> = plans.iter().map(|p| p.id.0.clone()).collect();
    assert_eq!(decided, ids, "each mutant decided once");
    assert_eq!(runs.load(Ordering::SeqCst), 64, "each mutant ran once");
    let Some(RunEvent::RunFinished(last)) = ran.events.last() else {
        panic!("{:?}", ran.events.last());
    };
    let counts: Vec<(Verdict, u32)> = last.counts.iter().map(|t| (t.verdict, t.count)).collect();
    assert_eq!(counts, [(Verdict::Killed, 32), (Verdict::Survived, 32)]);
}

#[test]
#[ignore = "measurement: prints what a fact on the run's stream costs as it grows"]
fn test_measure_the_fan_in_s_cost_as_the_run_s_stream_grows() {
    // The kernel loads a stream from its snapshot and the events after it
    // for every input it handles; what is left grows with the stream: one
    // fact per mutant on one run stream meets 1, 2, ... n events in turn.
    // A minute or more: run only when asked, with CHERENKOV_MEASURE set to
    // the number of mutants (2,000 when it is not a number).
    let Some(asked) = std::env::var_os("CHERENKOV_MEASURE") else {
        return;
    };
    let n: usize = asked.to_str().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let ids: Vec<String> = many(n).iter().map(|p| p.id.0.clone()).collect();
    runtime.block_on(async {
        let core = new_core().await;
        let run = new_run();
        let plan = plan_run(&many(n), &settings(), 1);
        let planning = std::time::Instant::now();
        core.command(run_cover(&run), &plan).await.unwrap();
        eprintln!("planning {n} mutants: {:?}", planning.elapsed());
        // Ready, as a run is before any mutant is decided: its snapshot
        // holds the plans until then and not after.
        let ready = std::time::Instant::now();
        core.report(run_cover(&run), &crate::test_plans::prepared(), "program 0")
            .await
            .unwrap();
        eprintln!(
            "the run ready, handing {n} mutants on: {:?}",
            ready.elapsed()
        );
        let mut batch = std::time::Instant::now();
        for (i, id) in ids.iter().enumerate() {
            let fact = cherenkov_run_domain::DecisionReported {
                mutant: id.clone(),
                verdict: cherenkov_run_domain::Verdict::Killed,
                decision: None,
            };
            core.report(run_cover(&run), &fact, id).await.unwrap();
            if (i + 1) % 500 == 0 {
                eprintln!(
                    "facts {}..{}: {:?} per fact",
                    i + 1 - 500,
                    i + 1,
                    batch.elapsed() / 500
                );
                batch = std::time::Instant::now();
            }
        }
    });
    let plans = many(n);
    let set = || -> Vec<Box<dyn MutantExecutor + Send>> {
        vec![Box::new(Fake {
            runs: Arc::new(AtomicUsize::new(0)),
        })]
    };
    let workers = Workers {
        sets: (0..8).map(|_| set()).collect(),
        jobs: 2,
        explain,
    };
    let whole = std::time::Instant::now();
    run(&plans, Some(workers)).status.unwrap();
    eprintln!(
        "a whole run of {n} instant mutants on 8 workers: {:?}",
        whole.elapsed()
    );
}

/// The core, but its first read of a mutant's stream fails: the store was
/// down for a moment.
struct Flaky {
    inner: crate::testing::Core,
    reads: AtomicUsize,
    recovery: cherenkov_run_ports::Recovery,
}

impl Edge for Flaky {
    async fn handle(
        &self,
        book: angzarr_embedded::v1::CommandBook,
    ) -> Result<cherenkov_run_ports::Reported, cherenkov_run_ports::EdgeError> {
        self.inner.handle(book).await
    }

    async fn events(
        &self,
        stream: &angzarr_embedded::v1::Cover,
        from: u64,
    ) -> Result<Vec<angzarr_embedded::v1::EventPage>, cherenkov_run_ports::EdgeError> {
        if stream.domain == cherenkov_run_domain::MUTANT_DOMAIN
            && self.reads.fetch_add(1, Ordering::SeqCst) == 0
        {
            return Err(cherenkov_run_ports::EdgeError {
                cause: angzarr_diagnostic::Report::new("test-store-down"),
                recovery: self.recovery,
            });
        }
        self.inner.events(stream, from).await
    }

    fn next_commit(&self) -> angzarr_embedded::angzarr_ports::CommitWait {
        self.inner.next_commit()
    }
}

impl cherenkov_run_ports::Serving for Flaky {
    fn serve<'a>(
        &'a self,
        subscriptions: cherenkov_run_ports::Subscriptions,
        more: &'a mut (dyn FnMut(&angzarr_embedded::angzarr_bus::Served) -> bool + Send),
    ) -> impl std::future::Future<Output = Result<(), cherenkov_run_ports::EdgeError>> + 'a {
        self.inner.serve(subscriptions, more)
    }
}

/// A run of the two plans on a core whose first mutant read fails with
/// `recovery`.
fn run_on_flaky(recovery: cherenkov_run_ports::Recovery) -> (Ran, Arc<AtomicUsize>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let runs = Arc::new(AtomicUsize::new(0));
    let ran = runtime.block_on(async {
        let core = Arc::new(Flaky {
            inner: new_core().await,
            reads: AtomicUsize::new(0),
            recovery,
        });
        let (output, printed) = recording();
        let status = super::run(
            &core,
            new_run(),
            &plans(),
            &settings(),
            Some(workers(&runs)),
            Readers::printing(output),
        )
        .await;
        let printed = printed.lock().unwrap().clone();
        Ran {
            status,
            printed,
            events: Vec::new(),
        }
    });
    (ran, runs)
}

#[test]
fn test_a_run_whose_handler_fails_once_for_now_is_served_again_and_completes() {
    let (ran, runs) = run_on_flaky(cherenkov_run_ports::Recovery::Retry);
    assert_eq!(ran.status, Ok(Some(RunStatus::Complete)));
    assert_eq!(runs.load(Ordering::SeqCst), 2, "each mutant ran once");
}

#[test]
fn test_a_run_whose_handler_fails_for_good_is_given_up_on_and_reported() {
    let (ran, runs) = run_on_flaky(cherenkov_run_ports::Recovery::Bug);
    assert!(
        matches!(ran.status, Err(RunLayerError::Kernel { .. })),
        "{:?}",
        ran.status
    );
    assert!(
        runs.load(Ordering::SeqCst) < 2,
        "the mutant it failed on was not run"
    );
}
