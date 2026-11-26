use cherenkov_mutation::execution::{Failure, Observed, TestResult, Tier};
use cherenkov_run_ports::executor::{ExecError, Selection};

use coded_diagnostic::Report;

use super::*;
use crate::reason::Reason;
use crate::test_plans::plans;

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(1),
    }
}

/// Kills `a`'s mutant with `test_a` failing; `b`'s survives. Only
/// `test_a` calls `a`, and no test calls `b` when `b_called` is false.
struct Fake {
    runs: Vec<String>,
    b_called: bool,
}

impl MutantExecutor for Fake {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        let function = plan.function.clone().unwrap_or_default();
        self.runs.push(function.clone());
        Ok(match function.as_str() {
            "a" => Observed {
                outcome: exited(101),
                tests: vec![killer("test_a")],
            },
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn driven(
        &mut self,
        plans: &[MutantPlan],
        _: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        Ok(Some(
            plans
                .iter()
                .map(|p| match p.function.as_deref() {
                    Some("a") => Driven::Tests(vec!["test_a".to_string()]),
                    _ if self.b_called => Driven::Every,
                    _ => Driven::NotCalled,
                })
                .collect(),
        ))
    }

    fn describe(&self) -> Option<String> {
        Some("calc-test".to_string())
    }
}

/// `test` failed, at the line the fake's output names.
fn killer(test: &str) -> TestReport {
    TestReport {
        test: test.to_string(),
        result: TestResult::Failed(Failure {
            location: Some("tests/calc.rs:5:5".to_string()),
            message: "assertion failed".to_string(),
            got: Some("-1".to_string()),
            expected: Some("5".to_string()),
        }),
    }
}

/// What preparing and then running every plan in every program in turn on
/// one worker told: for each plan, what each program made of it.
#[derive(Default)]
struct Told {
    prepared: Vec<Preparation>,
    ran: Vec<Vec<Ran>>,
}

/// The programs the run would record of `prepared`, as a runner reads them:
/// the budget of a passing baseline, and what recording found.
fn programs_of(plans: &[MutantPlan], prepared: &[Preparation]) -> Programs {
    let wall = |p: &Preparation| match &p.baseline {
        RawOutcome::Exited { wall, .. } => *wall,
        _ => Duration::ZERO,
    };
    Programs::new(
        prepared
            .iter()
            .map(|p| Budget::for_mutant(wall(p)))
            .collect(),
        prepared
            .iter()
            .map(|p| {
                let driven = p.recording.as_ref()?.driven.as_ref()?;
                Some(
                    plans
                        .iter()
                        .zip(driven)
                        .map(|(plan, driven)| (plan.id.0.clone(), driven.clone()))
                        .collect(),
                )
            })
            .collect(),
        prepared.iter().map(|p| p.describe.clone()).collect(),
    )
}

/// Prepare `sets`' first set, then run every plan in every program, in plan
/// order, on a worker of each set in turn.
fn run(
    plans: &[MutantPlan],
    mut sets: Vec<Vec<Box<dyn MutantExecutor + Send>>>,
    jobs: usize,
    told: &mut Told,
) -> Result<(), Failed> {
    let mut failed = None;
    let first = std::mem::take(&mut sets[0]);
    sets[0] = prepare(plans, first, jobs, &mut |p| match p {
        Ok(preparation) => told.prepared.push(preparation),
        Err(f) => failed = Some(f),
    });
    if let Some(failed) = failed {
        return Err(failed);
    }
    let programs = programs_of(plans, &told.prepared);
    let mut workers: Vec<Worker> = sets.into_iter().map(Worker::new).collect();
    let n = workers.len();
    // The run refuses a program whose baseline failed: nothing is run.
    if told.prepared.iter().any(|p| p.recording.is_none()) {
        return Ok(());
    }
    for (k, plan) in plans.iter().enumerate() {
        let ran = (0..programs.len())
            .map(|program| workers[k % n].run_in(program, plan, &programs))
            .collect();
        told.ran.push(ran);
    }
    Ok(())
}

fn run_fake(b_called: bool) -> Told {
    let plans = plans();
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Fake {
        runs: Vec::new(),
        b_called,
    })];
    let mut told = Told::default();
    run(&plans, vec![set], 1, &mut told).unwrap();
    told
}

#[test]
fn test_each_program_is_reported_prepared_with_how_its_mutants_choose_tests() {
    let told = run_fake(true);
    assert_eq!(told.prepared.len(), 1);
    assert_eq!(told.prepared[0].program, 0);
    assert_eq!(told.prepared[0].describe.as_deref(), Some("calc-test"));
    let recording = told.prepared[0].recording.as_ref().unwrap();
    assert_eq!(recording.selection, Selection::Recorded);
    assert_eq!(told.ran.len(), 2);
}

#[test]
fn test_a_run_that_fails_a_test_is_told_with_the_tests_that_killed_it_and_those_it_ran() {
    let told = run_fake(true);
    let [a] = &told.ran[0][..] else {
        panic!("{:?}", told.ran[0]);
    };
    assert_eq!(a.program, 0);
    assert_eq!(a.tests, ["test_a"]);
    assert_eq!(
        a.seen,
        Seen::Observed {
            raw: exited(101),
            killed_by: vec![killer("test_a")]
        }
    );
    let [b] = &told.ran[1][..] else {
        panic!("{:?}", told.ran[1]);
    };
    assert!(b.tests.is_empty(), "every test ran it");
    assert_eq!(
        b.seen,
        Seen::Observed {
            raw: exited(0),
            killed_by: Vec::new()
        },
        "no test failed, so none is named"
    );
}

#[test]
fn test_a_mutant_no_test_calls_is_told_not_called_without_a_run() {
    let told = run_fake(false);
    let [b] = &told.ran[1][..] else {
        panic!("{:?}", told.ran[1]);
    };
    assert_eq!(b.seen, Seen::NotCalled);
    assert_eq!(b.took, Duration::ZERO);
}

fn closed() -> ExecError {
    ExecError::of(&Report::new("test-session-closed"))
}

/// Loses the debugger on its first mutant.
struct Loses;

impl MutantExecutor for Loses {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Err(closed())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

#[test]
fn test_a_mutant_the_debugger_fails_on_is_told_with_what_failed_and_the_run_goes_on() {
    let plans = plans();
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Loses)];
    let mut told = Told::default();
    run(&plans, vec![set], 1, &mut told).unwrap();
    assert_eq!(told.ran.len(), 2, "the run went on");
    assert!(told.ran.iter().flatten().all(|ran| ran.seen
        == Seen::Failed {
            why: closed().cause
        }));
}

/// Fails its baseline.
struct Broken;

impl MutantExecutor for Broken {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(Observed {
            outcome: exited(101),
            tests: vec![killer("test_a")],
        })
    }
}

#[test]
fn test_a_program_whose_tests_fail_unmutated_is_reported_with_the_failing_tests_and_no_recording() {
    // The run judges the baseline; the preparer says what it observed.
    let plans = plans();
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Broken)];
    let mut told = Told::default();
    run(&plans, vec![set], 1, &mut told).unwrap();
    let [broken] = &told.prepared[..] else {
        panic!("{:?}", told.prepared);
    };
    assert_eq!(broken.baseline, exited(101));
    assert_eq!(
        broken.baseline_failures,
        ["test_a at tests/calc.rs:5:5: got -1, expected 5"]
    );
    assert_eq!(broken.recording, None);
    assert!(told.ran.is_empty());
}

#[test]
fn test_a_program_the_executor_cannot_baseline_is_a_failure_of_the_preparation() {
    struct Dead;
    impl MutantExecutor for Dead {
        fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
            Ok(exited(0).into())
        }

        fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
            Err(ExecError::of(&Report::new("test-no-debugger")))
        }
    }
    let plans = plans();
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Dead)];
    let error = run(&plans, vec![set], 1, &mut Told::default()).unwrap_err();
    assert_eq!(error.why, Report::new("test-no-debugger"));
}

/// Records which of its program's sessions are open, shared by a set.
struct Tracked {
    program: usize,
    open: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<usize>>>,
}

impl MutantExecutor for Tracked {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        self.open.lock().unwrap().insert(self.program);
        // Survives: every program runs every mutant.
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        self.open.lock().unwrap().insert(self.program);
        Ok(exited(0).into())
    }

    fn close_session(&mut self) {
        self.open.lock().unwrap().remove(&self.program);
    }
}

#[test]
fn test_a_worker_keeps_its_open_sessions_to_its_cap() {
    // A worker that has touched every test program of a large workspace
    // would hold a debugger session (about a gigabyte) for each.
    let plans = plans();
    let open = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let set: Vec<Box<dyn MutantExecutor + Send>> = (0..3)
        .map(|program| {
            Box::new(Tracked {
                program,
                open: open.clone(),
            }) as Box<dyn MutantExecutor + Send>
        })
        .collect();
    run(&plans, vec![set], 1, &mut Told::default()).unwrap();
    let left = open.lock().unwrap().len();
    assert!(left <= SESSIONS_PER_WORKER, "{left} sessions open");
}

#[test]
fn test_a_mutant_in_awaiting_code_runs_every_test_whatever_the_recording_saw() {
    // A future is polled where no test is on the stack, or not at the
    // function's entry: the recording can miss the tests that call it.
    let mut plans = plans();
    plans[0].original = "let x = store.pending().await?;".to_string();
    plans[1].original = "x + 1".to_string();
    let mut driven = vec![Driven::NotCalled, Driven::NotCalled];
    widen_awaiting(&plans, &mut driven);
    assert_eq!(driven, [Driven::Every, Driven::NotCalled]);
}

/// Exits 101 (libtest's failure) with output that names no failing test:
/// the report was lost or the failure was not a test's.
struct FailsWithoutReport;

impl MutantExecutor for FailsWithoutReport {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(Observed {
            outcome: exited(101),
            tests: vec![TestReport {
                test: "a".to_string(),
                result: TestResult::Passed,
            }],
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

#[test]
fn test_a_failing_exit_with_no_failed_test_in_the_output_is_killed_not_survived() {
    let plan = plans().remove(0);
    let mut executor = FailsWithoutReport;
    let ran = run_one(
        0,
        &mut executor,
        Budget::from_baseline(Duration::from_secs(1)),
        Driven::Every,
        &plan,
    );
    let Seen::Observed { raw, killed_by } = ran.seen else {
        panic!("{:?}", ran.seen);
    };
    assert_eq!(
        classify(&raw).verdict,
        cherenkov_mutation::verdict::Verdict::Killed
    );
    assert!(killed_by.is_empty(), "no failed test to name");
}

/// Exits 0 although its output names a failed test.
struct PassesWithAFailure;

impl MutantExecutor for PassesWithAFailure {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(Observed {
            outcome: exited(0),
            tests: vec![killer("test_a")],
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

#[test]
fn test_a_passing_exit_whose_output_names_a_failed_test_is_a_failure_not_a_survivor() {
    let plan = plans().remove(0);
    let mut executor = PassesWithAFailure;
    let ran = run_one(
        0,
        &mut executor,
        Budget::from_baseline(Duration::from_secs(1)),
        Driven::Every,
        &plan,
    );
    let Seen::Failed { why } = ran.seen else {
        panic!("a contradictory run was judged: {:?}", ran.seen);
    };
    assert_eq!(
        why,
        Report::of(&Reason::ExitDisagrees {
            tests: "test_a".to_string()
        })
    );
}

/// Ends as `outcome` with a failed `test_a` reported.
struct EndsWithAFailure(RawOutcome);

impl MutantExecutor for EndsWithAFailure {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(Observed {
            outcome: self.0.clone(),
            tests: vec![killer("test_a")],
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

fn seen_ending(outcome: RawOutcome) -> Seen {
    let plan = plans().remove(0);
    run_one(
        0,
        &mut EndsWithAFailure(outcome),
        Budget::from_baseline(Duration::from_secs(1)),
        Driven::Every,
        &plan,
    )
    .seen
}

#[test]
fn test_a_run_that_reached_nothing_keeps_its_outcome_and_drops_the_reports() {
    for outcome in [
        RawOutcome::NotReached {
            why: Report::new("cherenkov-exec-dap-no-code"),
        },
        RawOutcome::Unsupported {
            why: Report::new("cherenkov-exec-dap-no-code"),
        },
        RawOutcome::Unviable {
            why: Report::new("cherenkov-exec-dap-no-code"),
        },
    ] {
        assert_eq!(
            seen_ending(outcome.clone()),
            Seen::Observed {
                raw: outcome,
                killed_by: Vec::new()
            }
        );
    }
}

#[test]
fn test_a_crash_or_a_failing_exit_keeps_the_reports_that_name_the_killer() {
    for outcome in [
        exited(1),
        RawOutcome::Crashed {
            kind: "panic".into(),
            message: "m".into(),
            tier: Tier::P,
            wall: Duration::from_millis(1),
        },
    ] {
        assert_eq!(
            seen_ending(outcome.clone()),
            Seen::Observed {
                raw: outcome,
                killed_by: vec![killer("test_a")]
            }
        );
    }
}

/// A gate only a canceller opens.
#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    opened: std::sync::Condvar,
    cancels: std::sync::atomic::AtomicUsize,
    /// Set when a program using this gate has passed its baseline.
    passed: std::sync::atomic::AtomicBool,
}

impl Gate {
    fn cancels(&self) -> usize {
        self.cancels.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl cherenkov_run_ports::executor::Cancel for Gate {
    fn cancel(&self) {
        self.cancels
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.open.lock().unwrap() = true;
        self.opened.notify_all();
    }
}

/// Its baseline waits until its canceller is used, then fails; the
/// sessions it closes are counted.
struct HangsUntilCancelled {
    gate: Arc<Gate>,
    closed: Arc<std::sync::atomic::AtomicUsize>,
}

impl MutantExecutor for HangsUntilCancelled {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        let mut open = self.gate.open.lock().unwrap();
        while !*open {
            open = self.gate.opened.wait(open).unwrap();
        }
        Err(ExecError::of(&Report::new("test-cancelled")))
    }

    fn close_session(&mut self) {
        self.closed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn canceller(&self) -> Arc<dyn cherenkov_run_ports::executor::Cancel> {
        self.gate.clone()
    }
}

/// Passes at once; its canceller counts.
struct PassesAtOnce(Arc<Gate>);

impl MutantExecutor for PassesAtOnce {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        self.0
            .passed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(exited(0).into())
    }

    fn canceller(&self) -> Arc<dyn cherenkov_run_ports::executor::Cancel> {
        self.0.clone()
    }
}

#[test]
fn test_a_failed_run_cancels_the_program_still_preparing_and_it_closes_its_session() {
    let gate = Arc::new(Gate::default());
    let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (told, done) = std::sync::mpsc::channel();
    let (g, c) = (gate.clone(), closed.clone());
    std::thread::spawn(move || {
        let set: Vec<Box<dyn MutantExecutor + Send>> = vec![
            Box::new(Broken),
            Box::new(HangsUntilCancelled { gate: g, closed: c }),
        ];
        let kept = prepare(&plans(), set, 2, &mut |_| {});
        let _ = told.send(kept.len());
    });
    let kept = done
        .recv_timeout(Duration::from_secs(5))
        .expect("prepare returned while a program still hung");
    assert_eq!(kept, 1, "only the failed program is handed back");
    assert_eq!(gate.cancels(), 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while closed.load(std::sync::atomic::Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(closed.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn test_a_run_that_prepares_every_program_cancels_none() {
    let gates: Vec<Arc<Gate>> = (0..2).map(|_| Arc::new(Gate::default())).collect();
    let set: Vec<Box<dyn MutantExecutor + Send>> = gates
        .iter()
        .map(|g| Box::new(PassesAtOnce(g.clone())) as Box<dyn MutantExecutor + Send>)
        .collect();
    prepare(&plans(), set, 2, &mut |_| {});
    assert_eq!(gates.iter().map(|g| g.cancels()).sum::<usize>(), 0);
}

/// Fails its baseline only after the program of `after` has passed its own
/// (and a while for that program to be told), so the failure is the last.
struct BrokenAfter(Arc<Gate>);

impl MutantExecutor for BrokenAfter {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        while !self.0.passed.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(200));
        Err(ExecError::of(&Report::new("test-broken")))
    }
}

#[test]
fn test_a_failed_run_does_not_cancel_a_program_that_already_finished() {
    let done = Arc::new(Gate::default());
    let hung = Arc::new(Gate::default());
    let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![
        Box::new(PassesAtOnce(done.clone())),
        Box::new(BrokenAfter(done.clone())),
        Box::new(HangsUntilCancelled {
            gate: hung.clone(),
            closed,
        }),
    ];
    let mut told = Vec::new();
    prepare(&plans(), set, 3, &mut |p| told.push(p.is_ok()));
    assert_eq!(done.cancels(), 0);
    assert_eq!(hung.cancels(), 1);
}
