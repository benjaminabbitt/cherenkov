//! `SyntaxPort` over tree-sitter. Languages are data: a `concepts.scm`
//! query, a `tokens.toml` table and a `profile.toml`, embedded at build time.
//!
//! Query capture names form the contract with the core:
//! `@concept.<concept>` marks a concept node, `@stmt.<kind>` a statement,
//! and bare role names (`@op`, `@cond`, ...) its role captures.

pub mod profiles;
pub mod rust_chains;
pub mod rust_modules;
pub mod rust_names;
pub mod rust_scope;
pub mod rust_tests;
pub mod rust_types;
pub mod rust_values;

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::ports::{SyntaxError, SyntaxPort};
use cherenkov_mutation::profile::Profile;
use cherenkov_mutation::syntax::{
    ConceptMatch, FileSyntax, LanguageId, Pos, SkipKind, SkipRegion, Span, Statement, StmtKind,
};
use std::collections::{BTreeMap, BTreeSet};
use streaming_iterator::StreamingIterator;

use profiles::{CaptureRole, capture_role};

pub use profiles::{LoadedProfile, ProfileError, ProfileSource};

/// A concept node: (start byte, concept, end byte), ordered by position.
type NodeKey = (usize, Concept, usize);
/// The node's span and every distinct role capture across its query matches.
type FoldedMatch = (Span, Vec<(Capture, Span)>);

pub struct TreeSitterSyntax {
    profiles: Vec<LoadedProfile>,
    /// Calls to functions and methods with these names (their last path
    /// segment) are skipped, arguments and all.
    skip_calls: Vec<String>,
}

/// The calls cargo-mutants skips unless told otherwise.
pub const DEFAULT_SKIP_CALLS: [&str; 1] = ["with_capacity"];

impl TreeSitterSyntax {
    pub fn new(profiles: Vec<LoadedProfile>) -> Self {
        TreeSitterSyntax {
            profiles,
            skip_calls: DEFAULT_SKIP_CALLS.map(String::from).to_vec(),
        }
    }

    /// Skip calls named in `names` instead of the default list.
    pub fn with_skip_calls(mut self, names: Vec<String>) -> Self {
        self.skip_calls = names;
        self
    }

    /// Python, Rust and Go, from the profiles embedded in this crate.
    pub fn builtin() -> Result<Self, ProfileError> {
        Ok(TreeSitterSyntax::new(profiles::builtin()?))
    }

    /// The core's view of every loaded profile.
    pub fn profiles(&self) -> Vec<Profile> {
        self.profiles.iter().map(|p| p.profile.clone()).collect()
    }
}

impl SyntaxPort for TreeSitterSyntax {
    fn analyze(
        &self,
        lang: &LanguageId,
        rel: &Path,
        source: Arc<str>,
    ) -> Result<FileSyntax, SyntaxError> {
        let loaded = self
            .profiles
            .iter()
            .find(|p| &p.profile.lang == lang)
            .ok_or_else(|| SyntaxError::UnknownLanguage(lang.clone()))?;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&loaded.grammar)
            .map_err(|e| SyntaxError::Parser(e.to_string()))?;
        let tree = parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| SyntaxError::Parser("parse did not complete".into()))?;

        let names = loaded.query.capture_names();
        // Repeated grammar fields yield one query match per occurrence; fold
        // them into one concept match per (concept, node).
        let mut concepts: BTreeMap<NodeKey, FoldedMatch> = BTreeMap::new();
        let mut statements: BTreeSet<(usize, usize, StmtKind, Span)> = BTreeSet::new();
        let mut skips: BTreeSet<(usize, usize, SkipKind, Span)> = BTreeSet::new();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&loaded.query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            let mut node_concepts = Vec::new();
            let mut roles = Vec::new();
            for cap in m.captures() {
                let span = span_of(&cap.node);
                match capture_role(names[cap.index as usize]) {
                    Some(CaptureRole::Concept(c)) => node_concepts.push((c, span)),
                    Some(CaptureRole::Statement(kind)) => {
                        statements.insert((span.start.byte, span.end.byte, kind, span));
                    }
                    Some(CaptureRole::Role(r)) => roles.push((r, span)),
                    Some(CaptureRole::Skip(kind)) => {
                        if kind != SkipKind::CapacityCall
                            || calls_skipped(&cap.node, &source, &self.skip_calls)
                        {
                            skips.insert((span.start.byte, span.end.byte, kind, span));
                        }
                    }
                    Some(CaptureRole::Helper) => {}
                    // Names were validated when the profile was loaded.
                    None => {}
                }
            }
            for (concept, span) in node_concepts {
                let (_, caps) = concepts
                    .entry((span.start.byte, concept, span.end.byte))
                    .or_insert_with(|| (span, Vec::new()));
                for r in &roles {
                    if !caps.contains(r) {
                        caps.push(*r);
                    }
                }
            }
        }

        let mut extra = match loaded.extra_matches {
            Some(find) => find(tree.root_node()),
            None => Vec::new(),
        };
        let mut matches: Vec<ConceptMatch> = concepts
            .into_iter()
            .map(|((_, concept, _), (span, mut caps))| {
                caps.sort_by_key(|(c, s)| (s.start.byte, *c));
                ConceptMatch {
                    concept,
                    span,
                    captures: caps,
                }
            })
            .chain(extra.drain(..))
            .collect();
        matches.sort_by_key(|m| (m.span.start.byte, m.concept, m.span.end.byte));
        let statements = statements
            .into_iter()
            .map(|(_, _, kind, span)| Statement { kind, span })
            .collect();

        Ok(FileSyntax {
            lang: lang.clone(),
            path: rel.to_path_buf(),
            has_errors: tree.root_node().has_error(),
            source,
            statements,
            matches,
            skips: skips
                .into_iter()
                .map(|(_, _, kind, span)| SkipRegion { kind, span })
                .collect(),
        })
    }
}

/// Whether the call expression `node` calls a function or method named in
/// `names`: by the last segment of its path, as cargo-mutants compares it.
fn calls_skipped(node: &tree_sitter::Node<'_>, source: &str, names: &[String]) -> bool {
    let mut callee = node.child_by_field_name("function");
    while let Some(c) = callee.filter(|c| c.kind() == "generic_function") {
        callee = c.child_by_field_name("function");
    }
    let name = callee.and_then(|c| match c.kind() {
        "identifier" => Some(c),
        "scoped_identifier" => c.child_by_field_name("name"),
        "field_expression" => c.child_by_field_name("field"),
        _ => None,
    });
    name.is_some_and(|n| names.iter().any(|x| x == &source[n.byte_range()]))
}

/// The span from the start of `first` to the end of `last`.
fn span_between(first: &tree_sitter::Node<'_>, last: &tree_sitter::Node<'_>) -> Span {
    Span {
        start: span_of(first).start,
        end: span_of(last).end,
    }
}

fn span_of(node: &tree_sitter::Node<'_>) -> Span {
    let pos = |byte: usize, p: tree_sitter::Point| Pos {
        byte,
        line: p.row as u32 + 1,
        col: p.column as u32 + 1,
    };
    Span {
        start: pos(node.start_byte(), node.start_position()),
        end: pos(node.end_byte(), node.end_position()),
    }
}
