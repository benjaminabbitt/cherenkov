//! Loading language profiles from their data files.

use std::collections::{BTreeMap, BTreeSet};

use cherenkov_core::concept::{Capture, Concept};
use cherenkov_core::profile::{FileRules, Profile, ReturnValues};
use cherenkov_core::syntax::{LanguageId, SkipKind, StmtKind};
use cherenkov_core::token::{Token, TokenTable};
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

pub struct LoadedProfile {
    pub profile: Profile,
    pub grammar: tree_sitter::Language,
    pub query: tree_sitter::Query,
}

#[derive(Debug)]
pub enum ProfileError {
    Query {
        lang: String,
        message: String,
    },
    /// A capture name that is not `concept.<concept>`, `stmt.<kind>` or a role.
    UnknownCapture {
        lang: String,
        name: String,
    },
    Toml {
        lang: String,
        file: &'static str,
        message: String,
    },
    UnknownToken {
        lang: String,
        name: String,
    },
    Tokens {
        lang: String,
        message: String,
    },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Query { lang, message } => write!(f, "{lang}: concepts.scm: {message}"),
            ProfileError::UnknownCapture { lang, name } => {
                write!(f, "{lang}: concepts.scm: unknown capture @{name}")
            }
            ProfileError::Toml {
                lang,
                file,
                message,
            } => write!(f, "{lang}: {file}: {message}"),
            ProfileError::UnknownToken { lang, name } => {
                write!(f, "{lang}: tokens.toml: unknown token {name:?}")
            }
            ProfileError::Tokens { lang, message } => write!(f, "{lang}: tokens.toml: {message}"),
        }
    }
}

impl std::error::Error for ProfileError {}

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
        message: e.to_string(),
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
            message: e.to_string(),
        })?;

    let profile_toml: ProfileToml =
        toml::from_str(src.profile_toml).map_err(|e| toml_err("profile.toml", e))?;

    let query = tree_sitter::Query::new(&src.grammar, src.concepts_scm).map_err(|e| {
        ProfileError::Query {
            lang: lang.clone(),
            message: e.to_string(),
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
            comments: cherenkov_core::profile::Comments {
                line: profile_toml.line_comment,
                block: profile_toml.block_comment,
            },
            return_values: src.return_values,
            files: FileRules {
                extensions: profile_toml.extensions,
                exclude: profile_toml.exclude,
            },
        },
        grammar: src.grammar.clone(),
        query,
    })
}

pub fn builtin() -> Result<Vec<LoadedProfile>, ProfileError> {
    [
        ProfileSource {
            lang: "go",
            grammar: tree_sitter_go::LANGUAGE.into(),
            concepts_scm: include_str!("../../../profiles/go/concepts.scm"),
            tokens_toml: include_str!("../../../profiles/go/tokens.toml"),
            profile_toml: include_str!("../../../profiles/go/profile.toml"),
            return_values: None,
        },
        ProfileSource {
            lang: "python",
            grammar: tree_sitter_python::LANGUAGE.into(),
            concepts_scm: include_str!("../../../profiles/python/concepts.scm"),
            tokens_toml: include_str!("../../../profiles/python/tokens.toml"),
            profile_toml: include_str!("../../../profiles/python/profile.toml"),
            return_values: None,
        },
        ProfileSource {
            lang: "rust",
            grammar: tree_sitter_rust::LANGUAGE.into(),
            concepts_scm: include_str!("../../../profiles/rust/concepts.scm"),
            tokens_toml: include_str!("../../../profiles/rust/tokens.toml"),
            profile_toml: include_str!("../../../profiles/rust/profile.toml"),
            return_values: Some(ReturnValues(crate::rust_values::return_values)),
        },
    ]
    .iter()
    .map(load)
    .collect()
}
