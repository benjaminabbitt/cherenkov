//! Pruning a durable store file: the older finished runs go, an unfinished
//! one and the mutant streams a kept run plans stay, and a store pruned is
//! the store a later run goes on in.

use std::path::Path;

mod common;

use cherenkov_run_application::layer::Workers;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use common::{Kept, exited, explain, plans_of, readers, scratch, settings};

struct Quick;

impl MutantExecutor for Quick {
    fn run(
        &mut self,
        _: &cherenkov_mutation::plan::MutantPlan,
        _: cherenkov_mutation::execution::Budget,
    ) -> Result<cherenkov_mutation::execution::RawOutcome, ExecError> {
        Ok(exited(0))
    }

    fn baseline(
        &mut self,
        _: cherenkov_mutation::execution::Budget,
    ) -> Result<cherenkov_mutation::execution::RawOutcome, ExecError> {
        Ok(exited(0))
    }
}

fn quick() -> Workers {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Quick);
    Workers {
        sets: vec![vec![executor]],
        jobs: 1,
        explain,
    }
}

/// A run of `n` mutants, finished, in the store file at `path`.
fn finished(path: &Path, n: usize) {
    let started = cherenkov_wiring::start_in_file(
        path,
        &plans_of(n),
        &settings(),
        Some(quick()),
        readers(&Kept::default()),
    )
    .unwrap();
    assert!(started.status.is_some());
}

#[test]
fn a_store_file_is_pruned_to_the_last_finished_runs() {
    let dir = scratch("prune-file");
    let path = dir.join("runs.sqlite");
    for _ in 0..3 {
        finished(&path, 2);
    }
    let dry = cherenkov_wiring::prune_in_file(&path, 1, true).unwrap();
    assert_eq!((dry.runs.len(), dry.kept_finished), (2, 1));
    let again = cherenkov_wiring::prune_in_file(&path, 1, true).unwrap();
    assert_eq!(dry, again, "a dry run removes nothing");
    let pruned = cherenkov_wiring::prune_in_file(&path, 1, false).unwrap();
    assert_eq!(pruned, dry);
    let after = cherenkov_wiring::prune_in_file(&path, 1, false).unwrap();
    assert!(after.runs.is_empty());
    assert_eq!(after.kept_finished, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_run_goes_on_in_a_store_that_was_pruned() {
    let dir = scratch("prune-then-run");
    let path = dir.join("runs.sqlite");
    finished(&path, 2);
    finished(&path, 3);
    let pruned = cherenkov_wiring::prune_in_file(&path, 0, false).unwrap();
    assert_eq!(pruned.runs.len(), 2);
    assert_eq!(pruned.mutants, 3, "no run that stays plans them");
    finished(&path, 2);
    let after = cherenkov_wiring::prune_in_file(&path, 5, true).unwrap();
    assert_eq!(after.kept_finished, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
