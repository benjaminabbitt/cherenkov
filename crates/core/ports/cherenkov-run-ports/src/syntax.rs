//! The syntax port: what a parser adapter hands the core.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::syntax::{FileSyntax, LanguageId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    UnknownLanguage(LanguageId),
    Parser(String),
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyntaxError::UnknownLanguage(l) => write!(f, "no profile for language {l}"),
            SyntaxError::Parser(m) => write!(f, "parser failure: {m}"),
        }
    }
}

impl std::error::Error for SyntaxError {}

pub trait SyntaxPort {
    fn analyze(
        &self,
        lang: &LanguageId,
        rel: &Path,
        source: Arc<str>,
    ) -> Result<FileSyntax, SyntaxError>;
}
