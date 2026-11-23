//! Tier P: derive a one-time instruction patch from a source-level mutant
//! plan and the instructions of its function (design section 8). Pure: the
//! driver supplies the instructions, the ISA table supplies the rules.
//! A plan that cannot be derived this way is never patched.

use cherenkov_core::edit::{Edit, Lit};
use cherenkov_core::plan::{Action, MutantPlan, Target};
use cherenkov_core::syntax::{Pos, Span};
use cherenkov_core::token::Token;

use crate::isa::{Isa, Jcc, Matched, Signedness};
use crate::shapes::{Check, Kind, Shape};
use crate::x86;

mod column;
mod compare;

/// Every shape of comparison patch the rules can make.
pub(crate) fn compare_universe(isa: &Isa) -> Vec<Shape> {
    compare::universe(isa)
}

use column::{Shaped, arithmetic_at, is_rust, memory_source, shape_of};

/// One instruction of the function under mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    pub pc: u64,
    pub bytes: Vec<u8>,
    pub file: String,
    pub line: u32,
    /// The line-table row's column (1-based), 0 when unknown.
    pub column: u32,
}

/// One mutant's machine-code change: one or more byte ranges, applied and
/// removed together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub addr: u64,
    pub original: Vec<u8>,
    pub replacement: Vec<u8>,
}

impl Patch {
    fn single(addr: u64, original: Vec<u8>, replacement: Vec<u8>) -> Patch {
        Patch {
            regions: vec![Region {
                addr,
                original,
                replacement,
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpatchable {
    /// No instruction comes from that source position (line, column).
    NoCodeAt {
        line: u32,
        col: u32,
    },
    /// The operator/action pair has no tier P rule yet.
    UnsupportedEdit(String),
    /// A condition must compile to exactly one conditional jump on its line.
    ConditionalJumps {
        line: u32,
        found: usize,
    },
    /// The jump's destination is not an instruction of this function.
    UnknownDestination(u64),
    /// The jump lands on the condition's own line, so its polarity is unknown.
    AmbiguousPolarity {
        line: u32,
    },
    /// The condition code matches neither `lhs op rhs` nor its swapped form.
    ConditionMismatch {
        cc: String,
        token: String,
    },
    NoInstructions {
        line: u32,
    },
    /// A call statement whose instructions contain no call.
    NoCall {
        line: u32,
    },
    /// An arithmetic swap needs exactly one add/sub-family instruction of
    /// the source operator on the statement's lines.
    ArithmeticInstructions {
        line: u32,
        found: usize,
    },
    /// A return flip found no store of the returned boolean constant.
    NoConstant {
        line: u32,
    },
    /// The result type has no known zero value (it may not be returned in registers).
    NoDefault,
    /// The value is not the result type's all-zero bits, which is all a
    /// return stub can produce.
    NonZeroValue(String),
    /// The ISA table has no return stub for this debugger's calling convention.
    NoReturnStub,
    /// A jump in the function lands inside the bytes a return stub replaces.
    JumpIntoStub(u64),
    /// The function is shorter than the return stub.
    FunctionTooShort {
        bytes: usize,
        stub: usize,
    },
    /// The statement is last in the function; there is nothing to jump to.
    NoResumePoint {
        line: u32,
    },
    /// No jump encoding fits in the statement's bytes.
    TooShort {
        line: u32,
        bytes: usize,
    },
}

impl std::fmt::Display for Unpatchable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unpatchable::UnsupportedEdit(what) => write!(f, "no tier P rule for {what}"),
            Unpatchable::NoCodeAt { line, col } => write!(f, "no code at {line}:{col}"),
            Unpatchable::ConditionalJumps { line, found } => {
                write!(
                    f,
                    "expected one conditional jump on line {line}, found {found}"
                )
            }
            Unpatchable::UnknownDestination(pc) => {
                write!(f, "jump destination {pc:#x} is outside the function")
            }
            Unpatchable::AmbiguousPolarity { line } => {
                write!(f, "conditional jump on line {line} lands on its own line")
            }
            Unpatchable::ConditionMismatch { cc, token } => {
                write!(f, "condition code {cc} does not implement `{token}`")
            }
            Unpatchable::NoInstructions { line } => write!(f, "no instructions for line {line}"),
            Unpatchable::ArithmeticInstructions { line, found } => write!(
                f,
                "expected one add/sub instruction of the source operator on line {line}, found {found}"
            ),
            Unpatchable::NoConstant { line } => {
                write!(f, "no store of the returned boolean on line {line}")
            }
            Unpatchable::NoDefault => write!(f, "the result type has no known zero value"),
            Unpatchable::NonZeroValue(v) => write!(f, "a return stub cannot produce the value {v}"),
            Unpatchable::NoReturnStub => write!(f, "no return stub for this calling convention"),
            Unpatchable::JumpIntoStub(pc) => {
                write!(f, "a jump lands at {pc:#x}, inside the return stub")
            }
            Unpatchable::FunctionTooShort { bytes, stub } => {
                write!(
                    f,
                    "function is {bytes} bytes, shorter than the {stub}-byte return stub"
                )
            }
            Unpatchable::NoCall { line } => {
                write!(f, "statement on line {line} contains no call instruction")
            }
            Unpatchable::NoResumePoint { line } => {
                write!(f, "nothing follows the statement on line {line}")
            }
            Unpatchable::TooShort { line, bytes } => {
                write!(
                    f,
                    "statement on line {line} is {bytes} bytes, too short for a jump"
                )
            }
        }
    }
}

impl std::error::Error for Unpatchable {}

/// Derive the patch for `plan`. `file` is the path the instructions report
/// for the plan's source file; instructions from other files are ignored.
pub fn derive(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    derive_shaped(plan, file, insns, isa).map(|(patch, _)| patch)
}

/// [`derive`], with the shape of the patch where it is an arithmetic one
/// (`isa/tierp-shapes.toml`).
pub fn derive_shaped(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
    isa: &Isa,
) -> Result<(Patch, Option<Shape>), Unpatchable> {
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    if let Some(done) = arithmetic_route(plan, &ours, isa) {
        return done.map(|s| (s.patch, Some(s.shape)));
    }
    if let Some(done) = comparison_route(plan, &ours, isa) {
        return done.map(|s| (s.patch, Some(s.shape)));
    }
    derive_other(plan, file, insns, isa).map(|patch| (patch, None))
}

/// An operator replacement that swaps arithmetic: found by the column
/// (inside a call argument, a field, a method chain, a returned value, a
/// condition), or in a statement by its line first.
fn arithmetic_route(
    plan: &MutantPlan,
    ours: &[&Insn],
    isa: &Isa,
) -> Option<Result<Shaped, Unpatchable>> {
    match (&plan.action, &plan.edit) {
        (
            Action::EvalThenJump {
                resume: Target::AfterStatement(stmt),
                ..
            },
            Edit::ReplaceToken { from, with, .. },
        ) => Some(arithmetic_swap(plan, stmt, *from, *with, ours, isa)),
        (_, Edit::ReplaceToken { from, with, .. }) if is_arithmetic(*from) => {
            Some(arithmetic_at(plan, *from, *with, ours, isa))
        }
        _ => None,
    }
}

/// A comparison operator replacement, found by the column of its operands.
/// `None` where its code is not found that way: another rule may place it.
fn comparison_route(
    plan: &MutantPlan,
    ours: &[&Insn],
    isa: &Isa,
) -> Option<Result<Shaped, Unpatchable>> {
    let Edit::ReplaceToken { from, with, .. } = &plan.edit else {
        return None;
    };
    if !(compare::is_comparison(*from) && compare::is_comparison(*with)) {
        return None;
    }
    match compare::comparison_at(plan, *from, *with, ours, isa) {
        Ok(shaped) => Some(Ok(shaped)),
        Err(compare::Refused::Elsewhere) => None,
        Err(compare::Refused::Declined(why)) => Some(Err(why)),
    }
}

fn derive_other(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    // A context's own rule first; negating a computed boolean works in any
    // context, where the code that consumes the value can be patched.
    derive_in_context(plan, file, insns, isa).or_else(|e| {
        if negatable(plan) {
            // Where the context has no rule of its own, the negation's reason
            // is the informative one.
            negate_value(plan, file, insns).map_err(|n| match (&e, &n) {
                (_, Unpatchable::NoCodeAt { .. }) | (Unpatchable::UnsupportedEdit(_), _) => n,
                _ => e,
            })
        } else {
            Err(e)
        }
    })
}

/// Mutants that negate a boolean the program computes: `==` <-> `!=`, and
/// deleting a `!`.
pub fn negatable(plan: &MutantPlan) -> bool {
    matches!(
        plan.edit,
        Edit::ReplaceToken {
            from: Token::Eq,
            with: Token::Ne,
            ..
        } | Edit::ReplaceToken {
            from: Token::Ne,
            with: Token::Eq,
            ..
        } | Edit::DeleteToken {
            token: Token::Not,
            ..
        }
    )
}

/// Negate the value of the plan's expression where it is consumed: a `!`'s
/// own `xor $1` back to the `and $1` that normalises a bool; else the first
/// instruction after the expression's code: `test $1` and a conditional
/// jump (the jump inverted), `and $1` <-> `xor $1`, or `setcc` (inverted).
fn negate_value(plan: &MutantPlan, file: &str, insns: &[Insn]) -> Result<Patch, Unpatchable> {
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    let (from, to) = match (&plan.edit, &plan.operands[..]) {
        (Edit::ReplaceToken { .. }, [lhs, rhs]) => (lhs.start, rhs.end),
        (Edit::DeleteToken { .. }, [operand]) => (plan.site.start, operand.end),
        _ => {
            return Err(Unpatchable::UnsupportedEdit(
                "negation without operands".into(),
            ));
        }
    };
    let span = ((from.line, from.col), (to.line, to.col));
    let region: Vec<usize> = (0..ours.len())
        .filter(|&k| {
            ours[k].column != 0 && (span.0..span.1).contains(&(ours[k].line, ours[k].column))
        })
        .collect();
    let Some(&last) = region.iter().max_by_key(|&&k| ours[k].pc) else {
        return Err(Unpatchable::NoCodeAt {
            line: from.line,
            col: from.col,
        });
    };
    let decoded = |insn: &Insn| x86::decode(insn.pc, &insn.bytes);
    let single =
        |insn: &Insn, bytes: Vec<u8>| Ok(Patch::single(insn.pc, insn.bytes.clone(), bytes));
    // Deleting a `!`: its own negation (`xor al, 1` or `0xff`) becomes the
    // plain normalisation (`and al, 1`).
    if matches!(plan.edit, Edit::DeleteToken { .. }) {
        // A bool's `!`: `xor al, 1`, or `xor al, 0xff` normalised by the
        // `and al, 1` after it. Without one, `xor al, 0xff` is an
        // integer's bitwise not, which deleting `!` does not undo this way.
        let bool_not = |k: usize| {
            let undone = decoded(ours[k]).and_then(|d| d.undone_not())?;
            let normalised = ours[k].bytes == [0x34, 0x01]
                || ours
                    .get(k + 1)
                    .is_some_and(|n| n.pc == ours[k].pc + 2 && n.bytes == [0x24, 0x01]);
            normalised.then_some(undone)
        };
        // The deleted `!`'s own: positioned at it, else the first.
        let at_not = |k: &&usize| (ours[**k].line, ours[**k].column) == (from.line, from.col);
        let candidates: Vec<usize> = region
            .iter()
            .copied()
            .filter(|&k| bool_not(k).is_some())
            .collect();
        if let Some(&k) = candidates.iter().find(at_not).or(candidates.first())
            && let Some(b) = bool_not(k)
        {
            return single(ours[k], b);
        }
    }
    // The bool tested and branched on inside the expression's own code
    // (rustc folds `!` into the branch it decides): that jump inverted. The
    // bool is one the expression stored (`matches!`), tested in memory; a
    // call's result tested in `al` may be only the last operand's
    // (`!(p() && q())`), not the whole expression's.
    if let [.., test, jump] = &region[..]
        && *jump == last
        && decoded(ours[*test]).is_some_and(|d| d.tests_low_bit_in_memory())
        && let Some(b) = decoded(ours[*jump]).and_then(|d| d.inverted_jump())
    {
        return single(ours[*jump], b);
    }
    let Some(next) = ours
        .get(last + 1)
        .copied()
        .filter(|i| i.pc == ours[last].pc + ours[last].bytes.len() as u64)
    else {
        return Err(Unpatchable::UnsupportedEdit(
            "no code after the expression".into(),
        ));
    };
    let Some(consumer) = decoded(next) else {
        return Err(Unpatchable::UnsupportedEdit(
            "undecodable code after the expression".into(),
        ));
    };
    // `test al, 1` then a conditional jump: the jump inverted.
    if consumer.reads_bool_al()
        && consumer.negated_bool().is_none()
        && let Some(jump) = ours.get(last + 2).copied()
        && let Some(b) = decoded(jump).and_then(|d| d.inverted_jump())
    {
        return single(jump, b);
    }
    // `and al, 1` / `xor al, 1` / `setcc`: its negated form.
    if let Some(b) = consumer.negated_bool() {
        return single(next, b);
    }
    // A conditional jump on flags the expression set.
    if let Some(b) = consumer.inverted_jump() {
        return single(next, b);
    }
    Err(Unpatchable::UnsupportedEdit(format!(
        "negation consumed by {:02x?}",
        next.bytes
    )))
}

fn derive_in_context(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    match (&plan.action, &plan.edit) {
        (Action::Redirect { from, to }, Edit::DeleteMatch) => redirect_arm(from, to, &ours, isa),
        (
            Action::EvalBranch {
                if_true: Target::BlockStart(then),
                ..
            },
            Edit::ReplaceWithLiteral {
                lit: Lit::Bool(value),
                ..
            },
        ) => branch_to_constant(plan, then, *value, &ours, isa),
        (
            Action::EvalBranch {
                if_true: Target::BlockStart(then),
                ..
            },
            Edit::WrapNot { .. },
        ) => {
            let site = branch_site(&plan.stop_at, then, guard_arm(plan), &ours, isa)?;
            let cc = isa.inverse(site.jcc.cc);
            Ok(site.patch(isa, cc))
        }
        (
            Action::EvalBranch {
                if_true: Target::BlockStart(then),
                ..
            },
            Edit::ReplaceToken { from, with, .. },
        ) => branch_token_swap(plan, then, *from, *with, &ours, isa),
        (
            Action::JumpOnly {
                resume: Target::AfterStatement(stmt),
            },
            Edit::DeleteStatement,
        ) => delete_statement(plan.stop_at.line, stmt.end.line, &ours, isa),
        (
            Action::EvalReturn { .. },
            Edit::ReplaceWithLiteral {
                lit: Lit::Bool(new),
                ..
            },
        ) => return_flip(plan.stop_at.line, !*new, &ours, isa),
        (
            Action::ReturnDefault {
                value_text,
                zero_bits,
                ..
            },
            _,
        ) => return_default(value_text.as_deref(), *zero_bits, &ours, isa),
        // A comparison whose result is a value: the `setcc` that makes the
        // bool, its condition code swapped as a branch's would be.
        (_, Edit::ReplaceToken { from, with, .. })
            if [Signedness::Signed, Signedness::Unsigned]
                .into_iter()
                .any(|s| isa.relational(s, *from).is_some()) =>
        {
            compared_value(plan, *from, *with, &ours, isa)
        }
        (action, _) => Err(Unpatchable::UnsupportedEdit(format!(
            "{} {}",
            plan.operator,
            action.kind()
        ))),
    }
}

/// An arithmetic or bitwise operator, plain or compound.
fn is_arithmetic(t: Token) -> bool {
    let base = t.compound_base().unwrap_or(t);
    matches!(
        base,
        Token::Add
            | Token::Sub
            | Token::Mul
            | Token::Div
            | Token::BitAnd
            | Token::BitOr
            | Token::BitXor
    )
}

/// A deleted match arm: its body's first instructions become a jump to the
/// catch-all arm's body (positions need the line table's columns; arms
/// share lines with their patterns).
fn redirect_arm(from: &Span, to: &Span, ours: &[&Insn], isa: &Isa) -> Result<Patch, Unpatchable> {
    let at = |pos: &Pos| {
        ours.iter()
            .copied()
            .filter(|i| i.column != 0 && (i.line, i.column) == (pos.line, pos.col))
            .min_by_key(|i| i.pc)
    };
    let none = |pos: &Pos| Unpatchable::NoCodeAt {
        line: pos.line,
        col: pos.col,
    };
    let start = at(&from.start).ok_or_else(|| none(&from.start))?;
    let target = at(&to.start).ok_or_else(|| none(&to.start))?;
    // Room: the run of instructions from the body's start in its row.
    let mut original = Vec::new();
    for i in ours.iter().filter(|i| i.pc >= start.pc) {
        if i.pc != start.pc + original.len() as u64
            || (i.line, i.column) != (start.line, start.column)
        {
            break;
        }
        original.extend_from_slice(&i.bytes);
    }
    let mut bytes = isa
        .encode_jmp(start.pc, target.pc, original.len())
        .ok_or(Unpatchable::UnknownDestination(target.pc))?;
    // Past the jump, the body's old bytes are never executed.
    bytes.extend_from_slice(&original[bytes.len()..]);
    Ok(Patch::single(start.pc, original, bytes))
}

/// A condition replaced by the constant `value`: the jump always or never
/// goes its way (an unconditional jump to its destination, or to the next
/// instruction), padded to the jump's own length.
fn branch_to_constant(
    plan: &MutantPlan,
    then: &Span,
    value: bool,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let site = branch_site(&plan.stop_at, then, guard_arm(plan), ours, isa)?;
    let len = site.jcc.len;
    let pc = site.insn.pc;
    let dest = (pc + len as u64).wrapping_add_signed(site.jcc.rel);
    let take = value == site.taken_when_true;
    let to = if take { dest } else { pc + len as u64 };
    let mut bytes = isa
        .encode_jmp(pc, to, len)
        .ok_or(Unpatchable::UnknownDestination(to))?;
    // Filler past an unconditional jump is never executed.
    bytes.resize(len, site.insn.bytes[len - 1]);
    Ok(Patch::single(pc, site.insn.bytes.clone(), bytes))
}

/// A comparison's token swapped (`from` -> `with`) in a branch's
/// condition: the jump's condition code swapped.
fn branch_token_swap(
    plan: &MutantPlan,
    then: &Span,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let site = branch_site(&plan.stop_at, then, guard_arm(plan), ours, isa)?;
    // The condition code that holds when the source condition does.
    let holds = if site.taken_when_true {
        site.jcc.cc
    } else {
        isa.inverse(site.jcc.cc)
    };
    let mutated = mutated_condition(isa, holds, from, with).ok_or_else(|| {
        Unpatchable::ConditionMismatch {
            cc: isa.cc_name(site.jcc.cc).unwrap_or("?").to_string(),
            token: from.name().to_string(),
        }
    })?;
    let cc = if site.taken_when_true {
        mutated
    } else {
        isa.inverse(mutated)
    };
    Ok(site.patch(isa, cc))
}

/// `+` swapped with `-` in a statement spanning up to `stmt`'s last line;
/// where its line has no single such instruction, the one at the
/// operator's column.
fn arithmetic_swap(
    plan: &MutantPlan,
    stmt: &Span,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Shaped, Unpatchable> {
    let lines = (plan.stop_at.line, stmt.end.line);
    let by_line = match (from, with) {
        (Token::Add, Token::Sub) => arithmetic(plan, lines, false, ours, isa),
        (Token::Sub, Token::Add) => arithmetic(plan, lines, true, ours, isa),
        _ => Err(Unpatchable::UnsupportedEdit(format!(
            "{} {} -> {}",
            plan.operator,
            from.name(),
            with.name()
        ))),
    };
    by_line.or_else(|refused| arithmetic_at(plan, from, with, ours, isa).map_err(|_| refused))
}

/// A body replaced by a default value: the function's entry becomes a stub
/// that returns it.
fn return_default(
    value_text: Option<&str>,
    zero_bits: bool,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    if value_text.is_none() {
        return Err(Unpatchable::NoDefault);
    }
    // The stub zeroes registers: exact only for an all-zero value.
    if !zero_bits {
        return Err(Unpatchable::NonZeroValue(
            value_text.unwrap_or_default().to_string(),
        ));
    }
    let stub = isa.return_stub().ok_or(Unpatchable::NoReturnStub)?;
    entry_stub(stub, ours, isa)
}

/// The last `setcc` positioned in a comparison's operands, its condition
/// code mapped from `from` to `with`.
fn compared_value(
    plan: &MutantPlan,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let [lhs, rhs] = plan.operands[..] else {
        return Err(Unpatchable::UnsupportedEdit(
            "a comparison without both operands".into(),
        ));
    };
    let span = ((lhs.start.line, lhs.start.col), (rhs.end.line, rhs.end.col));
    let set = ours
        .iter()
        .filter(|i| i.column != 0 && (span.0..span.1).contains(&(i.line, i.column)))
        .filter_map(|i| x86::decode(i.pc, &i.bytes).map(|d| (*i, d)))
        .filter(|(_, d)| d.setcc_condition().is_some())
        .max_by_key(|(i, _)| i.pc);
    let Some((insn, d)) = set else {
        return Err(Unpatchable::UnsupportedEdit(
            "a comparison made into a value without setcc".into(),
        ));
    };
    let holds = d.setcc_condition().unwrap_or_default();
    let mutated = mutated_condition(isa, holds, from, with).ok_or_else(|| {
        Unpatchable::ConditionMismatch {
            cc: isa.cc_name(holds).unwrap_or("?").to_string(),
            token: from.name().to_string(),
        }
    })?;
    let bytes = d
        .with_setcc_condition(mutated)
        .ok_or_else(|| Unpatchable::UnsupportedEdit("setcc re-encoding".into()))?;
    Ok(Patch::single(insn.pc, insn.bytes.clone(), bytes))
}

struct BranchSite<'a> {
    insn: &'a Insn,
    jcc: Jcc,
    taken_when_true: bool,
}

impl BranchSite<'_> {
    fn patch(&self, isa: &Isa, cc: u8) -> Patch {
        Patch::single(
            self.insn.pc,
            self.insn.bytes.clone(),
            isa.with_cc(&self.insn.bytes, &self.jcc, cc),
        )
    }
}

/// The one conditional jump on the condition's line, and whether it is
/// taken when the condition holds (it lands in the then-block) or when it
/// fails (it lands anywhere else).
/// The arm a guard belongs to: a condition whose then-target is on its own
/// line, with the arm as its false target's statement.
fn guard_arm(plan: &MutantPlan) -> Option<&Span> {
    match &plan.action {
        Action::EvalBranch {
            if_true: Target::BlockStart(then),
            if_false: Target::AfterStatement(arm),
            ..
        } if then.start.line == plan.stop_at.line => Some(arm),
        _ => None,
    }
}

/// Whether an instruction's source position lies in `[from, to)`. With no
/// column (0), only the line is compared, as for a whole-line condition.
fn within(i: &Insn, from: (u32, u32), to: (u32, u32)) -> bool {
    if i.column == 0 {
        return i.line == from.0;
    }
    (from..to).contains(&(i.line, i.column))
}

fn branch_site<'a>(
    stop: &Pos,
    then: &Span,
    within_arm: Option<&Span>,
    insns: &[&'a Insn],
    isa: &Isa,
) -> Result<BranchSite<'a>, Unpatchable> {
    // The condition's code: positioned from its start up to the then-block
    // (a guard shares its line with its body; the column separates them).
    let cond = ((stop.line, stop.col), (then.start.line, then.start.col));
    let line = stop.line;
    let jumps: Vec<(&Insn, Jcc)> = insns
        .iter()
        .filter(|i| within(i, cond.0, cond.1))
        .filter_map(|i| isa.decode_jcc(&i.bytes).map(|j| (*i, j)))
        .collect();
    let [(insn, jcc)] = jumps[..] else {
        return Err(Unpatchable::ConditionalJumps {
            line,
            found: jumps.len(),
        });
    };
    let dest = (insn.pc + jcc.len as u64).wrapping_add_signed(jcc.rel);
    let landing = insns
        .iter()
        .position(|i| i.pc == dest)
        .ok_or(Unpatchable::UnknownDestination(dest))?;
    // Compiler-generated code (a line-0 row) has no column of its own: it
    // belongs to the next instruction that has one, which it leads into.
    let columns_known = insns.iter().any(|i| i.column != 0);
    let dest_insn = if columns_known {
        insns[landing..]
            .iter()
            .find(|i| i.column != 0)
            .unwrap_or(&insns[landing])
    } else {
        &insns[landing]
    };
    if within(dest_insn, cond.0, cond.1) {
        return Err(Unpatchable::AmbiguousPolarity { line });
    }
    let at = (dest_insn.line, dest_insn.column);
    let taken_when_true = if dest_insn.column == 0 {
        (then.start.line..=then.end.line).contains(&dest_insn.line)
    } else {
        ((then.start.line, then.start.col)..=(then.end.line, then.end.col)).contains(&at)
            // A guard's true edge enters its arm (its bindings, then the
            // body); the false edge leaves for the next arm.
            || within_arm.is_some_and(|arm| {
                ((arm.start.line, arm.start.col)..=(arm.end.line, arm.end.col)).contains(&at)
            })
    };
    Ok(BranchSite {
        insn,
        jcc,
        taken_when_true,
    })
}

/// The condition code for `lhs with rhs`, given the code `holds` that
/// implements `lhs from rhs` in either operand order and signedness.
fn mutated_condition(isa: &Isa, holds: u8, from: Token, with: Token) -> Option<u8> {
    // `e` and `ne` are the same code for signed and unsigned operands and
    // for either operand order: an ordering made of them is a guess.
    if matches!(from, Token::Eq | Token::Ne) && !matches!(with, Token::Eq | Token::Ne) {
        return None;
    }
    [Signedness::Signed, Signedness::Unsigned]
        .into_iter()
        .find_map(|s| {
            if isa.relational(s, from) == Some(holds) {
                return isa.relational(s, with);
            }
            let (from_sw, with_sw) = (isa.swapped(from)?, isa.swapped(with)?);
            if isa.relational(s, from_sw) == Some(holds) {
                return isa.relational(s, with_sw);
            }
            None
        })
}

/// Jump from the statement's first byte to just past the last call on
/// its path. The path starts at the statement's first instruction, falls
/// through, and follows unconditional jumps that stay on the statement's
/// lines; blocks of those lines that the path never reaches (panic and
/// unwinding paths) are not part of it. Ending after the last call keeps
/// code the line table attributes to the statement but that belongs to the
/// next one (Rust loads the next statement's operands there), and the
/// path takes in drops of the call's result, which must not run on a
/// value that was never written.
fn delete_statement(
    first: u32,
    last: u32,
    insns: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let lines = first..=last;
    let start = (0..insns.len())
        .find(|&k| lines.contains(&insns[k].line))
        .ok_or(Unpatchable::NoInstructions { line: first })?;
    let mut path = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut k = start;
    while let Some(insn) = insns.get(k) {
        if !lines.contains(&insn.line) || !seen.insert(k) {
            break;
        }
        path.push(k);
        k = match (isa.decode_jcc(&insn.bytes), isa.jump_rel(&insn.bytes)) {
            // An unconditional jump continues the path at its destination.
            (None, Some(rel)) => {
                let dest = (insn.pc + insn.bytes.len() as u64).wrapping_add_signed(rel);
                match insns.iter().position(|i| i.pc == dest) {
                    Some(d) => d,
                    None => break,
                }
            }
            _ => k + 1,
        };
    }
    let last_call = path
        .iter()
        .rposition(|&k| isa.is_call(&insns[k].bytes))
        .ok_or(Unpatchable::NoCall { line: first })?;
    // A statement that branches before its last call decides at run time
    // where it goes (`f(x?)` may return early), so no straight path through
    // it skips it. A jump to a panic block (an overflow check: `lea`, then
    // a call) is no such branch.
    let panics = |pc: u64| {
        insns.iter().position(|i| i.pc == pc).is_some_and(|t| {
            x86::decode(insns[t].pc, &insns[t].bytes).is_some_and(|d| d.is_lea())
                && insns
                    .get(t + 1)
                    .and_then(|i| x86::decode(i.pc, &i.bytes))
                    .is_some_and(|d| d.is_call())
        })
    };
    let branches = path[..last_call].iter().any(|&k| {
        x86::decode(insns[k].pc, &insns[k].bytes)
            .and_then(|d| d.jump())
            .is_some_and(|j| j.conditional && !panics(j.target))
    });
    if branches {
        return Err(Unpatchable::UnsupportedEdit(
            "a statement that branches before its last call".into(),
        ));
    }
    let resume = match path.get(last_call + 1) {
        Some(&k) => insns[k],
        None => *insns
            .get(path[last_call] + 1)
            .ok_or(Unpatchable::NoResumePoint { line: first })?,
    };
    // The jump is written over the statement's first bytes, which must be
    // contiguous on the path up to its first jump or its last call.
    let room: usize = path
        .iter()
        .take(last_call + 1)
        .enumerate()
        .take_while(|&(n, &k)| {
            n == 0 || insns[k].pc == insns[path[n - 1]].pc + insns[path[n - 1]].bytes.len() as u64
        })
        .map(|(_, &k)| insns[k].bytes.len())
        .sum();
    let at = insns[start].pc;
    let jump = isa
        .encode_jmp(at, resume.pc, room)
        .ok_or(Unpatchable::TooShort {
            line: first,
            bytes: room,
        })?;
    let code: Vec<u8> = path
        .iter()
        .flat_map(|&k| insns[k].bytes.iter().copied())
        .collect();
    Ok(Patch::single(at, code[..jump.len()].to_vec(), jump))
}

/// Swap the one add-family instruction (or sub-family, when `to_add`) on
/// the statement's lines.
fn arithmetic(
    plan: &MutantPlan,
    (first, last): (u32, u32),
    to_add: bool,
    insns: &[&Insn],
    isa: &Isa,
) -> Result<Shaped, Unpatchable> {
    let swaps: Vec<(&Insn, Matched)> = insns
        .iter()
        .filter(|i| (first..=last).contains(&i.line))
        .filter_map(|i| isa.arith_match(&i.bytes, to_add).map(|m| (*i, m)))
        .collect();
    let [(insn, matched)] = &swaps[..] else {
        return Err(Unpatchable::ArithmeticInstructions {
            line: first,
            found: swaps.len(),
        });
    };
    // `a + b` with `a` loaded from memory is `add b, [a]`: a `sub` there
    // computes `b - a`.
    if !to_add && memory_source(insn) {
        return Err(Unpatchable::UnsupportedEdit(
            "an addition whose left operand is loaded from memory".into(),
        ));
    }
    // The overflow check after the operation must hold for the swapped one
    // too: only the overflow flag's (`jo`, signed) does. rustc checks
    // unsigned arithmetic by comparing the result (`cmp; jb`), which a
    // swapped operation trips for every nonzero operand.
    // (Code without a check, as Go's or wrapping arithmetic, is fine.)
    let mut after: Vec<&&Insn> = insns
        .iter()
        .filter(|i| i.pc > insn.pc && i.line == insn.line)
        .collect();
    after.sort_by_key(|i| i.pc);
    let first_check = after
        .iter()
        .take(4)
        .filter_map(|i| x86::decode(i.pc, &i.bytes).and_then(|d| d.condition()))
        .next();
    if matches!(first_check, Some(0x2 | 0x3)) {
        return Err(Unpatchable::UnsupportedEdit(
            "arithmetic whose overflow check is not the overflow flag's".into(),
        ));
    }
    let check = match first_check {
        Some(_) => Check::Overflow,
        None => Check::Unchecked,
    };
    let (from, with) = if to_add {
        (Token::Sub, Token::Add)
    } else {
        (Token::Add, Token::Sub)
    };
    let shape = shape_of(matched, Kind::Int(check), from, with);
    if is_rust(plan) && !isa.admits(&shape) {
        return Err(Unpatchable::UnsupportedEdit(format!(
            "no harvested rustc shape `{shape}`"
        )));
    }
    Ok(Shaped {
        patch: Patch::single(insn.pc, insn.bytes.clone(), matched.replacement.clone()),
        shape,
    })
}

/// Flip every store of the returned constant `value` on the return's line.
fn return_flip(line: u32, value: bool, insns: &[&Insn], isa: &Isa) -> Result<Patch, Unpatchable> {
    let regions: Vec<Region> = insns
        .iter()
        .filter(|i| i.line == line)
        .filter_map(|i| {
            isa.bool_flip(&i.bytes, value).map(|r| Region {
                addr: i.pc,
                original: i.bytes.clone(),
                replacement: r,
            })
        })
        .collect();
    if regions.is_empty() {
        return Err(Unpatchable::NoConstant { line });
    }
    Ok(Patch { regions })
}

/// Write `stub` over the function's first bytes, unless something jumps
/// into them. The entry is the lowest instruction address.
fn entry_stub(stub: &[u8], insns: &[&Insn], isa: &Isa) -> Result<Patch, Unpatchable> {
    let entry = insns
        .first()
        .ok_or(Unpatchable::NoInstructions { line: 0 })?
        .pc;
    let code: Vec<u8> = insns.iter().flat_map(|i| i.bytes.iter().copied()).collect();
    if code.len() < stub.len() {
        return Err(Unpatchable::FunctionTooShort {
            bytes: code.len(),
            stub: stub.len(),
        });
    }
    let inside = entry + 1..entry + stub.len() as u64;
    for i in insns {
        if let Some(rel) = isa.jump_rel(&i.bytes) {
            let dest = (i.pc + i.bytes.len() as u64).wrapping_add_signed(rel);
            if inside.contains(&dest) {
                return Err(Unpatchable::JumpIntoStub(dest));
            }
        }
    }
    Ok(Patch::single(
        entry,
        code[..stub.len()].to_vec(),
        stub.to_vec(),
    ))
}

/// Where a condition's code starts, and its two exits: where execution goes
/// when the condition holds and when it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchExits {
    pub stop: u64,
    pub if_true: u64,
    pub if_false: u64,
}

/// The exits of the plan's condition, for evaluating a mutated condition
/// and continuing at the exit it selects (tier B). The condition's code is
/// what is reachable from its start through instructions positioned inside
/// it (from its start up to its then-target; compiler-generated code takes
/// the position of the instruction it leads into). Exactly one exit must
/// enter the then-target (or, for a guard, its arm) and one other.
pub fn branch_exits(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
    isa: &Isa,
) -> Result<BranchExits, Unpatchable> {
    let Action::EvalBranch {
        if_true: Target::BlockStart(then),
        ..
    } = &plan.action
    else {
        return Err(Unpatchable::UnsupportedEdit("exits of a non-branch".into()));
    };
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    if !ours.iter().any(|i| i.column != 0) {
        return Err(Unpatchable::UnsupportedEdit("exits without columns".into()));
    }
    let index = |pc: u64| ours.iter().position(|i| i.pc == pc);
    // A position for every instruction: its own, or the next positioned one's.
    let position = |k: usize| {
        ours[k..]
            .iter()
            .find(|i| i.column != 0)
            .map(|i| (i.line, i.column))
    };
    let from = (plan.stop_at.line, plan.stop_at.col);
    let to = (then.start.line, then.start.col);
    let inside = |k: usize| position(k).is_some_and(|p| (from..to).contains(&p));
    // The first instruction positioned in the condition (its first token
    // may compile to nothing: `!flag` starts by loading `flag`).
    let stop = (0..ours.len())
        .filter(|&k| ours[k].column != 0 && inside(k))
        .min_by_key(|&k| ours[k].pc)
        .ok_or(Unpatchable::NoCodeAt {
            line: from.0,
            col: from.1,
        })?;
    let mut seen = std::collections::BTreeSet::new();
    let mut exits = std::collections::BTreeSet::new();
    let mut work = vec![stop];
    while let Some(k) = work.pop() {
        if !seen.insert(k) {
            continue;
        }
        let insn = ours[k];
        let next = insn.pc + insn.bytes.len() as u64;
        let mut successors = Vec::new();
        if let Some(rel) = isa.jump_rel(&insn.bytes) {
            successors.push(next.wrapping_add_signed(rel));
            if isa.decode_jcc(&insn.bytes).is_some() {
                successors.push(next);
            }
        } else {
            successors.push(next);
        }
        for pc in successors {
            match index(pc) {
                Some(j) if inside(j) => work.push(j),
                _ => {
                    exits.insert(pc);
                }
            }
        }
    }
    let arm = guard_arm(plan);
    let enters_then = |pc: u64| {
        index(pc).and_then(position).is_some_and(|p| {
            ((then.start.line, then.start.col)..=(then.end.line, then.end.col)).contains(&p)
                || arm.is_some_and(|a| {
                    ((a.start.line, a.start.col)..=(a.end.line, a.end.col)).contains(&p)
                })
        })
    };
    let (yes, no): (Vec<u64>, Vec<u64>) = exits.iter().partition(|&&pc| enters_then(pc));
    let ([if_true], [if_false]) = (&yes[..], &no[..]) else {
        return Err(Unpatchable::UnsupportedEdit(format!(
            "condition code with exits {:x?} into the then-target and {:x?} elsewhere",
            yes, no
        )));
    };
    Ok(BranchExits {
        stop: ours[stop].pc,
        if_true: *if_true,
        if_false: *if_false,
    })
}

/// A constant stored to a stack slot (`[rsp+disp]`, `size` bytes): how a
/// short-circuit operator's result is produced when it is a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotStore {
    pub disp: i64,
    pub size: u64,
    pub value: u64,
}

/// Where the mutant goes along one successor of a jump: `to`, after
/// `store` when the operator's result is a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goto {
    pub to: u64,
    pub store: Option<SlotStore>,
}

impl Goto {
    fn to(to: u64) -> Self {
        Goto { to, store: None }
    }
}

/// A jump out of the left operand of a swapped `&&`/`||`: which successor
/// the program takes is read from the flags (`cc`, `None` when the jump is
/// unconditional), then the mutant continues where that successor leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Steer {
    pub pc: u64,
    pub cc: Option<u8>,
    pub taken: Goto,
    pub fall: Goto,
}

/// Of the functions with code on a line (each its instructions), the one
/// with the most instructions positioned inside `site`: a closure's body
/// shares its line with the function around it. The first when none is.
pub fn at_site(candidates: &[&Vec<Insn>], site: &Span) -> usize {
    at_site_all(candidates, site).first().copied().unwrap_or(0)
}

/// The candidates that are copies of the function with the most
/// instructions inside `site`: each copy of a generic function (`pick<u8>`,
/// `pick<u32>`) starts at the same source position as the others, and a
/// mutant changes them all, however much code each has inside `site` (a
/// comparison is inline for one type and a call for another). A closure on
/// the line starts elsewhere. Only the first when none has any code there.
pub fn at_site_all(candidates: &[&Vec<Insn>], site: &Span) -> Vec<usize> {
    let (from, to) = (
        (site.start.line, site.start.col),
        (site.end.line, site.end.col),
    );
    let inside = |insns: &Vec<Insn>| {
        insns
            .iter()
            .filter(|i| i.column != 0 && (from..to).contains(&(i.line, i.column)))
            .count()
    };
    let scores: Vec<usize> = candidates.iter().map(|c| inside(c)).collect();
    let Some((best, &top)) = scores
        .iter()
        .enumerate()
        .max_by_key(|&(k, &n)| (n, usize::MAX - k))
    else {
        return Vec::new();
    };
    if top == 0 {
        return vec![0];
    }
    let entry = |insns: &Vec<Insn>| {
        insns
            .iter()
            .min_by_key(|i| i.pc)
            .map(|i| (i.line, i.column))
    };
    let start = entry(candidates[best]);
    (0..scores.len())
        .filter(|&k| scores[k] > 0 && entry(candidates[k]) == start)
        .collect()
}

/// Whether the plan swaps `&&` and `||`.
pub fn is_logical_swap(plan: &MutantPlan) -> bool {
    matches!(
        plan.edit,
        Edit::ReplaceToken {
            from: Token::And | Token::Or,
            with: Token::And | Token::Or,
            ..
        }
    )
}

/// Swap `L && R` and `L || R` without evaluating anything, in any context:
/// the program computes L itself and leaves L's code along edges that say
/// its value. Edges that went on into R short-circuit instead, to where N
/// (the operator's result) takes that value; edges that short-circuited go
/// on into R. Where N takes the value is where R's own exits for it lead,
/// or, when N is a value rather than a branch, the constant block for the
/// other value with its constant replaced.
pub fn swap_logical(
    plan: &MutantPlan,
    file: &str,
    insns: &[Insn],
) -> Result<Vec<Steer>, Unpatchable> {
    let (goes_on, lhs, rhs) = logical_swap(plan)?;
    let flow = Flow::new(file, insns);
    let (a, b) = (flow.range(&lhs), flow.range(&rhs));
    let from_left = flow.leaving(a);
    if from_left.iter().any(|&(_, _, by_jump)| !by_jump) {
        return unsupported("a left operand that is a value, not a branch");
    }
    let (right, short) = flow.left_exits(&from_left, b)?;
    let continued = flow.continuation(b, short, goes_on)?;
    let lead = |to: u64| match () {
        _ if flow.inside(to, a) => Goto::to(to),
        _ if flow.inside(flow.resolve(to), b) => continued,
        _ => Goto::to(right),
    };
    let mut froms: Vec<usize> = from_left.iter().map(|&(k, _, _)| k).collect();
    froms.dedup();
    froms.into_iter().map(|k| flow.steer(k, &lead)).collect()
}

fn unsupported<T>(what: &str) -> Result<T, Unpatchable> {
    Err(Unpatchable::UnsupportedEdit(what.to_string()))
}

/// Whether the swapped operator goes on into the right operand when the
/// left one holds (`&&` does, `||` does when it fails), and the operands.
fn logical_swap(plan: &MutantPlan) -> Result<(bool, Span, Span), Unpatchable> {
    let Edit::ReplaceToken { from, with, .. } = plan.edit else {
        return unsupported("short circuit of a non-swap");
    };
    // `&&` goes on into R when L holds, `||` when it fails.
    let goes_on = match (from, with) {
        (Token::And, Token::Or) => true,
        (Token::Or, Token::And) => false,
        _ => return unsupported("short circuit of a non-logical swap"),
    };
    let [lhs, rhs] = plan.operands[..] else {
        return unsupported("short circuit without both operands");
    };
    Ok((goes_on, lhs, rhs))
}

/// A span as its (line, column) start and end.
type Range = ((u32, u32), (u32, u32));

/// The single address of `pcs`, if they are one.
fn unique(mut pcs: Vec<u64>) -> Option<u64> {
    pcs.sort_unstable();
    pcs.dedup();
    match pcs[..] {
        [pc] => Some(pc),
        _ => None,
    }
}

/// One file's instructions by address, decoded, and the questions about
/// their flow that a swapped short circuit needs answered.
struct Flow<'a> {
    ours: Vec<(&'a Insn, Option<x86::Instr>)>,
}

impl<'a> Flow<'a> {
    fn new(file: &str, insns: &'a [Insn]) -> Self {
        let mut ours: Vec<(&Insn, Option<x86::Instr>)> = insns
            .iter()
            .filter(|i| i.file == file)
            .map(|i| (i, x86::decode(i.pc, &i.bytes)))
            .collect();
        ours.sort_by_key(|(i, _)| i.pc);
        Flow { ours }
    }

    fn range(&self, s: &Span) -> Range {
        ((s.start.line, s.start.col), (s.end.line, s.end.col))
    }

    fn index(&self, pc: u64) -> Option<usize> {
        self.ours.binary_search_by_key(&pc, |(i, _)| i.pc).ok()
    }

    fn jump(&self, k: usize) -> Option<x86::Jump> {
        self.ours[k].1.as_ref().and_then(x86::Instr::jump)
    }

    fn next(&self, k: usize) -> u64 {
        self.ours[k].0.pc + self.ours[k].0.bytes.len() as u64
    }

    /// Plain jumps followed to where they lead.
    fn resolve(&self, mut pc: u64) -> u64 {
        for _ in 0..8 {
            match self.index(pc).and_then(|k| self.jump(k)) {
                Some(j) if !j.conditional => pc = j.target,
                _ => break,
            }
        }
        pc
    }

    /// `mov [rsp+d], imm; jmp join`: the result of a short circuit as a
    /// value. Belongs to no operand, wherever the line table puts it.
    fn constant(&self, k: usize) -> Option<(SlotStore, u64)> {
        let (disp, value, size) = self.ours[k].1.as_ref()?.stack_store()?;
        let then = (k + 1 < self.ours.len())
            .then(|| self.jump(k + 1))
            .flatten()
            .filter(|j| !j.conditional)?;
        Some((
            SlotStore {
                disp,
                size,
                value: value?,
            },
            then.target,
        ))
    }

    fn in_constant(&self, k: usize) -> bool {
        self.constant(k).is_some() || (k > 0 && self.constant(k - 1).is_some())
    }

    /// The (line, column) of the first positioned instruction from `k` on.
    fn position(&self, k: usize) -> Option<(u32, u32)> {
        self.ours[k..]
            .iter()
            .find(|(i, _)| i.column != 0)
            .map(|(i, _)| (i.line, i.column))
    }

    /// A jump lands in an operand where the next positioned instruction is
    /// in it (a jump's destination may have no position of its own) ...
    fn lands(&self, k: usize, (from, to): Range) -> bool {
        !self.in_constant(k) && self.position(k).is_some_and(|p| (from..to).contains(&p))
    }

    fn inside(&self, pc: u64, range: Range) -> bool {
        self.index(pc).is_some_and(|k| self.lands(k, range))
    }

    /// ... but an operand's own code is only what is positioned in it: an
    /// unpositioned instruction just before it (a block's closing `jmp`) is
    /// not.
    fn member(&self, k: usize, range: Range) -> bool {
        self.ours[k].0.column != 0 && self.lands(k, range)
    }

    /// Every edge leaving an operand's code: (from, to, by a jump).
    fn leaving(&self, range: Range) -> Vec<(usize, u64, bool)> {
        let mut edges = Vec::new();
        for k in (0..self.ours.len()).filter(|&k| self.member(k, range)) {
            let successors = match self.jump(k) {
                Some(j) if j.conditional => vec![(j.target, true), (self.next(k), true)],
                Some(j) => vec![(j.target, true)],
                None => vec![(self.next(k), false)],
            };
            edges.extend(
                successors
                    .into_iter()
                    .filter(|&(to, _)| !self.inside(to, range))
                    .map(|(to, by_jump)| (k, to, by_jump)),
            );
        }
        edges
    }

    /// Where the left operand's exits lead: the one way into the right
    /// operand `b`, and the one way past it (the short circuit).
    fn left_exits(
        &self,
        from_left: &[(usize, u64, bool)],
        b: Range,
    ) -> Result<(u64, u64), Unpatchable> {
        let into_right = |to: u64| self.inside(self.resolve(to), b);
        let (on, off): (Vec<_>, Vec<_>) = from_left.iter().partition(|&&(_, to, _)| into_right(to));
        let (Some(right), Some(short)) = (
            unique(on.iter().map(|&&(_, to, _)| self.resolve(to)).collect()),
            unique(off.iter().map(|&&(_, to, _)| self.resolve(to)).collect()),
        ) else {
            return unsupported(
                "a left operand whose exits are not one way into the right and one past it",
            );
        };
        Ok((right, short))
    }

    /// Where N takes the value L had on the edges into R: R's other exit.
    fn continuation(&self, b: Range, short: u64, goes_on: bool) -> Result<Goto, Unpatchable> {
        let from_right = self.leaving(b);
        let others: Vec<u64> = from_right
            .iter()
            .filter(|&&(_, to, by_jump)| by_jump && self.resolve(to) != short)
            .map(|&(_, to, _)| self.resolve(to))
            .collect();
        let valued = from_right.iter().any(|&(_, _, by_jump)| !by_jump);
        let short_value = u64::from(!goes_on);
        match (unique(others.clone()), others.is_empty() && valued) {
            (Some(to), _) => Ok(Goto::to(to)),
            // N is a value: R computes it into the slot the constant block
            // writes; the mutant writes the other constant there instead.
            (None, true) => match self.index(short).and_then(|k| self.constant(k)) {
                Some((store, join)) if store.value == short_value => Ok(Goto {
                    to: join,
                    store: Some(SlotStore {
                        value: u64::from(goes_on),
                        ..store
                    }),
                }),
                _ => unsupported("a short-circuit value not stored as a constant"),
            },
            _ => unsupported("a right operand whose exits are not one way each"),
        }
    }

    /// The steering for the jump at `k`, each successor led by `lead`.
    fn steer(&self, k: usize, lead: &dyn Fn(u64) -> Goto) -> Result<Steer, Unpatchable> {
        let j = self.jump(k).expect("an edge by a jump leaves from a jump");
        let cc = self.ours[k].1.as_ref().and_then(x86::Instr::condition);
        if j.conditional && cc.is_none() {
            return unsupported("a jump whose condition is unknown");
        }
        let taken = lead(j.target);
        let fall = if j.conditional {
            lead(self.next(k))
        } else {
            taken
        };
        Ok(Steer {
            pc: self.ours[k].0.pc,
            cc,
            taken,
            fall,
        })
    }
}

/// Where to flip a negated boolean that comes out of a call (`f() == g()`
/// is `PartialEq::eq`, `!s.is_empty()`): the return address of the last
/// call in the expression, where the result is in `al` (System V). Flipping
/// it there negates the comparison, or undoes the `!` applied next.
pub fn flip_after_call(plan: &MutantPlan, file: &str, insns: &[Insn]) -> Result<u64, Unpatchable> {
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    let (from, to) = match (&plan.edit, &plan.operands[..]) {
        (Edit::ReplaceToken { .. }, [lhs, rhs]) => (lhs.start, rhs.end),
        (Edit::DeleteToken { .. }, [operand]) => (operand.start, operand.end),
        _ => {
            return Err(Unpatchable::UnsupportedEdit(
                "negation without operands".into(),
            ));
        }
    };
    let span = ((from.line, from.col), (to.line, to.col));
    // A call whose result is used as a bool: the next instruction reads al
    // (a panic call, which never returns, is followed by `ud2` instead).
    let reads_bool = |pc: u64| {
        ours.iter()
            .find(|i| i.pc == pc)
            .and_then(|i| x86::decode(i.pc, &i.bytes))
            .is_some_and(|d| d.reads_bool_al())
    };
    ours.iter()
        .filter(|i| i.column != 0 && (span.0..span.1).contains(&(i.line, i.column)))
        .filter(|i| x86::decode(i.pc, &i.bytes).is_some_and(|d| d.is_call()))
        .map(|i| i.pc + i.bytes.len() as u64)
        .filter(|&ret| reads_bool(ret))
        .max()
        .ok_or(Unpatchable::UnsupportedEdit(
            "a negated boolean not computed by a call".into(),
        ))
}

/// Where a checked arithmetic operation between the plan's operands panics
/// on overflow: the target of the check after it (`jo`, or `jb`/`jae` on an
/// unsigned result) when that block loads the panic's location and calls.
/// `None` for unchecked (wrapping) arithmetic.
pub fn overflow_panic(plan: &MutantPlan, file: &str, insns: &[Insn]) -> Option<u64> {
    let [lhs, rhs] = plan.operands[..] else {
        return None;
    };
    let span = ((lhs.start.line, lhs.start.col), (rhs.end.line, rhs.end.col));
    let mut ours: Vec<&Insn> = insns.iter().filter(|i| i.file == file).collect();
    ours.sort_by_key(|i| i.pc);
    let at = |pc: u64| ours.iter().position(|i| i.pc == pc);
    let decoded = |k: usize| x86::decode(ours[k].pc, &ours[k].bytes);
    ours.iter()
        .enumerate()
        .filter(|(_, i)| {
            // The panic block is positioned at the expression, so search
            // its line from the left operand on.
            i.column != 0 && (i.line, i.column) >= span.0 && i.line <= span.1.0
        })
        .filter_map(|(k, _)| {
            let d = decoded(k)?;
            let cc = d.condition()?;
            let j = d.jump()?;
            matches!(cc, 0x0 | 0x2 | 0x3).then_some(j.target)
        })
        .find(|&target| {
            at(target).is_some_and(|t| {
                decoded(t).is_some_and(|d| d.is_lea())
                    && t + 1 < ours.len()
                    && decoded(t + 1).is_some_and(|d| d.is_call())
            })
        })
}
