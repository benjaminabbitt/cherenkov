//! Tier P patches of a deleted match arm: the arm body's first instruction
//! becomes a jump to the catch-all arm's body. A body is found by its
//! span, not by the exact position of its first token: rustc gives a body
//! its first row at the first expression that makes code, which is often
//! not the body's first character. Plans come from the real planner on a
//! snippet; the instructions are rustc 1.96's debug code shapes, with the
//! columns of its line table.

use std::path::Path;
use std::sync::Arc;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Insn, Unpatchable, derive};
use cherenkov_mutation::operator_set::registry;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_mutation::plan_sets::plan_file_sets;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "s.rs";

/// The deletion of the arm `Some(v)` of the function `body`, which starts
/// on line 3 of a file.
fn plan(body: &str) -> MutantPlan {
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
        .find(|p| p.mutation == Some("match-arm/delete"))
        .unwrap_or_else(|| panic!("no match-arm/delete plan"))
}

/// Rows of `(pc, bytes, line, column)`.
fn code(rows: &[(u64, &str, u32, u32)]) -> Vec<Insn> {
    rows.iter()
        .map(|&(pc, hex, line, column)| Insn {
            pc,
            bytes: (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect(),
            file: FILE.into(),
            line,
            column,
        })
        .collect()
}

fn derived(body: &str, insns: &[Insn]) -> Result<(u64, String), Unpatchable> {
    let patch = derive(&plan(body), FILE, insns, &Isa::amd64_unfiltered())?;
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

/// A `mov dword [rsp+0x1c], 0`: eight bytes, room for a near jump.
const MOV: &str = "c744241c00000000";

/// The short `jmp` a patch at `from` writes to reach `to`, then the rest of
/// the eight bytes of [`MOV`] it replaces.
fn jump(from: u64, to: u64) -> String {
    let rel = i8::try_from(to as i64 - (from as i64 + 2)).unwrap();
    format!("eb{:02x}241c00000000", rel as u8)
}

/// Line 5 is `Some(v) => v + 1,` (pattern at column 9, body at 20), line 6
/// is `_ => return 0,` (`return` at 14, its value at 21).
const ARMS: &str = "pub fn f(x: Option<u8>) -> u8 {
    match x {
        Some(v) => v + 1,
        _ => return 0,
    }
}";

#[test]
fn test_derive_deleted_arm_redirects_from_the_first_code_in_its_body() {
    // The body `v + 1` has rows at its `+` (22) and `1` (24) but none at
    // its first character; the catch-all's body has a row at its value.
    let insns = code(&[
        (0x1000, MOV, 6, 21),
        (0x1010, MOV, 5, 22),
        (0x1020, MOV, 5, 24),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x1000)));
}

#[test]
fn test_derive_deleted_arm_with_code_at_its_body_start_redirects_from_there() {
    let insns = code(&[
        (0x1000, MOV, 6, 21),
        (0x1010, MOV, 5, 20),
        (0x1020, MOV, 5, 24),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x1000)));
}

#[test]
fn test_derive_deleted_arm_goes_to_the_lowest_address_of_the_catch_all_body() {
    // The catch-all's rows are not in source order: its lowest address is
    // where the body is entered.
    let insns = code(&[
        (0x0ff0, MOV, 6, 14),
        (0x1000, MOV, 6, 21),
        (0x1010, MOV, 5, 22),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x0ff0)));
}

#[test]
fn test_derive_deleted_arm_ignores_the_rows_of_its_pattern_when_its_body_has_code() {
    // Row (5,9) is the pattern's: a body with code of its own is redirected
    // from there, never from the pattern.
    let insns = code(&[
        (0x1000, MOV, 6, 21),
        (0x1008, MOV, 5, 9),
        (0x1010, MOV, 5, 22),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x1000)));
}

/// Line 5 is `Some(v) => v,`: the body makes no code, for `v` is already
/// where the match result goes; the binding of `v` (column 14) is the arm's
/// only code.
const BARE_BODY: &str = "pub fn f(x: Option<u8>) -> u8 {
    match x {
        Some(v) => v,
        _ => return 0,
    }
}";

#[test]
fn test_derive_deleted_arm_without_body_code_redirects_from_its_binding() {
    let insns = code(&[(0x1000, MOV, 6, 21), (0x1010, MOV, 5, 14)]);
    let got = derived(BARE_BODY, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x1000)));
}

#[test]
fn test_derive_deleted_arm_without_body_code_is_declined_when_its_pattern_code_jumps_conditionally()
{
    // `jne` among the pattern's rows: the arm's test is in there, not only
    // its binding.
    let insns = code(&[
        (0x1000, MOV, 6, 21),
        (0x1010, MOV, 5, 14),
        (0x1018, "7505", 5, 14),
    ]);
    let got = derived(BARE_BODY, &insns);
    assert!(matches!(got, Err(Unpatchable::NoCodeAt { .. })), "{got:?}");
}

#[test]
fn test_derive_deleted_arm_without_any_code_in_body_or_pattern_is_declined() {
    let insns = code(&[(0x1000, MOV, 6, 21)]);
    let got = derived(BARE_BODY, &insns);
    assert!(matches!(got, Err(Unpatchable::NoCodeAt { .. })), "{got:?}");
}

#[test]
fn test_derive_deleted_arm_into_a_catch_all_without_code_is_declined() {
    let insns = code(&[(0x1010, MOV, 5, 22)]);
    let got = derived(ARMS, &insns);
    assert!(
        matches!(got, Err(Unpatchable::NoCodeAt { line: 6, .. })),
        "{got:?}"
    );
}

/// `mov rax, [rsp]`: reloads the pointer the arm's value is written
/// through, attributed to the row before the body's first.
const RELOAD: &str = "488b0424";

#[test]
fn test_derive_deleted_arm_enters_the_catch_all_at_the_start_of_its_block() {
    // The catch-all's first row (6,21) is entered after a reload that the
    // line table gives to the match (4,15): the jump to the row would skip
    // it. The block starts after the `jmp` before it.
    let insns = code(&[
        (0x0ff4, "eb00", 4, 15),
        (0x0ff6, RELOAD, 4, 15),
        (0x0ffa, MOV, 6, 21),
        (0x1010, MOV, 5, 22),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x0ff6)));
}

#[test]
fn test_derive_deleted_arm_does_not_enter_the_catch_all_before_a_jump_destination() {
    // Code falls through into the catch-all's row from a block that has no
    // terminator before it, but a jump comes in at `0x0ffa`: that is where
    // the body's block starts, not at the code before it.
    let insns = code(&[
        (0x0ff0, "eb08", 4, 15),
        (0x0ff2, RELOAD, 4, 15),
        (0x0ff6, "90", 4, 15),
        (0x0ff7, "90", 4, 15),
        (0x0ff8, "90", 4, 15),
        (0x0ff9, "90", 4, 15),
        (0x0ffa, MOV, 6, 21),
        (0x1010, MOV, 5, 22),
    ]);
    let got = derived(ARMS, &insns).unwrap();
    assert_eq!(got, (0x1010, jump(0x1010, 0x0ffa)));
}
