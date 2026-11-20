//! The operator catalog, written once for every language (design section 4.9).

use crate::concept::{Capture, Concept};
use crate::edit::Edit;
use crate::edit::Lit;
use crate::profile::Profile;
use crate::syntax::{ConceptMatch, FileSyntax, SkipKind, Span, Statement, StmtKind};
use crate::token::Token;

/// What an operator may look at when deciding its edits.
pub struct MatchCx<'a> {
    pub file: &'a FileSyntax,
    pub profile: &'a Profile,
    pub m: &'a ConceptMatch,
    /// The innermost statement containing the match, if any.
    pub stmt: Option<&'a Statement>,
}

impl<'a> MatchCx<'a> {
    /// The context the planner gives an operator for `m`: the innermost
    /// statement containing it (smallest span; ties broken by position).
    pub fn new(file: &'a FileSyntax, profile: &'a Profile, m: &'a ConceptMatch) -> Self {
        let stmt = innermost_statement(file, &m.span);
        MatchCx {
            file,
            profile,
            m,
            stmt,
        }
    }

    /// Source text of a capture captured exactly once.
    pub fn text(&self, capture: Capture) -> Option<&'a str> {
        Some(self.m.one(capture)?.text(&self.file.source))
    }

    /// Abstract token of a capture captured exactly once.
    pub fn token(&self, capture: Capture) -> Option<Token> {
        self.profile.tokens.token(self.text(capture)?)
    }
}

/// Every concept match operators see: the adapter's matches plus one
/// `Statement` match per statement (span only, no captures), sorted by
/// position, then concept.
pub fn operator_inputs(file: &FileSyntax) -> Vec<ConceptMatch> {
    let statements = file.statements.iter().map(|s| ConceptMatch {
        concept: Concept::Statement,
        span: s.span,
        captures: Vec::new(),
    });
    let mut all: Vec<ConceptMatch> = file.matches.iter().cloned().chain(statements).collect();
    all.sort_by(|a, b| {
        (a.span.start.byte, a.concept, a.span.end.byte).cmp(&(
            b.span.start.byte,
            b.concept,
            b.span.end.byte,
        ))
    });
    all
}

/// Innermost statement containing `span`: smallest, ties to the earliest.
pub(crate) fn innermost_statement<'a>(file: &'a FileSyntax, span: &Span) -> Option<&'a Statement> {
    file.statements
        .iter()
        .filter(|s| s.span.contains(span))
        .min_by_key(|s| (s.span.len(), s.span.start.byte))
}

pub trait Operator: Send + Sync {
    /// Region kinds this operator plans nothing in (set by its set).
    fn skips(&self) -> &[SkipKind] {
        &[]
    }
    fn id(&self) -> &'static str;
    fn concepts(&self) -> &'static [Concept];
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit>;
}

/// Statement deletion of a call statement (`log(x)` removed).
pub struct DeleteStatement;
/// Arithmetic operator replacement: `+`/`-`, `*`/`/`.
pub struct Aor;
/// Boundary relational replacement: `<`/`<=`, `>`/`>=`.
pub struct Ror;
/// Condition negation of `if` and loop conditions.
pub struct NegateCondition;
/// `return true` to `return false` and back; boolean literals only.
pub struct ReturnBoolFlip;
/// Function body replaced by a default return of its result type.
pub struct BodyDefault;

impl Operator for DeleteStatement {
    fn id(&self) -> &'static str {
        "delete-statement"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Statement]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        match cx.stmt {
            Some(s) if s.kind == StmtKind::Call && s.span == cx.m.span => {
                vec![Edit::DeleteStatement]
            }
            _ => vec![],
        }
    }
}

impl Operator for Aor {
    fn id(&self) -> &'static str {
        "aor"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Binary]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let Some(from) = cx.token(Capture::Op) else {
            return vec![];
        };
        let with = match from {
            Token::Add => Token::Sub,
            Token::Sub => Token::Add,
            Token::Mul => Token::Div,
            Token::Div => Token::Mul,
            _ => return vec![],
        };
        vec![Edit::ReplaceToken {
            capture: Capture::Op,
            from,
            with,
        }]
    }
}

impl Operator for Ror {
    fn id(&self) -> &'static str {
        "ror"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Binary]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let Some(from) = cx.token(Capture::Op) else {
            return vec![];
        };
        let with = match from {
            Token::Lt => Token::Le,
            Token::Le => Token::Lt,
            Token::Gt => Token::Ge,
            Token::Ge => Token::Gt,
            _ => return vec![],
        };
        vec![Edit::ReplaceToken {
            capture: Capture::Op,
            from,
            with,
        }]
    }
}

impl Operator for NegateCondition {
    fn id(&self) -> &'static str {
        "negate-condition"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Condition]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        match cx.m.one(Capture::Cond) {
            Some(_) => vec![Edit::WrapNot {
                capture: Capture::Cond,
            }],
            None => vec![],
        }
    }
}

impl Operator for ReturnBoolFlip {
    fn id(&self) -> &'static str {
        "return-bool-flip"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Return]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        match cx
            .text(Capture::Value)
            .and_then(|t| cx.profile.tokens.literal_bool(t))
        {
            Some(b) => vec![Edit::ReplaceWithLiteral {
                capture: Capture::Value,
                lit: Lit::Bool(!b),
            }],
            None => vec![],
        }
    }
}

impl Operator for BodyDefault {
    fn id(&self) -> &'static str {
        "body-default"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Function]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let Some(returns) = cx.text(Capture::Returns) else {
            return vec![];
        };
        if cx.m.one(Capture::Body).is_none() || cx.profile.unit_types.contains(returns.trim()) {
            return vec![];
        }
        vec![Edit::ReplaceWithLiteral {
            capture: Capture::Body,
            lit: Lit::Default,
        }]
    }
}

/// One operator family as a replacement table: each source token and the
/// tokens it is replaced with, in order. Built from a set's named mutations
/// (operator_set.rs), so one implementation serves every set.
pub struct TokenSwap {
    id: &'static str,
    concepts: &'static [Concept],
    table: Vec<(Token, Vec<Token>)>,
}

impl TokenSwap {
    pub fn new(
        id: &'static str,
        concepts: &'static [Concept],
        table: Vec<(Token, Vec<Token>)>,
    ) -> Self {
        TokenSwap {
            id,
            concepts,
            table,
        }
    }
}

impl Operator for TokenSwap {
    fn id(&self) -> &'static str {
        self.id
    }
    fn concepts(&self) -> &'static [Concept] {
        self.concepts
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let Some(from) = cx.token(Capture::Op) else {
            return vec![];
        };
        self.table
            .iter()
            .filter(|(t, _)| *t == from)
            .flat_map(|(_, withs)| withs.iter())
            .map(|&with| Edit::ReplaceToken {
                capture: Capture::Op,
                from,
                with,
            })
            .collect()
    }
}

/// Unary operator deletion: `!x` -> `x`, `-x` -> `x`, for the listed unary
/// tokens; other prefix operators are left alone. Built from a set's named
/// mutations, like [`TokenSwap`].
pub struct DeleteUnary {
    id: &'static str,
    tokens: Vec<Token>,
}

impl DeleteUnary {
    pub fn new(id: &'static str, tokens: Vec<Token>) -> Self {
        DeleteUnary { id, tokens }
    }
}

impl Operator for DeleteUnary {
    fn id(&self) -> &'static str {
        self.id
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Unary]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let token = cx
            .text(Capture::Op)
            .and_then(|t| cx.profile.tokens.unary(t));
        match token {
            Some(token) if self.tokens.contains(&token) => vec![Edit::DeleteToken {
                capture: Capture::Op,
                token,
            }],
            _ => vec![],
        }
    }
}

/// A match guard replaced with each listed boolean.
pub struct MatchGuard {
    id: &'static str,
    values: Vec<bool>,
}

impl MatchGuard {
    pub fn new(id: &'static str, values: Vec<bool>) -> Self {
        MatchGuard { id, values }
    }
}

impl Operator for MatchGuard {
    fn id(&self) -> &'static str {
        self.id
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Arm]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        if cx.m.one(Capture::Guard).is_none() {
            return vec![];
        }
        self.values
            .iter()
            .map(|&b| Edit::ReplaceWithLiteral {
                capture: Capture::Guard,
                lit: Lit::Bool(b),
            })
            .collect()
    }
}

/// Arm deletion, only from a match with a catch-all arm (so the match stays
/// exhaustive), and never the catch-all itself or a guarded arm, whose
/// guard -> false is the same mutant.
pub struct DeleteArm;

impl Operator for DeleteArm {
    fn id(&self) -> &'static str {
        "match-arm"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Arm]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let m = cx.m;
        let Some(scope) = m.one(Capture::Scope) else {
            return vec![];
        };
        if m.one(Capture::CatchAll).is_some() || m.one(Capture::Guard).is_some() {
            return vec![];
        }
        let has_catch_all = cx.file.matches.iter().any(|o| {
            o.concept == Concept::Arm
                && o.one(Capture::Scope) == Some(scope)
                && o.one(Capture::CatchAll).is_some()
        });
        if has_catch_all {
            vec![Edit::DeleteMatch]
        } else {
            vec![]
        }
    }
}

/// Struct literal field deletion. Which literals take defaults for omitted
/// fields is the profile's to say, through what it maps to `FieldInit`.
pub struct DeleteField;

impl Operator for DeleteField {
    fn id(&self) -> &'static str {
        "struct-field"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::FieldInit]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        if cx.m.one(Capture::Scope).is_some() {
            vec![Edit::DeleteMatch]
        } else {
            vec![]
        }
    }
}

/// Function body replaced by each value the profile's generator gives for
/// the result type (cargo-mutants' FnValue). Empty bodies, and a value equal
/// to the body itself, give no mutant.
pub struct FnValue;

impl Operator for FnValue {
    fn id(&self) -> &'static str {
        "fn-value"
    }
    fn concepts(&self) -> &'static [Concept] {
        &[Concept::Function]
    }
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit> {
        let (Some(generate), Some(body)) = (cx.profile.return_values, cx.m.one(Capture::Body))
        else {
            return vec![];
        };
        let src: &str = &cx.file.source;
        let contents =
            crate::syntax::body_contents(src, body, cx.profile.body_braces, &cx.profile.comments);
        if contents.is_empty() {
            return vec![];
        }
        let current = contents
            .text(src)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let returns = cx.m.one(Capture::Returns).map(|r| r.text(src).trim());
        let mut seen = std::collections::BTreeSet::new();
        (generate.0)(returns, &[])
            .into_iter()
            .filter(|v| *v != current && seen.insert(v.clone()))
            .map(|v| Edit::ReplaceWithLiteral {
                capture: Capture::Body,
                lit: Lit::Text(v),
            })
            .collect()
    }
}

/// The breadth-first prototype operator set.
pub fn catalog() -> Vec<Box<dyn Operator>> {
    vec![
        Box::new(DeleteStatement),
        Box::new(Aor),
        Box::new(Ror),
        Box::new(NegateCondition),
        Box::new(ReturnBoolFlip),
        Box::new(BodyDefault),
    ]
}
