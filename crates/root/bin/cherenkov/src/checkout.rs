//! What a run left in the checkout it ran in. Cherenkov mutates in place and
//! runs each test program in its package directory, so a mutant of
//! path-building code can make a test write beside the source. The run
//! compares the checkout's git state before and after and says what is new;
//! it never changes, restores or removes anything.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// A checkout's untracked and modified files at a moment: each path with its
/// porcelain status and, where the file is there, a hash of its content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<String, (String, Option<u64>)>);

impl Snapshot {
    /// The snapshot `porcelain` (`git status --porcelain=v1 -z`) describes,
    /// `content` giving a path's content hash.
    pub fn parse(porcelain: &[u8], content: impl Fn(&str) -> Option<u64>) -> Snapshot {
        let text = String::from_utf8_lossy(porcelain);
        let mut entries = text.split('\0').filter(|e| e.len() > 3);
        let mut found = BTreeMap::new();
        while let Some(entry) = entries.next() {
            let (status, path) = entry.split_at(2);
            let path = &path[1..];
            if status.contains(['R', 'C']) {
                // A rename or copy is followed by the name it came from.
                entries.next();
            }
            found.insert(path.to_string(), (status.to_string(), content(path)));
        }
        Snapshot(found)
    }

    /// `root`'s snapshot, or none outside a git repository or without git.
    /// Only the files below `root` are looked at; the few that are dirty are
    /// hashed, so a file dirty at the start is told from one changed since.
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
        let top = git(root, &["rev-parse", "--show-toplevel"])?;
        let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
        Some(Snapshot::parse(&status, |path| hash_of(&top.join(path))))
    }
}

/// A hash of the file's content, for comparing it with itself later in the
/// same process; none when it cannot be read (deleted).
fn hash_of(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
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

/// The paths present or changed in `after` that were not so in `before`: a
/// path already dirty at the start is told only if its status or content
/// changed.
pub fn written_since(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    after
        .0
        .iter()
        .filter(|(path, now)| before.0.get(*path) != Some(*now))
        .map(|(path, _)| path.clone())
        .collect()
}

#[cfg(test)]
#[path = "checkout.test.rs"]
mod tests;
