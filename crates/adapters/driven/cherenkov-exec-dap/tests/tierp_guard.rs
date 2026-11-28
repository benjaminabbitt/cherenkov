//! The exits of a match guard whose arm has an empty body, for tier B:
//! with no code in the arm to land on, the true exit is the one that goes on
//! past the match, and the false exit the one that falls to the next arm,
//! positioned just past this arm. Plans come from the real planner on a
//! snippet; the instructions are the shape of rustc 1.96's debug code for
//! it, with the columns of its line table.

use std::path::Path;
use std::sync::Arc;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Insn, Unpatchable, branch_exits};
use cherenkov_mutation::operator_set::registry;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_mutation::plan_sets::plan_file_sets;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "s.rs";

/// Line 5 is the guarded arm with its empty body, line 6 the arm after it.
const EMPTY_ARM: &str = "pub fn f(k: &str) -> u8 {
    match k {
        \"a\" if k.is_empty() => {}
        _ => return 1,
    }
    2
}";

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

/// The guard `k.is_empty()` is on line 5, column 18: its result is tested,
/// `jne` goes past the match (line 8), `jmp` on to the next arm (line 6).
fn guard(past_match: (u32, u32), next_arm: (u32, u32)) -> Vec<Insn> {
    code(&[
        (0x1000, "a801", 5, 18),
        (0x1002, "7504", 5, 18),
        (0x1004, "eb0c", 5, 18),
        (0x1008, "90", past_match.0, past_match.1),
        (0x1012, "90", next_arm.0, next_arm.1),
    ])
}

#[test]
fn test_guard_of_an_empty_arm_exits_true_past_the_match_and_false_to_the_next_arm() {
    for mutation in ["match-guard/true", "match-guard/false"] {
        let plan = plan(EMPTY_ARM, mutation);
        let exits = branch_exits(
            &plan,
            FILE,
            &guard((8, 5), (6, 9)),
            &Isa::amd64_unfiltered(),
        )
        .unwrap_or_else(|e| panic!("{mutation}: {e:?}"));
        assert_eq!(exits.stop, 0x1000, "{mutation}");
        assert_eq!(exits.if_true, 0x1008, "{mutation}: past the match");
        assert_eq!(exits.if_false, 0x1012, "{mutation}: the next arm");
    }
}

#[test]
fn test_guard_of_an_empty_arm_takes_the_exit_nearest_past_it_as_false_in_either_order() {
    // The exits swap their addresses: the positions decide, not the jumps.
    let plan = plan(EMPTY_ARM, "match-guard/true");
    let exits = branch_exits(
        &plan,
        FILE,
        &guard((6, 9), (8, 5)),
        &Isa::amd64_unfiltered(),
    )
    .unwrap();
    assert_eq!((exits.if_true, exits.if_false), (0x1012, 0x1008));
}

#[test]
fn test_guard_of_an_empty_arm_with_no_exit_past_the_arm_is_declined() {
    let plan = plan(EMPTY_ARM, "match-guard/true");
    let got = branch_exits(
        &plan,
        FILE,
        &guard((3, 5), (3, 6)),
        &Isa::amd64_unfiltered(),
    );
    assert!(
        matches!(got, Err(Unpatchable::UnsupportedEdit(_))),
        "{got:?}"
    );
}

/// The same arm, with code in its body: the exit into the body is the true
/// one, as ever.
const FULL_ARM: &str = "pub fn f(k: &str) -> u8 {
    match k {
        \"a\" if k.is_empty() => { g(); }
        _ => return 1,
    }
    2
}";

#[test]
fn test_guard_of_an_arm_with_code_exits_true_into_its_body() {
    let plan = plan(FULL_ARM, "match-guard/true");
    let insns = code(&[
        (0x1000, "a801", 5, 18),
        (0x1002, "7504", 5, 18),
        (0x1004, "eb0c", 5, 18),
        (0x1008, "90", 5, 36),
        (0x1012, "90", 6, 9),
    ]);
    let exits = branch_exits(&plan, FILE, &insns, &Isa::amd64_unfiltered()).unwrap();
    assert_eq!((exits.if_true, exits.if_false), (0x1008, 0x1012));
}
