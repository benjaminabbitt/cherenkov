//! Comparisons found by column. The instruction that reads the flags of
//! `lhs < rhs` (a conditional jump when the comparison steers code, a
//! `setcc` when it is a value) is the last one positioned inside the
//! comparison's own span, wherever the rest of its line goes: another
//! comparison joined by `&&`, a call in an operand, a closure. Swapping the
//! operator is a swap of that instruction's condition code.
//!
//! The condition code says which comparison the instruction reads, in which
//! operand order, and whether the code jumps when it holds or when it does
//! not, for a given operator: of the eight readings (two operand orders,
//! two polarities, signed or unsigned) at most one has that code, so the
//! mutated code follows from it. Where two readings disagree on it (`==`
//! to `<`: the code `e` does not say whether the operands are signed, or
//! which one `cmp` has first) nothing is patched. A float comparison is read
//! by the carry and zero flags with an unordered result in both, so a swap
//! stays exact only between `ja` and `jae`, and `==`/`!=`, which also read
//! the parity flag, is not patched.

use cherenkov_core::plan::MutantPlan;
use cherenkov_core::token::Token;

use super::column::{Shaped, is_rust};
use super::{Insn, Patch, Unpatchable};
use crate::isa::{Isa, Signedness};
use crate::shapes::{Consumer, Domain, Kind, Shape};
use crate::x86;

/// Why a comparison is not patched here.
pub(super) enum Refused {
    /// Its code is not found by column: another rule may still find it.
    Elsewhere,
    /// Its code is found and the swap would not be exact.
    Declined(Unpatchable),
}

const COMPARISONS: [Token; 6] = [
    Token::Lt,
    Token::Le,
    Token::Gt,
    Token::Ge,
    Token::Eq,
    Token::Ne,
];

/// Whether `t` is a comparison operator.
pub(super) fn is_comparison(t: Token) -> bool {
    COMPARISONS.contains(&t)
}

/// The condition code of the same instruction after `from` becomes `with`,
/// for an instruction reading `cc`; `None` where the readings of `cc` do not
/// agree on one, or a float would read an unordered result differently.
pub(crate) fn mutated_cc(
    isa: &Isa,
    domain: Domain,
    from: Token,
    with: Token,
    cc: u8,
) -> Option<u8> {
    let signs: &[Signedness] = match domain {
        Domain::Int => &[Signedness::Signed, Signedness::Unsigned],
        Domain::Float => &[Signedness::Unsigned],
    };
    let mut found: Option<u8> = None;
    for &s in signs {
        for swap in [false, true] {
            let (f, w) = if swap {
                (isa.swapped(from)?, isa.swapped(with)?)
            } else {
                (from, with)
            };
            let (Some(holds), Some(becomes)) = (isa.relational(s, f), isa.relational(s, w)) else {
                continue;
            };
            if domain == Domain::Float && !(ordered(isa, holds) && ordered(isa, becomes)) {
                continue;
            }
            for inverse in [false, true] {
                let (read, made) = if inverse {
                    (isa.inverse(holds), isa.inverse(becomes))
                } else {
                    (holds, becomes)
                };
                if read != cc {
                    continue;
                }
                if found.is_some_and(|p| p != made) {
                    return None;
                }
                found = Some(made);
            }
        }
    }
    found
}

/// Whether `cc` is false for an unordered float comparison: above, or
/// above or equal.
fn ordered(isa: &Isa, cc: u8) -> bool {
    [Token::Gt, Token::Ge]
        .iter()
        .any(|&t| isa.relational(Signedness::Unsigned, t) == Some(cc))
}

/// Every shape of comparison patch the rules can make.
pub(crate) fn universe(isa: &Isa) -> Vec<Shape> {
    let mut out = Vec::new();
    for from in COMPARISONS {
        for with in COMPARISONS.into_iter().filter(|&w| w != from) {
            for domain in [Domain::Int, Domain::Float] {
                for cc in 0..16u8 {
                    if mutated_cc(isa, domain, from, with, cc).is_none() {
                        continue;
                    }
                    for consumer in [Consumer::Jump, Consumer::Set] {
                        out.push(shape(isa, consumer, domain, from, with, cc));
                    }
                }
            }
        }
    }
    out
}

fn shape(isa: &Isa, consumer: Consumer, domain: Domain, from: Token, with: Token, cc: u8) -> Shape {
    Shape {
        kind: Kind::Compare(consumer, domain),
        width: 0,
        from,
        with,
        encoding: isa.cc_name(cc).unwrap_or("?").to_string(),
        memory: false,
    }
}

/// An instruction that reads the flags, and what it reads.
struct Reader<'a> {
    k: usize,
    insn: &'a Insn,
    consumer: Consumer,
    cc: u8,
}

fn reader<'a>(k: usize, insn: &'a Insn, isa: &Isa) -> Option<Reader<'a>> {
    if let Some(j) = isa.decode_jcc(&insn.bytes) {
        return Some(Reader {
            k,
            insn,
            consumer: Consumer::Jump,
            cc: j.cc,
        });
    }
    let cc = x86::decode(insn.pc, &insn.bytes)?.setcc_condition()?;
    Some(Reader {
        k,
        insn,
        consumer: Consumer::Set,
        cc,
    })
}

/// What the instruction before `k` compares, when it is what sets the
/// flags `k` reads.
fn compared(ours: &[&Insn], k: usize) -> Option<Domain> {
    let before = ours.get(k.checked_sub(1)?)?;
    x86::decode(before.pc, &before.bytes)?.compare_domain()
}

/// The swap of the comparison `from` -> `with` of `plan`, in the
/// instruction positioned in its span that reads its flags.
pub(super) fn comparison_at(
    plan: &MutantPlan,
    from: Token,
    with: Token,
    ours: &[&Insn],
    isa: &Isa,
) -> Result<Shaped, Refused> {
    let [lhs, rhs] = plan.operands[..] else {
        return Err(Refused::Elsewhere);
    };
    let span = ((lhs.start.line, lhs.start.col), (rhs.end.line, rhs.end.col));
    let last = ours
        .iter()
        .enumerate()
        .filter(|(_, i)| i.column != 0 && (span.0..span.1).contains(&(i.line, i.column)))
        .filter_map(|(k, i)| reader(k, i, isa))
        .max_by_key(|r| r.insn.pc)
        .ok_or(Refused::Elsewhere)?;
    let decline = |why: String| Refused::Declined(Unpatchable::UnsupportedEdit(why));
    let domain = compared(ours, last.k)
        .ok_or_else(|| decline("a comparison whose flags are not set by a compare".into()))?;
    let name = isa.cc_name(last.cc).unwrap_or("?");
    let made = mutated_cc(isa, domain, from, with, last.cc).ok_or_else(|| {
        Refused::Declined(Unpatchable::ConditionMismatch {
            cc: name.to_string(),
            token: format!("{} to {}", from.name(), with.name()),
        })
    })?;
    let shaped = shape(isa, last.consumer, domain, from, with, last.cc);
    if is_rust(plan) && !isa.admits(&shaped) {
        return Err(decline(format!("no harvested rustc shape `{shaped}`")));
    }
    Ok(Shaped {
        patch: rewrite(&last, made, isa).ok_or_else(|| decline("condition re-encoding".into()))?,
        shape: shaped,
    })
}

fn rewrite(r: &Reader<'_>, cc: u8, isa: &Isa) -> Option<Patch> {
    let bytes = match r.consumer {
        Consumer::Jump => {
            let j = isa.decode_jcc(&r.insn.bytes)?;
            isa.with_cc(&r.insn.bytes, &j, cc)
        }
        Consumer::Set => x86::decode(r.insn.pc, &r.insn.bytes)?.with_setcc_condition(cc)?,
    };
    Some(Patch::single(r.insn.pc, r.insn.bytes.clone(), bytes))
}
