//! Pruning a durable store file: the older finished runs go, an unfinished
//! one and the mutant streams a kept run plans stay, and a store pruned is
//! the store a later run goes on in. Only the purge build prunes.

#![cfg(feature = "purge")]

use std::path::Path;

mod common;

use cherenkov_run_application::layer::Workers;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_wiring::Sampling;
use common::{Kept, exited, plans_of, readers, settings};
use test_dir::TestDir;

struct Quick;

impl MutantExecutor for Quick {
    fn run(
        &mut self,
        _: &cherenkov_mutation::plan::MutantPlan,
        _: cherenkov_mutation::execution::Budget,
    ) -> Result<cherenkov_mutation::execution::Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn baseline(
        &mut self,
        _: cherenkov_mutation::execution::Budget,
    ) -> Result<cherenkov_mutation::execution::Observed, ExecError> {
        Ok(exited(0).into())
    }
}

fn quick() -> Workers {
    let executor: Box<dyn MutantExecutor + Send> = Box::new(Quick);
    Workers {
        sets: vec![vec![executor]],
        jobs: 1,
    }
}

/// A run of `n` mutants, finished, in the store file at `path`.
fn finished(path: &Path, n: usize, sampling: &Sampling) {
    let started = cherenkov_wiring::start_in_file(
        path,
        &plans_of(n),
        &settings(),
        Some(quick()),
        readers(&Kept::default()),
        sampling.clone(),
    )
    .unwrap();
    assert!(started.status.is_some());
}

#[test]
fn a_store_file_is_pruned_to_the_last_finished_runs() {
    let dir = TestDir::new("cherenkov-prune-file");
    let path = dir.join("runs.sqlite");
    let sampling = cherenkov_wiring::testing::sampling();
    for _ in 0..3 {
        finished(&path, 2, &sampling);
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
}

#[test]
fn a_run_goes_on_in_a_store_that_was_pruned() {
    let dir = TestDir::new("cherenkov-prune-then-run");
    let path = dir.join("runs.sqlite");
    let sampling = cherenkov_wiring::testing::sampling();
    finished(&path, 2, &sampling);
    finished(&path, 3, &sampling);
    let pruned = cherenkov_wiring::prune_in_file(&path, 0, false).unwrap();
    assert_eq!(pruned.runs.len(), 2);
    assert_eq!(pruned.mutants, 3, "no run that stays plans them");
    finished(&path, 2, &sampling);
    let after = cherenkov_wiring::prune_in_file(&path, 5, true).unwrap();
    assert_eq!(after.kept_finished, 1);
}
