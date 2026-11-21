//! Asking for operator sets: a comma-separated list of registered set names
//! and `.toml` files that define a set of one's own (the caller reads and
//! parses the file; this crate does no I/O).
//!
//! ```toml
//! name = "mine"                     # shown in provenance
//! summary = "what it is for"        # optional
//! source = "where it comes from"    # optional
//! mutations = ["ror/lt->le"]        # names from `cherenkov mutations`
//! skips = ["test", "annotated"]     # optional region kinds to plan nothing in
//! ```

use serde::Deserialize;

use crate::operator_set::{OperatorSet, mutation_named, named};
use crate::syntax::SkipKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    /// No registered set has this name.
    Unknown(String),
    /// The custom set's file could not be read.
    Unreadable { path: String, reason: String },
    /// The custom set in this file is not a set.
    Invalid { path: String, reason: String },
    /// The custom set names a mutation `cherenkov mutations` does not list.
    UnknownMutation(String),
    /// The custom set names a skip kind that does not exist.
    UnknownSkip(String),
}

impl SetError {
    /// What is wrong with a custom set.
    pub fn reason(&self) -> String {
        match self {
            SetError::UnknownMutation(m) => format!("no mutation named {m}"),
            SetError::UnknownSkip(k) => format!("no skip kind named {k}"),
            SetError::Invalid { reason, .. } | SetError::Unreadable { reason, .. } => {
                reason.clone()
            }
            SetError::Unknown(name) => format!("no operator set named {name}"),
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
        let custom = load(item).map_err(|reason| SetError::Unreadable {
            path: item.to_string(),
            reason,
        })?;
        return custom_set(custom).map_err(|e| SetError::Invalid {
            path: item.to_string(),
            reason: e.reason(),
        });
    }
    named(item).ok_or_else(|| SetError::Unknown(item.to_string()))
}

/// The set a file defines. Its strings live as long as the process: a set
/// is made once per run.
pub fn custom_set(c: CustomSet) -> Result<OperatorSet, SetError> {
    let mutations = c
        .mutations
        .iter()
        .map(|m| {
            mutation_named(m)
                .map(|found| found.name)
                .ok_or_else(|| SetError::UnknownMutation(m.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let skips = c
        .skips
        .iter()
        .map(|s| SkipKind::from_name(s).ok_or_else(|| SetError::UnknownSkip(s.clone())))
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

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}
