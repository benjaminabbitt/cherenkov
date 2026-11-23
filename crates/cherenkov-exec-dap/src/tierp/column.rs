//! Arithmetic operators found by column. A line-table row names the line
//! and the column where an expression starts, so the instruction of an
//! operator inside a call argument, a struct field, a method chain or a
//! compound assignment through a pointer is the one positioned at its
//! left operand's start, however many other instructions its line holds.
//!
//! A swap is made only where it is exact: an integer `+`/`-` whose overflow
//! check follows it in the way rustc emits it, a floating-point operation
//! (no check exists), and the bitwise compound assignments. Anything that
//! would need a different instruction shape (`+` to `*` on integers) is not
//! patched.

use cherenkov_core::plan::MutantPlan;
use cherenkov_core::token::Token;

use super::{Insn, Patch, Region, Unpatchable};
use crate::isa::Isa;
use crate::x86;

/// How many instructions after an operation its overflow check may take:
/// the result's store, `seto`, a compare, the jump.
const CHECK_REACH: usize = 6;

/// One way of swapping an operator's instruction.
#[derive(Clone, Copy)]
enum Swap {
    /// `add` <-> `sub`, checked for overflow.
    Integer { to_add: bool },
    /// Scalar floating point, which has no check.
    Float { from: Token, with: Token },
    /// `and`, `or`, `xor` of a compound assignment.
    Bitwise { from: Token, with: Token },
}

/// The binary operator a compound assignment applies, and whether it is one.
fn base(t: Token) -> (Token, bool) {
    t.compound_base().map_or((t, false), |b| (b, true))
}

/// The swaps that realize `from` -> `with` exactly. A plain operator may
/// only change to one of its own precedence, which regroups nothing; a
/// compound assignment's right side is its whole expression, so any
/// floating-point operation (and any bitwise one) may stand for another.
fn swaps(from: Token, with: Token) -> Vec<Swap> {
    let ((f, compound), (w, other)) = (base(from), base(with));
    let arithmetic = |t: Token| matches!(t, Token::Add | Token::Sub | Token::Mul | Token::Div);
    let bitwise = |t: Token| matches!(t, Token::BitAnd | Token::BitOr | Token::BitXor);
    let same_class = matches!(
        (f, w),
        (Token::Add, Token::Sub)
            | (Token::Sub, Token::Add)
            | (Token::Mul, Token::Div)
            | (Token::Div, Token::Mul)
    );
    let mut out = Vec::new();
    if compound != other || f == w {
        return out;
    }
    if matches!((f, w), (Token::Add, Token::Sub) | (Token::Sub, Token::Add)) {
        out.push(Swap::Integer {
            to_add: w == Token::Add,
        });
    }
    if same_class || (compound && arithmetic(f) && arithmetic(w)) {
        out.push(Swap::Float { from: f, with: w });
    }
    if compound && bitwise(f) && bitwise(w) {
        out.push(Swap::Bitwise { from: f, with: w });
    }
    out
}

/// A numeric literal, which the compiler may put on the right of a
/// commutative operation.
fn is_literal(text: &str) -> bool {
    text.trim_start_matches('-')
        .starts_with(|c: char| c.is_ascii_digit())
}

/// The mutated operator, of the plan's expression that starts at its first
/// operand, found among the instructions positioned there.
pub(super) fn arithmetic_at(
    plan: &MutantPlan,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Patch, Unpatchable> {
    let unsupported = || {
        Unpatchable::UnsupportedEdit(format!(
            "{} {} -> {} by column",
            plan.operator,
            from.name(),
            with.name()
        ))
    };
    let lhs = plan.operands.first().ok_or_else(unsupported)?;
    let swaps = swaps(from, with);
    if swaps.is_empty() {
        return Err(unsupported());
    }
    // `1 + x` is compiled as `x + 1`: a swap to `-` would compute `x - 1`.
    let commutes_in_source = matches!(from, Token::Add | Token::Mul);
    let needs_order = matches!(with, Token::Sub | Token::Div);
    let literal_first = plan.operand_texts.first().is_some_and(|t| is_literal(t));
    if commutes_in_source && needs_order && literal_first {
        return Err(unsupported());
    }
    let at = (lhs.start.line, lhs.start.col);
    let here: Vec<usize> = (0..ours.len())
        .filter(|&k| ours[k].column != 0 && (ours[k].line, ours[k].column) == at)
        .collect();
    if here.is_empty() {
        return Err(Unpatchable::NoCodeAt {
            line: at.0,
            col: at.1,
        });
    }
    let mut found: Vec<Vec<Region>> = here
        .iter()
        .flat_map(|&k| swaps.iter().filter_map(move |s| regions(*s, ours, k, isa)))
        .collect();
    match found.len() {
        1 => Ok(Patch {
            regions: found.remove(0),
        }),
        n => Err(Unpatchable::ArithmeticInstructions {
            line: at.0,
            found: n,
        }),
    }
}

/// The bytes that make instruction `k` do the swapped operation, with the
/// compare of its overflow check removed where there is one.
fn regions(swap: Swap, ours: &[&Insn], k: usize, isa: &Isa) -> Option<Vec<Region>> {
    let insn = ours[k];
    let (replacement, check) = match swap {
        Swap::Integer { to_add } => {
            let replacement = isa.arith_swap(&insn.bytes, to_add)?;
            (replacement, Some(overflow_check(ours, k)?))
        }
        Swap::Float { from, with } => (isa.float_swap(&insn.bytes, from, with)?, None),
        Swap::Bitwise { from, with } => (isa.bitwise_swap(&insn.bytes, from, with)?, None),
    };
    let mut out = vec![Region {
        addr: insn.pc,
        original: insn.bytes.clone(),
        replacement,
    }];
    if let Some(cmp) = check.flatten() {
        out.push(Region {
            addr: cmp.pc,
            original: cmp.bytes.clone(),
            replacement: vec![0x90; cmp.bytes.len()],
        });
    }
    Some(out)
}

/// The check rustc emits after the checked operation `k`, as the compare
/// to drop (`Some(Some(cmp))`), or that the flags the operation set are
/// the ones the jump reads (`Some(None)`); `None` when no check follows in
/// a way that keeps the swapped operation exact.
///
/// Signed arithmetic is checked with the overflow flag (`seto; jo`), which
/// every add/sub variant sets as its own. Unsigned arithmetic is checked
/// by comparing the result with an operand (`cmp; jb`), which a swapped
/// operation would trip wrongly: without the compare, the jump reads the
/// carry flag, which is the swapped operation's own carry or borrow.
fn overflow_check<'a>(ours: &[&'a Insn], k: usize) -> Option<Option<&'a Insn>> {
    let mut compare = None;
    let mut end = ours[k].pc + ours[k].bytes.len() as u64;
    for next in ours.iter().skip(k + 1).take(CHECK_REACH) {
        if next.pc != end || next.line != ours[k].line {
            return None;
        }
        end += next.bytes.len() as u64;
        let d = x86::decode(next.pc, &next.bytes)?;
        if d.is_move() || d.setcc_condition().is_some() {
            continue;
        }
        if d.is_compare() && compare.is_none() {
            compare = Some(*next);
            continue;
        }
        return match (d.condition()?, compare) {
            (0x0 | 0x1, None) => Some(None),
            (0x2 | 0x3, cmp) => Some(cmp),
            _ => None,
        };
    }
    None
}
