//! What is planned and which of it is kept, as cargo-mutants decides: test
//! code is left alone unless asked for (in a file, by the sets' region
//! skips; a file by the modules that lead to it), and the planned mutants
//! can be picked by file glob and by cargo-mutants' mutant name, from the
//! command line and `.cargo/mutants.toml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cherenkov_core::operator_set::OperatorSet;
use cherenkov_core::plan::MutantPlan;
use cherenkov_core::ports::SourceReader;
use cherenkov_core::profile::Profile;
use cherenkov_core::session::{ProjectPlan, plan_project};
use cherenkov_core::syntax::LanguageId;
use cherenkov_fs::FsSourceReader;
use cherenkov_syntax::rust_modules::module_files;
use cherenkov_syntax::{DEFAULT_SKIP_CALLS, TreeSitterSyntax};
use clap::Args;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::RegexSet;

use crate::config::{self, Config};
use crate::error::CliError;
use crate::names::name_all;

/// The options that choose which mutants there are, shared by `plan` and
/// `run`; cargo-mutants' names for them where it has one.
#[derive(Args, Debug, Default)]
pub struct SelectArgs {
    /// Mutate test code too: `#[cfg(test)]` and `#[test]` items and the
    /// files only they reach. cargo-mutants never does.
    #[arg(long = "include-tests")]
    pub include_tests: bool,
    /// Keep only mutants whose name (`path:line:col: replace X with Y in
    /// fn`, as cargo-mutants spells it) matches this regex (repeatable).
    #[arg(
        short = 'F',
        long = "examine-re",
        visible_alias = "re",
        value_name = "REGEX"
    )]
    pub examine_re: Vec<String>,
    /// Drop mutants whose name matches this regex (repeatable).
    #[arg(short = 'E', long = "exclude-re", value_name = "REGEX")]
    pub exclude_re: Vec<String>,
    /// Keep only mutants in files matching this glob (repeatable).
    #[arg(short = 'f', long = "file", value_name = "GLOB")]
    pub file: Vec<String>,
    /// Skip calls to functions and methods with these names, arguments
    /// and all (comma separated; `with_capacity` is always skipped unless
    /// `--skip-calls-defaults false`).
    #[arg(long = "skip-calls", value_delimiter = ',', value_name = "NAME")]
    pub skip_calls: Vec<String>,
    /// Whether to skip the calls cargo-mutants skips by default.
    #[arg(long = "skip-calls-defaults", value_name = "BOOL")]
    pub skip_calls_defaults: Option<bool>,
    /// Read this configuration file instead of `.cargo/mutants.toml`.
    #[arg(long, value_name = "FILE", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,
    /// Read no configuration file.
    #[arg(long = "no-config")]
    pub no_config: bool,
    /// Run the tests of this package only (repeatable); a `--package`
    /// argument for cargo.
    #[arg(long = "test-package", value_name = "PACKAGE")]
    pub test_package: Vec<String>,
}

/// Which mutants are planned and kept.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Mutate test code too (cargo-mutants never does).
    pub include_tests: bool,
    /// File globs (`--exclude`, `exclude_globs`) whose files are not read.
    pub exclude_globs: Vec<String>,
    /// Calls (by name) skipped with their arguments.
    pub skip_calls: Vec<String>,
    /// Arguments for cargo from the configuration and `--test-package`.
    pub cargo_args: Vec<String>,
    examine_globs: Option<GlobSet>,
    examine_re: Option<RegexSet>,
    exclude_re: Option<RegexSet>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            include_tests: false,
            exclude_globs: Vec::new(),
            skip_calls: DEFAULT_SKIP_CALLS.map(String::from).to_vec(),
            cargo_args: Vec::new(),
            examine_globs: None,
            examine_re: None,
            exclude_re: None,
        }
    }
}

/// The patterns a cargo-mutants glob stands for: without a `/` it matches
/// at any depth, files and directories; with one, from the root.
fn glob_patterns(glob: &str) -> Vec<String> {
    if glob.contains('/') {
        vec![glob.to_string()]
    } else {
        vec![format!("**/{glob}"), format!("**/{glob}/**")]
    }
}

fn invalid(what: &str, e: &dyn std::fmt::Display) -> CliError {
    CliError::Unreadable {
        path: what.to_string(),
        reason: e.to_string(),
    }
}

fn glob_set(globs: &[String]) -> Result<GlobSet, CliError> {
    let mut set = GlobSetBuilder::new();
    for pattern in globs.iter().flat_map(|g| glob_patterns(g)) {
        set.add(Glob::new(&pattern).map_err(|e| invalid(&pattern, &e))?);
    }
    set.build().map_err(|e| invalid("globs", &e))
}

fn regex_set(patterns: Vec<String>) -> Result<Option<RegexSet>, CliError> {
    if patterns.is_empty() {
        return Ok(None);
    }
    RegexSet::new(&patterns)
        .map(Some)
        .map_err(|e| invalid("regex", &e))
}

impl Selection {
    /// The selection `args` and `root`'s configuration file ask for;
    /// `exclude` are the `--exclude` globs. Lists given on the command
    /// line and in the file are added together.
    pub fn load(root: &Path, args: &SelectArgs, exclude: &[String]) -> Result<Self, CliError> {
        let config = match (&args.config, args.no_config) {
            (_, true) => Config::default(),
            (Some(path), false) => config::read(path)?,
            (None, false) => match config::tree_config_path(root) {
                Some(path) => config::read(&path)?,
                None => Config::default(),
            },
        };
        Selection::merge(args, exclude, config)
    }

    fn merge(args: &SelectArgs, exclude: &[String], config: Config) -> Result<Self, CliError> {
        let both = |a: &[String], b: &[String]| [a, b].concat();
        let defaults = args
            .skip_calls_defaults
            .or(config.skip_calls_defaults)
            .unwrap_or(true);
        let mut skip_calls = both(&args.skip_calls, &config.skip_calls);
        if defaults {
            skip_calls.extend(DEFAULT_SKIP_CALLS.map(String::from));
        }
        let packages = args
            .test_package
            .iter()
            .chain(&config.test_package)
            .flat_map(|p| ["--package".to_string(), p.clone()]);
        let examine = both(&args.file, &config.examine_globs);
        let exclude_globs: Vec<String> = both(exclude, &config.exclude_globs)
            .iter()
            .flat_map(|g| glob_patterns(g))
            .collect();
        Ok(Selection {
            include_tests: args.include_tests,
            exclude_globs,
            skip_calls,
            cargo_args: config
                .additional_cargo_args
                .iter()
                .cloned()
                .chain(packages)
                .collect(),
            examine_globs: if examine.is_empty() {
                None
            } else {
                Some(glob_set(&examine)?)
            },
            examine_re: regex_set(both(&args.examine_re, &config.examine_re))?,
            exclude_re: regex_set(both(&args.exclude_re, &config.exclude_re))?,
        })
    }

    fn filters_names(&self) -> bool {
        self.examine_re.is_some() || self.exclude_re.is_some()
    }

    /// Whether a mutant of `file` with this `name` is kept.
    fn keeps(&self, file: &Path, name: Option<&str>) -> bool {
        let in_file = self.examine_globs.as_ref().is_none_or(|g| g.is_match(file));
        let examined = match (&self.examine_re, name) {
            (Some(re), Some(name)) => re.is_match(name),
            _ => true,
        };
        let excluded =
            matches!((&self.exclude_re, name), (Some(re), Some(name)) if re.is_match(name));
        in_file && examined && !excluded
    }
}

/// `root`'s mutants for the `profiles`, planned with `set` and kept as
/// `selection` says.
pub fn plan_selected(
    root: &Path,
    syntax: &TreeSitterSyntax,
    profiles: &[Profile],
    set: &OperatorSet,
    selection: &Selection,
) -> Result<ProjectPlan, CliError> {
    let unreadable = |e: &dyn std::fmt::Display| CliError::Unreadable {
        path: root.display().to_string(),
        reason: e.to_string(),
    };
    let reader = FsSourceReader::new(root, profiles).map_err(|e| unreadable(&e))?;
    let operators = set.operators_for(selection.include_tests);
    let mut project =
        plan_project(&reader, syntax, profiles, &operators).map_err(|e| CliError::Planning {
            reason: format!("{}: {e}", root.display()),
        })?;
    if !selection.include_tests {
        let skipped = test_files(&reader).map_err(|e| unreadable(&e))?;
        project.plans.retain(|p| !skipped.contains(&p.file));
    }
    project.plans = kept(root, project.plans, selection);
    Ok(project)
}

/// `plans` that `selection` keeps.
fn kept(root: &Path, plans: Vec<MutantPlan>, selection: &Selection) -> Vec<MutantPlan> {
    let names = selection.filters_names().then(|| name_all(root, &plans));
    plans
        .into_iter()
        .enumerate()
        .filter(|(i, p)| {
            let name = names.as_ref().map(|n| n[*i].as_str());
            selection.keeps(&p.file, name)
        })
        .map(|(_, p)| p)
        .collect()
}

/// The Rust files only test code reaches: those a `#[cfg(test)]` (or
/// `#[mutants::skip]`) module declares, and the modules of those files.
fn test_files(reader: &dyn SourceReader) -> std::io::Result<BTreeSet<PathBuf>> {
    let Ok(files) = reader.list(&LanguageId::new("rust")) else {
        return Ok(BTreeSet::new());
    };
    let known: BTreeSet<&PathBuf> = files.iter().collect();
    let mut declared = Vec::new();
    for file in &files {
        let modules = module_files(file, &reader.read(file)?);
        declared.push((file.clone(), modules));
    }
    let mut skipped: BTreeSet<PathBuf> = BTreeSet::new();
    loop {
        let before = skipped.len();
        for (file, modules) in &declared {
            let whole = skipped.contains(file);
            for module in modules.iter().filter(|m| whole || m.skipped) {
                skipped.extend(
                    module
                        .candidates
                        .iter()
                        .filter(|c| known.contains(c))
                        .cloned(),
                );
            }
        }
        if skipped.len() == before {
            return Ok(skipped);
        }
    }
}
