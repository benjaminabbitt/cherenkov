//! Running a gate executable over cases, surviving the ones that crash it.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// What a case's sweep produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The digest of every result, and how many calls panicked.
    Ran { digest: u64, panics: u64 },
    /// The process died (a signal) while running the case.
    Crashed,
}

fn parse(stdout: &str) -> Vec<(usize, Outcome)> {
    stdout
        .lines()
        .filter_map(|l| {
            let mut f = l.split(' ');
            let id = f.next()?.parse().ok()?;
            let digest = f.next()?.parse().ok()?;
            let panics = f.next()?.parse().ok()?;
            Some((id, Outcome::Ran { digest, panics }))
        })
        .collect()
}

/// Run `exe` over `ids`, split among as many threads as the machine has
/// cores; each case's outcome is its own, so the result does not depend on
/// the split.
pub fn sweep(exe: &Path, ids: &[usize]) -> BTreeMap<usize, Outcome> {
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let size = ids.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .chunks(size)
            .map(|chunk| scope.spawn(move || sweep_chunk(exe, chunk)))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a sweep thread"))
            .collect()
    })
}

/// Run `exe` over `ids` in turn; a case that kills the process is `Crashed`
/// and the rest are run again without it.
fn sweep_chunk(exe: &Path, ids: &[usize]) -> BTreeMap<usize, Outcome> {
    let mut out = BTreeMap::new();
    let mut todo: Vec<usize> = ids.to_vec();
    while !todo.is_empty() {
        let run = Command::new(exe)
            .args(todo.iter().map(usize::to_string))
            .output()
            .expect("the gate executable runs");
        let done = parse(&String::from_utf8_lossy(&run.stdout));
        let (n, finished) = (todo.len(), done.len());
        out.extend(done);
        if finished < n {
            out.insert(todo[finished], Outcome::Crashed);
            todo.drain(..=finished);
        } else {
            todo.clear();
        }
    }
    out
}

/// Every input of one case and what it did, for a mismatch's report.
pub fn dump(exe: &Path, id: usize) -> Vec<String> {
    let run = Command::new(exe)
        .args(["dump", &id.to_string()])
        .output()
        .expect("the gate executable runs");
    String::from_utf8_lossy(&run.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}
