use coded_diagnostic::Diagnostic;
use std::path::PathBuf;

use cherenkov_exec_dap::toolchain::test_binaries_from_cargo_json;

#[test]
fn test_cargo_json_yields_test_executables_only() {
    let stdout = r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"calc"},"profile":{"test":false},"executable":null}
{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"calc"},"profile":{"test":true},"executable":"/t/out/calc-6a9d"}
{"reason":"compiler-artifact","target":{"kind":["test"],"name":"calc"},"profile":{"test":true},"executable":"/t/out/calc-c649"}
{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"tool"},"profile":{"test":false},"executable":"/t/out/tool"}
{"reason":"build-finished","success":true}
not json at all
"#;
    assert_eq!(
        test_binaries_from_cargo_json(stdout),
        [
            PathBuf::from("/t/out/calc-6a9d"),
            PathBuf::from("/t/out/calc-c649")
        ]
    );
}

// --- The code under test is what a non-test build compiles: rustc's
// dep-info for it lists every source file, and leaves out modules that
// only exist under `cfg(test)` (inline or `#[path = "x.test.rs"]`).

use cherenkov_exec_dap::toolchain::{dep_info_of, dep_info_sources};

#[test]
fn test_a_checked_crate_s_dep_info_sits_beside_its_metadata() {
    let filenames = ["/w/target/debug/deps/libdeps-147def452609373e.rmeta".to_string()];
    assert_eq!(
        dep_info_of(&filenames),
        Some(PathBuf::from(
            "/w/target/debug/deps/deps-147def452609373e.d"
        ))
    );
    assert_eq!(
        dep_info_of(&["/w/target/debug/build/x/out".to_string()]),
        None
    );
}

#[test]
fn test_dep_info_names_the_rust_sources_of_its_first_rule_from_the_workspace_root() {
    let text = "/w/target/debug/deps/deps-1.d: crates/deps/src/lib.rs crates/deps/src/a\\ b.rs /reg/serde/src/lib.rs crates/deps/data.toml\n\ncrates/deps/src/lib.rs:\ncrates/deps/src/a\\ b.rs:\n";
    assert_eq!(
        dep_info_sources(text, std::path::Path::new("/w")),
        [
            PathBuf::from("/w/crates/deps/src/lib.rs"),
            PathBuf::from("/w/crates/deps/src/a b.rs"),
            PathBuf::from("/reg/serde/src/lib.rs"),
        ]
    );
}

// --- `cargo test` runs each test binary in its package's directory, and
// tests depend on it (a nested `cargo build` finds the project's
// rust-toolchain.toml from there): so does Cherenkov.

use cherenkov_exec_dap::toolchain::{TestProgram, test_programs_from_cargo_json};

#[test]
fn test_cargo_json_yields_each_test_executable_with_its_package_directory() {
    let stdout = r#"{"reason":"compiler-artifact","manifest_path":"/w/crates/calc/Cargo.toml","profile":{"test":true},"executable":"/t/out/calc-6a9d"}
{"reason":"compiler-artifact","manifest_path":"/w/crates/tool/Cargo.toml","profile":{"test":false},"executable":"/t/out/tool"}
"#;
    assert_eq!(
        test_programs_from_cargo_json(stdout),
        [TestProgram {
            binary: PathBuf::from("/t/out/calc-6a9d"),
            package_dir: PathBuf::from("/w/crates/calc"),
        }]
    );
}

#[test]
fn test_a_test_program_runs_with_the_environment_cargo_test_gives_it() {
    use cherenkov_exec_dap::toolchain::cargo_test_env;
    let program = TestProgram {
        binary: PathBuf::from("/t/out/calc-6a9d"),
        package_dir: PathBuf::from("/w/crates/calc"),
    };
    let env = cargo_test_env(&program, std::path::Path::new("/bin/cargo"), Some("1.96.1"));
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("CARGO"), Some("/bin/cargo"));
    assert_eq!(get("CARGO_MANIFEST_DIR"), Some("/w/crates/calc"));
    assert_eq!(
        get("CARGO_MANIFEST_PATH"),
        Some("/w/crates/calc/Cargo.toml")
    );
    // rustup's pin, which nested cargo and rustc calls inherit.
    assert_eq!(get("RUSTUP_TOOLCHAIN"), Some("1.96.1"));
    let unpinned = cargo_test_env(&program, std::path::Path::new("/bin/cargo"), None);
    assert!(unpinned.iter().all(|(n, _)| n != "RUSTUP_TOOLCHAIN"));
}

#[test]
fn test_a_workspace_s_crates_are_its_members_lib_and_bin_targets_as_rustc_names_them() {
    use cherenkov_exec_dap::toolchain::workspace_from_metadata;
    let meta: serde_json::Value = serde_json::from_str(
        r#"{"workspace_root":"/w","packages":[
            {"targets":[{"kind":["lib"],"name":"read-model"},{"kind":["test"],"name":"it"}]},
            {"targets":[{"kind":["bin"],"name":"ctxloom"}]}]}"#,
    )
    .unwrap();
    assert_eq!(
        workspace_from_metadata(&meta),
        (
            PathBuf::from("/w"),
            vec!["ctxloom".to_string(), "read_model".to_string()]
        )
    );
}

// --- The debugger needs a debug build whatever the project's profiles say
// (a size-optimized, stripped test profile has no line table and inlines
// the functions mutants are placed in), unless the caller set it.

use cherenkov_exec_dap::toolchain::debug_profile_env;

#[test]
fn test_the_test_build_is_unoptimized_with_debug_info_and_symbols() {
    let env = debug_profile_env(|_| false);
    let has = |k: &str, v: &str| env.iter().any(|(a, b)| a == k && b == v);
    for profile in ["TEST", "DEV"] {
        assert!(has(&format!("CARGO_PROFILE_{profile}_OPT_LEVEL"), "0"));
        assert!(has(&format!("CARGO_PROFILE_{profile}_DEBUG"), "2"));
        assert!(has(&format!("CARGO_PROFILE_{profile}_STRIP"), "false"));
    }
}

#[test]
fn test_a_profile_setting_the_caller_made_is_kept() {
    let env = debug_profile_env(|name| name == "CARGO_PROFILE_TEST_OPT_LEVEL");
    assert!(!env.iter().any(|(k, _)| k == "CARGO_PROFILE_TEST_OPT_LEVEL"));
    assert!(env.iter().any(|(k, _)| k == "CARGO_PROFILE_TEST_DEBUG"));
}

#[test]
fn test_a_workspace_s_packages_are_its_members_names_and_directories() {
    use cherenkov_exec_dap::toolchain::{Package, packages_from_metadata};
    let meta: serde_json::Value = serde_json::json!({"packages": [
        {"name": "calc-core", "manifest_path": "/w/crates/core/Cargo.toml", "dependencies": []},
        {"name": "calc", "manifest_path": "/w/Cargo.toml", "dependencies": [
            {"name": "calc-core", "path": "/w/crates/core"},
            {"name": "serde"}
        ]}
    ]});
    assert_eq!(
        packages_from_metadata(&meta),
        [
            Package {
                name: "calc-core".into(),
                dir: PathBuf::from("/w/crates/core"),
                deps: vec![]
            },
            Package {
                name: "calc".into(),
                dir: PathBuf::from("/w"),
                deps: vec!["calc-core".into()]
            },
        ]
    );
}

#[test]
fn test_a_file_belongs_to_the_package_with_the_deepest_directory_above_it() {
    use cherenkov_exec_dap::toolchain::{Package, package_of};
    let packages = [
        Package {
            name: "calc".into(),
            dir: PathBuf::from("/w"),
            deps: vec![],
        },
        Package {
            name: "calc-core".into(),
            dir: PathBuf::from("/w/crates/core"),
            deps: vec![],
        },
    ];
    let of = |f: &str| package_of(&packages, std::path::Path::new(f));
    assert_eq!(of("/w/crates/core/src/lib.rs"), Some("calc-core"));
    assert_eq!(of("/w/src/main.rs"), Some("calc"));
    assert_eq!(
        of("/w/crates/corex/src/lib.rs"),
        Some("calc"),
        "a sibling's prefix is not its parent"
    );
    assert_eq!(of("/elsewhere/lib.rs"), None);
}

#[test]
fn test_cargo_is_told_to_build_only_the_named_packages_tests() {
    use cherenkov_exec_dap::toolchain::package_args;
    assert_eq!(
        package_args(&["a".to_string(), "b".to_string()]),
        ["--package", "a", "--package", "b"]
    );
    assert!(package_args(&[]).is_empty());
}

/// A stand-in for cargo that answers `metadata` and `test` with canned
/// output (or fails, when `ok` is false) and logs its arguments.
struct FakeCargo {
    dir: PathBuf,
    /// One fake at a time: a script being written must not be inherited
    /// open by another test's child process ("text file busy").
    _serial: std::sync::MutexGuard<'static, ()>,
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl FakeCargo {
    fn new(tag: &str, ok: bool) -> FakeCargo {
        use std::os::unix::fs::PermissionsExt;
        let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cherenkov-fake-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let metadata = r#"{"workspace_root":"/w","packages":[{"name":"a","manifest_path":"/w/a/Cargo.toml","dependencies":[]}]}"#;
        let test = r#"{"reason":"compiler-artifact","manifest_path":"/w/a/Cargo.toml","profile":{"test":true},"executable":"/t/a-1"}"#;
        let exit = if ok { 0 } else { 1 };
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$1\" in\n metadata) echo '{metadata}';;\n test) echo '{test}';;\nesac\nexit {exit}\n",
            log = dir.join("log").display()
        );
        let path = dir.join("cargo");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        FakeCargo {
            dir,
            _serial: serial,
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("cargo")
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Drop for FakeCargo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn test_the_workspace_s_packages_are_what_cargo_metadata_says() {
    use cherenkov_exec_dap::toolchain::workspace_packages;
    let cargo = FakeCargo::new("meta", true);
    let found = workspace_packages(&cargo.path(), std::path::Path::new("/")).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "a");
    assert_eq!(found[0].dir, PathBuf::from("/w/a"));
}

#[test]
fn test_a_cargo_metadata_that_fails_is_an_error() {
    use cherenkov_exec_dap::toolchain::workspace_packages;
    let cargo = FakeCargo::new("metafail", false);
    let error = workspace_packages(&cargo.path(), std::path::Path::new("/")).unwrap_err();
    assert_eq!(error.code(), "cherenkov-exec-dap-toolchain-failed");
    assert!(error.to_string().contains("cargo metadata"), "{error}");
}

#[test]
fn test_test_programs_are_built_for_the_named_packages_and_the_workspace_without() {
    use cherenkov_exec_dap::toolchain::{
        build_rust_test_programs_of, build_rust_test_programs_with,
    };
    let cargo = FakeCargo::new("build", true);
    let here = std::path::Path::new("/");
    let named = build_rust_test_programs_of(
        &cargo.path(),
        here,
        &["--locked".to_string()],
        &["a".to_string(), "b".to_string()],
    )
    .unwrap();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].binary, PathBuf::from("/t/a-1"));
    let all =
        build_rust_test_programs_with(&cargo.path(), here, &["--locked".to_string()]).unwrap();
    assert_eq!(all, named);
    assert_eq!(
        cargo.log(),
        [
            "test --no-run --message-format=json --locked --package a --package b",
            "test --no-run --message-format=json --locked"
        ]
    );
}

#[test]
fn test_a_cargo_test_build_that_fails_is_an_error() {
    use cherenkov_exec_dap::toolchain::build_rust_test_programs_of;
    let cargo = FakeCargo::new("buildfail", false);
    let error = build_rust_test_programs_of(&cargo.path(), std::path::Path::new("/"), &[], &[])
        .unwrap_err();
    assert_eq!(error.code(), "cherenkov-exec-dap-toolchain-failed");
    assert!(error.to_string().contains("cargo test --no-run"), "{error}");
}
