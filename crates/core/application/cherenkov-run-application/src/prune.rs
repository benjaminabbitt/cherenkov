//! Pruning: the store keeps every run, and this removes the older ones. The
//! last `keep` finished runs stay, and every unfinished run (a run an
//! earlier process left, which the next run of its plan carries on).
//!
//! A mutant's stream is shared: each run that plans the mutant appends its
//! decision to the one stream, whose root is derived from the mutant's id
//! alone. So a run's stream goes with its run, and a mutant's stream goes
//! only when a removed run planned it and no run that stays does, finished
//! or not.
//!
//! The caller holds the store (nothing else writes it), and each purge is
//! guarded by the head the prune read: a stream written since is refused,
//! and the prune reads the store again and decides again, a few times.

use std::collections::{BTreeMap, BTreeSet};

use angzarr_embedded::v1::Uuid;
use cherenkov_run_domain::{MUTANT_DOMAIN, RUN_DOMAIN, mutant_root};
use cherenkov_run_ports::{Listed, Listing, Purging};

use crate::error::RunLayerError;
use crate::resume::{beginning, ended};

/// How many times a prune reads the store and decides again after a purge
/// lost a race.
const ATTEMPTS: usize = 5;

/// What a prune removed, or, for a dry run, would.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// The runs removed, oldest first.
    pub runs: Vec<Uuid>,
    /// The mutant streams removed.
    pub mutants: usize,
    /// The events of every stream removed.
    pub events: u64,
    /// The finished runs that stay.
    pub kept_finished: usize,
    /// The unfinished runs, which always stay.
    pub kept_unfinished: usize,
}

/// A run the store holds: its stream, whether it ended, and the roots of
/// the mutant streams its plan names.
struct Held {
    listed: Listed,
    finished: bool,
    mutants: BTreeSet<String>,
}

/// What a prune removes: whole streams, and the tally of what stays.
struct Decision {
    runs: Vec<Listed>,
    mutants: Vec<Listed>,
    kept_finished: usize,
    kept_unfinished: usize,
}

/// Keep the last `keep` finished runs and every unfinished one, and remove
/// the rest, with the mutant streams no run that stays plans. With
/// `dry_run`, remove nothing and answer what would go.
pub async fn prune<C: Listing + Purging>(
    core: &C,
    keep: usize,
    dry_run: bool,
) -> Result<Pruned, RunLayerError> {
    let mut pruned = Pruned::default();
    let mut attempt = 1;
    loop {
        let decision = decide(&held(core).await?, mutant_streams(core).await?, keep);
        match remove(core, &decision, dry_run, &mut pruned).await {
            Err(RunLayerError::Unavailable { .. }) if attempt < ATTEMPTS => attempt += 1,
            done => return done.map(|()| pruned),
        }
    }
}

/// Every run the store holds, oldest first (a run's id is ordered by when it
/// was made).
async fn held<C: Listing>(core: &C) -> Result<Vec<Held>, RunLayerError> {
    let mut runs = Vec::new();
    for listed in core.streams(RUN_DOMAIN).await? {
        if listed.head == 0 {
            continue;
        }
        let last = core
            .window(&listed.cover, listed.head - 1, listed.head)
            .await?;
        let finished = ended(&last);
        let mutants = planned_roots(core, &listed).await?;
        runs.push(Held {
            listed,
            finished,
            mutants,
        });
    }
    runs.sort_by_key(|run| root_key(&run.listed));
    Ok(runs)
}

/// The key a stream is ordered and told apart by: its root.
fn root_key(listed: &Listed) -> String {
    listed
        .cover
        .root
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default()
}

/// The roots of the mutant streams the run's plan names.
async fn planned_roots<C: Listing>(
    core: &C,
    run: &Listed,
) -> Result<BTreeSet<String>, RunLayerError> {
    let found = beginning(core, &run.cover).await?;
    let mut roots = BTreeSet::new();
    for executable in found.planned.iter().flat_map(|planned| &planned.mutants) {
        let Some(mutant) = &executable.mutant else {
            continue;
        };
        let root = mutant_root(&mutant.id).map_err(|e| RunLayerError::kernel(&e))?;
        roots.insert(root.to_string());
    }
    Ok(roots)
}

/// The mutant streams the store holds, by root.
async fn mutant_streams<C: Listing>(core: &C) -> Result<BTreeMap<String, Listed>, RunLayerError> {
    let listed = core.streams(MUTANT_DOMAIN).await?;
    Ok(listed
        .into_iter()
        .map(|stream| (root_key(&stream), stream))
        .collect())
}

/// What goes, of `runs` (oldest first) and the mutant streams `streams`.
fn decide(runs: &[Held], streams: BTreeMap<String, Listed>, keep: usize) -> Decision {
    let finished: Vec<&Held> = runs.iter().filter(|run| run.finished).collect();
    let dropped = finished.len().saturating_sub(keep);
    let gone = |run: &&Held| run.finished && finished[..dropped].iter().any(|d| same(d, run));
    let (removed, staying): (Vec<&Held>, Vec<&Held>) = runs.iter().partition(gone);
    let wanted: BTreeSet<&String> = staying.iter().flat_map(|run| &run.mutants).collect();
    let unwanted: BTreeSet<&String> = removed
        .iter()
        .flat_map(|run| &run.mutants)
        .filter(|root| !wanted.contains(root))
        .collect();
    Decision {
        mutants: unwanted
            .into_iter()
            .filter_map(|root| streams.get(root).cloned())
            .collect(),
        kept_finished: staying.iter().filter(|run| run.finished).count(),
        kept_unfinished: staying.iter().filter(|run| !run.finished).count(),
        runs: removed.iter().map(|run| run.listed.clone()).collect(),
    }
}

/// Whether two runs are the same stream.
fn same(a: &Held, b: &Held) -> bool {
    root_key(&a.listed) == root_key(&b.listed)
}

/// Carry out `decision` into `pruned`: the mutants' streams after the
/// runs', each purge guarded by the head it was read at.
async fn remove<C: Purging>(
    core: &C,
    decision: &Decision,
    dry_run: bool,
    pruned: &mut Pruned,
) -> Result<(), RunLayerError> {
    pruned.kept_finished = decision.kept_finished;
    pruned.kept_unfinished = decision.kept_unfinished;
    for run in &decision.runs {
        pruned.events += purge(core, run, dry_run).await?;
        pruned.runs.push(run.cover.root.clone().unwrap_or_default());
    }
    for mutant in &decision.mutants {
        pruned.events += purge(core, mutant, dry_run).await?;
        pruned.mutants += 1;
    }
    Ok(())
}

/// How many events `stream` holds, removed unless `dry_run`.
async fn purge<C: Purging>(core: &C, stream: &Listed, dry_run: bool) -> Result<u64, RunLayerError> {
    if dry_run {
        return Ok(stream.head);
    }
    Ok(core.purge(&stream.cover, stream.head).await?)
}
