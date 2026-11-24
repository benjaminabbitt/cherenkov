//! The workers' work, synchronous: baseline every test program and record
//! what its tests call ([`prepare`], which says what it found and judges
//! nothing: the run does), then, on each [`Worker`], run one mutant in one
//! program and say what was observed ([`Worker::run_in`]). Whether another
//! program is worth running, and what the mutant's verdict is, are the
//! mutant's to decide. Everything goes through the [`MutantExecutor`] port.
//! Which worker runs which mutant is the runner's, on the
//! bus.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use cherenkov_mutation::execution::{Budget, RawOutcome, classify};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Driven, MutantExecutor, Recording};

/// The first, unmutated run of each test program is given this long.
const FIRST_RUN: Duration = Duration::from_secs(120);

/// Debugger sessions a worker keeps open at once. A session on a large
/// test binary holds about four times the binary's size in memory, and a
/// worker that has touched every program of a workspace would hold one for
/// each.
pub const SESSIONS_PER_WORKER: usize = 2;

/// A test that failed, as the test runner's output says it did.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Killer {
    pub test: String,
    /// `file:line:col` of the failure.
    pub location: Option<String>,
    pub message: String,
    /// For an equality assertion, the value the mutant produced.
    pub got: Option<String>,
    /// For an equality assertion, the value the test expects.
    pub expected: Option<String>,
}

impl Killer {
    /// `test at location: got X, expected Y`, or `test at location: message`.
    pub fn line(&self) -> String {
        let at = self
            .location
            .as_ref()
            .map(|l| format!(" at {l}"))
            .unwrap_or_default();
        match (&self.got, &self.expected) {
            (Some(got), Some(expected)) => {
                format!("{}{at}: got {got}, expected {expected}", self.test)
            }
            _ => format!("{}{at}: {}", self.test, self.message),
        }
    }
}

/// The failing tests a test program's output reports.
pub type Explain = fn(&str) -> Vec<Killer>;

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
        killed_by: Vec<Killer>,
    },
    /// The debugger (or the harness driving it) failed on this mutant: it
    /// costs this mutant only, and the executor starts a fresh session for
    /// the next.
    Failed { reason: String },
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

/// Baseline every test program of `set` and record what its tests call,
/// telling `report` each program's preparation, or why it could not be
/// prepared, the moment it is done; `jobs` programs are prepared at once.
/// The first program that cannot be prepared ends the batch. Each program's
/// session is closed after: a worker opens again only what it uses.
pub fn prepare(
    plans: &[MutantPlan],
    set: &mut [Box<dyn MutantExecutor + Send>],
    jobs: usize,
    explain: Explain,
    report: &mut dyn FnMut(Result<Preparation, Failed>),
) {
    if set.is_empty() {
        report(Err(Failed {
            program: None,
            reason: "no executors".into(),
        }));
        return;
    }
    // Each test program is baselined and its calls recorded on a thread of
    // its own: they are independent sessions, and a workspace has dozens.
    let mut failed = false;
    for (b, batch) in set.chunks_mut(jobs.max(1)).enumerate() {
        let offset = b * jobs.max(1);
        std::thread::scope(|scope| {
            let (sender, done) = std::sync::mpsc::channel();
            for (i, executor) in batch.iter_mut().enumerate() {
                let sender = sender.clone();
                scope.spawn(move || {
                    let describe = executor.describe();
                    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        prepare_one(executor.as_mut(), plans, explain)
                    }))
                    .unwrap_or_else(|_| Err("a test program's preparation panicked".into()));
                    let _ = sender.send((offset + i, describe, prepared));
                });
            }
            drop(sender);
            for (program, describe, prepared) in done {
                failed |= prepared.is_err();
                report(match prepared {
                    Ok(preparation) => Ok(Preparation {
                        program,
                        ..preparation
                    }),
                    Err(reason) => Err(Failed {
                        program: describe,
                        reason,
                    }),
                });
            }
        });
        if failed {
            break;
        }
    }
    for executor in set.iter_mut() {
        executor.close_session();
    }
}

/// Why a test program could not be prepared at all: named, when the
/// executor says what it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed {
    pub program: Option<String>,
    pub reason: String,
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
    pub fn run_in(
        &mut self,
        program: usize,
        plan: &MutantPlan,
        programs: &Programs,
        explain: Explain,
    ) -> Ran {
        let (Some(executor), Some(budget)) = (self.set.get_mut(program), programs.budget(program))
        else {
            return Ran {
                program,
                took: Duration::ZERO,
                tests: Vec::new(),
                seen: Seen::Failed {
                    reason: format!("the worker has no test program {program}"),
                },
            };
        };
        let driven = programs
            .driven(program, &plan.id.0)
            .unwrap_or(Driven::Every);
        let ran = run_one(program, executor.as_mut(), budget, driven, plan, explain);
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
    explain: Explain,
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
    let seen = match executor.run_with_output(plan, budget) {
        Ok((raw, output)) => {
            let detected = classify(&raw).verdict.detected();
            let killed_by = explain(&output);
            if !detected && !killed_by.is_empty() {
                // The exit status and the report disagree: neither can be
                // trusted, and a survivor is not to be claimed from either.
                let tests: Vec<&str> = killed_by.iter().map(|k| k.test.as_str()).collect();
                Seen::Failed {
                    reason: format!(
                        "the run's exit status says it passed (exit code 0) but its output reports failed test(s): {}",
                        tests.join(", ")
                    ),
                }
            } else if detected {
                Seen::Observed { raw, killed_by }
            } else {
                Seen::Observed {
                    raw,
                    killed_by: Vec::new(),
                }
            }
        }
        Err(e) => Seen::Failed {
            reason: e.to_string(),
        },
    };
    Ran {
        program,
        took: started.elapsed(),
        tests,
        seen,
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
    explain: Explain,
) -> Result<Preparation, String> {
    let started = Instant::now();
    let (baseline, output) = executor
        .baseline_with_output(Budget::from_baseline(FIRST_RUN))
        .map_err(|e| e.to_string())?;
    let describe = executor.describe();
    let RawOutcome::Exited { code: 0, wall, .. } = &baseline else {
        // The run judges the baseline. A program it is bound to refuse is
        // not recorded: it says which tests failed, and no more.
        return Ok(Preparation {
            program: 0,
            describe,
            baseline_failures: explain(&output).iter().map(Killer::line).collect(),
            baseline,
            took: started.elapsed(),
            recording: None,
        });
    };
    // The recording run is one long run with its own, generous budget; each
    // mutant's is short, so a hung one is decided in seconds.
    let mut recording = executor
        .recording(plans, Budget::from_baseline(*wall))
        .map_err(|e| e.to_string())?;
    if let Some(driven) = &mut recording.driven {
        widen_awaiting(plans, driven);
    }
    Ok(Preparation {
        program: 0,
        describe,
        baseline,
        baseline_failures: Vec::new(),
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
