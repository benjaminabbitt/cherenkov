//! What `cherenkov plan` mutates and what it selects, as cargo-mutants does:
//! test code is left alone unless asked for, `.cargo/mutants.toml` and the
//! `--exclude-re`/`--examine-re` options pick mutants by cargo-mutants' name.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Project(PathBuf);

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Project {
        let dir = std::env::temp_dir().join(format!("cherenkov-sel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        }
        Project(dir)
    }

    /// `cherenkov plan` with `args`; the stdout lines (JSON) and stderr.
    fn plan(&self, args: &[&str]) -> (Vec<String>, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_cherenkov"))
            .arg("plan")
            .arg(&self.0)
            .args(["--format", "json"])
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(out.status.success(), "{stderr}");
        let lines = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        (lines, stderr)
    }

    /// The files of the planned mutants.
    fn files(&self, args: &[&str]) -> Vec<String> {
        let mut files: Vec<String> = self
            .plan(args)
            .0
            .iter()
            .filter_map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).ok()?;
                (v["event"] == "mutant.planned")
                    .then(|| v["file"].as_str().map(String::from))
                    .flatten()
            })
            .collect();
        files.sort();
        files.dedup();
        files
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _: &Path = &self.0;
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const LIB: &str = "pub fn f(a: i32) -> i32 { a + 1 }\n\
    #[cfg(test)]\n#[path = \"x.test.rs\"]\nmod tests;\n\
    #[cfg(test)]\nmod inline { pub fn z(a: i32) -> i32 { a + 1 } }\n";
const TESTS: &str = "mod deeper;\nfn t(a: i32) -> i32 { a - 1 }\n";
const DEEPER: &str = "fn d(a: i32) -> i32 { a * 2 }\n";

fn with_tests(name: &str) -> Project {
    Project::new(
        name,
        &[
            ("src/lib.rs", LIB),
            ("src/x.test.rs", TESTS),
            ("src/x.test/deeper.rs", DEEPER),
        ],
    )
}

#[test]
fn test_modules_and_their_files_are_not_mutated_by_default() {
    let p = with_tests("default");
    assert_eq!(p.files(&[]), ["src/lib.rs"]);
    assert_eq!(p.files(&["--operators", "cargo-mutants"]), ["src/lib.rs"]);
}

#[test]
fn test_include_tests_mutates_test_code_too() {
    let p = with_tests("include");
    assert_eq!(
        p.files(&["--include-tests"]),
        ["src/lib.rs", "src/x.test.rs", "src/x.test/deeper.rs"]
    );
}
