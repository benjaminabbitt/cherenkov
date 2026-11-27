//! The workers' work, synchronous: baseline every test program and record
//! what its tests call ([`prepare`], which says what it found and judges
//! nothing: the run does), then, on each [`Worker`], run one mutant in one
//! program and say what was observed ([`Worker::run_in`]). Whether another
//! program is worth running, and what the mutant's verdict is, are the
//! mutant's to decide. Everything goes through the [`MutantExecutor`] port.
//! Which worker runs which mutant is the runner's, on the
//! bus.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, TestReport, classify};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Cancel, Driven, MutantExecutor, Recording};
use coded_diagnostic::Report;

use crate::reason::Reason;

/// The first, unmutated run of each test program is given this long.
const FIRST_RUN: Duration = Duration::from_secs(120);

/// Debugger sessions a worker keeps open at once. A session on a large
/// test binary holds about four times the binary's size in memory, and a
/// worker that has touched every program of a workspace would hold one for
/// each.
pub const SESSIONS_PER_WORKER: usize = 2;

/// What preparing one test program found: its unmutated run as the
/// executor observed it, and, when that passed, what recording it found.
/// Whether the program can run mutants is the run's to judge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preparation {
    /// Its index among the programs.
    pub program: usize,
    /// What it runs (a test binary's path), if the executor says.
    pub describe: Option<String>,
    pub baseline: RawOutcome,
    /// When the baseline did not pass, the failing tests its output names.
    pub baseline_failures: Vec<String>,
    /// When the baseline did not pass, the tail of its output, for
    /// diagnosis only.
    pub baseline_output_tail: Option<String>,
    pub took: Duration,
    /// What recording found; none for a baseline that did not pass, which
    /// the run will refuse.
    pub recording: Option<Recording>,
}

/// What one test program made of one mutant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    /// The program's index among the run's.
    pub program: usize,
    pub took: Duration,
    /// The tests the mutant ran with in this program; none: every test.
    pub tests: Vec<String>,
    pub seen: Seen,
}

/// What came of running a mutant in a test program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    /// No test of the program calls the mutant's function: it was not run.
    NotCalled,
    /// The executor ran it and observed this; for a run that detected the
    /// mutant, the failing tests the program's output names.
    Observed {
        raw: RawOutcome,
        killed_by: Vec<TestReport>,
    },
    /// The debugger (or the harness driving it) failed on this mutant: it
    /// costs this mutant only, and the executor starts a fresh session for
    /// the next.
    Failed { why: Report },
}

/// What the run recorded of its prepared programs, which every worker runs
/// its mutants with: each program's time budget and, per program, which
/// tests call each mutant's function (none when the program cannot tell).
#[derive(Clone, Debug, Default)]
pub struct Programs {
    budgets: Vec<Budget>,
    /// By the mutant's id.
    drives: Vec<Option<BTreeMap<String, Driven>>>,
    /// What each program runs, where the executor says.
    names: Vec<Option<String>>,
}

impl Programs {
    /// The programs, in the order given: budgets, drives and names.
    pub fn new(
        budgets: Vec<Budget>,
        drives: Vec<Option<BTreeMap<String, Driven>>>,
        names: Vec<Option<String>>,
    ) -> Programs {
        Programs {
            budgets,
            drives,
            names,
        }
    }

    /// Whether no program was prepared.
    pub fn is_empty(&self) -> bool {
        self.budgets.is_empty()
    }

    /// How many programs were prepared.
    pub fn len(&self) -> usize {
        self.budgets.len()
    }

    /// The time one mutant may run in program `program`.
    pub fn budget(&self, program: usize) -> Option<Budget> {
        self.budgets.get(program).copied()
    }

    /// Each program's time budget for one mutant, in milliseconds.
    pub fn budgets_ms(&self) -> impl Iterator<Item = u128> + '_ {
        self.budgets.iter().map(|b| b.timeout.as_millis())
    }

    /// What program `program` runs, where the executor said.
    pub fn name(&self, program: usize) -> Option<&str> {
        self.names.get(program)?.as_deref()
    }

    /// Which tests call mutant `id`'s function in program `program`; none
    /// where the program cannot tell.
    pub fn driven(&self, program: usize, id: &str) -> Option<Driven> {
        self.drives.get(program)?.as_ref()?.get(id).cloned()
    }
}

/// The executors of a worker: one for each test program.
type Executors = Vec<Box<dyn MutantExecutor + Send>>;

/// A program's preparation, with the executor that did it handed back.
struct Finished {
    program: usize,
    describe: Option<String>,
    prepared: Result<Preparation, Report>,
    executor: Box<dyn MutantExecutor + Send>,
}

/// Baseline every test program of `set` and record what its tests call,
/// telling `report` each program's preparation, or why it could not be
/// prepared, the moment it is done; `jobs` programs are prepared at once.
/// The first program that cannot be prepared, or whose baseline fails (the
/// run refuses it), ends the batch: the others are not waited out. A
/// program still preparing is cancelled (its debugger and the test process
/// under it are killed before this returns) and abandoned to its own
/// thread, which closes its executor's session when its call returns; its
/// executor is not
/// handed back. Each session is closed after: a worker opens again only what
/// it uses. Returns the executors, in the programs' order, that were not
/// abandoned.
pub fn prepare(
    plans: &[MutantPlan],
    set: Executors,
    jobs: usize,
    report: &mut dyn FnMut(Result<Preparation, Failed>),
) -> Executors {
    if set.is_empty() {
        report(Err(Failed {
            program: None,
            why: Report::of(&Reason::NoExecutors),
        }));
        return set;
    }
    let plans: Arc<[MutantPlan]> = plans.into();
    let mut back: Vec<Option<Box<dyn MutantExecutor + Send>>> = Vec::new();
    back.resize_with(set.len(), || None);
    let mut pending = set.into_iter().enumerate();
    loop {
        let batch: Vec<_> = pending.by_ref().take(jobs.max(1)).collect();
        if batch.is_empty() {
            break;
        }
        if prepare_batch(&plans, batch, &mut back, report) {
            keep_unstarted(pending, &mut back);
            break;
        }
    }
    back.into_iter()
        .flatten()
        .map(|mut executor| {
            executor.close_session();
            executor
        })
        .collect()
}

/// The programs a failed run never started keep their executors.
fn keep_unstarted(
    pending: impl Iterator<Item = (usize, Box<dyn MutantExecutor + Send>)>,
    back: &mut [Option<Box<dyn MutantExecutor + Send>>],
) {
    for (program, executor) in pending {
        back[program] = Some(executor);
    }
}

/// Prepare `batch` at once, each program on a thread of its own: they are
/// independent sessions, and a workspace has dozens. Tells `report` each as
/// it is done, and whether one failed; on the first failure, stops waiting
/// for the others, keeping the programs already done.
fn prepare_batch(
    plans: &Arc<[MutantPlan]>,
    batch: Vec<(usize, Box<dyn MutantExecutor + Send>)>,
    back: &mut [Option<Box<dyn MutantExecutor + Send>>],
    report: &mut dyn FnMut(Result<Preparation, Failed>),
) -> bool {
    let cancels: Vec<(usize, Arc<dyn Cancel>)> =
        batch.iter().map(|(p, e)| (*p, e.canceller())).collect();
    let (sender, done) = std::sync::mpsc::channel();
    for (program, executor) in batch {
        spawn_preparation(program, executor, plans.clone(), sender.clone());
    }
    drop(sender);
    let mut failed = false;
    for finished in done.iter() {
        failed |= settle(finished, back, report);
        if failed {
            break;
        }
    }
    if failed {
        // What has already finished is told; what has not is cancelled and
        // abandoned.
        for finished in done.try_iter() {
            settle(finished, back, report);
        }
        cancel_unfinished(&cancels, back);
        // A cancelled program may send after the drain above and before
        // this returns: its Finished would be buffered and dropped with the
        // receiver, its session never closed. The reaper owns the receiver
        // and closes whatever arrives; it ends when the last thread has sent.
        std::thread::spawn(move || close_abandoned(done));
    }
    failed
}

/// End the session of every program of the batch whose executor is not back:
/// it is still preparing, in a call only its canceller can interrupt.
fn cancel_unfinished(
    cancels: &[(usize, Arc<dyn Cancel>)],
    back: &[Option<Box<dyn MutantExecutor + Send>>],
) {
    for (program, cancel) in cancels {
        if back[*program].is_none() {
            cancel.cancel();
        }
    }
}

/// Keep a finished program's executor and tell `report` what it found;
/// whether it failed.
fn settle(
    finished: Finished,
    back: &mut [Option<Box<dyn MutantExecutor + Send>>],
    report: &mut dyn FnMut(Result<Preparation, Failed>),
) -> bool {
    let Finished {
        program,
        describe,
        prepared,
        executor,
    } = finished;
    back[program] = Some(executor);
    let failed = refused(&prepared);
    report(match prepared {
        Ok(preparation) => Ok(Preparation {
            program,
            ..preparation
        }),
        Err(why) => Err(Failed {
            program: describe,
            why,
        }),
    });
    failed
}

/// Whether the run is bound to refuse this program: it could not be
/// prepared, or its unmutated tests did not pass (nothing was recorded).
fn refused(prepared: &Result<Preparation, Report>) -> bool {
    prepared
        .as_ref()
        .map_or(true, |preparation| preparation.recording.is_none())
}

/// Close the session of every executor that arrives on `done` until every
/// sender is gone: the programs a failed batch abandoned, each closed the
/// moment it finishes, whether its thread sent before or after the batch
/// stopped listening.
fn close_abandoned(done: std::sync::mpsc::Receiver<Finished>) {
    for abandoned in done {
        let mut executor = abandoned.executor;
        executor.close_session();
    }
}

/// Prepare program `program` on a thread of its own, sending it back with
/// its executor. When nobody waits any more (the run failed), the batch has
/// cancelled the program (its call returns an error soon), and the thread
/// closes the executor's session itself as soon as the call returns (when
/// the send fails) or the batch's reaper does (when it succeeds).
fn spawn_preparation(
    program: usize,
    mut executor: Box<dyn MutantExecutor + Send>,
    plans: Arc<[MutantPlan]>,
    sender: std::sync::mpsc::Sender<Finished>,
) {
    std::thread::spawn(move || {
        let describe = executor.describe();
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prepare_one(executor.as_mut(), &plans)
        }))
        .unwrap_or_else(|_| Err(Report::of(&Reason::PreparationPanicked)));
        let finished = Finished {
            program,
            describe,
            prepared,
            executor,
        };
        if let Err(std::sync::mpsc::SendError(mut abandoned)) = sender.send(finished) {
            abandoned.executor.close_session();
        }
    });
}

/// Why a test program could not be prepared at all: named, when the
/// executor says what it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed {
    pub program: Option<String>,
    pub why: Report,
}

/// One worker: its own executor for each test program, and the sessions
/// it holds open, at most [`SESSIONS_PER_WORKER`].
pub struct Worker {
    set: Vec<Box<dyn MutantExecutor + Send>>,
    open: OpenSessions,
}

impl Worker {
    pub fn new(set: Vec<Box<dyn MutantExecutor + Send>>) -> Worker {
        Worker {
            set,
            open: OpenSessions::default(),
        }
    }

    /// Run `plan` in test program `program`; then close the sessions beyond
    /// the cap, least recently used first.
    pub fn run_in(&mut self, program: usize, plan: &MutantPlan, programs: &Programs) -> Ran {
        let (Some(executor), Some(budget)) = (self.set.get_mut(program), programs.budget(program))
        else {
            return Ran {
                program,
                took: Duration::ZERO,
                tests: Vec::new(),
                seen: Seen::Failed {
                    why: Report::of(&Reason::NoProgram {
                        program: program as u64,
                    }),
                },
            };
        };
        let driven = programs
            .driven(program, &plan.id.0)
            .unwrap_or(Driven::Every);
        let ran = run_one(program, executor.as_mut(), budget, driven, plan);
        if ran.seen != Seen::NotCalled {
            self.open.used(program, &mut self.set);
        }
        ran
    }
}

/// Run `plan` in one test program, with the tests that call its function.
fn run_one(
    program: usize,
    executor: &mut (dyn MutantExecutor + Send),
    budget: Budget,
    driven: Driven,
    plan: &MutantPlan,
) -> Ran {
    let started = Instant::now();
    let tests = match driven {
        Driven::NotCalled => {
            return Ran {
                program,
                took: Duration::ZERO,
                tests: Vec::new(),
                seen: Seen::NotCalled,
            };
        }
        // Only the tests that call the mutated function can detect it.
        Driven::Tests(tests) => {
            executor.select_tests(Some(&tests));
            tests
        }
        Driven::Every => {
            executor.select_tests(None);
            Vec::new()
        }
    };
    let seen = match executor.run(plan, budget) {
        Ok(observed) => seen_of(observed),
        Err(e) => Seen::Failed {
            why: Report::of(&e),
        },
    };
    Ran {
        program,
        took: started.elapsed(),
        tests,
        seen,
    }
}

/// What an executor's observation of a run says: a kill names the tests
/// that failed. Exit status and reports are checked against each other only
/// where the outcome says the tests passed; a run that reached nothing
/// (not reached, unsupported, unviable) keeps its outcome and drops what
/// the reports say.
fn seen_of(observed: Observed) -> Seen {
    let detected = classify(&observed.outcome).verdict.detected();
    let killed_by = observed.failures();
    let passed = matches!(observed.outcome, RawOutcome::Exited { code: 0, .. });
    if passed && !killed_by.is_empty() {
        // The exit status and the report disagree: neither can be
        // trusted, and a survivor is not to be claimed from either.
        let tests: Vec<&str> = killed_by.iter().map(|k| k.test.as_str()).collect();
        return Seen::Failed {
            why: Report::of(&Reason::ExitDisagrees {
                tests: tests.join(", "),
            }),
        };
    }
    Seen::Observed {
        raw: observed.outcome,
        killed_by: if detected { killed_by } else { Vec::new() },
    }
}

/// The sessions one worker has open, most recently used first.
#[derive(Default)]
struct OpenSessions(Vec<usize>);

impl OpenSessions {
    /// Program `k` was just used: keep it, and close the least recently
    /// used beyond the cap.
    fn used(&mut self, k: usize, executors: &mut [Box<dyn MutantExecutor + Send>]) {
        self.0.retain(|&j| j != k);
        self.0.insert(0, k);
        while self.0.len() > SESSIONS_PER_WORKER {
            if let Some(j) = self.0.pop() {
                executors[j].close_session();
            }
        }
    }
}

/// One test program's baseline and, when it passed, what its tests call.
fn prepare_one(
    executor: &mut (dyn MutantExecutor + Send),
    plans: &[MutantPlan],
) -> Result<Preparation, Report> {
    let started = Instant::now();
    let Observed {
        outcome: baseline,
        tests,
        output_tail,
    } = executor
        .baseline(Budget::from_baseline(FIRST_RUN))
        .map_err(|e| Report::of(&e))?;
    let describe = executor.describe();
    let RawOutcome::Exited { code: 0, wall, .. } = &baseline else {
        // The run judges the baseline. A program it is bound to refuse is
        // not recorded: it says which tests failed, and no more.
        return Ok(Preparation {
            program: 0,
            describe,
            baseline_failures: tests
                .iter()
                .filter(|t| t.is_failed())
                .map(TestReport::line)
                .collect(),
            baseline,
            baseline_output_tail: output_tail,
            took: started.elapsed(),
            recording: None,
        });
    };
    // The recording run is one long run with its own, generous budget; each
    // mutant's is short, so a hung one is decided in seconds.
    let mut recording = executor
        .recording(plans, Budget::from_baseline(*wall))
        .map_err(|e| Report::of(&e))?;
    if let Some(driven) = &mut recording.driven {
        widen_awaiting(plans, driven);
    }
    Ok(Preparation {
        program: 0,
        describe,
        baseline,
        baseline_failures: Vec::new(),
        baseline_output_tail: None,
        took: started.elapsed(),
        recording: Some(recording),
    })
}

/// A mutant whose code awaits runs with every test: its future may be polled
/// off a test's stack, or past the function's entry, so the recording can
/// miss the tests that call it.
fn widen_awaiting(plans: &[MutantPlan], driven: &mut [Driven]) {
    for (plan, driven) in plans.iter().zip(driven) {
        if plan.original.contains(".await") {
            *driven = Driven::Every;
        }
    }
}

#[cfg(test)]
#[path = "workers.test.rs"]
mod tests;
