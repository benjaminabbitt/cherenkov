//! A run on the durable store survives the process: a child process running
//! it is killed with SIGKILL part way through its mutants, and a run begun
//! after it carries on the same run, runs no mutant that was decided again,
//! and ends with the verdicts an uninterrupted run ends with.
//!
//! The child is this test binary run again on its own `child` test, told by
//! the environment what to do; the test is a no-op without it.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;

use cherenkov_mutation::execution::{Budget, RawOutcome};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_application::layer::Workers;
use cherenkov_run_domain::RunStatus;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use common::{Kept, exited, explain, plans_of, readers, scratch, settings};

const MUTANTS: usize = 6;

fn plans() -> Vec<MutantPlan> {
    plans_of(MUTANTS)
}

/// The mutant the child is killed in: the fourth to begin.
const HELD: usize = 4;

/// Kills the mutants in `a`. Appends a line to the ledger file as each mutant
/// begins, and, in the child, stays in the `HELD`th one for good.
struct Logged {
    ledger: PathBuf,
    hold: bool,
}

impl Logged {
    fn note(&self, line: &str) -> usize {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ledger)
            .unwrap();
        writeln!(file, "{line}").unwrap();
        std::fs::read_to_string(&self.ledger)
            .unwrap()
            .lines()
            .count()
    }
}

impl MutantExecutor for Logged {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        let begun = self.note(&plan.id.0);
        if self.hold && begun == HELD {
            std::thread::sleep(Duration::from_secs(3600));
        }
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101),
            _ => exited(0),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }
}

fn workers(ledger: &Path, hold: bool) -> Workers {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Logged {
        ledger: ledger.to_path_buf(),
        hold,
    });
    Workers {
        sets: vec![vec![executor]],
        jobs: 1,
        explain,
    }
}

/// What the child does when the test binary is run as one: start the run in
/// the store the environment names and stay in it until killed.
#[test]
fn child() {
    let (Ok(store), Ok(ledger)) = (
        std::env::var("CHERENKOV_CHILD_STORE"),
        std::env::var("CHERENKOV_CHILD_LEDGER"),
    ) else {
        return;
    };
    let kept = Kept::default();
    let _ = cherenkov_wiring::start_in_file(
        Path::new(&store),
        &plans(),
        &settings(),
        Some(workers(Path::new(&ledger), true)),
        readers(&kept),
    );
}

/// The child process, started on `store` and `ledger`.
fn spawn_child(store: &Path, ledger: &Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .env("CHERENKOV_CHILD_STORE", store)
        .env("CHERENKOV_CHILD_LEDGER", ledger)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Wait until the ledger holds `lines` lines.
fn wait_for(ledger: &Path, lines: usize) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while std::fs::read_to_string(ledger).map_or(0, |text| text.lines().count()) < lines {
        assert!(
            Instant::now() < deadline,
            "the child never reached mutant {lines}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn ledger_lines(ledger: &Path) -> Vec<String> {
    std::fs::read_to_string(ledger)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn a_run_killed_with_sigkill_is_resumed_and_ends_as_an_uninterrupted_run_does() {
    let dir = scratch("sigkill");
    let (store, ledger) = (dir.join("runs.sqlite"), dir.join("ledger"));

    let mut child = spawn_child(&store, &ledger);
    wait_for(&ledger, HELD);
    child.kill().unwrap();
    child.wait().unwrap();
    let before = ledger_lines(&ledger);
    assert_eq!(
        before.len(),
        HELD,
        "the child began {HELD} mutants: {before:?}"
    );

    let resumed_file = Kept::default();
    let resumed = cherenkov_wiring::start_in_file(
        &store,
        &plans(),
        &settings(),
        Some(workers(&ledger, false)),
        readers(&resumed_file),
    )
    .unwrap();
    assert!(resumed.resumed, "the run the child left was carried on");
    assert_eq!(resumed.status, Some(RunStatus::Complete));

    // The three the child decided ran once; the one it was in ran again; no
    // mutant is lost.
    let after = ledger_lines(&ledger);
    assert_eq!(after.len(), MUTANTS + 1, "{after:?}");
    for decided in &before[..HELD - 1] {
        assert_eq!(
            after.iter().filter(|id| *id == decided).count(),
            1,
            "{decided}"
        );
    }
    let held = &before[HELD - 1];
    assert_eq!(after.iter().filter(|id| *id == held).count(), 2);

    // The verdicts are the uninterrupted run's.
    let whole_file = Kept::default();
    let whole = cherenkov_wiring::start_in_file(
        &dir.join("whole.sqlite"),
        &plans(),
        &settings(),
        Some(workers(&dir.join("whole-ledger"), false)),
        readers(&whole_file),
    )
    .unwrap();
    assert!(!whole.resumed);
    assert_eq!(
        *resumed_file.0.lock().unwrap(),
        *whole_file.0.lock().unwrap()
    );
    assert_eq!(resumed_file.0.lock().unwrap().len(), 1, "one file written");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_run_after_one_that_finished_is_a_new_run_on_the_same_store() {
    let dir = scratch("finished");
    let (store, ledger) = (dir.join("runs.sqlite"), dir.join("ledger"));
    let ran = |hold| {
        cherenkov_wiring::start_in_file(
            &store,
            &plans(),
            &settings(),
            Some(workers(&ledger, hold)),
            readers(&Kept::default()),
        )
        .unwrap()
    };
    let (first, second) = (ran(false), ran(false));
    assert!(!first.resumed && !second.resumed);
    assert_ne!(first.run, second.run);
    assert_eq!(second.status, Some(RunStatus::Complete));
    assert_eq!(ledger_lines(&ledger).len(), 2 * MUTANTS);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The run's and a mutant's streams, each with the snapshot of its state the
/// kernel keeps at the head, and what a reopened store holds of them.
#[test]
fn the_snapshots_of_a_run_are_kept_in_the_store_at_the_head_of_each_stream() {
    use angzarr_ports::{EventStore, SnapshotStore, StreamRef};
    use cherenkov_run_application::covers::{mutant_cover, run_cover};

    let dir = scratch("snapshots");
    let (path, ledger) = (dir.join("runs.sqlite"), dir.join("ledger"));
    let started = cherenkov_wiring::start_in_file(
        &path,
        &plans(),
        &settings(),
        Some(workers(&ledger, false)),
        readers(&Kept::default()),
    )
    .unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let store = cherenkov_run_store::open(&path).await.unwrap();
        for cover in [
            run_cover(&started.run),
            mutant_cover(&plans()[0].id.0).unwrap(),
        ] {
            let stream = StreamRef::try_from(&cover).unwrap();
            let head = store.head(stream).await.unwrap().next_sequence;
            let snapshot = store
                .read_snapshot(stream)
                .await
                .unwrap()
                .expect("a snapshot");
            assert_eq!(snapshot.sequence + 1, head, "{cover:?}: at the head");
        }
        store.close().await.unwrap();
    });
    std::fs::remove_dir_all(&dir).unwrap();
}
