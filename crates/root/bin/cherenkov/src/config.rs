//! `.cargo/mutants.toml`, cargo-mutants' configuration file, and what of it
//! applies here. Every key cargo-mutants has is accepted; those that only
//! steer its own build and test runs (`gitignore`, `copy_target`,
//! `cap_lints`, ...) are ignored. `test_package` and `test_workspace` choose
//! which packages' tests judge a mutant (`test_scope.rs`,
//! `docs/test-scope.md`). Re-derived from its
//! documented behaviour (MIT, Copyright (c) 2022-2025 Martin Pool; the
//! book's `skip_files`, `filter_mutants` and `skip_calls` pages).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use cherenkov_cli::error::CliError;

/// The keys that select or run mutants.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Arguments for every cargo invocation (`cargo build`, `cargo test`).
    pub additional_cargo_args: Vec<String>,
    /// Keep only the mutants of files matching these globs.
    pub examine_globs: Vec<String>,
    /// Keep only the mutants whose name matches one of these regexes.
    pub examine_re: Vec<String>,
    /// Drop the mutants of files matching these globs.
    pub exclude_globs: Vec<String>,
    /// Drop the mutants whose name matches one of these regexes.
    pub exclude_re: Vec<String>,
    /// Skip calls to functions and methods with these names.
    pub skip_calls: Vec<String>,
    /// Add the built-in `skip_calls` (`with_capacity`); on unless false.
    pub skip_calls_defaults: Option<bool>,
    /// Judge every mutant by the tests of these packages.
    pub test_package: Vec<String>,
    /// Judge every mutant by the whole workspace's tests.
    pub test_workspace: Option<bool>,
    /// Leave these tests (by the runner's name) out of judging; the key of
    /// `--skip-test`. cargo-mutants has none: it passes `--skip` through
    /// `additional_cargo_test_args`, which is not read here.
    pub skip_test: Vec<String>,
    /// Scale test timeouts. Cherenkov sets its own timeouts, so this is
    /// read and not used.
    pub timeout_multiplier: Option<f64>,
}

/// Where the project's configuration is: `.cargo/mutants.toml`, else
/// `mutants.toml` at the root.
pub fn tree_config_path(root: &Path) -> Option<PathBuf> {
    [root.join(".cargo/mutants.toml"), root.join("mutants.toml")]
        .into_iter()
        .find(|p| p.is_file())
}

pub fn read(path: &Path) -> Result<Config, CliError> {
    let bad = |e: &dyn std::fmt::Display| CliError::Unreadable {
        path: path.display().to_string(),
        detail: e.to_string(),
    };
    let text = std::fs::read_to_string(path).map_err(|e| bad(&e))?;
    toml::from_str(&text).map_err(|e| bad(&e))
}
