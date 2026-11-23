//! The tests a diff changes: the test functions (`#[test]`, in inline
//! `#[cfg(test)]` modules, in `*.test.rs` files and in `tests/*.rs`) whose
//! bodies overlap a line the diff adds.

use std::path::{Path, PathBuf};

use cherenkov_core::diff::AddedLines;
use cherenkov_syntax::rust_tests::test_functions;

/// A test function a diff changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedTest {
    /// Its file, relative to the run's directory.
    pub file: PathBuf,
    /// Its path in the file (`tests::test_a`).
    pub path: String,
}

/// The tests `added` changes. The diff's paths are from the repository's
/// root and `below` is the run's directory in it; `read` gives a file's
/// text by its path relative to the run's directory.
pub fn changed_tests(
    added: &AddedLines,
    below: &Path,
    read: &dyn Fn(&Path) -> Option<String>,
) -> Vec<ChangedTest> {
    let mut found = Vec::new();
    for (path, ranges) in added.files() {
        let Ok(file) = path.strip_prefix(below) else {
            continue;
        };
        if file.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Some(source) = read(file) else {
            continue;
        };
        found.extend(
            test_functions(&source)
                .into_iter()
                .filter(|t| {
                    ranges
                        .iter()
                        .any(|&(a, b)| a <= t.end_line && t.start_line <= b)
                })
                .map(|t| ChangedTest {
                    file: file.to_path_buf(),
                    path: t.path,
                }),
        );
    }
    found
}

/// The libtest names among `listed` of the `changed` tests. A test's path
/// in its file is the tail of its libtest name (the file's own module path
/// precedes it), so a name matches when it is the path or ends in
/// `::path`; a same-named test elsewhere is judged too, which only runs
/// more.
pub fn libtest_names(listed: &[String], changed: &[ChangedTest]) -> Vec<String> {
    listed
        .iter()
        .filter(|name| {
            changed
                .iter()
                .any(|c| **name == c.path || name.ends_with(&format!("::{}", c.path)))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "changed_tests.test.rs"]
mod tests;
