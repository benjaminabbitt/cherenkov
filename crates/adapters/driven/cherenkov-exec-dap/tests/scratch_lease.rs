//! The support library's scratch directory lives as long as a run uses it:
//! it is removed when the last user lets go, however that happens.

use std::path::PathBuf;
use std::sync::Mutex;

use cherenkov_exec_dap::gdb::{GdbConfig, GdbExecutor};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::support_lib::{ScratchLease, scratch_dir};

/// The scratch directory is one per process: these tests take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn marked_scratch() -> PathBuf {
    let marker = scratch_dir().join("default").join("marker");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "x").unwrap();
    marker
}

fn a_config() -> GdbConfig {
    GdbConfig {
        gdb: PathBuf::from("/nonexistent/gdb"),
        test_binary: PathBuf::from("/nonexistent/tests"),
        test_args: Vec::new(),
        cwd: None,
        env: Vec::new(),
        source_root: None,
    }
}

#[test]
fn test_the_scratch_directory_goes_when_the_last_lease_drops() {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let first = ScratchLease::acquire();
    let second = ScratchLease::acquire();
    let marker = marked_scratch();
    drop(first);
    assert!(marker.exists(), "a lease is still held");
    drop(second);
    assert!(!marker.exists(), "the last lease dropped");
    assert!(!scratch_dir().exists());
}

#[test]
fn test_the_scratch_directory_goes_when_the_run_that_held_it_panics() {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut marker = None;
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lease = ScratchLease::acquire();
        marker = Some(marked_scratch());
        panic!("the run fails");
    }));
    assert!(ended.is_err());
    assert!(!marker.unwrap().exists());
}

#[test]
fn test_an_executor_holds_the_scratch_directory_until_it_drops() {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let executor = GdbExecutor::new(a_config(), Isa::amd64());
    let marker = marked_scratch();
    assert!(marker.exists());
    drop(executor);
    assert!(!marker.exists(), "the executor's drop ended its use");
}
