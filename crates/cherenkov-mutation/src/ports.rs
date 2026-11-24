//! Port traits. The core defines them; adapters implement them.
//!
//! Only the ports M0 exercises are defined here. Test catalog, coverage,
//! executor and report ports arrive with the milestones that use them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::syntax::{FileSyntax, LanguageId};

/// Read-only by construction: no port in the core writes source.
pub trait SourceReader {
    /// Source files of `lang` under the project root, relative to it, sorted.
    fn list(&self, lang: &LanguageId) -> io::Result<Vec<PathBuf>>;
    /// Contents of a file given relative to the project root.
    fn read(&self, rel: &Path) -> io::Result<Arc<str>>;
}

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
