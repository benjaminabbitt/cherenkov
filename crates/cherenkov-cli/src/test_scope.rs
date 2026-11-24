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

use cherenkov_exec_dap::toolchain::{Package, package_of};
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::plan::MutantPlan;

use crate::changed_tests::ChangedTest;

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

/// `package` and the workspace packages it depends on, directly or not.
pub fn reach(packages: &[Package], package: &str) -> BTreeSet<String> {
    let mut reached = BTreeSet::new();
    let mut todo = vec![package.to_string()];
    while let Some(next) = todo.pop() {
        if !reached.insert(next.clone()) {
            continue;
        }
        let deps = packages.iter().find(|p| p.name == next).map(|p| &p.deps);
        todo.extend(deps.into_iter().flatten().cloned());
    }
    reached
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
    /// For each package with changed tests, the mutants only those tests
    /// judge, and the tests.
    only_changed: BTreeMap<String, Arc<BTreeSet<MutantId>>>,
    changed: BTreeMap<String, Vec<ChangedTest>>,
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
        let app: BTreeSet<MutantId> = plans.iter().map(|p| p.id.clone()).collect();
        let workspace = unplaced || *tests == TestPackages::Workspace;
        if workspace {
            // Every program judges every mutant.
            for package in packages {
                judges.insert(package.name.clone(), app.clone());
            }
        }
        Scope {
            workspace,
            unplaced,
            judges: judges.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            mutated,
            only_changed: BTreeMap::new(),
            changed: BTreeMap::new(),
        }
    }

    /// The packages whose test programs are built: `None` for the whole
    /// workspace's.
    pub fn build(&self) -> Option<Vec<String>> {
        (!self.workspace).then(|| self.judges.keys().cloned().collect())
    }

    /// `changed` tests, found in the diff, also judge: for each package
    /// that has one, the mutants of every function in the packages it
    /// reaches (itself and its workspace dependencies), among `plans`
    /// (all the mutants of the code). Those the package's program was not judging already are judged by the
    /// changed tests alone (so too a mutant of changed code in another
    /// package).
    pub fn with_changed_tests(
        mut self,
        changed: &[ChangedTest],
        plans: &[MutantPlan],
        packages: &[Package],
        root: &Path,
    ) -> Scope {
        for test in changed {
            if let Some(package) = package_of(packages, &root.join(&test.file)) {
                self.changed
                    .entry(package.to_string())
                    .or_default()
                    .push(test.clone());
            }
        }
        let owners: Vec<String> = self.changed.keys().cloned().collect();
        for owner in owners {
            let reached = reach(packages, &owner);
            let mut judged = self
                .judges
                .get(&owner)
                .map_or_else(BTreeSet::new, |j| (**j).clone());
            let mut only = BTreeSet::new();
            for plan in plans {
                let Some(package) = package_of(packages, &root.join(&plan.file)) else {
                    continue;
                };
                if !reached.contains(package) {
                    continue;
                }
                self.mutated.insert(package.to_string());
                if judged.insert(plan.id.clone()) {
                    only.insert(plan.id.clone());
                }
            }
            self.judges.insert(owner.clone(), Arc::new(judged));
            self.only_changed.insert(owner, Arc::new(only));
        }
        self
    }

    /// The plans to run: `changed_code`'s, and those of `every` plan that a
    /// changed test judges, once each.
    pub fn plans_to_run(
        &self,
        changed_code: Vec<MutantPlan>,
        every: &[MutantPlan],
    ) -> Vec<MutantPlan> {
        let kept: BTreeSet<MutantId> = changed_code.iter().map(|p| p.id.clone()).collect();
        let reached = self.judged_for_changed_tests();
        let mut plans = changed_code;
        plans.extend(
            every
                .iter()
                .filter(|p| !kept.contains(&p.id) && reached.contains(&p.id))
                .cloned(),
        );
        plans
    }

    /// The mutants some changed test judges.
    pub fn judged_for_changed_tests(&self) -> BTreeSet<MutantId> {
        self.only_changed
            .values()
            .flat_map(|o| o.iter().cloned())
            .collect()
    }

    /// The mutants only `package`'s changed tests judge.
    pub fn only_changed(&self, package: &str) -> Arc<BTreeSet<MutantId>> {
        self.only_changed.get(package).cloned().unwrap_or_default()
    }

    /// The tests the diff changes in `package`.
    pub fn changed_in(&self, package: &str) -> Vec<ChangedTest> {
        self.changed.get(package).cloned().unwrap_or_default()
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

/// Whether a test program is baselined and recorded with the changed tests
/// alone: it judges only mutants that the changed tests alone judge, it has
/// changed tests, and the user did not choose the tests (`--test`).
pub fn records_changed_tests_only(
    judged: &BTreeSet<MutantId>,
    only_changed: &BTreeSet<MutantId>,
    changed: &[String],
    user_tests: &[String],
) -> bool {
    !only_changed.is_empty()
        && judged.iter().all(|id| only_changed.contains(id))
        && !changed.is_empty()
        && user_tests.is_empty()
}

#[cfg(test)]
#[path = "test_scope.test.rs"]
pub(crate) mod tests;
