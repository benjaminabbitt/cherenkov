//! What a `SyntaxPort` adapter hands the core: spans, statements and
//! concept matches for one file. Plain data; no parser types leak in.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;

use crate::concept::{Capture, Concept};

/// Opaque language identifier ("python", "rust", "go"). Deliberately not an
/// enum: core code must never branch on language.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct LanguageId(pub String);

impl LanguageId {
    pub fn new(id: impl Into<String>) -> Self {
        LanguageId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A source position. `line` and `col` are 1-based; `col` counts bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Pos {
    pub byte: usize,
    pub line: u32,
    pub col: u32,
}

/// A half-open byte range `[start, end)` with its line/column endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    /// True when `other` lies entirely within `self` (equal spans contain each other).
    pub fn contains(&self, other: &Span) -> bool {
        self.start.byte <= other.start.byte && other.end.byte <= self.end.byte
    }

    pub fn len(&self) -> usize {
        self.end.byte - self.start.byte
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start.byte..self.end.byte]
    }
}

/// The position of `byte` in `source`.
pub fn pos_at(source: &str, byte: usize) -> Pos {
    let before = &source[..byte];
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Pos {
        byte,
        line,
        col: (byte - line_start) as u32 + 1,
    }
}

/// A function body's contents: between the braces when `braces`, trimmed
/// of surrounding whitespace and comments (cargo-mutants spans a body from
/// its first token to its last). Empty
/// contents give an empty span after the opening brace.
pub fn body_contents(
    source: &str,
    body: Span,
    braces: bool,
    comments: &crate::profile::Comments,
) -> Span {
    let text = body.text(source);
    let (mut start, mut end) = (body.start.byte, body.end.byte);
    if braces && text.starts_with('{') && text.ends_with('}') && text.len() >= 2 {
        start += 1;
        end -= 1;
    }
    let inner = &source[start..end];
    let lead = comments.leading(inner);
    let trail = comments.trailing(inner);
    let (s, e) = if lead == inner.len() {
        (start, start)
    } else {
        (start + lead, end - trail)
    };
    Span {
        start: pos_at(source, s),
        end: pos_at(source, e),
    }
}

/// Statement kinds the planner distinguishes (design section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StmtKind {
    /// An expression statement whose expression is a call.
    Call,
    /// Assignment, compound assignment, declaration with initializer, increment.
    Assign,
    Return,
    If,
    Loop,
}

impl StmtKind {
    pub const ALL: [StmtKind; 5] = [
        StmtKind::Call,
        StmtKind::Assign,
        StmtKind::Return,
        StmtKind::If,
        StmtKind::Loop,
    ];

    /// The capture-name suffix used in profile queries (`@stmt.<name>`).
    pub fn name(self) -> &'static str {
        match self {
            StmtKind::Call => "call",
            StmtKind::Assign => "assign",
            StmtKind::Return => "return",
            StmtKind::If => "if",
            StmtKind::Loop => "loop",
        }
    }

    pub fn from_name(name: &str) -> Option<StmtKind> {
        StmtKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Statement {
    pub kind: StmtKind,
    pub span: Span,
}

/// One grammar node captured as a concept, with its role captures.
///
/// A role may be captured more than once (repeated grammar fields such as a
/// chained comparison's operators); the core decides what that means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConceptMatch {
    pub concept: Concept,
    pub span: Span,
    pub captures: Vec<(Capture, Span)>,
}

impl ConceptMatch {
    pub fn all(&self, capture: Capture) -> impl Iterator<Item = Span> + '_ {
        self.captures
            .iter()
            .filter(move |(c, _)| *c == capture)
            .map(|(_, s)| *s)
    }

    /// The capture's span if it was captured exactly once.
    pub fn one(&self, capture: Capture) -> Option<Span> {
        let mut it = self.all(capture);
        let first = it.next()?;
        it.next().is_none().then_some(first)
    }

    /// The earliest capture of this role, by source position.
    pub fn first(&self, capture: Capture) -> Option<Span> {
        self.all(capture).min_by_key(|s| s.start.byte)
    }
}

/// Everything the planner needs to know about one source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSyntax {
    pub lang: LanguageId,
    /// Path relative to the project root, as reported in mutant listings.
    pub path: PathBuf,
    #[serde(skip)]
    pub source: Arc<str>,
    /// The parser recovered from syntax errors; the file is not planned.
    pub has_errors: bool,
    pub statements: Vec<Statement>,
    pub matches: Vec<ConceptMatch>,
    /// Regions a set may decline to mutate, by kind.
    pub skips: Vec<SkipRegion>,
}

/// Why a region might be left unmutated. Language-neutral: each profile
/// maps whatever its language has of each kind (`@skip.<kind>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipKind {
    /// Test code: test functions, test-only modules and items.
    Test,
    /// Code its author marked as not to be mutated.
    Annotated,
    /// Unsafe code.
    Unsafe,
    /// Constructors, which often have no viable alternative body.
    Constructor,
    /// Implementations of the language's default-value protocol.
    DefaultImpl,
    /// Allocation-capacity hints, whose value does not change behaviour.
    CapacityCall,
}

impl SkipKind {
    pub const ALL: [SkipKind; 6] = [
        SkipKind::Test,
        SkipKind::Annotated,
        SkipKind::Unsafe,
        SkipKind::Constructor,
        SkipKind::DefaultImpl,
        SkipKind::CapacityCall,
    ];

    /// The capture-name suffix used in profile queries (`@skip.<name>`).
    pub fn name(self) -> &'static str {
        match self {
            SkipKind::Test => "test",
            SkipKind::Annotated => "annotated",
            SkipKind::Unsafe => "unsafe",
            SkipKind::Constructor => "constructor",
            SkipKind::DefaultImpl => "default_impl",
            SkipKind::CapacityCall => "capacity_call",
        }
    }

    pub fn from_name(name: &str) -> Option<SkipKind> {
        SkipKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SkipRegion {
    pub kind: SkipKind,
    pub span: Span,
}
