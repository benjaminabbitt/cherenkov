//! The language-neutral concept vocabulary (design section 4.2).

use serde::Serialize;

/// A grammar construct, as captured by a language profile's queries.
///
/// Concepts are not operators: several operators match one concept and
/// differ in the edits they emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Concept {
    Binary,
    Unary,
    CompoundAssign,
    Literal,
    Condition,
    Return,
    Call,
    Assign,
    Expr,
    Name,
    Index,
    Statement,
    Block,
    Function,
    /// One arm of a match or switch.
    Arm,
    /// One field of a struct literal whose omitted fields take a default.
    FieldInit,
}

impl Concept {
    pub const ALL: [Concept; 16] = [
        Concept::Binary,
        Concept::Unary,
        Concept::CompoundAssign,
        Concept::Literal,
        Concept::Condition,
        Concept::Return,
        Concept::Call,
        Concept::Assign,
        Concept::Expr,
        Concept::Name,
        Concept::Index,
        Concept::Statement,
        Concept::Block,
        Concept::Function,
        Concept::Arm,
        Concept::FieldInit,
    ];

    /// The capture-name suffix used in profile queries (`@concept.<name>`).
    pub fn name(self) -> &'static str {
        match self {
            Concept::Binary => "binary",
            Concept::Unary => "unary",
            Concept::CompoundAssign => "compound_assign",
            Concept::Literal => "literal",
            Concept::Condition => "condition",
            Concept::Return => "return",
            Concept::Call => "call",
            Concept::Assign => "assign",
            Concept::Expr => "expr",
            Concept::Name => "name",
            Concept::Index => "index",
            Concept::Statement => "statement",
            Concept::Block => "block",
            Concept::Function => "function",
            Concept::Arm => "arm",
            Concept::FieldInit => "field_init",
        }
    }

    pub fn from_name(name: &str) -> Option<Concept> {
        Concept::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// A named role inside a concept match (`@op`, `@lhs`, ... in profile queries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capture {
    Op,
    Lhs,
    Rhs,
    Operand,
    Target,
    Value,
    Cond,
    Then,
    Else,
    Callee,
    Args,
    Ident,
    Base,
    Index,
    Name,
    Body,
    Returns,
    /// An arm's guard expression.
    Guard,
    /// The construct an arm or field belongs to (the match, the literal).
    Scope,
    /// Present on the catch-all arm of a match.
    CatchAll,
    /// Present on an `async` function.
    Async,
}

impl Capture {
    pub const ALL: [Capture; 21] = [
        Capture::Op,
        Capture::Lhs,
        Capture::Rhs,
        Capture::Operand,
        Capture::Target,
        Capture::Value,
        Capture::Cond,
        Capture::Then,
        Capture::Else,
        Capture::Callee,
        Capture::Args,
        Capture::Ident,
        Capture::Base,
        Capture::Index,
        Capture::Name,
        Capture::Body,
        Capture::Returns,
        Capture::Guard,
        Capture::Scope,
        Capture::CatchAll,
        Capture::Async,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Capture::Op => "op",
            Capture::Lhs => "lhs",
            Capture::Rhs => "rhs",
            Capture::Operand => "operand",
            Capture::Target => "target",
            Capture::Value => "value",
            Capture::Cond => "cond",
            Capture::Then => "then",
            Capture::Else => "else",
            Capture::Callee => "callee",
            Capture::Args => "args",
            Capture::Ident => "ident",
            Capture::Base => "base",
            Capture::Index => "index",
            Capture::Name => "name",
            Capture::Body => "body",
            Capture::Returns => "returns",
            Capture::Guard => "guard",
            Capture::Scope => "scope",
            Capture::CatchAll => "catch_all",
            Capture::Async => "async",
        }
    }

    pub fn from_name(name: &str) -> Option<Capture> {
        Capture::ALL.into_iter().find(|c| c.name() == name)
    }
}
