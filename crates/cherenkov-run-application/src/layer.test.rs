use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cherenkov_core::execution::{Budget, ExecError, RawOutcome, Tier};
use serde_json::Value;

use angzarr_embedded::OnRetryable;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use cherenkov_embedded_edge::RunStore;

use super::*;
use crate::test_plans::{many, plans};
use crate::testing::{new_core, new_core_on, new_run};
use crate::workers::Killer;
use cherenkov_run_ports::Edge;

/// [`super::run`] on a runtime of its own, over `store`, as a process does.
fn run<S: RunStore>(
    store: S,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    sink: &mut dyn Sink,
) -> Result<Option<RunStatus>, RunLayerError> {
    run_retrying(
        store,
        OnRetryable::default(),
        plans,
        settings,
        workers,
        sink,
    )
}

/// [`run`], the kernel retrying as `retry` says.
fn run_retrying<S: RunStore>(
    store: S,
    retry: OnRetryable,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    sink: &mut dyn Sink,
) -> Result<Option<RunStatus>, RunLayerError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let core = Arc::new(new_core_on(store, retry).await);
        super::run(&core, new_run(), plans, settings, workers, sink).await
    })
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
        language: "rust".to_string(),
        operators: "breadth".to_string(),
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

/// Every line of `out`, parsed: each must be one JSON document.
fn documents(out: Vec<u8>) -> Vec<Value> {
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}")))
        .collect()
}

fn names(docs: &[Value]) -> Vec<&str> {
    docs.iter().map(|d| d["event"].as_str().unwrap()).collect()
}

#[test]
fn test_a_run_writes_its_events_one_document_per_line_from_its_start_to_its_finish() {
    let runs = Arc::new(AtomicUsize::new(0));
    let mut out = JsonLines(Vec::new());
    let exit = run(
        MemoryStore::new(),
        &plans(),
        &settings(),
        Some(workers(&runs)),
        &mut out,
    )
    .unwrap();
    let out = out.0;
    assert_eq!(exit, Some(RunStatus::Complete));
    let docs = documents(out);
    assert_eq!(
        names(&docs),
        [
            "run.started",
            "mutant.planned",
            "mutant.planned",
            "program.prepared",
            "mutant.decided",
            "mutant.decided",
            "run.finished",
        ]
    );
    let run_id = docs[0]["run"].clone();
    assert!(docs.iter().all(|d| d["run"] == run_id));
    let mut verdicts: Vec<(&str, &str)> = docs
        .iter()
        .filter(|d| d["event"] == "mutant.decided")
        .map(|d| {
            (
                d["function"].as_str().unwrap(),
                d["verdict"].as_str().unwrap(),
            )
        })
        .collect();
    verdicts.sort();
    assert_eq!(verdicts, [("a", "killed"), ("b", "survived")]);
    // Each decision says what the mutant changed, from the plan.
    let a = docs
        .iter()
        .find(|d| d["event"] == "mutant.decided" && d["function"] == "a")
        .unwrap();
    assert_eq!(
        (a["original"].clone(), a["mutated"].clone()),
        ("+".into(), "-".into())
    );
    assert_eq!(a["description"], "eval-return \"x - 1\" [force-return]");
    let prepared = docs
        .iter()
        .find(|d| d["event"] == "program.prepared")
        .unwrap();
    assert_eq!(
        prepared["selection"], "every-test",
        "the fake records nothing"
    );
    assert!(prepared["reason"].is_string());
    assert_eq!(
        docs[6]["counts"],
        serde_json::json!({"killed": 1, "survived": 1})
    );
}

#[test]
fn test_a_dry_run_plans_the_run_and_runs_nothing() {
    let mut out = JsonLines(Vec::new());
    let status = run(MemoryStore::new(), &plans(), &settings(), None, &mut out).unwrap();
    let out = out.0;
    assert_eq!(status, None, "it did not finish");
    assert_eq!(
        names(&documents(out)),
        ["run.started", "mutant.planned", "mutant.planned"]
    );
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
    let mut out = JsonLines(Vec::new());
    let error = run(
        MemoryStore::new(),
        &plans(),
        &settings(),
        Some(broken),
        &mut out,
    )
    .unwrap_err();
    let RunLayerError::Workers { reason } = error else {
        panic!("{error:?}");
    };
    assert!(reason.contains("unmutated tests do not pass"), "{reason}");
    // The stream says so too, and ends.
    let docs = documents(out.0);
    let failed = docs
        .iter()
        .find(|d| d["event"] == "program.failed")
        .unwrap();
    assert!(
        failed["reason"]
            .as_str()
            .unwrap()
            .contains("unmutated tests do not pass")
    );
    let last = docs.last().unwrap();
    assert_eq!(last["event"], "run.finished");
    assert_eq!(last["status"], "failed");
    assert_eq!(last["exit"], 0, "the exit code is the program's to say");
}

/// Each line written, sent on as soon as it is complete.
struct Lines {
    to: std::sync::mpsc::Sender<String>,
    partial: Vec<u8>,
}

impl std::io::Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.partial.extend_from_slice(bytes);
        while let Some(end) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let _ = self
                .to
                .send(String::from_utf8_lossy(&line).trim_end().to_string());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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

/// The next document from `lines` whose event is `event`, within ten
/// seconds: none when the run holds it back.
fn next_event(lines: &std::sync::mpsc::Receiver<String>, event: &str) -> Option<Value> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let line = lines.recv_timeout(left).ok()?;
        let doc: Value = serde_json::from_str(&line).unwrap();
        if doc["event"] == event {
            return Some(doc);
        }
    }
}

#[test]
fn test_each_event_is_written_as_it_happens_while_later_work_is_still_running() {
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
    let (to, lines) = std::sync::mpsc::channel();
    let running = std::thread::spawn(move || {
        let mut out = JsonLines(Lines {
            to,
            partial: Vec::new(),
        });
        run(
            MemoryStore::new(),
            &plans(),
            &settings(),
            Some(workers),
            &mut out,
        )
    });
    let first = next_event(&lines, "program.prepared");
    prepared.open();
    assert_eq!(
        first.map(|d| d["program"].clone()),
        Some(Value::from("quick")),
        "the quick program's preparation waited for the slow one's"
    );
    let first = next_event(&lines, "mutant.decided");
    decided.open();
    assert_eq!(
        first.map(|d| d["function"].clone()),
        Some(Value::from("a")),
        "a's verdict waited for b's"
    );
    assert_eq!(running.join().unwrap().unwrap(), Some(RunStatus::Complete));
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
    let mut out = JsonLines(Vec::new());
    assert_eq!(
        run(
            MemoryStore::new(),
            &plans(),
            &settings(),
            Some(workers),
            &mut out
        )
        .unwrap(),
        Some(RunStatus::Complete)
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
    let mut out = JsonLines(Vec::new());
    let store = crate::yielding::Yielding::default();
    let exit = run_retrying(
        store.clone(),
        strict(),
        &plans,
        &settings(),
        Some(workers),
        &mut out,
    )
    .unwrap();
    assert_eq!(exit, Some(RunStatus::Complete));
    assert_eq!(store.conflicts(), 0, "a write lost a race on its stream");
    let docs = documents(out.0);
    let mut decided: Vec<&str> = docs
        .iter()
        .filter(|d| d["event"] == "mutant.decided")
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    decided.sort_unstable();
    let ids: Vec<String> = plans.iter().map(|p| p.id.0.clone()).collect();
    assert_eq!(decided, ids, "each mutant decided once");
    assert_eq!(runs.load(Ordering::SeqCst), 64, "each mutant ran once");
    let last = docs.last().unwrap();
    assert_eq!(last["event"], "run.finished");
    assert_eq!(
        last["counts"],
        serde_json::json!({"killed": 32, "survived": 32})
    );
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
    let mut out = JsonLines(std::io::sink());
    run(
        MemoryStore::new(),
        &plans,
        &settings(),
        Some(workers),
        &mut out,
    )
    .unwrap();
    eprintln!(
        "a whole run of {n} instant mutants on 8 workers: {:?}",
        whole.elapsed()
    );
}
