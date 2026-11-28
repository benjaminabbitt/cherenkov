//! Tier P patches of a deleted `!` on a bool that rustc negates with
//! `xor al, 0xff` and normalises with an `and ..., 1` some instructions
//! later, after the bool was copied to the register the next call takes.
//! Plans come from the real planner on a snippet; the instructions are
//! rustc 1.96's debug code for deps' `constraint_changes` closure, with the
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

/// Line 5 is `(!kept).then(|| 1)`: `(` at column 9, `!` at 10, `kept` at 11.
const SNIPPET: &str = "pub fn f(held: &[bool]) -> Vec<u8> {
    held.iter().filter_map(|kept| {
        let kept = *kept;
        (!kept).then(|| 1)
    }).collect()
}";

fn plan() -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = format!("use crate::support::*;\n#[inline(never)]\n{SNIPPET}\n");
    let file = syntax
        .analyze(&rust, Path::new(FILE), Arc::from(src.as_str()))
        .unwrap();
    plan_file_sets(&file, &profile, &registry(), false)
        .plans
        .into_iter()
        .find(|p| p.mutation == Some("unary/delete-not"))
        .unwrap_or_else(|| panic!("no unary/delete-not plan"))
}

/// Rows of `(pc, bytes, column)`, all on line 6 of the file.
fn code(rows: &[(u64, &str, u32)]) -> Vec<Insn> {
    rows.iter()
        .map(|&(pc, hex, column)| Insn {
            pc,
            bytes: (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect(),
            file: FILE.into(),
            line: 6,
            column,
        })
        .collect()
}

fn derived(insns: &[Insn]) -> Result<(u64, String), Unpatchable> {
    let patch = derive(&plan(), FILE, insns, &Isa::amd64_unfiltered())?;
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

/// The closure's code: `kept` loaded (column 11), negated at the
/// parenthesised expression (9), the closure for `then` built (21), and
/// the bool copied to `esi` and normalised for the call (19).
fn negated_then_normalised() -> Vec<Insn> {
    code(&[
        (0x2000, "8a44243f", 11),   // mov al, [rsp+0x3f]
        (0x2004, "34ff", 9),        // xor al, 0xff
        (0x2006, "4889742440", 21), // mov [rsp+0x40], rsi
        (0x200b, "4889542448", 21), // mov [rsp+0x48], rdx
        (0x2010, "48894c2450", 21), // mov [rsp+0x50], rcx
        (0x2015, "0fb6f0", 19),     // movzx esi, al
        (0x2018, "83e601", 19),     // and esi, 1
        (0x201b, "488d542440", 19), // lea rdx, [rsp+0x40]
    ])
}

#[test]
fn test_delete_not_on_a_bool_normalised_after_a_copy_becomes_the_plain_normalisation() {
    let got = derived(&negated_then_normalised()).unwrap();
    // `xor al, 0xff` becomes `and al, 1`: the bool as it was.
    assert_eq!(got, (0x2004, "2401".to_string()));
}

#[test]
fn test_delete_not_is_declined_when_the_negated_byte_escapes_before_a_normalisation() {
    // The byte is stored whole before `and esi, 1`: it is no bool there.
    let insns = code(&[
        (0x2000, "8a44243f", 11),
        (0x2004, "34ff", 9),
        (0x2006, "88442420", 21), // mov [rsp+0x20], al
        (0x200a, "0fb6f0", 19),
        (0x200d, "83e601", 19),
    ]);
    let got = derived(&insns);
    assert!(
        matches!(got, Err(Unpatchable::UnsupportedEdit(_))),
        "{got:?}"
    );
}

#[test]
fn test_delete_not_is_declined_when_nothing_normalises_the_negated_byte() {
    // An integer's bitwise not: deleting its `!` does not undo it this way.
    let insns = code(&[
        (0x2000, "8a44243f", 11),
        (0x2004, "34ff", 9),
        (0x2006, "0fb6f0", 19),
        (0x2009, "488d542440", 19),
    ]);
    let got = derived(&insns);
    assert!(
        matches!(got, Err(Unpatchable::UnsupportedEdit(_))),
        "{got:?}"
    );
}

#[test]
fn test_delete_not_is_declined_when_a_jump_comes_before_any_normalisation() {
    let insns = code(&[
        (0x2000, "8a44243f", 11),
        (0x2004, "34ff", 9),
        (0x2006, "eb04", 19), // jmp +4
        (0x2008, "83e001", 19),
    ]);
    let got = derived(&insns);
    assert!(
        matches!(got, Err(Unpatchable::UnsupportedEdit(_))),
        "{got:?}"
    );
}
