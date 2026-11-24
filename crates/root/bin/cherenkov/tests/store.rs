//! The store's lock and its upkeep, through the real binary: a second run on
//! a held store is refused at once naming the holder, `--store` and
//! `--ephemeral` let a run go separately, `store unlock` clears what a
//! killed holder left, and `store prune` keeps the last N finished runs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use cherenkov_run_store::lock::{acquire, holder_file, lock_file};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../fixtures")
}

fn cherenkov(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cherenkov-store-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `cherenkov run` of the Rust fixture on the store `args` name.
fn run(args: &[&str]) -> Output {
    let dir = fixtures().join("rust");
    let mut all = vec!["run", dir.to_str().unwrap()];
    all.extend_from_slice(args);
    cherenkov(&all)
}

const REFUSED: i32 = 48;

#[test]
fn test_a_second_run_on_a_held_store_is_refused_at_once_naming_the_holder() {
    let dir = scratch("second-run");
    let store = dir.join("runs.sqlite");
    let _held = acquire(&store).unwrap();
    let began = Instant::now();
    let out = run(&["--store", store.to_str().unwrap(), "--format", "text"]);
    assert_eq!(out.status.code(), Some(REFUSED), "{}", stderr(&out));
    assert!(began.elapsed() < Duration::from_secs(20), "not waited for");
    let said = stderr(&out);
    assert!(
        said.contains(&format!("pid {}", std::process::id())),
        "{said}"
    );
    assert!(said.contains("in use"), "{said}");
    assert!(stdout(&out).is_empty(), "no run began");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_the_refusal_is_a_coded_document_under_json() {
    let dir = scratch("json");
    let store = dir.join("runs.sqlite");
    let _held = acquire(&store).unwrap();
    let out = run(&["--store", store.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(REFUSED));
    let document: serde_json::Value = serde_json::from_str(stderr(&out).trim()).unwrap();
    assert_eq!(document["code"], "cherenkov-cli-store-locked");
    assert!(document["args"]["holder"].as_str().unwrap().contains("pid"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_run_on_another_store_or_on_none_is_not_held_up_by_the_lock() {
    let dir = scratch("bypass");
    let held = dir.join("held.sqlite");
    let _held = acquire(&held).unwrap();
    // No cargo: each run gets as far as looking for the tool, past the lock.
    let nowhere = "/nonexistent/cargo";
    for args in [
        vec!["--store", dir.join("other.sqlite").to_str().unwrap()],
        vec!["--ephemeral", "--store", held.to_str().unwrap()],
    ] {
        let mut args = args;
        args.extend(["--cargo", nowhere]);
        let out = run(&args);
        let said = stderr(&out);
        assert!(!said.contains("in use"), "{args:?}: {said}");
        assert!(said.contains("cargo"), "{args:?}: {said}");
    }
    assert!(
        !holder_file(&dir.join("other.sqlite")).exists(),
        "the lock a run took is let go of"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_unlock_of_a_held_store_is_refused_and_names_the_holder() {
    let dir = scratch("unlock-held");
    let store = dir.join("runs.sqlite");
    let _held = acquire(&store).unwrap();
    let out = cherenkov(&["store", "unlock", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(REFUSED), "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains(&format!("pid {}", std::process::id())),
        "{said}"
    );
    assert!(said.contains("still running"), "{said}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_unlock_clears_what_a_killed_holder_left_and_a_run_may_follow() {
    let dir = scratch("unlock-left");
    let store = dir.join("runs.sqlite");
    drop(acquire(&store).unwrap());
    std::fs::write(
        holder_file(&store),
        r#"{"pid":4000000,"started":1,"command":"cherenkov run /project"}"#,
    )
    .unwrap();
    let out = cherenkov(&["store", "unlock", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("pid 4000000"), "{}", stdout(&out));
    assert!(!holder_file(&store).exists());
    drop(acquire(&store).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_unlock_of_a_store_nobody_holds_says_so_and_makes_nothing() {
    let dir = scratch("unlock-free");
    let store = dir.join("sub").join("runs.sqlite");
    let out = cherenkov(&["store", "unlock", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("not locked"), "{}", stdout(&out));
    assert!(!lock_file(&store).exists() && !dir.join("sub").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_prune_refuses_while_a_run_holds_the_store() {
    let dir = scratch("prune-held");
    let store = dir.join("runs.sqlite");
    populate(&store, 2);
    let _held = acquire(&store).unwrap();
    let out = cherenkov(&[
        "store",
        "prune",
        "--keep",
        "1",
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(REFUSED), "{}", stderr(&out));
    assert!(stderr(&out).contains("in use"), "{}", stderr(&out));
    drop(_held);
    let kept = cherenkov(&[
        "store",
        "prune",
        "--keep",
        "9",
        "--dry-run",
        "--store",
        store.to_str().unwrap(),
    ]);
    assert!(
        stdout(&kept).contains("kept 2 finished"),
        "{}",
        stdout(&kept)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_prune_of_a_path_with_no_store_is_refused_and_makes_none() {
    let dir = scratch("prune-none");
    let store = dir.join("runs.sqlite");
    let out = cherenkov(&[
        "store",
        "prune",
        "--keep",
        "1",
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(REFUSED), "{}", stderr(&out));
    assert!(stderr(&out).contains("no store there"), "{}", stderr(&out));
    assert!(!store.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_prune_says_what_it_would_remove_and_a_dry_run_removes_nothing() {
    let dir = scratch("prune-dry");
    let store = dir.join("runs.sqlite");
    populate(&store, 3);
    let path = store.to_str().unwrap();
    let dry = cherenkov(&[
        "store",
        "prune",
        "--keep",
        "1",
        "--dry-run",
        "--store",
        path,
    ]);
    assert_eq!(dry.status.code(), Some(0), "{}", stderr(&dry));
    let said = stdout(&dry);
    assert!(said.contains("would remove 2 runs"), "{said}");
    assert!(said.contains("kept 1 finished run"), "{said}");
    let again = cherenkov(&[
        "store",
        "prune",
        "--keep",
        "1",
        "--dry-run",
        "--store",
        path,
    ]);
    assert_eq!(stdout(&again), said, "nothing was removed");
    let done = cherenkov(&["store", "prune", "--keep", "1", "--store", path]);
    assert_eq!(done.status.code(), Some(0), "{}", stderr(&done));
    assert!(
        stdout(&done).contains("removed 2 runs"),
        "{}",
        stdout(&done)
    );
    let after = cherenkov(&["store", "prune", "--keep", "1", "--store", path]);
    assert!(
        stdout(&after).contains("nothing to remove"),
        "{}",
        stdout(&after)
    );
    assert!(!holder_file(&store).exists(), "the lock was let go of");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_prune_without_how_many_to_keep_is_a_usage_error() {
    let out = cherenkov(&["store", "prune"]);
    assert_eq!(out.status.code(), Some(REFUSED));
}

/// `runs` finished runs of no mutants in the store file `path`.
fn populate(path: &Path, runs: usize) {
    use angzarr_embedded::ProjectorHandler;
    use angzarr_embedded::angzarr_router::error::CodedError;
    use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
    use cherenkov_run_application::host::Readers;
    use cherenkov_run_application::layer::{Settings, Workers};

    /// An output that prints nothing.
    struct Silent;

    impl ProjectorHandler for Silent {
        fn name(&self) -> &str {
            "output"
        }

        async fn dispatch(&self, _: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
            Ok(ProjectResponse::default())
        }
    }

    for _ in 0..runs {
        let settings = Settings {
            root: "/project".to_string(),
            operators: Vec::new(),
            jobs: 1,
        };
        let workers = Workers {
            sets: vec![Vec::new()],
            jobs: 1,
        };
        let readers = Readers::printing(Box::new(Silent));
        let started =
            cherenkov_wiring::start_in_file(path, &[], &settings, Some(workers), readers).unwrap();
        assert!(started.status.is_some(), "the run finished");
    }
}
