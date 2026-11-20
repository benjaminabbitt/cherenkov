//! Named mutations and named operator sets (docs/operator-sets.md).
//!
//! Every mutation is defined once, in a superset, under a stable name, and
//! implemented by a reusable [`Rule`]. A set is a list of names drawn from
//! the superset, so sets share mutations instead of reimplementing them:
//! `ror/lt->le` is the same mutation, with the same IDs, in every set that
//! lists it. A run uses exactly one set and reports which.

use crate::concept::Concept;
use crate::operator::{
    BodyDefault, DeleteArm, DeleteField, DeleteStatement, DeleteUnary, FnValue, MatchGuard,
    NegateCondition, Operator, ReturnBoolFlip, TokenSwap,
};
use crate::syntax::SkipKind;
use crate::token::Token::{self, *};

/// How a named mutation is carried out. One implementation per rule kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Replace a binary operator token.
    ReplaceToken { from: Token, with: Token },
    /// Delete a unary operator token, keeping its operand.
    DeleteUnary { token: Token },
    /// Replace a match guard with a boolean.
    MatchGuard { value: bool },
    /// Delete a match arm, when the match has a catch-all.
    DeleteMatchArm,
    /// Delete a field of a struct literal whose omitted fields take a default.
    DeleteStructField,
    /// Delete a call statement.
    DeleteCallStatement,
    /// Negate an `if` or loop condition.
    NegateCondition,
    /// `return true` <-> `return false`.
    ReturnBoolFlip,
    /// Replace a function body with the profile's zero value of its result type.
    BodyDefault,
    /// Replace a function body with each value the profile's generator gives
    /// for its result type (cargo-mutants' FnValue table for Rust).
    FnValue,
}

/// How a source relates to a named mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The definition was taken from this source.
    Derived,
    /// The source has an identical mutation, perhaps under another name.
    Same,
    /// The source has a close mutation; the note says how it differs.
    Similar(&'static str),
}

/// Where a mutation comes from, or a peer that has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    /// The tool or paper.
    pub source: &'static str,
    /// What that source calls it, when it names it.
    pub name: Option<&'static str>,
    pub relation: Relation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mutation {
    /// Stable name, `family/detail`; sets refer to it.
    pub name: &'static str,
    /// The operator family, which is also the operator id in mutant IDs.
    pub family: &'static str,
    pub rule: Rule,
    /// Where the definition came from, and peers with the same mutation.
    /// At least one is [`Relation::Derived`].
    pub origins: &'static [Origin],
}

const fn tok(
    name: &'static str,
    family: &'static str,
    from: Token,
    with: Token,
    origins: &'static [Origin],
) -> Mutation {
    Mutation {
        name,
        family,
        rule: Rule::ReplaceToken { from, with },
        origins,
    }
}

/// Every mutation any set can use.
static MUTATIONS: &[Mutation] = &[
    // Relational, logical, arithmetic, shift, bitwise and compound-assignment
    // operators. cargo-mutants-derived entries cite the source line their
    // replacement was read from.
    tok(
        "ror/lt->le",
        "ror",
        Lt,
        Le,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("CONDITIONALS_BOUNDARY"),
                relation: Relation::Same,
            },
            Origin {
                source: "mutmut",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "go-mutesting",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/lt->eq",
        "ror",
        Lt,
        Eq,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/lt->gt",
        "ror",
        Lt,
        Gt,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/lt->ne",
        "ror",
        Lt,
        Ne,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/le->lt",
        "ror",
        Le,
        Lt,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("CONDITIONALS_BOUNDARY"),
                relation: Relation::Same,
            },
            Origin {
                source: "mutmut",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "go-mutesting",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/le->gt",
        "ror",
        Le,
        Gt,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/le->eq",
        "ror",
        Le,
        Eq,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/gt->ge",
        "ror",
        Gt,
        Ge,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("CONDITIONALS_BOUNDARY"),
                relation: Relation::Same,
            },
            Origin {
                source: "mutmut",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "go-mutesting",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/gt->eq",
        "ror",
        Gt,
        Eq,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/gt->lt",
        "ror",
        Gt,
        Lt,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/gt->ne",
        "ror",
        Gt,
        Ne,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/ge->gt",
        "ror",
        Ge,
        Gt,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("CONDITIONALS_BOUNDARY"),
                relation: Relation::Same,
            },
            Origin {
                source: "mutmut",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "go-mutesting",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/ge->lt",
        "ror",
        Ge,
        Lt,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/ge->eq",
        "ror",
        Ge,
        Eq,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/eq->ne",
        "ror",
        Eq,
        Ne,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("NEGATE_CONDITIONALS"),
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/eq->le",
        "ror",
        Eq,
        Le,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/eq->ge",
        "ror",
        Eq,
        Ge,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/ne->eq",
        "ror",
        Ne,
        Eq,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("NEGATE_CONDITIONALS"),
                relation: Relation::Same,
            },
            Origin {
                source: "Stryker",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Gremlins",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Mull",
                name: None,
                relation: Relation::Same,
            },
        ],
    ),
    tok(
        "ror/ne->lt",
        "ror",
        Ne,
        Lt,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "ror/ne->gt",
        "ror",
        Ne,
        Gt,
        &[Origin {
            source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "lcr/and->or",
        "lcr",
        And,
        Or,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "lcr/or->and",
        "lcr",
        Or,
        And,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/add->sub",
        "aor",
        Add,
        Sub,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/add->mul",
        "aor",
        Add,
        Mul,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/sub->add",
        "aor",
        Sub,
        Add,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/sub->div",
        "aor",
        Sub,
        Div,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/mul->add",
        "aor",
        Mul,
        Add,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/mul->div",
        "aor",
        Mul,
        Div,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/div->mul",
        "aor",
        Div,
        Mul,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/div->rem",
        "aor",
        Div,
        Rem,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/rem->div",
        "aor",
        Rem,
        Div,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/rem->add",
        "aor",
        Rem,
        Add,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "shift/shl->shr",
        "shift",
        Shl,
        Shr,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "shift/shr->shl",
        "shift",
        Shr,
        Shl,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/and->or",
        "bitwise",
        BitAnd,
        BitOr,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/and->xor",
        "bitwise",
        BitAnd,
        BitXor,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/or->and",
        "bitwise",
        BitOr,
        BitAnd,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/or->xor",
        "bitwise",
        BitOr,
        BitXor,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/xor->or",
        "bitwise",
        BitXor,
        BitOr,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/xor->and",
        "bitwise",
        BitXor,
        BitAnd,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/add_assign->sub_assign",
        "aor",
        AddAssign,
        SubAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/add_assign->mul_assign",
        "aor",
        AddAssign,
        MulAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/sub_assign->add_assign",
        "aor",
        SubAssign,
        AddAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/sub_assign->div_assign",
        "aor",
        SubAssign,
        DivAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/mul_assign->add_assign",
        "aor",
        MulAssign,
        AddAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/mul_assign->div_assign",
        "aor",
        MulAssign,
        DivAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/div_assign->rem_assign",
        "aor",
        DivAssign,
        RemAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "aor/div_assign->mul_assign",
        "aor",
        DivAssign,
        MulAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/rem_assign->div_assign",
        "aor",
        RemAssign,
        DivAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Dextool",
                name: Some("AORS"),
                relation: Relation::Derived,
            },
        ],
    ),
    tok(
        "aor/rem_assign->add_assign",
        "aor",
        RemAssign,
        AddAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "shift/shl_assign->shr_assign",
        "shift",
        ShlAssign,
        ShrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "shift/shr_assign->shl_assign",
        "shift",
        ShrAssign,
        ShlAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/and_assign->or_assign",
        "bitwise",
        BitAndAssign,
        BitOrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/or_assign->and_assign",
        "bitwise",
        BitOrAssign,
        BitAndAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/xor_assign->or_assign",
        "bitwise",
        BitXorAssign,
        BitOrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "bitwise/xor_assign->and_assign",
        "bitwise",
        BitXorAssign,
        BitAndAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // Logical connectors as the equality they equal on booleans.
    tok(
        "cor/and->eq",
        "cor",
        And,
        Eq,
        &[Origin {
            source: "Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    tok(
        "cor/or->ne",
        "cor",
        Or,
        Ne,
        &[Origin {
            source: "Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // Statements, conditions, returns.
    Mutation {
        name: "delete-statement/call",
        family: "delete-statement",
        rule: Rule::DeleteCallStatement,
        origins: &[
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Just et al. 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("VOID_METHOD_CALLS"),
                relation: Relation::Similar("removes calls to void methods only"),
            },
        ],
    },
    Mutation {
        name: "negate-condition",
        family: "negate-condition",
        rule: Rule::NegateCondition,
        origins: &[
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "PIT",
                name: Some("NEGATE_CONDITIONALS"),
                relation: Relation::Similar(
                    "negates the relational operator; this negates the whole condition",
                ),
            },
        ],
    },
    Mutation {
        name: "return-bool-flip",
        family: "return-bool-flip",
        rule: Rule::ReturnBoolFlip,
        origins: &[Origin {
            source: "Cherenkov design",
            name: None,
            relation: Relation::Derived,
        }],
    },
    Mutation {
        name: "body-default",
        family: "body-default",
        rule: Rule::BodyDefault,
        origins: &[
            Origin {
                source: "Cherenkov design",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "cargo-mutants",
                name: Some("FnValue"),
                relation: Relation::Similar(
                    "only the profile's zero value, not cargo-mutants' per-type table",
                ),
            },
        ],
    },
    Mutation {
        name: "fn-value",
        family: "fn-value",
        rule: Rule::FnValue,
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("FnValue"),
            relation: Relation::Derived,
        }],
    },
    // Match arms and guards, struct literal fields.
    Mutation {
        name: "match-arm/delete",
        family: "match-arm",
        rule: Rule::DeleteMatchArm,
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("MatchArm"),
            relation: Relation::Derived,
        }],
    },
    Mutation {
        name: "match-guard/true",
        family: "match-guard",
        rule: Rule::MatchGuard { value: true },
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("MatchArmGuard"),
            relation: Relation::Derived,
        }],
    },
    Mutation {
        name: "match-guard/false",
        family: "match-guard",
        rule: Rule::MatchGuard { value: false },
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("MatchArmGuard"),
            relation: Relation::Derived,
        }],
    },
    Mutation {
        name: "struct-field/delete",
        family: "struct-field",
        rule: Rule::DeleteStructField,
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("StructField"),
            relation: Relation::Derived,
        }],
    },
    // Unary operators: deleted, never replaced.
    Mutation {
        name: "unary/delete-not",
        family: "unary",
        rule: Rule::DeleteUnary { token: Not },
        origins: &[
            Origin {
                source: "cargo-mutants",
                name: Some("UnaryOperator"),
                relation: Relation::Derived,
            },
            Origin {
                source: "cosmic-ray",
                name: None,
                relation: Relation::Same,
            },
            Origin {
                source: "Dextool",
                name: Some("UOI"),
                relation: Relation::Same,
            },
        ],
    },
    Mutation {
        name: "unary/delete-neg",
        family: "unary",
        rule: Rule::DeleteUnary { token: Neg },
        origins: &[Origin {
            source: "cargo-mutants",
            name: Some("UnaryOperator"),
            relation: Relation::Derived,
        }],
    },
];

/// The superset of named mutations.
pub fn mutations() -> &'static [Mutation] {
    MUTATIONS
}

/// The mutation called `name`.
pub fn mutation_named(name: &str) -> Option<&'static Mutation> {
    MUTATIONS.iter().find(|m| m.name == name)
}

/// The mutation an operator's edit carries out: by family and token pair
/// for token rules, by family alone otherwise.
pub fn mutation_for(operator: &str, edit: &crate::edit::Edit) -> Option<&'static Mutation> {
    MUTATIONS.iter().find(|m| {
        m.family == operator
            && match (m.rule, edit) {
                (
                    Rule::ReplaceToken { from, with },
                    crate::edit::Edit::ReplaceToken {
                        from: f, with: w, ..
                    },
                ) => from == *f && with == *w,
                (Rule::ReplaceToken { .. }, _) => false,
                (Rule::DeleteUnary { token }, crate::edit::Edit::DeleteToken { token: t, .. }) => {
                    token == *t
                }
                (Rule::DeleteUnary { .. }, _) => false,
                (
                    Rule::MatchGuard { value },
                    crate::edit::Edit::ReplaceWithLiteral {
                        lit: crate::edit::Lit::Bool(b),
                        ..
                    },
                ) => value == *b,
                (Rule::MatchGuard { .. }, _) => false,
                _ => true,
            }
    })
}

pub struct OperatorSet {
    pub name: &'static str,
    pub summary: &'static str,
    /// Where the set's rules come from.
    pub source: &'static str,
    /// Names from [`mutations`], in the order their edits are generated.
    pub mutations: &'static [&'static str],
    /// Rules of the set's specification not implemented yet, listed so a
    /// partial set is never mistaken for the full one.
    pub pending: &'static [&'static str],
    /// Region kinds the set plans nothing in.
    pub skips: &'static [SkipKind],
}

/// An operator that declines the regions its set skips.
struct Honouring {
    inner: Box<dyn Operator>,
    skips: &'static [SkipKind],
}

impl Operator for Honouring {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn concepts(&self) -> &'static [Concept] {
        self.inner.concepts()
    }
    fn mutate(&self, cx: &crate::operator::MatchCx<'_>) -> Vec<crate::edit::Edit> {
        self.inner.mutate(cx)
    }
    fn skips(&self) -> &[SkipKind] {
        self.skips
    }
}

impl OperatorSet {
    /// The operators that carry out this set's mutations: one per family,
    /// in order of first appearance. Panics on a name not in the superset,
    /// which is a mistake in a hard-coded set.
    pub fn operators(&self) -> Vec<Box<dyn Operator>> {
        enum Slot {
            Tokens(&'static str, Vec<(Token, Vec<Token>)>),
            /// Rules of one family carried out by one parameterized operator.
            Family(&'static str, Vec<Rule>),
            Rule(Rule),
        }
        let mut slots: Vec<Slot> = Vec::new();
        for name in self.mutations {
            let m = MUTATIONS
                .iter()
                .find(|m| m.name == *name)
                .unwrap_or_else(|| panic!("set {}: no mutation named {name:?}", self.name));
            match m.rule {
                Rule::ReplaceToken { from, with } => {
                    let pos = slots
                        .iter()
                        .position(|s| matches!(s, Slot::Tokens(f, _) if *f == m.family));
                    let pos = pos.unwrap_or_else(|| {
                        slots.push(Slot::Tokens(m.family, Vec::new()));
                        slots.len() - 1
                    });
                    if let Slot::Tokens(_, table) = &mut slots[pos] {
                        match table.iter_mut().find(|(t, _)| *t == from) {
                            Some((_, withs)) => withs.push(with),
                            None => table.push((from, vec![with])),
                        }
                    }
                }
                Rule::DeleteUnary { .. } | Rule::MatchGuard { .. } => {
                    let pos = slots
                        .iter()
                        .position(|s| matches!(s, Slot::Family(f, _) if *f == m.family));
                    match pos {
                        Some(i) => {
                            if let Slot::Family(_, rules) = &mut slots[i] {
                                rules.push(m.rule);
                            }
                        }
                        None => slots.push(Slot::Family(m.family, vec![m.rule])),
                    }
                }
                rule => {
                    if !slots
                        .iter()
                        .any(|s| matches!(s, Slot::Rule(r) if *r == rule))
                    {
                        slots.push(Slot::Rule(rule));
                    }
                }
            }
        }
        slots
            .into_iter()
            .map(|slot| -> Box<dyn Operator> {
                match slot {
                    Slot::Tokens(family, table) => Box::new(TokenSwap::new(family, BINARY, table)),
                    Slot::Family(family, rules) => family_operator(family, &rules),
                    Slot::Rule(Rule::DeleteCallStatement) => Box::new(DeleteStatement),
                    Slot::Rule(Rule::NegateCondition) => Box::new(NegateCondition),
                    Slot::Rule(Rule::ReturnBoolFlip) => Box::new(ReturnBoolFlip),
                    Slot::Rule(Rule::BodyDefault) => Box::new(BodyDefault),
                    Slot::Rule(Rule::FnValue) => Box::new(FnValue),
                    Slot::Rule(Rule::DeleteMatchArm) => Box::new(DeleteArm),
                    Slot::Rule(Rule::DeleteStructField) => Box::new(DeleteField),
                    Slot::Rule(
                        Rule::ReplaceToken { .. }
                        | Rule::DeleteUnary { .. }
                        | Rule::MatchGuard { .. },
                    ) => {
                        unreachable!("parameterized rules are grouped by family")
                    }
                }
            })
            .map(|op| -> Box<dyn Operator> {
                if self.skips.is_empty() {
                    op
                } else {
                    Box::new(Honouring {
                        inner: op,
                        skips: self.skips,
                    })
                }
            })
            .collect()
    }
}

/// The operator for a family of parameterized rules (all of one kind).
fn family_operator(family: &'static str, rules: &[Rule]) -> Box<dyn Operator> {
    match rules[0] {
        Rule::DeleteUnary { .. } => Box::new(DeleteUnary::new(
            family,
            rules
                .iter()
                .filter_map(|r| match r {
                    Rule::DeleteUnary { token } => Some(*token),
                    _ => None,
                })
                .collect(),
        )),
        Rule::MatchGuard { .. } => Box::new(MatchGuard::new(
            family,
            rules
                .iter()
                .filter_map(|r| match r {
                    Rule::MatchGuard { value } => Some(*value),
                    _ => None,
                })
                .collect(),
        )),
        other => unreachable!("{other:?} is not a parameterized rule"),
    }
}

/// Token rules apply to binary operators and compound assignments alike;
/// each token occurs in only one of them.
const BINARY: &[Concept] = &[Concept::Binary, Concept::CompoundAssign];

/// Every named set; the first is the default.
pub fn registry() -> Vec<OperatorSet> {
    vec![breadth(), cargo_mutants(), sufficient()]
}

/// The `breadth` set: one mutation per action kind.
fn breadth() -> OperatorSet {
    OperatorSet {
        name: "breadth",
        summary: "the six prototype operators: one per action kind (design section 4.9)",
        source: "docs/design.md section 4.9",
        mutations: &[
            "delete-statement/call",
            "aor/add->sub",
            "aor/sub->add",
            "aor/mul->div",
            "aor/div->mul",
            "ror/lt->le",
            "ror/le->lt",
            "ror/gt->ge",
            "ror/ge->gt",
            "negate-condition",
            "return-bool-flip",
            "body-default",
        ],
        pending: &[],
        skips: &[],
    }
}

/// The `cargo-mutants` set: what cargo-mutants generates.
fn cargo_mutants() -> OperatorSet {
    OperatorSet {
        name: "cargo-mutants",
        summary: "what cargo-mutants generates, for a mutant-by-mutant comparison",
        source: "cargo-mutants 26.2.0",
        mutations: &[
            "ror/eq->ne",
            "ror/ne->eq",
            "ror/lt->eq",
            "ror/lt->gt",
            "ror/lt->le",
            "ror/gt->eq",
            "ror/gt->lt",
            "ror/gt->ge",
            "ror/le->gt",
            "ror/ge->lt",
            "lcr/and->or",
            "lcr/or->and",
            "aor/add->sub",
            "aor/add->mul",
            "aor/sub->add",
            "aor/sub->div",
            "aor/mul->add",
            "aor/mul->div",
            "aor/div->rem",
            "aor/div->mul",
            "aor/rem->div",
            "aor/rem->add",
            "shift/shl->shr",
            "shift/shr->shl",
            "bitwise/and->or",
            "bitwise/and->xor",
            "bitwise/or->and",
            "bitwise/or->xor",
            "bitwise/xor->or",
            "bitwise/xor->and",
            "aor/add_assign->sub_assign",
            "aor/add_assign->mul_assign",
            "aor/sub_assign->add_assign",
            "aor/sub_assign->div_assign",
            "aor/mul_assign->add_assign",
            "aor/mul_assign->div_assign",
            "aor/div_assign->rem_assign",
            "aor/div_assign->mul_assign",
            "aor/rem_assign->div_assign",
            "aor/rem_assign->add_assign",
            "shift/shl_assign->shr_assign",
            "shift/shr_assign->shl_assign",
            "bitwise/and_assign->or_assign",
            "bitwise/or_assign->and_assign",
            "bitwise/xor_assign->or_assign",
            "bitwise/xor_assign->and_assign",
            "unary/delete-not",
            "unary/delete-neg",
            "match-arm/delete",
            "match-guard/true",
            "match-guard/false",
            "struct-field/delete",
            "fn-value",
        ],
        pending: &[
            "files: only those in a package's module tree, not behind #[cfg(test)] mod",
            "`&&` inside let chains (cargo-mutants swaps it; the mutant never compiles)",
        ],
        skips: &SkipKind::ALL,
    }
}

/// The `sufficient` set: non-redundant sufficient sets plus what real faults need.
fn sufficient() -> OperatorSet {
    OperatorSet {
        name: "sufficient",
        summary: "non-redundant sufficient sets plus operators real faults need",
        source: "docs/operator-sets.md (Kaminski 2013, Just & Schweiggert 2014, Just et al. 2014, mutest-rs 2026)",
        mutations: &[
            "ror/lt->le",
            "ror/lt->ne",
            "ror/gt->ge",
            "ror/gt->ne",
            "ror/le->lt",
            "ror/le->eq",
            "ror/ge->gt",
            "ror/ge->eq",
            "ror/eq->le",
            "ror/eq->ge",
            "ror/ne->lt",
            "ror/ne->gt",
            "cor/and->eq",
            "cor/or->ne",
            "aor/add->sub",
            "aor/sub->add",
            "aor/mul->div",
            "aor/div->mul",
            "aor/rem->div",
            "shift/shl->shr",
            "shift/shr->shl",
            "bitwise/and->or",
            "bitwise/or->and",
            "bitwise/xor->and",
            "aor/add_assign->sub_assign",
            "aor/sub_assign->add_assign",
            "aor/mul_assign->div_assign",
            "aor/div_assign->mul_assign",
            "aor/rem_assign->div_assign",
            "shift/shl_assign->shr_assign",
            "shift/shr_assign->shl_assign",
            "bitwise/and_assign->or_assign",
            "bitwise/or_assign->and_assign",
            "bitwise/xor_assign->and_assign",
            "delete-statement/call",
            "fn-value",
        ],
        pending: &[
            "ror: predicate -> true/false",
            "ror: type-specific limits (bool, floats)",
            "cor: lhs, rhs, true/false",
            "unary operator deletion",
            "statement deletion beyond call statements (assignments, return, break, continue)",
            "break <-> continue",
            "range bound .. <-> ..=",
            "call result -> Default::default()",
            "match arm deletion, match guards, struct field deletion",
        ],
        skips: &[SkipKind::Test, SkipKind::Annotated],
    }
}

/// The set called `name`.
pub fn named(name: &str) -> Option<OperatorSet> {
    registry().into_iter().find(|s| s.name == name)
}
