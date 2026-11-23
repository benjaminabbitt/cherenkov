//! Building test programs with the language's own toolchain, in its usual
//! output location (cargo's `target/`, or a scratch path for `go test -c`).

use std::path::{Path, PathBuf};
use std::process::Command;

use cherenkov_core::execution::ExecError;
use serde_json::Value;

/// Build a Go package's test binary without optimization or inlining, so
/// code generation is regular enough for tier P.
pub fn build_go_test_binary(go: &Path, package_dir: &Path, out: &Path) -> Result<(), ExecError> {
    let output = Command::new(go)
        .args(["test", "-c", "-gcflags=all=-N -l", "-o"])
        .arg(out)
        .current_dir(package_dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", go.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "go test -c failed in {}: {}",
            package_dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Build every test binary of a Cargo package or workspace (debug profile,
/// with debug info) and return their paths.
pub fn build_rust_test_binaries(
    cargo: &Path,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, ExecError> {
    let output = Command::new(cargo)
        .args(["test", "--no-run", "--message-format=json"])
        .current_dir(manifest_dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo test --no-run failed in {}: {}",
            manifest_dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(test_binaries_from_cargo_json(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// The profile settings a debugger-run build needs, as environment variables
/// for cargo: no optimization (so functions are not inlined away), full debug
/// info and symbols. Both the `test` and the `dev` profile are set, as an
/// integration test links its package's library built with `dev`. A setting
/// `is_set` says the caller made (a project's own `CARGO_PROFILE_*`) is kept.
pub fn debug_profile_env(is_set: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for profile in ["TEST", "DEV"] {
        for (key, value) in [("OPT_LEVEL", "0"), ("DEBUG", "2"), ("STRIP", "false")] {
            let name = format!("CARGO_PROFILE_{profile}_{key}");
            if !is_set(&name) {
                env.push((name, value.to_string()));
            }
        }
    }
    env
}

/// A test executable and the directory `cargo test` runs it in: its
/// package's, where the manifest is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestProgram {
    pub binary: PathBuf,
    pub package_dir: PathBuf,
}

/// [`build_rust_test_binaries`], with each binary's package directory.
pub fn build_rust_test_programs(
    cargo: &Path,
    manifest_dir: &Path,
) -> Result<Vec<TestProgram>, ExecError> {
    build_rust_test_programs_with(cargo, manifest_dir, &[])
}

/// [`build_rust_test_programs`], with arguments for cargo as the project
/// builds its tests (`--lib`, `--features test-utils`).
pub fn build_rust_test_programs_with(
    cargo: &Path,
    manifest_dir: &Path,
    cargo_args: &[String],
) -> Result<Vec<TestProgram>, ExecError> {
    build_rust_test_programs_of(cargo, manifest_dir, cargo_args, &[])
}

/// `--package NAME` for each of `packages`: cargo's selection of the
/// packages a command is for (none given, the command's default).
pub fn package_args(packages: &[String]) -> Vec<String> {
    packages
        .iter()
        .flat_map(|p| ["--package".to_string(), p.clone()])
        .collect()
}

/// [`build_rust_test_programs_with`], for the tests of `packages` only (the
/// whole workspace's when there are none).
pub fn build_rust_test_programs_of(
    cargo: &Path,
    manifest_dir: &Path,
    cargo_args: &[String],
    packages: &[String],
) -> Result<Vec<TestProgram>, ExecError> {
    let output = Command::new(cargo)
        .args(["test", "--no-run", "--message-format=json"])
        .args(cargo_args)
        .args(package_args(packages))
        .envs(debug_profile_env(|name| std::env::var_os(name).is_some()))
        .current_dir(manifest_dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo test --no-run failed in {}: {}",
            manifest_dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(test_programs_from_cargo_json(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Test executables in cargo's `--message-format=json` output, each with its
/// package directory.
pub fn test_programs_from_cargo_json(stdout: &str) -> Vec<TestProgram> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true)
        .filter_map(|m| {
            let binary = PathBuf::from(m["executable"].as_str()?);
            let manifest = PathBuf::from(m["manifest_path"].as_str()?);
            Some(TestProgram {
                binary,
                package_dir: manifest.parent()?.to_path_buf(),
            })
        })
        .collect()
}

/// Test executables named in cargo's `--message-format=json` output.
pub fn test_binaries_from_cargo_json(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true)
        .filter_map(|m| m["executable"].as_str().map(PathBuf::from))
        .collect()
}

/// libtest arguments that run only the named tests (exact names; several
/// filters are allowed). Empty when no test is named: every test runs.
pub fn rust_test_args(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut args = names.to_vec();
    args.push("--exact".to_string());
    args
}

/// `go test` binary arguments that run only the named tests. `-test.run`
/// takes one regex per `/`-separated level of subtest names; each name's
/// levels are matched literally. With several names of differing depth, a
/// level's alternation applies to every test that reaches it (Go's rule).
pub fn go_test_args(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut levels: Vec<Vec<String>> = Vec::new();
    for name in names {
        for (i, part) in name.split('/').enumerate() {
            if levels.len() <= i {
                levels.push(Vec::new());
            }
            let escaped = regex_escape(part);
            if !levels[i].contains(&escaped) {
                levels[i].push(escaped);
            }
        }
    }
    let pattern: Vec<String> = levels
        .iter()
        .map(|alts| format!("^({})$", alts.join("|")))
        .collect();
    vec!["-test.run".to_string(), pattern.join("/")]
}

/// Escape Go RE2 metacharacters.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if r"\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The directory of `dir` relative to its Cargo workspace root, as cargo
/// reports it (`cargo metadata`): the compiler records source paths relative
/// to the workspace root. Empty when `dir` is the root.
pub fn cargo_workspace_prefix(cargo: &Path, dir: &Path) -> Result<PathBuf, ExecError> {
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo metadata failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let meta: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ExecError(format!("cargo metadata output: {e}")))?;
    let root = PathBuf::from(meta["workspace_root"].as_str().unwrap_or_default());
    let dir = dir
        .canonicalize()
        .map_err(|e| ExecError(format!("{}: {e}", dir.display())))?;
    Ok(dir
        .strip_prefix(&root)
        .map(Path::to_path_buf)
        .unwrap_or_default())
}

/// The Rust source files of the code under test in `dir`'s package: what
/// a non-test build compiles, from rustc's dep-info (`cargo check`). Modules
/// that exist only under `cfg(test)`, inline or in their own file, are not
/// in it. Absolute paths, under `dir` only.
pub fn rust_code_files(cargo: &Path, dir: &Path) -> Result<Vec<PathBuf>, ExecError> {
    rust_code_files_with(cargo, dir, &[])
}

/// [`rust_code_files`], with arguments for cargo (features select code).
pub fn rust_code_files_with(
    cargo: &Path,
    dir: &Path,
    cargo_args: &[String],
) -> Result<Vec<PathBuf>, ExecError> {
    let output = Command::new(cargo)
        .args(["check", "--message-format=json"])
        .args(cargo_args)
        .current_dir(dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo check failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let dir = dir
        .canonicalize()
        .map_err(|e| ExecError(format!("{}: {e}", dir.display())))?;
    let root = dir
        .ancestors()
        .nth(cargo_workspace_prefix(cargo, &dir)?.components().count());
    let root = root.unwrap_or(&dir).to_path_buf();
    let mut files = Vec::new();
    for message in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == false)
    {
        let filenames: Vec<String> = message["filenames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str().map(String::from))
            .collect();
        let Some(dep_info) = dep_info_of(&filenames) else {
            continue;
        };
        let text = std::fs::read_to_string(&dep_info)
            .map_err(|e| ExecError(format!("{}: {e}", dep_info.display())))?;
        files.extend(
            dep_info_sources(&text, &root)
                .into_iter()
                .filter(|f| f.starts_with(&dir)),
        );
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// The dep-info file rustc writes beside a crate's metadata
/// (`deps/libNAME-HASH.rmeta` -> `deps/NAME-HASH.d`).
pub fn dep_info_of(filenames: &[String]) -> Option<PathBuf> {
    filenames.iter().find_map(|f| {
        let path = Path::new(f);
        let stem = path.file_name()?.to_str()?.strip_suffix(".rmeta")?;
        let stem = stem.strip_prefix("lib").unwrap_or(stem);
        Some(path.with_file_name(format!("{stem}.d")))
    })
}

/// The Rust sources a dep-info file's first rule depends on, relative
/// paths taken from the workspace `root` (where cargo runs rustc).
pub fn dep_info_sources(text: &str, root: &Path) -> Vec<PathBuf> {
    let Some(rule) = text.lines().find(|l| !l.trim().is_empty()) else {
        return Vec::new();
    };
    // `target: dep dep ...`; the target itself may not hold ": ".
    let Some((_, deps)) = rule.split_once(": ") else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut chars = deps.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => current.extend(chars.next()),
            ' ' => paths.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    paths.push(current);
    paths
        .into_iter()
        .filter(|p| p.ends_with(".rs"))
        .map(|p| root.join(p))
        .collect()
}

/// The environment `cargo test` runs a test program with, beyond the
/// caller's own: cargo's runtime variables, and rustup's toolchain pin
/// (`toolchain`, when rustup chose one), which nested `cargo` and `rustc`
/// calls inherit.
pub fn cargo_test_env(
    program: &TestProgram,
    cargo: &Path,
    toolchain: Option<&str>,
) -> Vec<(String, String)> {
    let dir = program.package_dir.display().to_string();
    let mut env = vec![
        ("CARGO".to_string(), cargo.display().to_string()),
        ("CARGO_MANIFEST_DIR".to_string(), dir.clone()),
        (
            "CARGO_MANIFEST_PATH".to_string(),
            program.package_dir.join("Cargo.toml").display().to_string(),
        ),
    ];
    if let Some(t) = toolchain {
        env.push(("RUSTUP_TOOLCHAIN".to_string(), t.to_string()));
    }
    env
}

/// The toolchain rustup chooses in `dir` (`rust-toolchain.toml`, an
/// override, or the default); `None` without rustup.
pub fn rustup_toolchain(dir: &Path) -> Option<String> {
    let out = Command::new("rustup")
        .args(["show", "active-toolchain"])
        .current_dir(dir)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let name = text.split_whitespace().next()?;
    (out.status.success() && !name.is_empty()).then(|| name.to_string())
}

/// The root of `dir`'s Cargo workspace and its members' crate names (each
/// lib and bin target, as rustc names them), from `cargo metadata`.
pub fn cargo_workspace(cargo: &Path, dir: &Path) -> Result<(PathBuf, Vec<String>), ExecError> {
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo metadata failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let meta: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ExecError(format!("cargo metadata output: {e}")))?;
    Ok(workspace_from_metadata(&meta))
}

/// [`cargo_workspace`] from `cargo metadata`'s output.
pub fn workspace_from_metadata(meta: &Value) -> (PathBuf, Vec<String>) {
    let root = PathBuf::from(meta["workspace_root"].as_str().unwrap_or_default());
    let mut crates: Vec<String> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["targets"].as_array().into_iter().flatten())
        .filter(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "lib" || k == "bin"))
        })
        .filter_map(|t| t["name"].as_str().map(|n| n.replace('-', "_")))
        .collect();
    crates.sort();
    crates.dedup();
    (root, crates)
}

/// A workspace member: its name and the directory of its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub dir: PathBuf,
}

/// The members of `dir`'s Cargo workspace, from `cargo metadata`.
pub fn workspace_packages(cargo: &Path, dir: &Path) -> Result<Vec<Package>, ExecError> {
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(dir)
        .output()
        .map_err(|e| ExecError(format!("running {}: {e}", cargo.display())))?;
    if !output.status.success() {
        return Err(ExecError(format!(
            "cargo metadata failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let meta: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| ExecError(format!("cargo metadata output: {e}")))?;
    Ok(packages_from_metadata(&meta))
}

/// [`workspace_packages`] from `cargo metadata`'s output.
pub fn packages_from_metadata(meta: &Value) -> Vec<Package> {
    meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let manifest = PathBuf::from(p["manifest_path"].as_str()?);
            Some(Package {
                name: p["name"].as_str()?.to_string(),
                dir: manifest.parent()?.to_path_buf(),
            })
        })
        .collect()
}

/// The package a file belongs to: the one whose directory is the deepest
/// above it (a package may sit inside another's directory).
pub fn package_of<'a>(packages: &'a [Package], file: &Path) -> Option<&'a str> {
    packages
        .iter()
        .filter(|p| file.starts_with(&p.dir))
        .max_by_key(|p| p.dir.components().count())
        .map(|p| p.name.as_str())
}
