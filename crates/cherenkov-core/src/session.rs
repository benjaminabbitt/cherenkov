//! The planning use case: drives the source and syntax ports over a project.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::operator::Operator;
use crate::plan::{Diagnostics, MutantPlan, plan_file};
use crate::ports::{SourceReader, SyntaxPort};
use crate::profile::Profile;
use crate::syntax::LanguageId;

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
            let planned = plan_file(&file, profile, catalog);
            diagnostics.merge(&planned.diagnostics);
            out.plans.extend(planned.plans);
        }
    }
    // Stable: each file keeps its own plan order.
    out.plans.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}
