//! Asking for operator sets: a comma-separated list of registered set names
//! and `.toml` files that define a set of one's own (the caller reads and
//! parses the file; this crate does no I/O).
//!
//! ```toml
//! name = "mine"                     # shown in provenance
//! summary = "what it is for"        # optional
//! source = "where it comes from"    # optional
//! mutations = ["ror/lt->le", "lcr"] # names from `cherenkov operators`: a
//!                                   # replacement, or an operator's whole table
//! skips = ["test", "annotated"]     # optional region kinds to plan nothing in
//! ```

use angzarr_diagnostic::{Arg, Diagnostic, Report, code};
use serde::Deserialize;

use crate::inventory::canonical_names;
use crate::operator_set::{OperatorSet, mutation_named, mutations, named};
use crate::syntax::SkipKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    /// No registered set has this name.
    Unknown { name: String },
    /// The custom set's file could not be read (`detail` is what the reader
    /// said).
    Unreadable { path: String, detail: String },
    /// The custom set in this file is not a set: `cause` says why.
    Invalid { path: String, cause: Report },
    /// The custom set names a mutation `cherenkov mutations` does not list.
    UnknownMutation { name: String },
    /// The custom set names another tool's name for mutations: the canonical
    /// names it stands for, comma-separated.
    Alias { alias: String, canonical: String },
    /// The custom set names a skip kind that does not exist.
    UnknownSkip { name: String },
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for SetError {}

impl Diagnostic for SetError {
    fn code(&self) -> &str {
        match self {
            SetError::Unknown { .. } => code!("unknown-set"),
            SetError::Unreadable { .. } => code!("unreadable-set"),
            SetError::Invalid { .. } => code!("invalid-set"),
            SetError::UnknownMutation { .. } => code!("unknown-mutation"),
            SetError::Alias { .. } => code!("mutation-alias"),
            SetError::UnknownSkip { .. } => code!("unknown-skip"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            SetError::Unknown { name }
            | SetError::UnknownMutation { name }
            | SetError::UnknownSkip { name } => vec![("name", name.as_str().into())],
            SetError::Unreadable { path, detail } => vec![
                ("path", path.as_str().into()),
                ("detail", detail.as_str().into()),
            ],
            SetError::Invalid { path, cause } => {
                vec![
                    ("path", path.as_str().into()),
                    ("cause", Arg::nested(cause)),
                ]
            }
            SetError::Alias { alias, canonical } => vec![
                ("alias", alias.as_str().into()),
                ("canonical", canonical.as_str().into()),
            ],
        }
    }
}

/// A set as a file defines it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomSet {
    pub name: String,
    pub summary: Option<String>,
    pub source: Option<String>,
    pub mutations: Vec<String>,
    #[serde(default)]
    pub skips: Vec<String>,
}

/// The sets `spec` asks for, in order, each once. An item ending in `.toml`
/// is read through `load` (path to the set, or the reason it failed); any other
/// is a registered set's name.
pub fn resolve_sets(
    spec: &str,
    load: impl Fn(&str) -> Result<CustomSet, String>,
) -> Result<Vec<OperatorSet>, SetError> {
    let mut sets: Vec<OperatorSet> = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let set = one_set(item, &load)?;
        if !sets.iter().any(|s| s.name == set.name) {
            sets.push(set);
        }
    }
    Ok(sets)
}

fn one_set(
    item: &str,
    load: &impl Fn(&str) -> Result<CustomSet, String>,
) -> Result<OperatorSet, SetError> {
    if item.ends_with(".toml") {
        let custom = load(item).map_err(|detail| SetError::Unreadable {
            path: item.to_string(),
            detail,
        })?;
        return custom_set(custom).map_err(|e| SetError::Invalid {
            path: item.to_string(),
            cause: Report::of(&e),
        });
    }
    named(item).ok_or_else(|| SetError::Unknown {
        name: item.to_string(),
    })
}

/// The set a file defines. Its strings live as long as the process: a set
/// is made once per run.
pub fn custom_set(c: CustomSet) -> Result<OperatorSet, SetError> {
    let mut mutations: Vec<&'static str> = Vec::new();
    for m in &c.mutations {
        for name in expand(m)? {
            if !mutations.contains(&name) {
                mutations.push(name);
            }
        }
    }
    let skips = c
        .skips
        .iter()
        .map(|s| SkipKind::from_name(s).ok_or_else(|| SetError::UnknownSkip { name: s.clone() }))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OperatorSet {
        name: leak(c.name),
        summary: leak(c.summary.unwrap_or_else(|| "a custom set".to_string())),
        source: leak(c.source.unwrap_or_else(|| "a custom set file".to_string())),
        mutations: Box::leak(mutations.into_boxed_slice()),
        pending: &[],
        skips: Box::leak(skips.into_boxed_slice()),
    })
}

/// The mutations a set file's entry names: a replacement's name is itself,
/// an operator's (family) name is its whole table.
fn expand(entry: &str) -> Result<Vec<&'static str>, SetError> {
    if let Some(found) = mutation_named(entry) {
        return Ok(vec![found.name]);
    }
    let table: Vec<&'static str> = mutations()
        .iter()
        .filter(|m| m.family == entry)
        .map(|m| m.name)
        .collect();
    if !table.is_empty() {
        return Ok(table);
    }
    let canonical = canonical_names(entry);
    if canonical.is_empty() {
        Err(SetError::UnknownMutation {
            name: entry.to_string(),
        })
    } else {
        Err(SetError::Alias {
            alias: entry.to_string(),
            canonical: canonical.join(", "),
        })
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}
