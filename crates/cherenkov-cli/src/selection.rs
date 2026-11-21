//! What is planned and which of it is kept, as cargo-mutants decides: test
//! code is left alone unless asked for (in a file, by the sets' region
//! skips; a file by the modules that lead to it), and the planned mutants
//! can be picked by file and by cargo-mutants' mutant name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cherenkov_core::operator_set::OperatorSet;
use cherenkov_core::ports::SourceReader;
use cherenkov_core::profile::Profile;
use cherenkov_core::session::{ProjectPlan, plan_project};
use cherenkov_core::syntax::LanguageId;
use cherenkov_fs::FsSourceReader;
use cherenkov_syntax::TreeSitterSyntax;
use cherenkov_syntax::rust_modules::module_files;

use crate::error::CliError;

/// Which mutants are planned.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Mutate test code too (cargo-mutants never does).
    pub include_tests: bool,
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
    Ok(project)
}

/// The Rust files only test code reaches: those a `#[cfg(test)]` (or
/// `#[mutants::skip]`) module declares, and the modules of those files.
fn test_files(reader: &dyn SourceReader) -> std::io::Result<BTreeSet<PathBuf>> {
    let files = match reader.list(&LanguageId::new("rust")) {
        Ok(files) => files,
        Err(_) => return Ok(BTreeSet::new()),
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
