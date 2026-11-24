//! The planning use case with fake ports: every file is either planned or
//! reported in `problems`, never silently dropped.

#[path = "../../../domain/cherenkov-mutation/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{FileProblem, plan_project};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::profile::Profile;
use cherenkov_mutation::syntax::{FileSyntax, LanguageId};
use cherenkov_run_ports::source::SourceReader;
use angzarr_diagnostic::Report;
use cherenkov_run_ports::syntax::{SyntaxError, SyntaxPort};
use common::{TOY_SRC, toy_file, toy_profile};

const SYNTAX_ERROR: &str = "<syntax error>";
const ANALYSIS_FAILS: &str = "<analysis fails>";

/// Files per language; a `None` body is unreadable.
#[derive(Default)]
struct FakeReader {
    files: BTreeMap<String, Vec<(&'static str, Option<&'static str>)>>,
    listing_fails: bool,
}

impl SourceReader for FakeReader {
    fn list(&self, lang: &LanguageId) -> io::Result<Vec<PathBuf>> {
        if self.listing_fails {
            return Err(io::Error::other("listing failed"));
        }
        Ok(self
            .files
            .get(lang.as_str())
            .into_iter()
            .flatten()
            .map(|(p, _)| PathBuf::from(p))
            .collect())
    }

    fn read(&self, rel: &Path) -> io::Result<Arc<str>> {
        let body = self
            .files
            .values()
            .flatten()
            .find(|(p, _)| Path::new(p) == rel)
            .and_then(|(_, b)| *b);
        body.map(Arc::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}

/// Analyzes TOY_SRC into the toy file; markers produce errors.
struct FakeSyntax;

impl SyntaxPort for FakeSyntax {
    fn analyze(
        &self,
        lang: &LanguageId,
        rel: &Path,
        source: Arc<str>,
    ) -> Result<FileSyntax, SyntaxError> {
        if &*source == ANALYSIS_FAILS {
            return Err(SyntaxError::Parser {
                detail: "boom".into(),
            });
        }
        let mut file = toy_file(rel.to_str().unwrap(), TOY_SRC);
        file.lang = lang.clone();
        file.has_errors = &*source == SYNTAX_ERROR;
        Ok(file)
    }
}

fn profiles() -> Vec<Profile> {
    let mut other = toy_profile();
    other.lang = LanguageId::new("toy2");
    vec![toy_profile(), other]
}

fn reader(files: &[(&str, &'static str, Option<&'static str>)]) -> FakeReader {
    let mut r = FakeReader::default();
    for &(lang, path, body) in files {
        r.files
            .entry(lang.to_string())
            .or_default()
            .push((path, body));
    }
    r
}

const TOY_PLANS: usize = 13;

#[test]
fn test_plan_project_plans_every_file_of_every_language_sorted_by_path() {
    let r = reader(&[
        ("toy2", "b.src", Some(TOY_SRC)),
        ("toy", "c.src", Some(TOY_SRC)),
        ("toy", "a.src", Some(TOY_SRC)),
    ]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    assert_eq!(out.plans.len(), 3 * TOY_PLANS);
    let files: Vec<&Path> = out.plans.iter().map(|p| p.file.as_path()).collect();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
    assert!(out.problems.is_empty());
}

#[test]
fn test_plan_project_reports_unreadable_file_and_plans_the_rest() {
    let r = reader(&[("toy", "a.src", Some(TOY_SRC)), ("toy", "locked.src", None)]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    assert_eq!(out.plans.len(), TOY_PLANS);
    assert_eq!(out.problems.len(), 1);
    assert_eq!(out.problems[0].0, PathBuf::from("locked.src"));
    assert!(matches!(&out.problems[0].1, FileProblem::Unreadable { detail } if detail.contains("denied")));
}

#[test]
fn test_plan_project_reports_analysis_failure_and_plans_the_rest() {
    let r = reader(&[
        ("toy", "a.src", Some(TOY_SRC)),
        ("toy", "bad.src", Some(ANALYSIS_FAILS)),
    ]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    assert_eq!(out.plans.len(), TOY_PLANS);
    assert_eq!(
        out.problems,
        [(
            PathBuf::from("bad.src"),
            FileProblem::Analysis {
                cause: Report::of(&SyntaxError::Parser {
                    detail: "boom".into()
                })
            }
        )]
    );
}

#[test]
fn test_plan_project_reports_file_with_syntax_errors_and_does_not_plan_it() {
    let r = reader(&[("toy", "broken.src", Some(SYNTAX_ERROR))]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    assert!(out.plans.is_empty());
    assert_eq!(
        out.problems,
        [(PathBuf::from("broken.src"), FileProblem::SyntaxErrors)]
    );
}

#[test]
fn test_plan_project_listing_failure_aborts() {
    let r = FakeReader {
        listing_fails: true,
        ..FakeReader::default()
    };
    assert!(plan_project(&r, &FakeSyntax, &profiles(), &catalog()).is_err());
}

#[test]
fn test_plan_project_merges_diagnostics_per_language() {
    let r = reader(&[
        ("toy", "a.src", Some(TOY_SRC)),
        ("toy", "b.src", Some(TOY_SRC)),
        ("toy2", "c.src", Some(TOY_SRC)),
    ]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    let toy = &out.diagnostics[&LanguageId::new("toy")];
    let toy2 = &out.diagnostics[&LanguageId::new("toy2")];
    assert_eq!(
        toy.concept_counts[&cherenkov_mutation::concept::Concept::Binary],
        10
    );
    assert_eq!(
        toy2.concept_counts[&cherenkov_mutation::concept::Concept::Binary],
        5
    );
}

#[test]
fn test_plan_project_language_without_files_has_no_diagnostics_entry() {
    let r = reader(&[("toy", "a.src", Some(TOY_SRC))]);
    let out = plan_project(&r, &FakeSyntax, &profiles(), &catalog()).unwrap();
    assert_eq!(
        out.diagnostics
            .keys()
            .map(|l| l.as_str())
            .collect::<Vec<_>>(),
        ["toy"]
    );
}

#[test]
fn test_plan_project_sets_merges_the_sets_per_file_and_tags_each_plan() {
    use super::plan_project_sets;
    use cherenkov_mutation::operator_set::named;
    let r = reader(&[
        ("toy", "a.src", Some(TOY_SRC)),
        ("toy", "b.src", Some(TOY_SRC)),
    ]);
    let sets = [named("breadth").unwrap(), named("sufficient").unwrap()];
    let out = plan_project_sets(&r, &FakeSyntax, &profiles(), &sets, true).unwrap();
    let alone = |name: &str| {
        let set = [named(name).unwrap()];
        plan_project_sets(&r, &FakeSyntax, &profiles(), &set, true)
            .unwrap()
            .plans
            .len()
    };
    assert!(
        out.plans.len() > alone("sufficient"),
        "the sets add to each other"
    );
    assert!(
        out.plans.len() < alone("sufficient") + alone("breadth"),
        "and overlap"
    );
    assert_eq!(
        out.diagnostics[&LanguageId::new("toy")].planned,
        out.plans.len()
    );
    assert!(
        out.provenance
            .values()
            .any(|s| s == &["breadth", "sufficient"])
    );
    assert!(out.plans.iter().all(|p| !out.provenance[&p.id].is_empty()));
}
