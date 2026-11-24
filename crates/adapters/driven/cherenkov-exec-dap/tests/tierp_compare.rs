//! Tier P patches of comparison operators, found by the column of their
//! operands. Plans come from the real planner on a snippet; the
//! instructions are rustc 1.96's debug code for it, with the columns of its
//! line table (`tests/tierp_gate` runs the same shapes natively).

use std::path::Path;
use std::sync::Arc;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Insn, Unpatchable, derive};
use cherenkov_mutation::operator_set::registry;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_mutation::plan_sets::plan_file_sets;
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "s.rs";

/// The plan `mutation` of the function `body`, on line 3 of a file.
fn plan(body: &str, mutation: &str) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = format!("use crate::support::*;\n#[inline(never)]\n{body}\n");
    let file = syntax
        .analyze(&rust, Path::new(FILE), Arc::from(src.as_str()))
        .unwrap();
    plan_file_sets(&file, &profile, &registry(), false)
        .plans
        .into_iter()
        .find(|p| p.mutation == Some(mutation))
        .unwrap_or_else(|| panic!("no {mutation} plan"))
}

fn code(rows: &[(u64, &str, u32)]) -> Vec<Insn> {
    rows.iter()
        .map(|&(pc, hex, column)| Insn {
            pc,
            bytes: (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect(),
            file: FILE.into(),
            line: 3,
            column,
        })
        .collect()
}

/// `if a < 3 && b > 3 { 1 } else { 2 }` over `i32`: `a < 3` jumps to the
/// right operand when it holds, `b > 3` jumps to the else store when not.
const AND_CHAIN: &str = "pub fn f(a: i32, b: i32) -> u8 { if a < 3 && b > 3 { 1 } else { 2 } }";

fn and_chain() -> Vec<Insn> {
    code(&[
        (0x1000, "83ff03", 37),
        (0x1003, "7c07", 37),
        (0x1005, "c64424f702", 65),
        (0x100a, "eb0e", 34),
        (0x100c, "8b4424f0", 0),
        (0x1010, "83f803", 46),
        (0x1013, "7ef0", 46),
        (0x1015, "c64424f701", 54),
    ])
}

fn derived(body: &str, mutation: &str, insns: &[Insn]) -> Result<(u64, String), Unpatchable> {
    let patch = derive(&plan(body, mutation), FILE, insns, &Isa::amd64_unfiltered())?;
    let [region] = &patch.regions[..] else {
        panic!("one region, got {patch:?}");
    };
    let hex: String = region
        .replacement
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((region.addr, hex))
}

#[test]
fn test_derive_comparison_in_an_and_chain_swaps_the_jump_of_its_left_operand() {
    // `a < 3` to `a <= 3`: `jl` (taken when it holds) becomes `jle`.
    let got = derived(AND_CHAIN, "ror/lt->le", &and_chain()).unwrap();
    assert_eq!(got, (0x1003, "7e07".to_string()));
}

#[test]
fn test_derive_comparison_in_an_and_chain_swaps_the_jump_of_its_right_operand() {
    // `b > 3` to `b >= 3`: `jle` (taken when it fails) becomes `jl`.
    let got = derived(AND_CHAIN, "ror/gt->ge", &and_chain()).unwrap();
    assert_eq!(got, (0x1013, "7cf0".to_string()));
}

#[test]
fn test_derive_equality_to_an_ordering_is_declined_for_its_signedness_is_unknown() {
    // `cmp edi, esi; jne`: the same code for signed and unsigned operands.
    let src = "pub fn f(a: u32, b: u32) -> u8 { if a == b { 1 } else { 2 } }";
    let insns = code(&[(0x2000, "39f7", 37), (0x2002, "7507", 37)]);
    let got = derived(src, "ror/eq->le", &insns);
    assert!(
        matches!(got, Err(Unpatchable::ConditionMismatch { .. })),
        "{got:?}"
    );
}

#[test]
fn test_derive_equality_to_inequality_inverts_the_jump() {
    let src = "pub fn f(a: u32, b: u32) -> u8 { if a == b { 1 } else { 2 } }";
    let insns = code(&[(0x2000, "39f7", 37), (0x2002, "7507", 37)]);
    let got = derived(src, "ror/eq->ne", &insns).unwrap();
    assert_eq!(got, (0x2002, "7407".to_string()));
}

const FLOAT_LT: &str = "pub fn f(a: f64, b: f64) -> u8 { if a < b { 1 } else { 2 } }";

fn float_lt() -> Vec<Insn> {
    code(&[(0x3000, "660f2ec8", 37), (0x3004, "7707", 37)])
}

#[test]
fn test_derive_float_less_than_to_greater_than_is_declined() {
    // `ucomisd; ja` reads `b > a`: `a > b` needs the operands the other
    // way round, and `jb` would be true for a NaN.
    let got = derived(FLOAT_LT, "ror/lt->gt", &float_lt());
    assert!(
        matches!(got, Err(Unpatchable::ConditionMismatch { .. })),
        "{got:?}"
    );
}

#[test]
fn test_derive_float_less_than_to_less_or_equal_swaps_above_for_above_or_equal() {
    let got = derived(FLOAT_LT, "ror/lt->le", &float_lt()).unwrap();
    assert_eq!(got, (0x3004, "7307".to_string()));
}

#[test]
fn test_derive_float_equality_is_declined_for_it_reads_the_parity_flag() {
    let src = "pub fn f(a: f64, b: f64) -> u8 { if a == b { 1 } else { 2 } }";
    // `ucomisd; setnp cl; sete al; and al, cl; test al, 1; jne`
    let insns = code(&[
        (0x4000, "660f2ec1", 37),
        (0x4004, "0f9bc1", 37),
        (0x4007, "0f94c0", 37),
        (0x400a, "20c8", 37),
    ]);
    assert!(derived(src, "ror/eq->ne", &insns).is_err());
}

// The line-based rules, for code whose line table has no columns (a row's
// column is 0): the one conditional jump on the condition's line, or the
// `setcc` of a comparison made into a value.

/// Instructions without columns, on the given lines of the file.
fn lines(rows: &[(u64, &str, u32)]) -> Vec<Insn> {
    code(&[])
        .into_iter()
        .chain(rows.iter().map(|&(pc, hex, line)| {
            let mut i = code(&[(pc, hex, 0)]).remove(0);
            i.line = line;
            i
        }))
        .collect()
}

const IF_EQ: &str = "pub fn f(a: u32, b: u32) -> u8 {\n    if a == b {\n        1\n    } else {\n        2\n    }\n}";
const IF_LT_FLOAT: &str = "pub fn f(a: f64, b: f64) -> u8 {\n    if a < b {\n        1\n    } else {\n        2\n    }\n}";

/// `cmp edi, esi; jne else`, the then-store, `jmp end`, the else-store.
fn eq_branch() -> Vec<Insn> {
    lines(&[
        (0x5000, "39f7", 4),
        (0x5002, "7507", 4),
        (0x5004, "c64424f701", 5),
        (0x5009, "eb05", 5),
        (0x500b, "c64424f702", 7),
        (0x5010, "c3", 9),
    ])
}

fn declined(got: Result<(u64, String), Unpatchable>) {
    assert!(
        matches!(got, Err(Unpatchable::ConditionMismatch { .. })),
        "{got:?}"
    );
}

#[test]
fn test_derive_by_line_equality_to_an_ordering_is_declined_for_its_signedness_is_unknown() {
    declined(derived(IF_EQ, "ror/eq->le", &eq_branch()));
    declined(derived(IF_EQ, "ror/eq->ge", &eq_branch()));
}

#[test]
fn test_derive_by_line_equality_to_inequality_still_inverts_the_jump() {
    let got = derived(IF_EQ, "ror/eq->ne", &eq_branch()).unwrap();
    assert_eq!(got, (0x5002, "7407".to_string()));
}

#[test]
fn test_derive_by_line_float_less_than_to_greater_than_is_declined() {
    // `ucomisd; ja else-less`: `jbe` over the then-store when `b > a` fails.
    let insns = lines(&[
        (0x6000, "660f2ec8", 4),
        (0x6004, "7607", 4),
        (0x6006, "c64424f701", 5),
        (0x600b, "eb05", 5),
        (0x600d, "c64424f702", 7),
        (0x6012, "c3", 9),
    ]);
    declined(derived(IF_LT_FLOAT, "ror/lt->gt", &insns));
}

#[test]
fn test_derive_by_line_float_less_than_to_less_or_equal_swaps_above_for_above_or_equal() {
    let insns = lines(&[
        (0x6000, "660f2ec8", 4),
        (0x6004, "7607", 4),
        (0x6006, "c64424f701", 5),
        (0x600b, "eb05", 5),
        (0x600d, "c64424f702", 7),
        (0x6012, "c3", 9),
    ]);
    // `jbe` (taken when `b > a` fails) becomes `jb`.
    let got = derived(IF_LT_FLOAT, "ror/lt->le", &insns).unwrap();
    assert_eq!(got, (0x6004, "7207".to_string()));
}

// A short-circuit swap is not a comparison: the column rule leaves it to the
// line rules, which refuse an `&&` chain with its two jumps.

#[test]
fn test_derive_logical_swap_is_not_read_as_a_comparison() {
    let got = derived(AND_CHAIN, "lcr/and->or", &and_chain());
    assert!(
        matches!(got, Err(Unpatchable::ConditionalJumps { found: 2, .. })),
        "{got:?}"
    );
}

#[test]
fn test_derive_logical_to_equality_swap_is_not_read_as_a_comparison() {
    // `&&` to `==`: only the new operator is a comparison.
    let got = derived(AND_CHAIN, "lcr/and->eq", &and_chain());
    assert!(
        matches!(got, Err(Unpatchable::ConditionalJumps { found: 2, .. })),
        "{got:?}"
    );
}
