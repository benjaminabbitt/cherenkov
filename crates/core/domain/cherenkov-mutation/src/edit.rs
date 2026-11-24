//! Abstract edits emitted by operators (design section 4.4).

use serde::Serialize;

use crate::concept::Capture;
use crate::token::Token;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lit {
    Bool(bool),
    /// The default value of the function's result type.
    Default,
    /// A value given as source text (a function body replacement).
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Edit {
    /// `from` is the original token, so a backend can map the edit onto
    /// machine code (tier P) without re-reading the source.
    ReplaceToken {
        capture: Capture,
        from: Token,
        with: Token,
    },
    /// Delete an operator token, leaving its operand: `!x` -> `x`.
    DeleteToken {
        capture: Capture,
        token: Token,
    },
    WrapNot {
        capture: Capture,
    },
    ReplaceWithLiteral {
        capture: Capture,
        lit: Lit,
    },
    DeleteStatement,
    /// Delete the whole concept match (an arm, a field) with the separator
    /// that follows it.
    DeleteMatch,
}

impl Edit {
    /// Stable, language-neutral description. Part of the mutant ID.
    pub fn describe(&self) -> String {
        match self {
            Edit::ReplaceToken {
                capture,
                from,
                with,
            } => format!(
                "replace-token {} {} -> {}",
                capture.name(),
                from.name(),
                with.name()
            ),
            Edit::DeleteToken { capture, token } => {
                format!("delete-token {} {}", capture.name(), token.name())
            }
            Edit::WrapNot { capture } => format!("wrap-not {}", capture.name()),
            Edit::ReplaceWithLiteral { capture, lit } => {
                let lit = match lit {
                    Lit::Bool(b) => b.to_string(),
                    Lit::Default => "default".to_string(),
                    Lit::Text(t) => format!("{t:?}"),
                };
                format!("replace-with-literal {} -> {lit}", capture.name())
            }
            Edit::DeleteStatement => "delete-statement".to_string(),
            Edit::DeleteMatch => "delete-match".to_string(),
        }
    }
}
