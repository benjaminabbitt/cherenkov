//! `SyntaxPort` over tree-sitter. Languages are data: a `concepts.scm`
//! query, a `tokens.toml` table and a `profile.toml`, embedded at build time.
//!
//! Query capture names form the contract with the core:
//! `@concept.<concept>` marks a concept node, `@stmt.<kind>` a statement,
//! and bare role names (`@op`, `@cond`, ...) its role captures.

pub mod profiles;
pub mod rust_modules;
pub mod rust_scope;
pub mod rust_types;
pub mod rust_values;

use std::path::Path;
use std::sync::Arc;

use cherenkov_core::concept::{Capture, Concept};
use cherenkov_core::ports::{SyntaxError, SyntaxPort};
use cherenkov_core::profile::Profile;
use cherenkov_core::syntax::{
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
}

impl TreeSitterSyntax {
    pub fn new(profiles: Vec<LoadedProfile>) -> Self {
        TreeSitterSyntax { profiles }
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
                        skips.insert((span.start.byte, span.end.byte, kind, span));
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

        let matches = concepts
            .into_iter()
            .map(|((_, concept, _), (span, mut caps))| {
                caps.sort_by_key(|(c, s)| (s.start.byte, *c));
                ConceptMatch {
                    concept,
                    span,
                    captures: caps,
                }
            })
            .collect();
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
