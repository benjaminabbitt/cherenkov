//! What a run left in the checkout it ran in. Cherenkov mutates in place and
//! runs each test program in its package directory, so a mutant of
//! path-building code can make a test write beside the source. The run
//! compares the checkout's git state before and after and says what is new;
//! it never changes, restores or removes anything.

use std::collections::BTreeMap;
use std::path::Path;

/// A checkout's untracked and modified files at a moment: each path with its
/// porcelain status and, where the file is there, a hash of its content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<String, (String, Option<u64>)>);

impl Snapshot {
    /// The snapshot `porcelain` (`git status --porcelain=v1 -z`) describes,
    /// `content` giving a path's content hash.
    pub fn parse(porcelain: &[u8], content: impl Fn(&str) -> Option<u64>) -> Snapshot {
        let _ = (porcelain, content);
        Snapshot::default()
    }

    /// `root`'s snapshot, or none outside a git repository or without git.
    pub fn take(root: &Path) -> Option<Snapshot> {
        let status = git(
            root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--",
                ".",
            ],
        )?;
        Some(Snapshot::parse(&status, |_| None))
    }
}

/// What `git args` prints in `dir`, when it succeeds.
fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The paths present or changed in `after` that were not so in `before`.
pub fn written_since(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let _ = (before, after);
    Vec::new()
}

#[cfg(test)]
#[path = "checkout.test.rs"]
mod tests;
