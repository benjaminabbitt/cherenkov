//! Hand-built `FileSyntax` for a synthetic C-like toy language, so core
//! tests need no parser and show that the core knows no real language.
#![allow(dead_code)] // each test binary uses a different subset of helpers

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::profile::{FileRules, Profile};
use cherenkov_mutation::syntax::{
    ConceptMatch, FileSyntax, LanguageId, Pos, Span, Statement, StmtKind,
};
use cherenkov_mutation::token::{Token, TokenTable};

pub fn toy_tokens() -> TokenTable {
    let spell = [
        (Token::Add, "+"),
        (Token::Sub, "-"),
        (Token::Mul, "*"),
        (Token::Div, "/"),
        (Token::Rem, "%"),
        (Token::BitAnd, "&"),
        (Token::BitOr, "|"),
        (Token::BitXor, "^"),
        (Token::Shl, "<<"),
        (Token::Shr, ">>"),
        (Token::AddAssign, "+="),
        (Token::SubAssign, "-="),
        (Token::MulAssign, "*="),
        (Token::DivAssign, "/="),
        (Token::RemAssign, "%="),
        (Token::BitAndAssign, "&="),
        (Token::BitOrAssign, "|="),
        (Token::BitXorAssign, "^="),
        (Token::ShlAssign, "<<="),
        (Token::ShrAssign, ">>="),
        (Token::Lt, "<"),
        (Token::Le, "<="),
        (Token::Gt, ">"),
        (Token::Ge, ">="),
        (Token::Eq, "=="),
        (Token::Ne, "!="),
        (Token::And, "&&"),
        (Token::Or, "||"),
        (Token::Assign, ":="),
        (Token::True, "yes"),
        (Token::False, "no"),
    ];
    TokenTable::new(spell.map(|(t, s)| (t, s.to_string())), "NOT[{}]").unwrap()
}

/// Toy profile: assignment is `:=`, booleans are `yes`/`no`, negation is `NOT[..]`.
pub fn toy_profile() -> Profile {
    Profile {
        lang: LanguageId::new("toy"),
        tokens: toy_tokens(),
        mapped: [
            Concept::Binary,
            Concept::Condition,
            Concept::Return,
            Concept::Call,
            Concept::Assign,
            Concept::Statement,
            Concept::Function,
        ]
        .into_iter()
        .collect(),
        disabled_operators: BTreeSet::new(),
        defaults: BTreeMap::from([("bool".to_string(), "no".to_string())]),
        unit_types: BTreeSet::from(["void".to_string()]),
        body_template: "{}".to_string(),
        body_braces: true,
        comments: Default::default(),
        return_values: None,
        files: FileRules::default(),
    }
}

pub fn pos(src: &str, byte: usize) -> Pos {
    let before = &src[..byte];
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Pos {
        byte,
        line,
        col: (byte - line_start) as u32 + 1,
    }
}

/// Span of the `nth` (0-based) occurrence of `needle` in `src`.
pub fn at(src: &str, needle: &str, nth: usize) -> Span {
    let start = src
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle:?} occurrence {nth} not in source"))
        .0;
    Span {
        start: pos(src, start),
        end: pos(src, start + needle.len()),
    }
}

pub struct Toy {
    pub file: FileSyntax,
}

impl Toy {
    pub fn new(path: &str, src: &str) -> Toy {
        Toy {
            file: FileSyntax {
                lang: LanguageId::new("toy"),
                path: PathBuf::from(path),
                source: Arc::from(src),
                has_errors: false,
                statements: Vec::new(),
                matches: Vec::new(),
                skips: Vec::new(),
            },
        }
    }

    pub fn span(&self, needle: &str, nth: usize) -> Span {
        at(&self.file.source, needle, nth)
    }

    pub fn stmt(mut self, kind: StmtKind, needle: &str, nth: usize) -> Toy {
        let span = self.span(needle, nth);
        self.file.statements.push(Statement { kind, span });
        self
    }

    /// `caps` are `(role, needle, nth)` triples located in the whole source.
    pub fn concept(
        mut self,
        concept: Concept,
        needle: &str,
        nth: usize,
        caps: &[(Capture, &str, usize)],
    ) -> Toy {
        let span = self.span(needle, nth);
        let captures = caps.iter().map(|&(c, n, i)| (c, self.span(n, i))).collect();
        self.file.matches.push(ConceptMatch {
            concept,
            span,
            captures,
        });
        self
    }

    pub fn build(self) -> FileSyntax {
        self.file
    }
}

/// The toy function used across planner and ID tests:
///
/// ```text
/// fn f() bool {
///     x := a + b;
///     log(x);
///     if a < b { return yes; } else { g(); }
///     while a > b { a := a - 1; }
///     g(a + 1);
///     return no;
/// }
/// ```
pub const TOY_SRC: &str = "\
fn f() bool {
    x := a + b;
    log(x);
    if a < b { return yes; } else { g(); }
    while a > b { a := a - 1; }
    g(a + 1);
    return no;
}
";

pub fn toy_file(path: &str, src: &str) -> FileSyntax {
    use Capture::*;
    Toy::new(path, src)
        .concept(
            Concept::Function,
            src.trim_end(),
            0,
            &[
                (Name, "f", 1),
                (Returns, "bool", 0),
                (Body, &src.trim_end()["fn f() bool ".len()..], 0),
            ],
        )
        // x := a + b;
        .stmt(StmtKind::Assign, "x := a + b;", 0)
        .concept(
            Concept::Assign,
            "x := a + b",
            0,
            &[(Target, "x", 0), (Value, "a + b", 0)],
        )
        .concept(
            Concept::Binary,
            "a + b",
            0,
            &[(Lhs, "a", 0), (Op, "+", 0), (Rhs, "b", 0)],
        )
        // log(x);
        .stmt(StmtKind::Call, "log(x);", 0)
        .concept(
            Concept::Call,
            "log(x)",
            0,
            &[(Callee, "log", 0), (Args, "(x)", 0)],
        )
        // if a < b { return yes; } else { g(); }
        .stmt(StmtKind::If, "if a < b { return yes; } else { g(); }", 0)
        .concept(
            Concept::Condition,
            "if a < b { return yes; } else { g(); }",
            0,
            &[
                (Cond, "a < b", 0),
                (Then, "{ return yes; }", 0),
                (Else, "{ g(); }", 0),
            ],
        )
        .concept(Concept::Binary, "a < b", 0, &[(Op, "<", 0)])
        .stmt(StmtKind::Return, "return yes;", 0)
        .concept(Concept::Return, "return yes", 0, &[(Value, "yes", 0)])
        .stmt(StmtKind::Call, "g();", 0)
        .concept(
            Concept::Call,
            "g()",
            0,
            &[(Callee, "g", 1), (Args, "()", 1)],
        )
        // while a > b { a := a - 1; }
        .stmt(StmtKind::Loop, "while a > b { a := a - 1; }", 0)
        .concept(
            Concept::Condition,
            "while a > b { a := a - 1; }",
            0,
            &[(Cond, "a > b", 0), (Then, "{ a := a - 1; }", 0)],
        )
        .concept(Concept::Binary, "a > b", 0, &[(Op, ">", 0)])
        .stmt(StmtKind::Assign, "a := a - 1;", 0)
        .concept(
            Concept::Assign,
            "a := a - 1",
            0,
            &[(Target, "a", 3), (Value, "a - 1", 0)],
        )
        .concept(Concept::Binary, "a - 1", 0, &[(Op, "-", 0)])
        // g(a + 1);
        .stmt(StmtKind::Call, "g(a + 1);", 0)
        .concept(
            Concept::Call,
            "g(a + 1)",
            0,
            &[(Callee, "g", 2), (Args, "(a + 1)", 0)],
        )
        .concept(Concept::Binary, "a + 1", 0, &[(Op, "+", 1)])
        // return no;
        .stmt(StmtKind::Return, "return no;", 0)
        .concept(Concept::Return, "return no", 0, &[(Value, "no", 0)])
        .build()
}
