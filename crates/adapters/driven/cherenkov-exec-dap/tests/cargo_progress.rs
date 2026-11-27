//! A running cargo's progress reaches the caller while cargo runs, not after
//! it exits: the build of the test programs and the viability check's
//! `cargo check` each say a crate is compiled as its artifact line arrives.
//! The fake cargo prints one artifact, then waits (bounded) for a gate file
//! that only the progress callback creates, so a caller that reads cargo's
//! output only at its end makes the fake fail instead of passing by timing.

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use cherenkov_exec_dap::progress::Compiling;
use cherenkov_exec_dap::toolchain::{build_rust_test_programs_with, rust_code_files_with};
use cherenkov_exec_dap::viability::{Candidate, check};
use cherenkov_mutation::id::MutantId;
use test_dir::TestDir;

const ARTIFACT: &str = r#"{"reason":"compiler-artifact","package_id":"p 0.1.0","target":{"name":"first"},"profile":{"test":false},"executable":null}"#;
const TEST: &str = r#"{"reason":"compiler-artifact","package_id":"a 0.1.0","target":{"name":"a"},"manifest_path":"/w/a/Cargo.toml","profile":{"test":true},"executable":"/t/a-1"}"#;

/// A cargo that prints an artifact, waits up to ten seconds for `gate` to
/// exist (failing if it never does), then prints the rest and succeeds.
fn gated_cargo(dir: &Path, gate: &Path) -> PathBuf {
    let path = dir.join("cargo");
    let script = format!(
        "#!/bin/sh\necho '{ARTIFACT}'\ni=0\nwhile [ ! -e '{gate}' ]; do\n  i=$((i+1))\n  [ $i -gt 200 ] && echo 'no progress before exit' >&2 && exit 1\n  sleep 0.05\ndone\necho '{TEST}'\nexit 0\n",
        gate = gate.display()
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn test_the_build_says_a_crate_is_compiled_before_cargo_exits() {
    let dir = TestDir::new("progress");
    let root = dir.path().to_path_buf();
    let gate = root.join("gate");
    let cargo = gated_cargo(&root, &gate);
    let mut said = Vec::new();
    let programs = build_rust_test_programs_with(&cargo, &root, &[], &mut |c: Compiling| {
        said.push(c);
        std::fs::write(&gate, "").unwrap();
    })
    .expect("cargo exits only after the first notice");
    assert_eq!(
        said[0],
        Compiling {
            done: 1,
            last: "first".into()
        }
    );
    assert_eq!(programs.len(), 1);
    assert_eq!(programs[0].binary, PathBuf::from("/t/a-1"));
}

#[test]
fn test_the_viability_check_says_a_crate_is_compiled_before_cargo_exits() {
    let dir = TestDir::new("progress");
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("Cargo.toml"), "").unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn f() -> u32 { 1 }\n").unwrap();
    let gate = root.join("gate");
    let bin = TestDir::new("progress");
    let cargo = gated_cargo(bin.path(), &gate);
    let candidate = Candidate {
        id: MutantId("m".into()),
        file: PathBuf::from("lib.rs"),
        start: 20,
        end: 23,
        with: "{ 0 }".into(),
    };
    let said = RefCell::new(Vec::new());
    let checked = check(
        &cargo,
        &root,
        &root,
        &[],
        &[candidate],
        &mut |_| {},
        &mut |c: Compiling| {
            said.borrow_mut().push(c);
            std::fs::write(&gate, "").unwrap();
        },
    )
    .expect("cargo exits only after the first notice");
    assert!(checked.rejected.is_empty());
    assert_eq!(
        said.borrow()[0],
        Compiling {
            done: 1,
            last: "first".into()
        }
    );
}

#[test]
fn test_the_source_facts_check_says_a_crate_is_compiled_before_cargo_exits() {
    let dir = TestDir::new("progress");
    let root = dir.path().to_path_buf();
    let gate = root.join("gate");
    let cargo = gated_cargo(&root, &gate);
    let mut said = Vec::new();
    // The fake prints no dep-info, so no source file is found; what is
    // asked is that the notice came before cargo exited successfully.
    let files = rust_code_files_with(&cargo, &root, &[], &mut |c: Compiling| {
        said.push(c);
        std::fs::write(&gate, "").unwrap();
    })
    .expect("cargo exits only after the first notice");
    assert_eq!(
        said[0],
        Compiling {
            done: 1,
            last: "first".into()
        }
    );
    assert!(files.is_empty());
}
