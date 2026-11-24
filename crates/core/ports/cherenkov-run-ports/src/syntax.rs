//! The syntax port: what a parser adapter hands the core.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::syntax::{FileSyntax, LanguageId};
use coded_diagnostic::{Arg, Diagnostic, code};

/// Why a file could not be analyzed: coded, with named data (the messages
/// are `locales/en-US/messages.ftl`'s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxError {
    /// No profile is loaded for the language.
    UnknownLanguage { lang: LanguageId },
    /// The parser failed; `detail` is what it said.
    Parser { detail: String },
    /// The parse did not complete (the parser gave up, or timed out).
    Incomplete,
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for SyntaxError {}

impl Diagnostic for SyntaxError {
    fn code(&self) -> &str {
        match self {
            SyntaxError::UnknownLanguage { .. } => code!("unknown-language"),
            SyntaxError::Parser { .. } => code!("parser"),
            SyntaxError::Incomplete => code!("incomplete"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            SyntaxError::UnknownLanguage { lang } => vec![("lang", lang.as_str().into())],
            SyntaxError::Parser { detail } => vec![("detail", detail.as_str().into())],
            SyntaxError::Incomplete => Vec::new(),
        }
    }
}

/// This crate's en-US messages, its name first.
pub const EN_US: (&str, &str) = (
    env!("CARGO_PKG_NAME"),
    include_str!("../locales/en-US/messages.ftl"),
);

/// One value of every error, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(SyntaxError::UnknownLanguage {
            lang: LanguageId::new("cobol"),
        }),
        Box::new(SyntaxError::Parser { detail: "d".into() }),
        Box::new(SyntaxError::Incomplete),
    ]
}

pub trait SyntaxPort {
    fn analyze(
        &self,
        lang: &LanguageId,
        rel: &Path,
        source: Arc<str>,
    ) -> Result<FileSyntax, SyntaxError>;
}

#[cfg(test)]
#[path = "syntax.test.rs"]
mod tests;
