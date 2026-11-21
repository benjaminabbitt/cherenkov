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

// --- cargo-mutants' names, config and filters ---------------------------

const NAMED: &str = "pub struct P { pub a: i32, pub b: i32 }\n\
pub mod m {\n\
    pub struct S<'a, X>(&'a X);\n\
    impl<'a, X> S<'a, X> {\n\
        pub fn get(&self, v: i32) -> Result<(), String> { if v > 1 { Ok(()) } else { Err(String::new()) } }\n\
    }\n\
    pub fn outer(x: i32) -> i32 {\n\
        fn inner(y: i32) -> i32 { y * 3 }\n\
        inner(x)\n\
    }\n\
}\n\
pub fn mk(a: i32, b: i32) -> P { P { a: a + 1, b } }\n\
pub fn arms(n: u8, k: i32) -> i32 {\n\
    match n {\n\
        1 | 2 => 1,\n\
        3 if k > 2 => 5,\n\
        _ => 0,\n\
    }\n\
}\n\
pub const C: usize = 2 * 3;\n";

impl Project {
    /// `cherenkov plan --names` with `args`: the mutants' names.
    fn names(&self, args: &[&str]) -> Vec<String> {
        let mut all = vec!["--operators", "cargo-mutants", "--names"];
        all.extend_from_slice(args);
        self.plan(&all).0
    }
}

#[test]
fn test_names_are_cargo_mutants_names() {
    let p = Project::new("names", &[("src/lib.rs", NAMED)]);
    let names = p.names(&[]);
    for want in [
        "src/lib.rs:5:51: replace m::S<'a, X>::get -> Result<(), String> with Ok(())",
        "src/lib.rs:5:56: replace > with < in m::S<'a, X>::get",
        "src/lib.rs:8:27: replace m::outer::inner -> i32 with 0",
        "src/lib.rs:8:29: replace * with / in m::outer::inner",
        "src/lib.rs:12:34: replace mk -> P with Default::default()",
        "src/lib.rs:12:43: replace + with * in mk",
        "src/lib.rs:15:1: delete match arm 1 | 2 in arms",
        "src/lib.rs:16:6: replace match guard k > 2 with true in arms",
        "src/lib.rs:20:24: replace * with +",
    ] {
        assert!(names.iter().any(|n| n == want), "{want}\n{names:#?}");
    }
}

fn config(name: &str, toml: &str) -> Project {
    Project::new(
        name,
        &[
            ("src/lib.rs", NAMED),
            ("src/other.rs", "pub fn o(a: i32) -> i32 { a + 1 }\n"),
            (".cargo/mutants.toml", toml),
        ],
    )
}

#[test]
fn test_exclude_re_in_the_config_drops_mutants_by_name() {
    let p = config(
        "exre",
        "exclude_re = ['replace \\+ with \\* in mk', 'in arms$']\n",
    );
    let names = p.names(&[]);
    assert!(!names.iter().any(|n| n.ends_with("replace + with * in mk")));
    assert!(names.iter().any(|n| n.ends_with("replace + with - in mk")));
    assert!(!names.iter().any(|n| n.ends_with(" in arms")), "{names:#?}");
}

#[test]
fn test_examine_re_keeps_only_matching_mutants_and_the_flags_add_to_the_config() {
    let p = config("exam", "examine_re = ['in mk$']\n");
    assert!(
        p.names(&[])
            .iter()
            .all(|n| n.ends_with(" in mk") || n.contains(" mk -> "))
    );
    // -F adds to the config's list; -E removes.
    let names = p.names(&["-F", "in arms$", "-E", "match guard"]);
    assert!(names.iter().any(|n| n.ends_with(" in arms")));
    assert!(names.iter().any(|n| n.ends_with(" in mk")));
    assert!(!names.iter().any(|n| n.contains("match guard")));
}

#[test]
fn test_globs_select_files_as_cargo_mutants_does() {
    let p = config("globs", "exclude_globs = ['other.rs']\n");
    assert_eq!(p.files(&["--operators", "cargo-mutants"]), ["src/lib.rs"]);
    let p = config("globs2", "");
    assert_eq!(p.files(&["-f", "src/o*.rs"]), ["src/other.rs"]);
    assert_eq!(p.files(&["-e", "lib.rs"]), ["src/other.rs"]);
}

#[test]
fn test_no_config_ignores_the_file_and_config_names_another() {
    let p = config("noconf", "exclude_globs = ['other.rs']\n");
    assert_eq!(p.files(&["--no-config"]), ["src/lib.rs", "src/other.rs"]);
    std::fs::write(p.0.join("alt.toml"), "exclude_globs = ['lib.rs']\n").unwrap();
    let alt = p.0.join("alt.toml");
    assert_eq!(
        p.files(&["--config", alt.to_str().unwrap()]),
        ["src/other.rs"]
    );
}

#[test]
fn test_a_root_mutants_toml_is_read_when_there_is_no_cargo_one() {
    let p = Project::new(
        "rootconf",
        &[
            ("src/lib.rs", NAMED),
            ("src/other.rs", "pub fn o(a: i32) -> i32 { a + 1 }\n"),
            ("mutants.toml", "exclude_globs = ['other.rs']\n"),
        ],
    );
    assert_eq!(p.files(&[]), ["src/lib.rs"]);
}

const CALLS: &str = "pub fn f(n: usize) -> usize {\n\
    let v: Vec<u8> = Vec::with_capacity(1 + 2);\n\
    let w = keep(3 + 4);\n\
    v.len() + w + n\n\
}\n\
fn keep(a: usize) -> usize { a }\n";

fn calls(name: &str, toml: &str) -> Vec<String> {
    let p = Project::new(
        name,
        &[("src/lib.rs", CALLS), (".cargo/mutants.toml", toml)],
    );
    p.names(&[])
}

#[test]
fn test_skip_calls_default_to_with_capacity_and_the_config_extends_or_replaces_them() {
    let has = |names: &[String], what: &str| names.iter().any(|n| n.contains(what));
    let default = calls("calls0", "");
    assert!(!has(&default, "replace + with - in f") || !has(&default, "1 + 2"));
    assert!(
        !default
            .iter()
            .any(|n| n.contains(":2:") && n.contains("replace +"))
    );
    assert!(
        default
            .iter()
            .any(|n| n.contains(":3:") && n.contains("replace +"))
    );
    let extended = calls("calls1", "skip_calls = ['keep']\n");
    assert!(
        !extended
            .iter()
            .any(|n| n.contains(":3:") && n.contains("replace +"))
    );
    let replaced = calls(
        "calls2",
        "skip_calls = ['keep']\nskip_calls_defaults = false\n",
    );
    assert!(
        replaced
            .iter()
            .any(|n| n.contains(":2:") && n.contains("replace +"))
    );
    assert!(
        !replaced
            .iter()
            .any(|n| n.contains(":3:") && n.contains("replace +"))
    );
}
