//! Loading language profiles from their data files.

use std::collections::{BTreeMap, BTreeSet};

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::profile::{FileRules, Profile, ReturnValues};
use cherenkov_mutation::syntax::{LanguageId, SkipKind, StmtKind};
use cherenkov_mutation::token::{Token, TokenTable};
use angzarr_diagnostic::{Arg, Diagnostic, Report, code};
use serde::Deserialize;

/// The three data files that make up a profile, plus its grammar.
pub struct ProfileSource<'a> {
    pub lang: &'a str,
    pub grammar: tree_sitter::Language,
    pub concepts_scm: &'a str,
    pub tokens_toml: &'a str,
    pub profile_toml: &'a str,
    /// The language's body replacement value rule, if it has one.
    pub return_values: Option<ReturnValues>,
}

/// Matches the language's queries cannot express, found in the parse tree.
pub type ExtraMatches = fn(tree_sitter::Node<'_>) -> Vec<cherenkov_mutation::syntax::ConceptMatch>;

pub struct LoadedProfile {
    pub profile: Profile,
    pub grammar: tree_sitter::Language,
    pub query: tree_sitter::Query,
    pub extra_matches: Option<ExtraMatches>,
}

/// Why a language profile did not load: coded, with named data (the
/// messages are `locales/en-US/messages.ftl`'s). `detail` is what the
/// parser of the file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// `concepts.scm` is not a query of the grammar.
    Query { lang: String, detail: String },
    /// A capture name that is not `concept.<concept>`, `stmt.<kind>` or a role.
    UnknownCapture { lang: String, name: String },
    /// `file` is not TOML of the right shape.
    Toml {
        lang: String,
        file: &'static str,
        detail: String,
    },
    /// `tokens.toml` names a token that does not exist.
    UnknownToken { lang: String, name: String },
    /// `tokens.toml` is not a token table: `cause` says why.
    Tokens { lang: String, cause: Report },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for ProfileError {}

impl Diagnostic for ProfileError {
    fn code(&self) -> &str {
        match self {
            ProfileError::Query { .. } => code!("query"),
            ProfileError::UnknownCapture { .. } => code!("unknown-capture"),
            ProfileError::Toml { .. } => code!("toml"),
            ProfileError::UnknownToken { .. } => code!("unknown-token"),
            ProfileError::Tokens { .. } => code!("tokens"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            ProfileError::Query { lang, detail } => {
                vec![("lang", lang.as_str().into()), ("detail", detail.as_str().into())]
            }
            ProfileError::UnknownCapture { lang, name }
            | ProfileError::UnknownToken { lang, name } => {
                vec![("lang", lang.as_str().into()), ("name", name.as_str().into())]
            }
            ProfileError::Toml { lang, file, detail } => vec![
                ("lang", lang.as_str().into()),
                ("file", (*file).into()),
                ("detail", detail.as_str().into()),
            ],
            ProfileError::Tokens { lang, cause } => {
                vec![("lang", lang.as_str().into()), ("cause", Arg::nested(cause))]
            }
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
    let lang = || "go".to_string();
    vec![
        Box::new(ProfileError::Query {
            lang: lang(),
            detail: "d".into(),
        }),
        Box::new(ProfileError::UnknownCapture {
            lang: lang(),
            name: "n".into(),
        }),
        Box::new(ProfileError::Toml {
            lang: lang(),
            file: "tokens.toml",
            detail: "d".into(),
        }),
        Box::new(ProfileError::UnknownToken {
            lang: lang(),
            name: "n".into(),
        }),
        Box::new(ProfileError::Tokens {
            lang: lang(),
            cause: Report::new("cherenkov-mutation-duplicate-spelling").arg("spelling", "&"),
        }),
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokensToml {
    not_template: String,
    #[serde(default)]
    tokens: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileToml {
    extensions: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    unit_types: BTreeSet<String>,
    #[serde(default)]
    disabled_operators: BTreeSet<String>,
    #[serde(default)]
    defaults: BTreeMap<String, String>,
    body_template: String,
    #[serde(default)]
    body_braces: bool,
    #[serde(default)]
    line_comment: Option<String>,
    #[serde(default)]
    block_comment: Option<(String, String)>,
    #[serde(default)]
    doc_comments: Vec<String>,
}

/// What a query capture name means to the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureRole {
    Concept(Concept),
    Statement(StmtKind),
    Role(Capture),
    Skip(SkipKind),
    Helper,
}

pub(crate) fn capture_role(name: &str) -> Option<CaptureRole> {
    if name.starts_with('_') {
        // A helper capture, there only for a predicate (`#eq? @_name "new"`).
        Some(CaptureRole::Helper)
    } else if let Some(k) = name.strip_prefix("skip.") {
        SkipKind::from_name(k).map(CaptureRole::Skip)
    } else if let Some(c) = name.strip_prefix("concept.") {
        Concept::from_name(c).map(CaptureRole::Concept)
    } else if let Some(k) = name.strip_prefix("stmt.") {
        StmtKind::from_name(k).map(CaptureRole::Statement)
    } else {
        Capture::from_name(name).map(CaptureRole::Role)
    }
}

pub fn load(src: &ProfileSource<'_>) -> Result<LoadedProfile, ProfileError> {
    let lang = src.lang.to_string();
    let toml_err = |file, e: toml::de::Error| ProfileError::Toml {
        lang: lang.clone(),
        file,
        detail: e.to_string(),
    };

    let tokens_toml: TokensToml =
        toml::from_str(src.tokens_toml).map_err(|e| toml_err("tokens.toml", e))?;
    let mut spellings = Vec::new();
    for (name, spelling) in tokens_toml.tokens {
        let token = Token::from_name(&name).ok_or_else(|| ProfileError::UnknownToken {
            lang: lang.clone(),
            name,
        })?;
        spellings.push((token, spelling));
    }
    let tokens =
        TokenTable::new(spellings, tokens_toml.not_template).map_err(|e| ProfileError::Tokens {
            lang: lang.clone(),
            cause: Report::of(&e),
        })?;

    let profile_toml: ProfileToml =
        toml::from_str(src.profile_toml).map_err(|e| toml_err("profile.toml", e))?;

    let query = tree_sitter::Query::new(&src.grammar, src.concepts_scm).map_err(|e| {
        ProfileError::Query {
            lang: lang.clone(),
            detail: e.to_string(),
        }
    })?;
    let mut mapped = BTreeSet::new();
    for name in query.capture_names() {
        match capture_role(name) {
            Some(CaptureRole::Concept(c)) => {
                mapped.insert(c);
            }
            Some(CaptureRole::Statement(_)) => {
                mapped.insert(Concept::Statement);
            }
            Some(CaptureRole::Role(_) | CaptureRole::Skip(_) | CaptureRole::Helper) => {}
            None => {
                return Err(ProfileError::UnknownCapture {
                    lang,
                    name: name.to_string(),
                });
            }
        }
    }

    Ok(LoadedProfile {
        profile: Profile {
            lang: LanguageId::new(src.lang),
            tokens,
            mapped,
            disabled_operators: profile_toml.disabled_operators,
            defaults: profile_toml.defaults,
            unit_types: profile_toml.unit_types,
            body_template: profile_toml.body_template,
            body_braces: profile_toml.body_braces,
            comments: cherenkov_mutation::profile::Comments {
                line: profile_toml.line_comment,
                block: profile_toml.block_comment,
                doc: profile_toml.doc_comments,
            },
            return_values: src.return_values,
            files: FileRules {
                extensions: profile_toml.extensions,
                exclude: profile_toml.exclude,
            },
        },
        grammar: src.grammar.clone(),
        query,
        extra_matches: None,
    })
}

pub fn builtin() -> Result<Vec<LoadedProfile>, ProfileError> {
    [
        ProfileSource {
            lang: "go",
            grammar: tree_sitter_go::LANGUAGE.into(),
            concepts_scm: include_str!("../../../../../profiles/go/concepts.scm"),
            tokens_toml: include_str!("../../../../../profiles/go/tokens.toml"),
            profile_toml: include_str!("../../../../../profiles/go/profile.toml"),
            return_values: None,
        },
        ProfileSource {
            lang: "python",
            grammar: tree_sitter_python::LANGUAGE.into(),
            concepts_scm: include_str!("../../../../../profiles/python/concepts.scm"),
            tokens_toml: include_str!("../../../../../profiles/python/tokens.toml"),
            profile_toml: include_str!("../../../../../profiles/python/profile.toml"),
            return_values: None,
        },
        ProfileSource {
            lang: "rust",
            grammar: tree_sitter_rust::LANGUAGE.into(),
            concepts_scm: include_str!("../../../../../profiles/rust/concepts.scm"),
            tokens_toml: include_str!("../../../../../profiles/rust/tokens.toml"),
            profile_toml: include_str!("../../../../../profiles/rust/profile.toml"),
            return_values: Some(ReturnValues(crate::rust_values::return_values)),
        },
    ]
    .iter()
    .map(|src| {
        let mut loaded = load(src)?;
        if src.lang == "rust" {
            loaded.extra_matches = Some(crate::rust_chains::let_chain_ands);
        }
        Ok(loaded)
    })
    .collect()
}

#[cfg(test)]
#[path = "profiles.test.rs"]
mod tests;
