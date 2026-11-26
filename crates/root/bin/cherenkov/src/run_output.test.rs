//! A run printed end to end: the application on the bus, the output the
//! CLI hands it, and the documents that come out (`docs/output-events.md`).

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, Tier};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_application::error::RunLayerError;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::{Settings, Workers};
use cherenkov_run_domain::{OperatorSet, RunStatus};
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;
use coded_diagnostic::Diagnostic;
use serde_json::Value;

use angzarr_ports::memory::MemoryStore;
use cherenkov_cli::output::{JsonLines, printing};

/// Two `aor` mutants, in `a` and in `b`.
fn plans() -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = "fn a(x: i32) -> i32 { x + 1 }\nfn b(x: i32) -> i32 { x - 1 }\n";
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
        .unwrap();
    let mut plans: Vec<MutantPlan> = plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .filter(|p| p.operator == "aor")
        .collect();
    plans.sort_by_key(|p| p.stop_at.line);
    plans.dedup_by_key(|p| p.stop_at.line);
    assert_eq!(plans.len(), 2);
    plans
}

/// The bytes a run printed, readable while and after it prints.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Shared {
    fn bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A run of `plans` in memory, printed as JSON Lines to `out`.
fn run(
    store: MemoryStore,
    plans: &[MutantPlan],
    settings: &Settings,
    workers: Option<Workers>,
    out: impl Write + Send + 'static,
) -> Result<Option<RunStatus>, RunLayerError> {
    cherenkov_wiring::run(
        store,
        plans,
        settings,
        workers,
        Readers::printing(printing(
            Box::new(JsonLines(out)),
            Arc::new(crate::catalog().unwrap()),
        )),
        cherenkov_wiring::testing::sampling(),
    )
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
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101).into(),
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn describe(&self) -> Option<String> {
        Some("calc-test".to_string())
    }
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
    let out = Shared::default();
    let exit = run(
        MemoryStore::new(),
        &plans(),
        &settings(),
        Some(workers(&runs)),
        out.clone(),
    )
    .unwrap();
    let out = out.bytes();
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
    let out = Shared::default();
    let status = run(MemoryStore::new(), &plans(), &settings(), None, out.clone()).unwrap();
    let out = out.bytes();
    assert_eq!(status, None, "it did not finish");
    assert_eq!(
        names(&documents(out)),
        ["run.started", "mutant.planned", "mutant.planned"]
    );
}

/// Fails its baseline.
struct Broken;

impl MutantExecutor for Broken {
    fn run(&mut self, _: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(101).into())
    }
}

#[test]
fn test_a_run_whose_workers_cannot_run_fails_saying_why() {
    let broken = Workers {
        sets: vec![vec![Box::new(Broken)]],
        jobs: 1,
    };
    let out = Shared::default();
    let error = run(
        MemoryStore::new(),
        &plans(),
        &settings(),
        Some(broken),
        out.clone(),
    )
    .unwrap_err();
    let RunLayerError::Workers { cause, .. } = error else {
        panic!("{error:?}");
    };
    assert_eq!(cause.code(), "cherenkov-run-domain-baseline-failed");
    // The stream says so too, and ends.
    let docs = documents(out.bytes());
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
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        if plan.function.as_deref() == Some("b") {
            self.decided.wait();
        }
        Ok(exited(101).into())
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        if self.program == "slow" {
            self.prepared.wait();
        }
        Ok(exited(0).into())
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
    };
    let (to, lines) = std::sync::mpsc::channel();
    let running = std::thread::spawn(move || {
        run(
            MemoryStore::new(),
            &plans(),
            &settings(),
            Some(workers),
            Lines {
                to,
                partial: Vec::new(),
            },
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

#[test]
fn test_two_runs_of_a_project_write_the_same_verdict_file() {
    let dir = test_dir::TestDir::new("cherenkov-verdicts-e2e");
    let (first, second) = (
        dir.join("one/verdicts.jsonl"),
        dir.join("two/verdicts.jsonl"),
    );
    for path in [&first, &second] {
        let runs = Arc::new(AtomicUsize::new(0));
        let readers = Readers::new(
            printing(
                Box::new(JsonLines(std::io::sink())),
                Arc::new(crate::catalog().unwrap()),
            ),
            Some(Arc::new(cherenkov_cli::output::JsonVerdictFile::new(
                path.clone(),
            ))),
        );
        let status = cherenkov_wiring::run(
            MemoryStore::new(),
            &plans(),
            &settings(),
            Some(workers(&runs)),
            readers,
            cherenkov_wiring::testing::sampling(),
        )
        .unwrap();
        assert_eq!(status, Some(RunStatus::Complete));
    }
    let (one, two) = (
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap(),
    );
    assert_eq!(
        one, two,
        "nothing in the file varies between identical runs"
    );
    let docs: Vec<Value> = one
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(docs.len(), 2);
    assert!(
        docs[0]["id"].as_str() < docs[1]["id"].as_str(),
        "sorted by id"
    );
    let mut verdicts: Vec<(&str, &str)> = docs
        .iter()
        .map(|d| {
            (
                d["function"].as_str().unwrap(),
                d["verdict"].as_str().unwrap(),
            )
        })
        .collect();
    verdicts.sort();
    assert_eq!(verdicts, [("a", "killed"), ("b", "survived")]);
}
