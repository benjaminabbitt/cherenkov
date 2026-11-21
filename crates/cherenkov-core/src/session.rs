//! The planning use case: drives the source and syntax ports over a project.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::operator::Operator;
use crate::operator_set::OperatorSet;
use crate::plan::{Diagnostics, MutantPlan, PlanOutput, Provenance, plan_file};
use crate::plan_sets::plan_file_sets;
use crate::ports::{SourceReader, SyntaxPort};
use crate::profile::Profile;
use crate::syntax::{FileSyntax, LanguageId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "message")]
pub enum FileProblem {
    Unreadable(String),
    Analysis(String),
    /// The parser recovered from syntax errors; no mutants are planned.
    SyntaxErrors,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProjectPlan {
    /// All plans, sorted by file then by each file's own plan order.
    pub plans: Vec<MutantPlan>,
    pub diagnostics: BTreeMap<LanguageId, Diagnostics>,
    pub problems: Vec<(PathBuf, FileProblem)>,
    /// Per mutant, the operator sets that produced it (planning with sets).
    pub provenance: Provenance,
}

/// Plan every file of every profiled language. A file that cannot be read
/// or parsed is reported in `problems`, never silently dropped. Errors from
/// listing a language's files abort the run.
pub fn plan_project(
    reader: &dyn SourceReader,
    syntax: &dyn SyntaxPort,
    profiles: &[Profile],
    catalog: &[Box<dyn Operator>],
) -> std::io::Result<ProjectPlan> {
    plan_project_with(reader, syntax, profiles, &|file, profile| {
        plan_file(file, profile, catalog)
    })
}

/// [`plan_project`] with several operator sets at once: one list of mutants,
/// a mutant with the same span and replacement planned once, each tagged with
/// the sets that produced it (`MutantPlan::sets`).
pub fn plan_project_sets(
    reader: &dyn SourceReader,
    syntax: &dyn SyntaxPort,
    profiles: &[Profile],
    sets: &[OperatorSet],
    include_tests: bool,
) -> std::io::Result<ProjectPlan> {
    plan_project_with(reader, syntax, profiles, &|file, profile| {
        plan_file_sets(file, profile, sets, include_tests)
    })
}

fn plan_project_with(
    reader: &dyn SourceReader,
    syntax: &dyn SyntaxPort,
    profiles: &[Profile],
    plan: &dyn Fn(&FileSyntax, &Profile) -> PlanOutput,
) -> std::io::Result<ProjectPlan> {
    let mut out = ProjectPlan::default();
    for profile in profiles {
        let files = reader.list(&profile.lang)?;
        if files.is_empty() {
            continue;
        }
        let diagnostics = out.diagnostics.entry(profile.lang.clone()).or_default();
        for rel in files {
            let source = match reader.read(&rel) {
                Ok(s) => s,
                Err(e) => {
                    out.problems
                        .push((rel, FileProblem::Unreadable(e.to_string())));
                    continue;
                }
            };
            let file = match syntax.analyze(&profile.lang, &rel, source) {
                Ok(f) => f,
                Err(e) => {
                    out.problems
                        .push((rel, FileProblem::Analysis(e.to_string())));
                    continue;
                }
            };
            if file.has_errors {
                out.problems.push((rel, FileProblem::SyntaxErrors));
                continue;
            }
            let planned = plan(&file, profile);
            diagnostics.merge(&planned.diagnostics);
            out.plans.extend(planned.plans);
            out.provenance.extend(planned.provenance);
        }
    }
    // Stable: each file keeps its own plan order.
    out.plans.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}
