//! A unified diff's added lines, to judge only what changed (`--in-diff`, as
//! cargo-mutants takes it): a mutant is in the diff when its span touches a
//! line the diff adds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The line ranges (1-based, inclusive) each file's diff adds, by the path
/// the diff names (from the repository root, `b/` side).
#[derive(Debug, Default, Clone)]
pub struct AddedLines {
    files: BTreeMap<PathBuf, Vec<(u32, u32)>>,
}

impl AddedLines {
    /// Read a unified diff (`git diff`, `diff -u`). A deleted file adds
    /// nothing; a new one adds all its lines.
    pub fn parse(text: &str) -> AddedLines {
        let mut files: BTreeMap<PathBuf, Vec<(u32, u32)>> = BTreeMap::new();
        let mut file: Option<PathBuf> = None;
        let mut next = 0u32;
        // Lines the current hunk still owes, old and new side: inside it a
        // line starting `+++ ` is an added line, not a file header.
        let (mut old_left, mut new_left) = (0u32, 0u32);
        for line in text.lines() {
            if old_left > 0 || new_left > 0 {
                match line.as_bytes().first() {
                    Some(b'+') => {
                        if let Some(f) = &file {
                            let ranges = files.entry(f.clone()).or_default();
                            match ranges.last_mut() {
                                Some((_, end)) if *end + 1 == next => *end = next,
                                _ => ranges.push((next, next)),
                            }
                        }
                        next += 1;
                        new_left = new_left.saturating_sub(1);
                    }
                    Some(b'-') => old_left = old_left.saturating_sub(1),
                    Some(b'\\') => {}
                    _ => {
                        next += 1;
                        old_left = old_left.saturating_sub(1);
                        new_left = new_left.saturating_sub(1);
                    }
                }
            } else if let Some(name) = line.strip_prefix("+++ ") {
                let name = name.split('\t').next().unwrap_or(name);
                file = (name != "/dev/null")
                    .then(|| PathBuf::from(name.strip_prefix("b/").unwrap_or(name)));
                if let Some(f) = &file {
                    files.entry(f.clone()).or_default();
                }
            } else if let Some(rest) = line.strip_prefix("@@ ") {
                let (old, new) = hunk_lengths(rest);
                (old_left, new_left) = (old, new);
                next = new_start(rest).unwrap_or(0);
            }
        }
        AddedLines { files }
    }

    /// Whether `path` (from the repository root) has an added line in
    /// `start..=end`.
    pub fn touches(&self, path: &Path, start: u32, end: u32) -> bool {
        self.files
            .get(path)
            .is_some_and(|ranges| ranges.iter().any(|&(a, b)| a <= end && start <= b))
    }

    /// [`AddedLines::touches`] for a file `relative` to `below`, a directory
    /// of the repository (the one the run was started in).
    pub fn touches_below(&self, below: &Path, relative: &Path, start: u32, end: u32) -> bool {
        self.touches(&below.join(relative), start, end)
    }

    /// The files the diff adds lines to, with the line ranges (1-based,
    /// inclusive) it adds, by the path the diff names.
    pub fn files(&self) -> impl Iterator<Item = (&Path, &[(u32, u32)])> {
        self.files.iter().map(|(f, r)| (f.as_path(), r.as_slice()))
    }

    /// Whether the diff adds anything.
    pub fn is_empty(&self) -> bool {
        self.files.values().all(Vec::is_empty)
    }
}

/// The new file's first line in a hunk header's remainder
/// (`-10,3 +10,4 @@ ...`).
fn new_start(rest: &str) -> Option<u32> {
    let plus = rest.split_whitespace().find(|w| w.starts_with('+'))?;
    plus[1..].split(',').next()?.parse().ok()
}

/// The old and new line counts of a hunk header's remainder (a missing
/// count is one).
fn hunk_lengths(rest: &str) -> (u32, u32) {
    let count = |sign: char| {
        rest.split_whitespace()
            .find(|w| w.starts_with(sign))
            .and_then(|w| match w[1..].split_once(',') {
                Some((_, n)) => n.parse().ok(),
                None => Some(1),
            })
            .unwrap_or(0)
    };
    (count('-'), count('+'))
}
