//! Tier P arithmetic by column: an operator inside a call argument, a
//! struct field or a method chain, and a compound assignment through a
//! pointer, whose instruction is found by the column of its expression in
//! the line table. The instructions are rustc's (debug) as GDB and the DWARF
//! line table give them for the `rust-ops` fidelity fixture.

use std::path::Path;
use std::sync::Arc;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Insn, Patch, Region, Unpatchable, derive};
use cherenkov_mutation::concept::Capture;
use cherenkov_mutation::edit::Edit;
use cherenkov_mutation::operator_set::named;
use cherenkov_mutation::plan::{Action, MutantPlan, plan_file};
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::syntax::{LanguageId, Pos, Span};
use cherenkov_mutation::token::Token;
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "src/lib.rs";

/// One instruction: address, bytes, source line, source column.
type Row = (u64, &'static str, u32, u32);

fn insns(rows: &[Row]) -> Vec<Insn> {
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

fn region(addr: u64, original: &str, replacement: &str) -> Region {
    let bytes = |hex: &str| -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    };
    Region {
        addr,
        original: bytes(original),
        replacement: bytes(replacement),
    }
}

/// The rules alone: which instruction, in which order; the shapes table that
/// decides what rustc's output may be patched is tested below.
fn isa() -> Isa {
    Isa::amd64_unfiltered()
}

/// A plan for `lhs from rhs` whose left operand starts at `line:col`,
/// realized `action`.
fn arith_plan(
    from: Token,
    with: Token,
    line: u32,
    col: u32,
    lhs_text: &str,
    action: Action,
) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src =
        include_str!("../../../../test/fidelity/cherenkov-fidelity/fixtures/rust-ops/src/lib.rs");
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
        .unwrap();
    let ops = named("cargo-mutants").unwrap().operators();
    let mut p = plan_file(&file, &profile, &ops)
        .plans
        .into_iter()
        .find(|p| p.mutation == Some("aor/add->sub") && p.function.as_deref() == Some("arith"))
        .unwrap();
    let at = |col: u32| Pos { byte: 0, line, col };
    p.edit = Edit::ReplaceToken {
        capture: Capture::Op,
        from,
        with,
    };
    p.stop_at = at(col);
    p.site = Span {
        start: at(col + 20),
        end: at(col + 21),
    };
    p.operands = vec![
        Span {
            start: at(col),
            end: at(col + 10),
        },
        Span {
            start: at(col + 22),
            end: at(col + 30),
        },
    ];
    p.operand_texts = vec![lhs_text.to_string(), "1".to_string()];
    p.action = action;
    p
}

fn in_place() -> Action {
    Action::InPlace {
        expr_text: String::new(),
    }
}

fn patch(regions: Vec<Region>) -> Patch {
    Patch { regions }
}

// `g2(s + 1)` with `s: u64`: `add $1, rdi; mov; cmp rax, rdi; jb panic`.
fn seq() -> Vec<Insn> {
    insns(&[
        (0xb97f0, "4883ec18", 604, 0),
        (0xb97f4, "4889f8", 604, 0),
        (0xb97f7, "4889442410", 604, 0),
        (0xb97fc, "4889c7", 605, 8),
        (0xb97ff, "4883c701", 605, 8),
        (0xb9803, "48897c2408", 605, 8),
        (0xb9808, "4839c7", 605, 8),
        (0xb980b, "720f", 605, 8),
        (0xb980d, "488b7c2408", 605, 8),
        (0xb9812, "e879feffff", 605, 5),
        (0xb9817, "4883c418", 606, 2),
        (0xb981b, "c3", 606, 2),
        (0xb981c, "488d3d057b0600", 605, 8),
        (0xb9823, "ff1517b10600", 605, 8),
    ])
}

#[test]
fn test_in_place_unsigned_add_to_sub_swaps_the_op_and_drops_the_compare_so_the_borrow_is_checked() {
    let p = arith_plan(Token::Add, Token::Sub, 605, 8, "s", in_place());
    assert_eq!(
        derive(&p, FILE, &seq(), &isa()),
        Ok(patch(vec![
            region(0xb97ff, "4883c701", "4883ef01"),
            region(0xb9808, "4839c7", "909090"),
        ]))
    );
}

// `g2(a - b)` with unsigned operands: `sub rsi, rax; mov; cmp rsi, rdi; jb`.
fn su() -> Vec<Insn> {
    insns(&[
        (0xb9700, "4883ec18", 612, 0),
        (0xb970e, "4889f8", 613, 8),
        (0xb9711, "4829f0", 613, 8),
        (0xb9714, "48890424", 613, 8),
        (0xb9718, "4839f7", 613, 8),
        (0xb971b, "720e", 613, 8),
        (0xb971d, "488b3c24", 613, 8),
        (0xb9721, "e86affffff", 613, 5),
        (0xb9726, "4883c418", 614, 2),
        (0xb972a, "c3", 614, 2),
        (0xb972b, "488d3d967b0600", 613, 8),
        (0xb9732, "ff1528b40600", 613, 8),
    ])
}

#[test]
fn test_in_place_unsigned_sub_to_add_swaps_the_op_and_drops_the_compare_so_the_carry_is_checked() {
    let p = arith_plan(Token::Sub, Token::Add, 613, 8, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &su(), &isa()),
        Ok(patch(vec![
            region(0xb9711, "4829f0", "4801f0"),
            region(0xb9718, "4839f7", "909090"),
        ]))
    );
}

// `s.u -= 1; s.x -= 2;` through `&mut Tally`: an unsigned one compared with
// an immediate, then a signed one.
fn subassign() -> Vec<Insn> {
    insns(&[
        (0xbb6c0, "4883ec28", 639, 0),
        (0xbb6ce, "488b4710", 640, 5),
        (0xbb6d2, "4889c1", 640, 5),
        (0xbb6d5, "4883e901", 640, 5),
        (0xbb6d9, "48894c2418", 640, 5),
        (0xbb6de, "4883f801", 640, 5),
        (0xbb6e2, "7221", 640, 5),
        (0xbb6e4, "488b442410", 640, 5),
        (0xbb6e9, "488b4c2418", 640, 5),
        (0xbb6ee, "48894810", 640, 5),
        (0xbb6f2, "488b00", 641, 5),
        (0xbb6f5, "4883e802", 641, 5),
        (0xbb6f9, "4889442408", 641, 5),
        (0xbb6fe, "0f90c0", 641, 5),
        (0xbb701, "7021", 641, 5),
        (0xbb703, "eb0d", 641, 5),
        (0xbb705, "488d3d8c610600", 640, 5),
        (0xbb70c, "ff154e940600", 640, 5),
        (0xbb712, "488b442410", 640, 5),
        (0xbb717, "488b4c2408", 640, 5),
        (0xbb71c, "488908", 641, 5),
        (0xbb71f, "4883c428", 642, 2),
        (0xbb723, "c3", 642, 2),
        (0xbb724, "488d3d85610600", 641, 5),
        (0xbb72b, "ff152f940600", 641, 5),
    ])
}

#[test]
fn test_compound_unsigned_sub_assign_to_add_assign_drops_the_immediate_compare() {
    let p = arith_plan(
        Token::SubAssign,
        Token::AddAssign,
        640,
        5,
        "s.u",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &subassign(), &isa()),
        Ok(patch(vec![
            region(0xbb6d5, "4883e901", "4883c101"),
            region(0xbb6de, "4883f801", "90909090"),
        ]))
    );
}

#[test]
fn test_compound_signed_sub_assign_keeps_the_overflow_flag_check() {
    let p = arith_plan(
        Token::SubAssign,
        Token::AddAssign,
        641,
        5,
        "s.x",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &subassign(), &isa()),
        Ok(patch(vec![region(0xbb6f5, "4883e802", "4883c002")]))
    );
}

// `g2.x += 1` through a `MutexGuard<Tally>`: the `deref_mut` call shares the
// statement's line and column; the increment is `inc rax; mov; seto; jo`.
fn guard() -> Vec<Insn> {
    insns(&[
        (0xba80d, "488b05fcb60600", 588, 5),
        (0xba814, "488d7c2428", 588, 5),
        (0xba819, "ffd0", 588, 5),
        (0xba81b, "4889442418", 588, 5),
        (0xba820, "eb23", 588, 5),
        (0xba84a, "488b00", 588, 5),
        (0xba84d, "48ffc0", 588, 5),
        (0xba850, "4889442410", 588, 5),
        (0xba855, "0f90c0", 588, 5),
        (0xba858, "7022", 588, 5),
        (0xba85a, "488b442418", 588, 5),
        (0xba85f, "488b4c2410", 588, 5),
        (0xba864, "488908", 588, 5),
    ])
}

#[test]
fn test_compound_through_a_guard_swaps_inc_for_dec() {
    let p = arith_plan(
        Token::AddAssign,
        Token::SubAssign,
        588,
        5,
        "g.x",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &guard(), &isa()),
        Ok(patch(vec![region(0xba84d, "48ffc0", "48ffc8")]))
    );
}

// `*m.entry(k).or_default() += 1` on a `u32`: two calls, then
// `add $1, eax; mov; cmp ecx, eax; jb`.
fn entry() -> Vec<Insn> {
    insns(&[
        (0xba672, "488d7c2410", 593, 8),
        (0xba677, "ff1533b50600", 593, 8),
        (0xba67d, "488d7c2410", 593, 17),
        (0xba682, "ff1520ae0600", 593, 17),
        (0xba688, "48890424", 593, 17),
        (0xba68c, "8b08", 593, 5),
        (0xba68e, "89c8", 593, 5),
        (0xba690, "83c001", 593, 5),
        (0xba693, "8944240c", 593, 5),
        (0xba697, "39c8", 593, 5),
        (0xba699, "720f", 593, 5),
        (0xba69b, "488b0424", 593, 5),
        (0xba69f, "8b4c240c", 593, 5),
        (0xba6a3, "8908", 593, 5),
        (0xba6a5, "4883c448", 594, 2),
        (0xba6a9, "c3", 594, 2),
        (0xba6aa, "488d3dff6e0600", 593, 5),
        (0xba6b1, "ff1589a20600", 593, 5),
    ])
}

#[test]
fn test_compound_through_entry_or_default_swaps_the_add_and_drops_the_compare() {
    let p = arith_plan(
        Token::AddAssign,
        Token::SubAssign,
        593,
        5,
        "*m.entry(k).or_default()",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &entry(), &isa()),
        Ok(patch(vec![
            region(0xba690, "83c001", "83e801"),
            region(0xba697, "39c8", "9090"),
        ]))
    );
}

// `sum3(a + 1, 2, 3)` with `a: i64`: `inc rdi; mov; seto; jo`, the call's
// other arguments positioned at the call's column.
fn call_arg() -> Vec<Insn> {
    insns(&[
        (0xbb270, "4883ec18", 567, 0),
        (0xbb279, "48ffc7", 568, 10),
        (0xbb27c, "48897c2408", 568, 10),
        (0xbb281, "0f90c0", 568, 10),
        (0xbb284, "7019", 568, 10),
        (0xbb286, "488b7c2408", 568, 10),
        (0xbb28b, "be02000000", 568, 5),
        (0xbb290, "ba03000000", 568, 5),
        (0xbb295, "e8a6ecffff", 568, 5),
        (0xbb29a, "4883c418", 569, 2),
        (0xbb29e, "c3", 569, 2),
        (0xbb29f, "488d3d4a650600", 568, 10),
        (0xbb2a6, "ff1594960600", 568, 10),
    ])
}

#[test]
fn test_in_place_signed_add_in_a_call_argument_swaps_inc_for_dec() {
    let p = arith_plan(Token::Add, Token::Sub, 568, 10, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &call_arg(), &isa()),
        Ok(patch(vec![region(0xbb279, "48ffc7", "48ffcf")]))
    );
}

fn statement(line: u32) -> Action {
    let at = |col| Pos { byte: 0, line, col };
    Action::EvalThenJump {
        text: String::new(),
        target: String::new(),
        value: String::new(),
        resume: cherenkov_mutation::plan::Target::AfterStatement(Span {
            start: at(5),
            end: at(20),
        }),
    }
}

#[test]
fn test_a_statement_whose_line_swap_is_refused_falls_back_to_the_column() {
    // The line holds one `sub`, but rustc checks it by comparing: the
    // line rule refuses, the column rule drops the compare.
    let p = arith_plan(
        Token::SubAssign,
        Token::AddAssign,
        640,
        5,
        "s.u",
        statement(640),
    );
    assert_eq!(
        derive(&p, FILE, &subassign(), &isa()),
        Ok(patch(vec![
            region(0xbb6d5, "4883e901", "4883c101"),
            region(0xbb6de, "4883f801", "90909090"),
        ]))
    );
}

// --- Refusals: never a swap that could differ from the source build.

#[test]
fn test_an_operation_without_an_overflow_check_at_the_column_is_refused() {
    let unchecked = insns(&[(0x100, "4883c701", 7, 8), (0x104, "c3", 8, 2)]);
    let p = arith_plan(Token::Add, Token::Sub, 7, 8, "s", in_place());
    assert_eq!(
        derive(&p, FILE, &unchecked, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 7, found: 0 })
    );
}

#[test]
fn test_a_check_after_an_instruction_that_sets_flags_is_refused() {
    // `add; test al, al; jo`: the flags the jump reads are not the add's.
    let other = insns(&[
        (0x100, "4883c701", 7, 8),
        (0x104, "84c0", 7, 8),
        (0x106, "700e", 7, 8),
    ]);
    let p = arith_plan(Token::Add, Token::Sub, 7, 8, "s", in_place());
    assert_eq!(
        derive(&p, FILE, &other, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 7, found: 0 })
    );
}

#[test]
fn test_two_checked_operations_at_one_column_are_ambiguous() {
    // `a + b + c`: both adds are positioned at the expression's start.
    let two = insns(&[
        (0x100, "4801f7", 7, 8),
        (0x103, "0f90c0", 7, 8),
        (0x106, "7010", 7, 8),
        (0x108, "4801d7", 7, 8),
        (0x10b, "0f90c0", 7, 8),
        (0x10e, "7010", 7, 8),
    ]);
    let p = arith_plan(Token::Add, Token::Sub, 7, 8, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &two, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 7, found: 2 })
    );
}

#[test]
fn test_an_add_at_another_column_is_not_the_operators() {
    let p = arith_plan(Token::Add, Token::Sub, 605, 30, "s", in_place());
    assert!(derive(&p, FILE, &seq(), &isa()).is_err());
}

#[test]
fn test_a_literal_left_operand_is_refused_for_a_swap_that_does_not_commute() {
    // `1 + x` is compiled with the constant as the right operand of the
    // add, so a `sub` computes `x - 1`, not `1 - x`.
    let p = arith_plan(Token::Add, Token::Sub, 568, 10, "1", in_place());
    assert!(derive(&p, FILE, &call_arg(), &isa()).is_err());
    // `1 - x` to `1 + x` commutes: allowed.
    let p = arith_plan(Token::Sub, Token::Add, 625, 8, "1", in_place());
    let sub = insns(&[
        (0x100, "29f7", 625, 8),
        (0x102, "897c240c", 625, 8),
        (0x106, "0f90c0", 625, 8),
        (0x109, "700e", 625, 8),
    ]);
    assert!(derive(&p, FILE, &sub, &isa()).is_ok());
}

#[test]
fn test_integer_mul_and_cross_operator_swaps_have_no_rule() {
    // `mul`/`imul` have other shapes than `add`/`sub`; `+` to `*` too.
    for (from, with) in [
        (Token::Add, Token::Mul),
        (Token::Mul, Token::Add),
        (Token::Mul, Token::Div),
        (Token::AddAssign, Token::MulAssign),
    ] {
        let p = arith_plan(from, with, 568, 10, "a", in_place());
        assert!(
            derive(&p, FILE, &call_arg(), &isa()).is_err(),
            "{from:?} -> {with:?}"
        );
    }
}

// `f(a - b) + f(a * b) + f(a / b)` on f64: scalar SSE, positioned at each
// operation's expression; the two `addsd` share the sum's column.
fn floats() -> Vec<Insn> {
    insns(&[
        (0xb9610, "4883ec38", 630, 0),
        (0xb962c, "f20f5cc1", 631, 7),
        (0xb9630, "e8bbffffff", 631, 5),
        (0xb9635, "f20f104c2410", 631, 5),
        (0xb964a, "f20f59c1", 631, 18),
        (0xb964e, "e89dffffff", 631, 16),
        (0xb9668, "f20f58d3", 631, 5),
        (0xb966c, "f20f11542420", 631, 5),
        (0xb9672, "f20f5ec1", 631, 29),
        (0xb9676, "e875ffffff", 631, 27),
        (0xb9684, "f20f58c1", 631, 5),
        (0xb9688, "4883c438", 632, 2),
    ])
}

#[test]
fn test_float_sub_to_add_swaps_the_opcode_at_its_column() {
    let p = arith_plan(Token::Sub, Token::Add, 631, 7, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &floats(), &isa()),
        Ok(patch(vec![region(0xb962c, "f20f5cc1", "f20f58c1")]))
    );
}

#[test]
fn test_float_mul_to_div_and_back_swap_the_opcode_at_their_columns() {
    let p = arith_plan(Token::Mul, Token::Div, 631, 18, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &floats(), &isa()),
        Ok(patch(vec![region(0xb964a, "f20f59c1", "f20f5ec1")]))
    );
    let p = arith_plan(Token::Div, Token::Mul, 631, 29, "a", in_place());
    assert_eq!(
        derive(&p, FILE, &floats(), &isa()),
        Ok(patch(vec![region(0xb9672, "f20f5ec1", "f20f59c1")]))
    );
}

#[test]
fn test_float_sums_sharing_a_column_are_ambiguous() {
    let p = arith_plan(Token::Add, Token::Sub, 631, 5, "f(a - b)", in_place());
    assert_eq!(
        derive(&p, FILE, &floats(), &isa()),
        Err(Unpatchable::ArithmeticInstructions {
            line: 631,
            found: 2
        })
    );
}

#[test]
fn test_float_swaps_across_precedence_are_refused_for_plain_operators_only() {
    // `a + b` as `a * b` regroups `x + a * b` neighbours: no rule; but
    // `x += a` as `x *= a` has no neighbours.
    let p = arith_plan(Token::Sub, Token::Mul, 631, 7, "a", in_place());
    assert!(derive(&p, FILE, &floats(), &isa()).is_err());
    let p = arith_plan(
        Token::SubAssign,
        Token::MulAssign,
        631,
        7,
        "s.w",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &floats(), &isa()),
        Ok(patch(vec![region(0xb962c, "f20f5cc1", "f20f59c1")]))
    );
}

#[test]
fn test_bitwise_compound_assignment_swaps_the_opcode_and_binary_bitwise_has_no_rule() {
    // `*flags |= bit`: `or rcx, rax`.
    let or = insns(&[(0x100, "4809c8", 9, 5), (0x103, "c3", 10, 1)]);
    let p = arith_plan(
        Token::BitOrAssign,
        Token::BitAndAssign,
        9,
        5,
        "*flags",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &or, &isa()),
        Ok(patch(vec![region(0x100, "4809c8", "4821c8")]))
    );
    let p = arith_plan(
        Token::BitOrAssign,
        Token::BitXorAssign,
        9,
        5,
        "*flags",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &or, &isa()),
        Ok(patch(vec![region(0x100, "4809c8", "4831c8")]))
    );
    // `a | b` as `a & b` regroups its neighbours (`&` binds tighter).
    let p = arith_plan(Token::BitOr, Token::BitAnd, 9, 5, "a", in_place());
    assert!(derive(&p, FILE, &or, &isa()).is_err());
}

#[test]
fn test_a_zeroing_xor_at_the_column_is_not_a_bitwise_operation() {
    let zero = insns(&[(0x100, "31c0", 9, 5), (0x102, "c3", 10, 1)]);
    let p = arith_plan(
        Token::BitXorAssign,
        Token::BitOrAssign,
        9,
        5,
        "x",
        in_place(),
    );
    assert_eq!(
        derive(&p, FILE, &zero, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 9, found: 0 })
    );
}

// --- Where the plan's action is not `InPlace`, and where it has no operands.

#[test]
fn test_a_compound_assignment_has_no_operands_and_starts_where_its_statement_does() {
    let mut p = arith_plan(
        Token::AddAssign,
        Token::SubAssign,
        588,
        5,
        "g.x",
        statement(588),
    );
    p.operands.clear();
    assert_eq!(
        derive(&p, FILE, &guard(), &isa()),
        Ok(patch(vec![region(0xba84d, "48ffc0", "48ffc8")]))
    );
}

#[test]
fn test_a_returned_value_and_a_condition_find_their_operator_by_column_too() {
    let returned = Action::EvalReturn {
        value_text: String::new(),
    };
    let p = arith_plan(Token::Add, Token::Sub, 568, 10, "a", returned);
    assert_eq!(
        derive(&p, FILE, &call_arg(), &isa()),
        Ok(patch(vec![region(0xbb279, "48ffc7", "48ffcf")]))
    );
    let branch = Action::EvalBranch {
        cond_text: String::new(),
        if_true: cherenkov_mutation::plan::Target::BlockStart(Span {
            start: Pos {
                byte: 0,
                line: 568,
                col: 1,
            },
            end: Pos {
                byte: 0,
                line: 568,
                col: 2,
            },
        }),
        if_false: cherenkov_mutation::plan::Target::AfterStatement(Span {
            start: Pos {
                byte: 0,
                line: 568,
                col: 1,
            },
            end: Pos {
                byte: 0,
                line: 568,
                col: 2,
            },
        }),
    };
    let p = arith_plan(Token::Add, Token::Sub, 568, 10, "a", branch);
    assert_eq!(
        derive(&p, FILE, &call_arg(), &isa()),
        Ok(patch(vec![region(0xbb279, "48ffc7", "48ffcf")]))
    );
}

// --- Commutative sources whose left operand rustc loaded from memory.
//
// rustc folds the memory operand into the commutative operation and puts the
// other operand in the register: `a += b` is `addsd mem(a), xmm1(b)` with the
// result in `xmm1`. Swapping the opcode to `subsd` computes `b - a`, where the
// compiled `a -= b` computes `a - b`. Such a swap is refused (it is exact
// for a non-commutative source, for a commutative target, and where the
// operand order is known).

/// `let mut a = a; a += b; a` on f64, as rustc (debug) compiles it.
fn float_assign(opcode: &str) -> Vec<Insn> {
    let op = format!("f20f{opcode}4c24e8");
    let rows: Vec<Row> = vec![
        (0x9d060, "f20f114424f0", 3, 0),
        (0x9d066, "f20f114c24f8", 3, 0),
        (0x9d06c, "f20f114424e8", 3, 47),
        (0x9d072, Box::leak(op.into_boxed_str()), 3, 50),
        (0x9d078, "f20f114c24e8", 3, 50),
        (0x9d07e, "f20f104424e8", 3, 58),
        (0x9d084, "c3", 3, 61),
    ];
    insns(&rows)
}

/// `*m += k` on isize: `add rsi, [rdi]` with the result in `rsi`.
fn int_through_a_reference(opcode: &str) -> Vec<Insn> {
    let op = format!("48{opcode}37");
    let rows: Vec<Row> = vec![
        (0x89a90, "4883ec28", 3, 0),
        (0x89a99, "48897c2418", 3, 0),
        (0x89aa3, Box::leak(op.into_boxed_str()), 3, 37),
        (0x89aa6, "4889742410", 3, 37),
        (0x89aab, "0f90c0", 3, 37),
        (0x89aae, "7012", 3, 37),
        (0x89ab0, "488b442408", 0, 37),
        (0x89ab5, "488b4c2410", 0, 37),
        (0x89aba, "488908", 3, 37),
        (0x89abd, "4883c428", 3, 47),
        (0x89ac1, "c3", 3, 47),
        (0x89ac2, "488d3ddf0e0800", 3, 37),
        (0x89ac9, "ff15f9780800", 3, 37),
    ];
    insns(&rows)
}

fn compound_plan(from: Token, with: Token, col: u32) -> MutantPlan {
    let mut p = arith_plan(from, with, 3, col, "a", statement(3));
    p.operands.clear();
    p
}

#[test]
fn test_float_assign_that_commutes_in_source_is_refused_for_a_swap_that_does_not() {
    for (from, with, opcode) in [
        (Token::AddAssign, Token::SubAssign, "58"),
        (Token::AddAssign, Token::DivAssign, "58"),
        (Token::MulAssign, Token::SubAssign, "59"),
        (Token::MulAssign, Token::DivAssign, "59"),
    ] {
        let p = compound_plan(from, with, 50);
        assert!(
            derive(&p, FILE, &float_assign(opcode), &isa()).is_err(),
            "{from:?} -> {with:?}"
        );
    }
}

#[test]
fn test_float_assign_that_commutes_in_source_is_swapped_for_a_swap_that_commutes() {
    let p = compound_plan(Token::AddAssign, Token::MulAssign, 50);
    assert_eq!(
        derive(&p, FILE, &float_assign("58"), &isa()),
        Ok(patch(vec![region(0x9d072, "f20f584c24e8", "f20f594c24e8")]))
    );
    let p = compound_plan(Token::SubAssign, Token::AddAssign, 50);
    assert_eq!(
        derive(&p, FILE, &float_assign("5c"), &isa()),
        Ok(patch(vec![region(0x9d072, "f20f5c4c24e8", "f20f584c24e8")]))
    );
}

#[test]
fn test_signed_assign_through_a_reference_is_refused_for_add_to_sub() {
    let p = compound_plan(Token::AddAssign, Token::SubAssign, 37);
    assert!(derive(&p, FILE, &int_through_a_reference("03"), &isa()).is_err());
}

#[test]
fn test_signed_assign_through_a_reference_is_swapped_for_sub_to_add() {
    let p = compound_plan(Token::SubAssign, Token::AddAssign, 37);
    assert_eq!(
        derive(&p, FILE, &int_through_a_reference("2b"), &isa()),
        Ok(patch(vec![region(0x89aa3, "482b37", "480337")]))
    );
}

// --- The shapes table: Rust code's arithmetic is patched only for a shape
// the gate (tests/tierp_gate) has passed.

use cherenkov_exec_dap::shapes::Table;

/// `g2(s + 1)` on u64 is `int w64 carry add->sub 83/0 reg`.
const SEQ_SHAPE: &str = "int w64 carry add->sub 83/0 reg";

fn table(pass: &[&str]) -> Table {
    let list = pass
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    Table::parse(&format!(
        "rustc = \"test\"\npanic_call = \"kept\"\npass = [{list}]\ndeclined = []\n"
    ))
    .unwrap()
}

#[test]
fn test_rust_arithmetic_of_a_shape_in_the_table_is_patched() {
    let p = arith_plan(Token::Add, Token::Sub, 605, 8, "s", in_place());
    let isa = Isa::amd64_unfiltered().with_shapes(table(&[SEQ_SHAPE]));
    assert!(derive(&p, FILE, &seq(), &isa).is_ok());
}

#[test]
fn test_rust_arithmetic_of_a_shape_the_table_lacks_is_declined() {
    let p = arith_plan(Token::Add, Token::Sub, 605, 8, "s", in_place());
    let isa = Isa::amd64_unfiltered().with_shapes(table(&[]));
    let refused = derive(&p, FILE, &seq(), &isa).unwrap_err().to_string();
    assert!(refused.contains(SEQ_SHAPE), "{refused}");
}

#[test]
fn test_arithmetic_of_other_languages_does_not_need_the_rustc_table() {
    let mut p = arith_plan(Token::Add, Token::Sub, 605, 8, "s", in_place());
    p.file = "main.go".into();
    let isa = Isa::amd64_unfiltered().with_shapes(table(&[]));
    assert!(derive(&p, FILE, &seq(), &isa).is_ok());
}

#[test]
fn test_a_statements_line_swap_is_declined_for_a_shape_the_table_lacks() {
    // Both the line's rule and the column's refuse it.
    let p = arith_plan(Token::Add, Token::Sub, 605, 8, "s", statement(605));
    let isa = Isa::amd64_unfiltered().with_shapes(table(&[]));
    assert!(derive(&p, FILE, &seq(), &isa).is_err());
}

// --- inc <-> dec under the unsigned check.

#[test]
fn test_an_unsigned_increment_is_not_swapped_for_a_decrement_because_dec_leaves_the_carry() {
    // `x += 1` on u64: `inc rax; mov; cmp; jb`. Without the compare the
    // `jb` would read the carry flag, which `inc` and `dec` do not write.
    let unsigned = insns(&[
        (0x100, "48ffc0", 9, 5),
        (0x103, "4889442408", 9, 5),
        (0x108, "4839c8", 9, 5),
        (0x10b, "720f", 9, 5),
    ]);
    let p = arith_plan(Token::Add, Token::Sub, 9, 5, "x", in_place());
    assert!(derive(&p, FILE, &unsigned, &isa()).is_err());
    // The signed check reads the overflow flag, which both write.
    let signed = insns(&[
        (0x100, "48ffc0", 9, 5),
        (0x103, "4889442408", 9, 5),
        (0x108, "0f90c1", 9, 5),
        (0x10b, "700f", 9, 5),
    ]);
    assert_eq!(
        derive(&p, FILE, &signed, &isa()),
        Ok(patch(vec![region(0x100, "48ffc0", "48ffc8")]))
    );
}

// --- The statement's own line rule, before the column's.

/// A statement of one line holding a single unchecked `op` (no check
/// follows) at no column the plan names, so only the line rule can find it.
fn lone(hex: &'static str) -> Vec<Insn> {
    insns(&[(0x100, hex, 20, 0), (0x103, "c3", 21, 2)])
}

#[test]
fn test_a_statement_sub_with_no_code_at_its_column_is_swapped_to_add_by_its_line() {
    let p = arith_plan(Token::Sub, Token::Add, 20, 5, "a", statement(20));
    assert_eq!(
        derive(&p, FILE, &lone("4829c8"), &isa()),
        Ok(patch(vec![region(0x100, "4829c8", "4801c8")]))
    );
}

#[test]
fn test_a_statement_add_with_no_code_at_its_column_is_swapped_to_sub_by_its_line() {
    let p = arith_plan(Token::Add, Token::Sub, 20, 5, "a", statement(20));
    assert_eq!(
        derive(&p, FILE, &lone("4801c8"), &isa()),
        Ok(patch(vec![region(0x100, "4801c8", "4829c8")]))
    );
}

#[test]
fn test_a_statement_add_whose_left_operand_is_in_memory_is_refused_for_sub() {
    // `add rax, [rbp-8]`: a `sub` there computes `b - a`.
    let p = arith_plan(Token::Add, Token::Sub, 20, 5, "a", statement(20));
    let refused = derive(&p, FILE, &lone("48034df8"), &isa())
        .unwrap_err()
        .to_string();
    assert!(refused.contains("loaded from memory"), "{refused}");
}

#[test]
fn test_a_statement_sub_with_its_second_operand_in_memory_is_swapped_to_add() {
    // `sub rcx, [rbp-8]` -> `add rcx, [rbp-8]` keeps the operand order.
    let p = arith_plan(Token::Sub, Token::Add, 20, 5, "a", statement(20));
    assert_eq!(
        derive(&p, FILE, &lone("482b4df8"), &isa()),
        Ok(patch(vec![region(0x100, "482b4df8", "48034df8")]))
    );
}

#[test]
fn test_a_statements_line_swap_follows_the_shapes_table() {
    let p = arith_plan(Token::Add, Token::Sub, 20, 5, "a", statement(20));
    let shape = "int w64 none add->sub 01 reg";
    let admitted = Isa::amd64_unfiltered().with_shapes(table(&[shape]));
    assert_eq!(
        derive(&p, FILE, &lone("4801c8"), &admitted),
        Ok(patch(vec![region(0x100, "4801c8", "4829c8")]))
    );
    let declined = Isa::amd64_unfiltered().with_shapes(table(&[]));
    let refused = derive(&p, FILE, &lone("4801c8"), &declined)
        .unwrap_err()
        .to_string();
    assert!(refused.contains(shape), "{refused}");
}

// --- Which swaps an edit asks for.

#[test]
fn test_a_plain_operator_swapped_for_a_compound_one_is_not_patched() {
    // `s + 1` -> `s -= 1` is no swap of one instruction.
    let p = arith_plan(Token::Add, Token::SubAssign, 605, 8, "s", in_place());
    assert!(matches!(
        derive(&p, FILE, &seq(), &isa()),
        Err(Unpatchable::UnsupportedEdit(_))
    ));
}

#[test]
fn test_an_edit_from_an_operator_to_itself_is_not_patched() {
    let p = arith_plan(
        Token::AddAssign,
        Token::AddAssign,
        640,
        5,
        "s.u",
        in_place(),
    );
    assert!(matches!(
        derive(&p, FILE, &subassign(), &isa()),
        Err(Unpatchable::UnsupportedEdit(_))
    ));
}

// --- The overflow check must follow its operation directly.

#[test]
fn test_a_check_on_another_line_than_its_operation_is_not_the_operations() {
    // `add rdi, 1; seto cl; jo`, the `seto` and the `jo` on line 8.
    let rows = insns(&[
        (0x100, "4883c701", 7, 8),
        (0x104, "0f90c1", 8, 8),
        (0x107, "700f", 8, 8),
    ]);
    let p = arith_plan(Token::Add, Token::Sub, 7, 8, "x", in_place());
    assert!(derive(&p, FILE, &rows, &isa()).is_err());
}

#[test]
fn test_a_check_after_a_gap_in_the_code_is_not_the_operations() {
    // The `seto; jo` of the same line, but not where the add ends.
    let rows = insns(&[
        (0x100, "4883c701", 7, 8),
        (0x110, "0f90c1", 7, 8),
        (0x113, "700f", 7, 8),
    ]);
    let p = arith_plan(Token::Add, Token::Sub, 7, 8, "x", in_place());
    assert!(derive(&p, FILE, &rows, &isa()).is_err());
}
