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
pub use crate::set_spec::{CustomSet, SetError, custom_set, resolve_sets};
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
    // ror/lt->le: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived from Cherenkov design; same as PIT CONDITIONALS_BOUNDARY; same as mutmut; same as go-mutesting; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/lt->eq: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/lt->eq",
        "ror",
        Lt,
        Eq,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/lt->gt: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/lt->gt",
        "ror",
        Lt,
        Gt,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/lt->ne: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/lt->ne",
        "ror",
        Lt,
        Ne,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/le->lt: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived from Cherenkov design; same as PIT CONDITIONALS_BOUNDARY; same as mutmut; same as go-mutesting; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/le->gt: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/le->gt",
        "ror",
        Le,
        Gt,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/le->eq: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/le->eq",
        "ror",
        Le,
        Eq,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/gt->ge: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived from Cherenkov design; same as PIT CONDITIONALS_BOUNDARY; same as mutmut; same as go-mutesting; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/gt->eq: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/gt->eq",
        "ror",
        Gt,
        Eq,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/gt->lt: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/gt->lt",
        "ror",
        Gt,
        Lt,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/gt->ne: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/gt->ne",
        "ror",
        Gt,
        Ne,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ge->gt: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived from Cherenkov design; same as PIT CONDITIONALS_BOUNDARY; same as mutmut; same as go-mutesting; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ge->lt: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra ROR.
    tok(
        "ror/ge->lt",
        "ror",
        Ge,
        Lt,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ge->eq: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/ge->eq",
        "ror",
        Ge,
        Eq,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/eq->ne: derived from cargo-mutants (visit.rs binary-operator replacement table); same as PIT NEGATE_CONDITIONALS; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/eq->le: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/eq->le",
        "ror",
        Eq,
        Le,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/eq->ge: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/eq->ge",
        "ror",
        Eq,
        Ge,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ne->eq: derived from cargo-mutants (visit.rs binary-operator replacement table); same as PIT NEGATE_CONDITIONALS; same as Stryker; same as Gremlins; same as Mull; same as Mothra ROR.
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
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ne->lt: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/ne->lt",
        "ror",
        Ne,
        Lt,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // ror/ne->gt: derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same as Mothra ROR.
    tok(
        "ror/ne->gt",
        "ror",
        Ne,
        Gt,
        &[
            Origin {
                source: "Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("ROR"),
                relation: Relation::Same,
            },
        ],
    ),
    // lcr/and->or: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra LCR.
    tok(
        "lcr/and->or",
        "lcr",
        And,
        Or,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("LCR"),
                relation: Relation::Same,
            },
        ],
    ),
    // lcr/or->and: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra LCR.
    tok(
        "lcr/or->and",
        "lcr",
        Or,
        And,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("LCR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/add->sub: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Dextool AORS; derived from Cherenkov design; same as Mothra AOR.
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/add->mul: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra AOR.
    tok(
        "aor/add->mul",
        "aor",
        Add,
        Mul,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/sub->add: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Dextool AORS; derived from Cherenkov design; same as Mothra AOR.
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/sub->div: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra AOR.
    tok(
        "aor/sub->div",
        "aor",
        Sub,
        Div,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/mul->add: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra AOR.
    tok(
        "aor/mul->add",
        "aor",
        Mul,
        Add,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/mul->div: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Dextool AORS; derived from Cherenkov design; same as Mothra AOR.
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/div->mul: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Dextool AORS; derived from Cherenkov design; same as Mothra AOR.
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/div->rem: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra AOR.
    tok(
        "aor/div->rem",
        "aor",
        Div,
        Rem,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/rem->div: derived from cargo-mutants (visit.rs binary-operator replacement table); derived from Dextool AORS; same as Mothra AOR.
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // aor/rem->add: derived from cargo-mutants (visit.rs binary-operator replacement table); same as Mothra AOR.
    tok(
        "aor/rem->add",
        "aor",
        Rem,
        Add,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // shift/shl->shr: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // shift/shr->shl: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/and->or: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/and->xor: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/or->and: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/or->xor: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/xor->or: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // bitwise/xor->and: derived from cargo-mutants (visit.rs binary-operator replacement table).
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
    // compound-assign/add->sub: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); derived from Dextool AORS; same as Mothra AOR.
    tok(
        "compound-assign/add->sub",
        "compound-assign",
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/add->mul: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); same as Mothra AOR.
    tok(
        "compound-assign/add->mul",
        "compound-assign",
        AddAssign,
        MulAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/sub->add: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); derived from Dextool AORS; same as Mothra AOR.
    tok(
        "compound-assign/sub->add",
        "compound-assign",
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/sub->div: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); same as Mothra AOR.
    tok(
        "compound-assign/sub->div",
        "compound-assign",
        SubAssign,
        DivAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/mul->add: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); same as Mothra AOR.
    tok(
        "compound-assign/mul->add",
        "compound-assign",
        MulAssign,
        AddAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/mul->div: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); derived from Dextool AORS; same as Mothra AOR.
    tok(
        "compound-assign/mul->div",
        "compound-assign",
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/div->rem: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); same as Mothra AOR.
    tok(
        "compound-assign/div->rem",
        "compound-assign",
        DivAssign,
        RemAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/div->mul: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); derived from Dextool AORS; same as Mothra AOR.
    tok(
        "compound-assign/div->mul",
        "compound-assign",
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/rem->div: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); derived from Dextool AORS; same as Mothra AOR.
    tok(
        "compound-assign/rem->div",
        "compound-assign",
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
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/rem->add: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms); same as Mothra AOR.
    tok(
        "compound-assign/rem->add",
        "compound-assign",
        RemAssign,
        AddAssign,
        &[
            Origin {
                source: "cargo-mutants",
                name: None,
                relation: Relation::Derived,
            },
            Origin {
                source: "Mothra",
                name: Some("AOR"),
                relation: Relation::Same,
            },
        ],
    ),
    // compound-assign/shl->shr: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/shl->shr",
        "compound-assign",
        ShlAssign,
        ShrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // compound-assign/shr->shl: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/shr->shl",
        "compound-assign",
        ShrAssign,
        ShlAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // compound-assign/and->or: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/and->or",
        "compound-assign",
        BitAndAssign,
        BitOrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // compound-assign/or->and: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/or->and",
        "compound-assign",
        BitOrAssign,
        BitAndAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // compound-assign/xor->or: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/xor->or",
        "compound-assign",
        BitXorAssign,
        BitOrAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // compound-assign/xor->and: derived from cargo-mutants (visit.rs binary-operator replacement table, assignment forms).
    tok(
        "compound-assign/xor->and",
        "compound-assign",
        BitXorAssign,
        BitAndAssign,
        &[Origin {
            source: "cargo-mutants",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // lcr/and->eq: derived from Just & Schweiggert 2014.
    tok(
        "lcr/and->eq",
        "lcr",
        And,
        Eq,
        &[Origin {
            source: "Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // lcr/or->ne: derived from Just & Schweiggert 2014.
    tok(
        "lcr/or->ne",
        "lcr",
        Or,
        Ne,
        &[Origin {
            source: "Just & Schweiggert 2014",
            name: None,
            relation: Relation::Derived,
        }],
    ),
    // delete-statement/call: derived from Cherenkov design; derived from Just et al. 2014; similar to PIT VOID_METHOD_CALLS (removes calls to void methods only); similar to Mothra SDL (deletes call statements only, not every statement).
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
            Origin {
                source: "Mothra",
                name: Some("SDL"),
                relation: Relation::Similar("deletes call statements only, not every statement"),
            },
        ],
    },
    // negate-condition: derived from Cherenkov design; similar to PIT NEGATE_CONDITIONALS (negates the relational operator; this negates the whole condition).
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
    // return-bool-flip: derived from Cherenkov design.
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
    // body-default: derived from Cherenkov design; similar to cargo-mutants FnValue (only the profile's zero value, not cargo-mutants' per-type table).
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
    // fn-value: derived from cargo-mutants FnValue (fnvalue.rs return-value table).
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
    // match-arm/delete: derived from cargo-mutants MatchArm (visit.rs match-arm deletion).
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
    // match-guard/true: derived from cargo-mutants MatchArmGuard (visit.rs match-guard replacement).
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
    // match-guard/false: derived from cargo-mutants MatchArmGuard (visit.rs match-guard replacement).
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
    // struct-field/delete: derived from cargo-mutants StructField (visit.rs struct-field deletion).
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
    // unary/delete-not: derived from cargo-mutants UnaryOperator (visit.rs unary-operator deletion); same as cosmic-ray; same as Dextool UOI.
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
    // unary/delete-neg: derived from cargo-mutants UnaryOperator (visit.rs unary-operator deletion).
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

/// An operator: one family of replacements, defined without reference to
/// a language. A set gives each family it uses a table of replacements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyDef {
    /// The operator id in mutant IDs, and the head of its replacements' names.
    pub id: &'static str,
    /// What the operator does, in terms of the concept it works on.
    pub definition: &'static str,
}

const fn def(id: &'static str, definition: &'static str) -> FamilyDef {
    FamilyDef { id, definition }
}

/// Every operator. The sources of each family are those of its replacements
/// (their comments in `MUTATIONS` and their `origins`):
///
/// - `ror`: Mothra ROR; cargo-mutants visit.rs; PIT CONDITIONALS_BOUNDARY;
///   Kaminski et al. 2013 and Just & Schweiggert 2014 (sufficient sets).
/// - `aor`: Mothra AOR; cargo-mutants visit.rs; Dextool AORS.
/// - `bitwise`, `shift`: cargo-mutants visit.rs; PIT, mutmut, Gremlins.
/// - `lcr`: Mothra LCR; cargo-mutants visit.rs; Just & Schweiggert 2014 (COR).
/// - `compound-assign`: cargo-mutants visit.rs assignment forms; Cherenkov design.
/// - `unary`, `match-arm`, `match-guard`, `struct-field`, `fn-value`:
///   cargo-mutants visit.rs and fnvalue.rs.
/// - `delete-statement`: Just et al. 2014; Mothra SDL; PIT VOID_METHOD_CALLS.
/// - `negate-condition`, `return-bool-flip`, `body-default`: Cherenkov design.
static FAMILIES: &[FamilyDef] = &[
    def(
        "ror",
        "Replace a relational operator with another relational operator.",
    ),
    def(
        "aor",
        "Replace an arithmetic operator with another arithmetic operator.",
    ),
    def(
        "bitwise",
        "Replace a bitwise operator with another bitwise operator.",
    ),
    def("shift", "Replace a shift operator with the opposite shift."),
    def(
        "lcr",
        "Replace a logical connector with the other connector, or with an equality test of its operands.",
    ),
    def(
        "compound-assign",
        "Replace the operator of a compound assignment with another operator of its kind.",
    ),
    def("unary", "Delete a unary operator, keeping its operand."),
    def(
        "match-guard",
        "Replace a match arm's guard with a constant boolean.",
    ),
    def(
        "match-arm",
        "Delete a match arm, when the match has a catch-all arm.",
    ),
    def(
        "struct-field",
        "Delete a field of a struct literal whose omitted fields take a default.",
    ),
    def(
        "fn-value",
        "Replace a function body with each value the language profile generates for its result type.",
    ),
    def("delete-statement", "Delete a call statement."),
    def("negate-condition", "Negate the condition of an if or loop."),
    def(
        "return-bool-flip",
        "Replace a returned boolean literal with its opposite.",
    ),
    def(
        "body-default",
        "Replace a function body with the zero value of its result type.",
    ),
];

/// An operator with every replacement any set gives it.
#[derive(Debug, Clone)]
pub struct Family {
    pub id: &'static str,
    pub definition: &'static str,
    pub replacements: Vec<&'static Mutation>,
}

/// The operators, each with the replacements of all sets: the inventory.
pub fn families() -> Vec<Family> {
    FAMILIES
        .iter()
        .map(|f| Family {
            id: f.id,
            definition: f.definition,
            replacements: MUTATIONS.iter().filter(|m| m.family == f.id).collect(),
        })
        .collect()
}

/// An operator as one set uses it: that set's replacements of the family.
#[derive(Debug, Clone)]
pub struct FamilyTable {
    pub family: &'static str,
    pub replacements: Vec<&'static Mutation>,
}

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
    skips: Vec<SkipKind>,
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
        &self.skips
    }
}

impl OperatorSet {
    /// The set's operators, each with its table of replacements, families in
    /// order of first appearance in the set.
    pub fn tables(&self) -> Vec<FamilyTable> {
        let mut tables: Vec<FamilyTable> = Vec::new();
        for name in self.mutations {
            let m = MUTATIONS
                .iter()
                .find(|m| m.name == *name)
                .unwrap_or_else(|| panic!("set {}: no mutation named {name:?}", self.name));
            match tables.iter_mut().find(|t| t.family == m.family) {
                Some(table) => table.replacements.push(m),
                None => tables.push(FamilyTable {
                    family: m.family,
                    replacements: vec![m],
                }),
            }
        }
        tables
    }

    /// The set's operators, declining test code too unless `include_tests`:
    /// cargo-mutants leaves test code alone, and so does every set by default.
    pub fn operators_for(&self, include_tests: bool) -> Vec<Box<dyn Operator>> {
        let mut skips = self.skips.to_vec();
        if !include_tests && !skips.contains(&SkipKind::Test) {
            skips.push(SkipKind::Test);
        }
        self.operators_skipping(&skips)
    }

    /// The operators that carry out this set's mutations: one per family,
    /// in order of first appearance. Panics on a name not in the superset,
    /// which is a mistake in a hard-coded set.
    pub fn operators(&self) -> Vec<Box<dyn Operator>> {
        self.operators_skipping(self.skips)
    }

    fn operators_skipping(&self, skips: &[SkipKind]) -> Vec<Box<dyn Operator>> {
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
                if skips.is_empty() {
                    op
                } else {
                    Box::new(Honouring {
                        inner: op,
                        skips: skips.to_vec(),
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
            "compound-assign/add->sub",
            "compound-assign/add->mul",
            "compound-assign/sub->add",
            "compound-assign/sub->div",
            "compound-assign/mul->add",
            "compound-assign/mul->div",
            "compound-assign/div->rem",
            "compound-assign/div->mul",
            "compound-assign/rem->div",
            "compound-assign/rem->add",
            "compound-assign/shl->shr",
            "compound-assign/shr->shl",
            "compound-assign/and->or",
            "compound-assign/or->and",
            "compound-assign/xor->or",
            "compound-assign/xor->and",
            "unary/delete-not",
            "unary/delete-neg",
            "match-arm/delete",
            "match-guard/true",
            "match-guard/false",
            "struct-field/delete",
            "fn-value",
        ],
        pending: &[
            "files: only those in a package's module tree (files behind #[cfg(test)] mod are skipped by default; orphan files are still planned)",
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
            "lcr/and->eq",
            "lcr/or->ne",
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
            "compound-assign/add->sub",
            "compound-assign/sub->add",
            "compound-assign/mul->div",
            "compound-assign/div->mul",
            "compound-assign/rem->div",
            "compound-assign/shl->shr",
            "compound-assign/shr->shl",
            "compound-assign/and->or",
            "compound-assign/or->and",
            "compound-assign/xor->and",
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
