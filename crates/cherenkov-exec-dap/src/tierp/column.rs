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
use crate::isa::{Isa, Matched};
use crate::shapes::{Check, Kind, Shape};
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

impl Swap {
    /// The operators it swaps (base operators, not compound ones).
    fn tokens(self) -> (Token, Token) {
        match self {
            Swap::Integer { to_add: true } => (Token::Sub, Token::Add),
            Swap::Integer { to_add: false } => (Token::Add, Token::Sub),
            Swap::Float { from, with } | Swap::Bitwise { from, with } => (from, with),
        }
    }
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

/// A patch and the shape it is made of.
pub(super) struct Shaped {
    pub patch: Patch,
    pub shape: Shape,
}

/// A patch of one instruction, before the choice between them.
struct Candidate {
    insn: usize,
    regions: Vec<Region>,
    shape: Shape,
}

/// Whether the plan is of a Rust file, whose compiler the shapes table
/// was harvested with.
pub(super) fn is_rust(plan: &MutantPlan) -> bool {
    plan.file.extension().is_some_and(|e| e == "rs")
}

/// The mutated operator, of the plan's expression that starts at its first
/// operand, found among the instructions positioned there.
pub(super) fn arithmetic_at(
    plan: &MutantPlan,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Shaped, Unpatchable> {
    let unsupported = |why: &str| {
        Unpatchable::UnsupportedEdit(format!(
            "{} {} -> {} by column{why}",
            plan.operator,
            from.name(),
            with.name()
        ))
    };
    // The expression starts at its left operand; a compound assignment
    // has none of its own, and starts where its statement does.
    let start = plan.operands.first().map_or(plan.stop_at, |s| s.start);
    let swaps = swaps(from, with);
    if swaps.is_empty() {
        return Err(unsupported(""));
    }
    // `1 + x` is compiled as `x + 1`: a swap to `-` would compute `x - 1`.
    let literal_first = plan.operand_texts.first().is_some_and(|t| is_literal(t));
    if reorders(from, with) && literal_first {
        return Err(unsupported(""));
    }
    let at = (start.line, start.col);
    let here: Vec<usize> = (0..ours.len())
        .filter(|&k| ours[k].column != 0 && (ours[k].line, ours[k].column) == at)
        .collect();
    if here.is_empty() {
        return Err(Unpatchable::NoCodeAt {
            line: at.0,
            col: at.1,
        });
    }
    let candidates: Vec<Candidate> = here
        .iter()
        .flat_map(|&k| {
            swaps
                .iter()
                .filter_map(move |s| candidate(*s, ours, k, isa))
        })
        .collect();
    let mut chosen = choose(candidates, reorders(from, with), ours, plan, isa)
        .map_err(|why| unsupported(&format!(": {why}")))?;
    match chosen.len() {
        1 => {
            let c = chosen.remove(0);
            Ok(Shaped {
                patch: Patch { regions: c.regions },
                shape: c.shape,
            })
        }
        n => Err(Unpatchable::ArithmeticInstructions {
            line: at.0,
            found: n,
        }),
    }
}

/// Whether the swap computes with its operands in the other order where the
/// compiler had them the other way round: a commutative operation swapped
/// to one that is not.
fn reorders(from: Token, with: Token) -> bool {
    matches!(base(from).0, Token::Add | Token::Mul)
        && matches!(base(with).0, Token::Sub | Token::Div)
}

/// The candidates that are right: not reversing their operands where the
/// left one was loaded from memory, and of a shape the gate has passed (for
/// Rust code). An error when every candidate is refused, saying why.
fn choose(
    candidates: Vec<Candidate>,
    reorders: bool,
    ours: &[&Insn],
    plan: &MutantPlan,
    isa: &Isa,
) -> Result<Vec<Candidate>, String> {
    let all = candidates.len();
    let mut refusal = None;
    let kept: Vec<Candidate> = candidates
        .into_iter()
        .filter(|c| {
            if reorders && memory_source(ours[c.insn]) {
                refusal = Some(
                    "the left operand is loaded from memory, so a swap would reverse the operands"
                        .to_string(),
                );
                return false;
            }
            if is_rust(plan) && !isa.admits(&c.shape) {
                refusal = Some(format!("no harvested rustc shape `{}`", c.shape));
                return false;
            }
            true
        })
        .collect();
    match (kept.is_empty(), all) {
        (true, 0) => Ok(kept),
        (true, _) => Err(refusal.unwrap_or_default()),
        _ => Ok(kept),
    }
}

/// Whether the instruction reads its second operand from memory.
pub(super) fn memory_source(insn: &Insn) -> bool {
    x86::decode(insn.pc, &insn.bytes).is_some_and(|d| d.reads_memory_source())
}

/// The patch that makes instruction `k` do the swapped operation, with the
/// compare of its overflow check removed where there is one, and its shape.
fn candidate(swap: Swap, ours: &[&Insn], k: usize, isa: &Isa) -> Option<Candidate> {
    let insn = ours[k];
    let (matched, kind, cmp) = match swap {
        Swap::Integer { to_add } => {
            let matched = isa.arith_match(&insn.bytes, to_add)?;
            let checked = overflow_check(ours, k)?;
            (matched, Kind::Int(checked.check), checked.compare)
        }
        Swap::Float { from, with } => {
            (isa.float_match(&insn.bytes, from, with)?, Kind::Float, None)
        }
        Swap::Bitwise { from, with } => (
            isa.bitwise_match(&insn.bytes, from, with)?,
            Kind::Bitwise,
            None,
        ),
    };
    let (from, with) = swap.tokens();
    let shape = shape_of(&matched, kind, from, with);
    let mut regions = vec![Region {
        addr: insn.pc,
        original: insn.bytes.clone(),
        replacement: matched.replacement,
    }];
    if let Some(cmp) = cmp {
        regions.push(Region {
            addr: cmp.pc,
            original: cmp.bytes.clone(),
            replacement: vec![0x90; cmp.bytes.len()],
        });
    }
    Some(Candidate {
        insn: k,
        regions,
        shape,
    })
}

/// The shape of a swapped instruction.
pub(super) fn shape_of(matched: &Matched, kind: Kind, from: Token, with: Token) -> Shape {
    Shape {
        kind,
        width: matched.width,
        from,
        with,
        encoding: matched.encoding.clone(),
        memory: matched.memory,
    }
}

/// An integer operation's check of its overflow: how, and the compare to
/// drop.
pub(super) struct Checked<'a> {
    pub check: Check,
    pub compare: Option<&'a Insn>,
}

/// The check rustc emits after the checked operation `k`; `None` when no
/// check follows in a way that keeps the swapped operation exact.
///
/// Signed arithmetic is checked with the overflow flag (`seto; jo`), which
/// every add/sub variant sets as its own. Unsigned arithmetic is checked
/// by comparing the result with an operand (`cmp; jb`), which a swapped
/// operation would trip wrongly: without the compare, the jump reads the
/// carry flag, which is the swapped operation's own carry or borrow.
fn overflow_check<'a>(ours: &[&'a Insn], k: usize) -> Option<Checked<'a>> {
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
            (0x0 | 0x1, None) => Some(Checked {
                check: Check::Overflow,
                compare: None,
            }),
            (0x2 | 0x3, compare) => Some(Checked {
                check: Check::Carry,
                compare,
            }),
            _ => None,
        };
    }
    None
}
