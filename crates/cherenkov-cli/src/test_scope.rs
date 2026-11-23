//! Which tests judge a mutant, as cargo-mutants decides it. Re-derived from
//! its documented behaviour (MIT, Copyright (c) 2022-2025 Martin Pool;
//! the book's "Workspace and package support" page, and `Options::from_args`
//! and the lab's per-mutant package choice in 26.2): described, not copied.
//!
//! - By default a mutant is judged by the tests of the package it is in.
//! - `--test-workspace=true` (or `test_workspace = true` in the
//!   configuration): every mutant is judged by the whole workspace's tests.
//! - `--test-package A,B` (or `test_package` in the configuration): every
//!   mutant is judged by the tests of those packages.
//! - Precedence, first that applies: `--test-workspace=true`,
//!   `--test-package`, the configuration's `test_workspace = true`, its
//!   `test_package`, else the mutated package. An explicit
//!   `--test-workspace=false` only stops the configuration's
//!   `test_workspace = true`; the configuration's `test_package` still
//!   applies then.
//!
//! The test programs that are built, prepared and recorded are those of the
//! packages that judge a planned mutant; a program is only ever given the
//! mutants it judges.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use cherenkov_core::id::MutantId;
use cherenkov_core::plan::MutantPlan;
use cherenkov_exec_dap::toolchain::{Package, package_of};

/// Which packages' tests judge a mutant.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TestPackages {
    /// The package the mutant is in.
    #[default]
    Mutated,
    /// Every package of the workspace.
    Workspace,
    /// These packages, for every mutant.
    Named(Vec<String>),
}

impl TestPackages {
    /// What the command line and the configuration ask for; the command
    /// line has priority.
    pub fn resolve(
        args_workspace: Option<bool>,
        args_packages: &[String],
        config_workspace: Option<bool>,
        config_packages: &[String],
    ) -> Self {
        if args_workspace == Some(true) {
            TestPackages::Workspace
        } else if !args_packages.is_empty() {
            TestPackages::Named(args_packages.to_vec())
        } else if args_workspace.is_none() && config_workspace == Some(true) {
            TestPackages::Workspace
        } else if !config_packages.is_empty() {
            TestPackages::Named(config_packages.to_vec())
        } else {
            TestPackages::Mutated
        }
    }
}

/// Which test programs a run builds and which mutants each judges.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Every program judges every mutant, as a whole-workspace run does.
    workspace: bool,
    /// For each package whose tests judge, the mutants they judge.
    judges: BTreeMap<String, Arc<BTreeSet<MutantId>>>,
    /// The packages the planned mutants are in.
    mutated: BTreeSet<String>,
    /// A mutant is in no package: nothing can be scoped to packages.
    unplaced: bool,
}

impl Scope {
    /// The scope of `plans` (their files are relative to `root`) in the
    /// workspace of `packages`. A mutant in no package (its file is
    /// outside every package's directory) cannot be routed: the whole
    /// workspace judges then.
    pub fn of(
        tests: &TestPackages,
        plans: &[MutantPlan],
        packages: &[Package],
        root: &Path,
    ) -> Scope {
        let mut judges: BTreeMap<String, BTreeSet<MutantId>> = BTreeMap::new();
        let mut mutated = BTreeSet::new();
        let mut unplaced = false;
        for plan in plans {
            let Some(package) = package_of(packages, &root.join(&plan.file)) else {
                unplaced = true;
                continue;
            };
            mutated.insert(package.to_string());
            let judging: Vec<&str> = match tests {
                TestPackages::Mutated => vec![package],
                TestPackages::Named(named) => named.iter().map(String::as_str).collect(),
                TestPackages::Workspace => Vec::new(),
            };
            for judge in judging {
                judges
                    .entry(judge.to_string())
                    .or_default()
                    .insert(plan.id.clone());
            }
        }
        Scope {
            workspace: unplaced || *tests == TestPackages::Workspace,
            unplaced,
            judges: judges.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            mutated,
        }
    }

    /// The packages whose test programs are built: `None` for the whole
    /// workspace's.
    pub fn build(&self) -> Option<Vec<String>> {
        (!self.workspace).then(|| self.judges.keys().cloned().collect())
    }

    /// Whether every test program judges every mutant.
    pub fn all_judge_all(&self) -> bool {
        self.workspace
    }

    /// The mutants `package`'s tests judge.
    pub fn judged_by(&self, package: &str) -> Arc<BTreeSet<MutantId>> {
        self.judges.get(package).cloned().unwrap_or_default()
    }

    /// The packages the planned mutants are in, which the viability check
    /// compiles.
    /// None when a mutant is in no package: the check is of the workspace.
    pub fn mutated(&self) -> Vec<String> {
        if self.unplaced {
            return Vec::new();
        }
        self.mutated.iter().cloned().collect()
    }
}

#[cfg(test)]
#[path = "test_scope.test.rs"]
mod tests;
