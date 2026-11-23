//! Compiling the driver twice with the pinned rustc: the originals and the
//! mutants, with a debug build's settings (debug assertions and overflow
//! checks on, no optimisation, debug info with columns).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cases::Case;
use crate::driver;

/// The `rustc -vV` of the toolchain on `PATH`.
pub fn rustc_version() -> String {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("rustc runs");
    String::from_utf8(out.stdout).expect("rustc prints text")
}

/// The `release` and `commit-hash` lines of `rustc -vV`, as one string.
pub fn rustc_identity() -> String {
    let field = |name: &str| {
        rustc_version()
            .lines()
            .find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_string()))
            .unwrap_or_default()
    };
    format!("{} ({})", field("release:"), field("commit-hash:"))
}

fn write_crate(dir: &Path, cases: &[Case], mutated: bool) {
    std::fs::create_dir_all(dir).expect("scratch directory");
    std::fs::write(dir.join("support.rs"), driver::SUPPORT).expect("write support");
    std::fs::write(dir.join("main.rs"), driver::main_source(cases)).expect("write main");
    for c in cases {
        let text = if mutated {
            &c.mutated
        } else {
            &c.snippet.source
        };
        std::fs::write(dir.join(format!("s_{}.rs", c.id)), text).expect("write snippet");
    }
}

fn rustc(dir: &Path, out: &Path) -> std::io::Result<std::process::Child> {
    Command::new("rustc")
        .current_dir(dir)
        .args([
            "--edition",
            "2024",
            "--crate-name",
            "gate",
            "-A",
            "warnings",
        ])
        .args(["-C", "opt-level=0", "-C", "debuginfo=2"])
        .args(["-C", "overflow-checks=on", "-C", "debug-assertions=on"])
        .args(["-C", "codegen-units=1", "-C", "panic=unwind"])
        .arg("main.rs")
        .arg("-o")
        .arg(out)
        .spawn()
}

/// The two executables: the originals and the mutants, built in `work`.
pub struct Built {
    pub original: PathBuf,
    pub mutant: PathBuf,
}

pub fn build(work: &Path, cases: &[Case]) -> Built {
    let mut children = Vec::new();
    let mut exes = Vec::new();
    for (name, mutated) in [("original", false), ("mutant", true)] {
        let dir = work.join(name);
        write_crate(&dir, cases, mutated);
        let exe = work.join(format!("{name}.exe"));
        children.push(rustc(&dir, &exe).expect("rustc starts"));
        exes.push(exe);
    }
    for mut child in children {
        assert!(
            child.wait().expect("rustc finishes").success(),
            "rustc failed"
        );
    }
    let mutant = exes.pop().expect("two executables");
    let original = exes.pop().expect("two executables");
    Built { original, mutant }
}
