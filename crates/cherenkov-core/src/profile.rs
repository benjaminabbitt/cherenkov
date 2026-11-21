//! A language profile as the core sees it: data only (design section 4.3).

use std::collections::{BTreeMap, BTreeSet};

use crate::concept::Concept;
use crate::syntax::LanguageId;
use crate::token::TokenTable;

/// Which files belong to the language. Globs are matched by the source
/// adapter against paths relative to the project root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileRules {
    pub extensions: Vec<String>,
    pub exclude: Vec<String>,
}

/// A language's generator of function-body replacement values: the result
/// type's source text (`None`: no result type) and the configured error
/// expressions, to the values in order. A function pointer, so profiles stay
/// data and each language supplies its own rule.
#[derive(Clone, Copy)]
pub struct ReturnValues(pub fn(Option<&str>, &[&str]) -> Vec<String>);

impl std::fmt::Debug for ReturnValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReturnValues(..)")
    }
}

impl PartialEq for ReturnValues {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.0, other.0)
    }
}

impl Eq for ReturnValues {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub lang: LanguageId,
    pub tokens: TokenTable,
    /// Concepts the profile's queries capture; the rest are reported unmapped.
    pub mapped: BTreeSet<Concept>,
    /// Operator ids this language disables (design section 4.7).
    pub disabled_operators: BTreeSet<String>,
    /// Return-type text to the default literal a body replacement returns.
    pub defaults: BTreeMap<String, String>,
    /// Return-type texts meaning "returns nothing" (no body replacement).
    pub unit_types: BTreeSet<String>,
    /// Source of a function body's contents that return a value; `{}` marks
    /// the value (Rust `{}`, Go `return {}`, Python `return {}`).
    pub body_template: String,
    /// Bodies are delimited by `{`/`}`; their contents lie between.
    pub body_braces: bool,
    /// Comment syntax, so a body's contents start at its first code.
    pub comments: Comments,
    /// Body replacement values by result type, when the language has a rule.
    pub return_values: Option<ReturnValues>,
    pub files: FileRules,
}

/// A language's comment syntax.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comments {
    /// Starts a comment that runs to the end of the line (`//`, `#`).
    pub line: Option<String>,
    /// Opens and closes a block comment (`/*`, `*/`).
    pub block: Option<(String, String)>,
    /// Comment openers that document the item after them (`///`, `/**`):
    /// the language reads them as part of that item, so they are code. One
    /// repeating its last character (`////`) is a plain comment.
    pub doc: Vec<String>,
}

impl Comments {
    /// Whether `text` starts with a documentation comment.
    fn documents(&self, text: &str) -> bool {
        self.doc.iter().any(|d| {
            text.starts_with(d.as_str()) && text[d.len()..].chars().next() != d.chars().last()
        })
    }

    /// Bytes of whitespace and comments at the start of `text`.
    pub fn leading(&self, text: &str) -> usize {
        let mut i = 0;
        loop {
            let rest = &text[i..];
            let trimmed = rest.trim_start();
            i += rest.len() - trimmed.len();
            if self.documents(trimmed) {
                return i;
            }
            if self.line.as_deref().is_some_and(|l| trimmed.starts_with(l)) {
                i += trimmed.find('\n').unwrap_or(trimmed.len());
            } else if let Some((open, close)) = self
                .block
                .as_ref()
                .filter(|(open, _)| trimmed.starts_with(open.as_str()))
            {
                match trimmed[open.len()..].find(close.as_str()) {
                    Some(end) => i += open.len() + end + close.len(),
                    None => return text.len(),
                }
            } else {
                return i;
            }
        }
    }

    /// Bytes of whitespace and comments at the end of `text`: everything
    /// after its last code. Found scanning forward, since a comment marker
    /// inside a string (`"http://x"`) is not a comment; strings are
    /// double-quoted, with backslash escapes.
    pub fn trailing(&self, text: &str) -> usize {
        let bytes = text.as_bytes();
        let mut code_end = 0;
        let mut i = 0;
        while i < bytes.len() {
            let rest = &text[i..];
            if self.line.as_deref().is_some_and(|l| rest.starts_with(l)) {
                i += rest.find('\n').unwrap_or(rest.len());
                continue;
            }
            if let Some((open, close)) = self
                .block
                .as_ref()
                .filter(|(open, _)| rest.starts_with(open.as_str()))
            {
                i += match rest[open.len()..].find(close.as_str()) {
                    Some(end) => open.len() + end + close.len(),
                    None => rest.len(),
                };
                continue;
            }
            if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(bytes.len());
                code_end = i;
                continue;
            }
            let c = rest.chars().next().unwrap_or(' ');
            i += c.len_utf8();
            if !c.is_whitespace() {
                code_end = i;
            }
        }
        text.len() - code_end
    }
}
