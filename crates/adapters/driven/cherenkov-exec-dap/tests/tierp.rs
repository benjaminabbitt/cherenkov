//! Tier P patch derivation. Plans come from the real planner on the Go
//! fixture; instructions are the fixture's Go 1.27 `-N -l` disassembly
//! (docs/m1-delve-findings.md), varied only where a test says so.

use std::path::Path;
use std::sync::Arc;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Insn, Patch, Region, Unpatchable, derive};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "/src/calc.go";

fn plan(operator: &str, line: u32) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let go = LanguageId::new("go");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == go)
        .unwrap();
    let src = include_str!("../../../../../fixtures/go/calc.go");
    let file = syntax
        .analyze(&go, Path::new("calc.go"), Arc::from(src))
        .unwrap();
    plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .find(|p| p.operator == operator && p.stop_at.line == line)
        .unwrap_or_else(|| panic!("no {operator} plan on line {line}"))
}

fn insns(rows: &[(u64, &str, u32)]) -> Vec<Insn> {
    rows.iter()
        .map(|&(pc, hex, line)| Insn {
            pc,
            bytes: hex_bytes(hex),
            file: FILE.into(),
            line,
            column: 0,
        })
        .collect()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn region(addr: u64, original: &str, replacement: &str) -> Region {
    Region {
        addr,
        original: hex_bytes(original),
        replacement: hex_bytes(replacement),
    }
}

fn patch(addr: u64, original: &str, replacement: &str) -> Patch {
    Patch {
        regions: vec![region(addr, original, replacement)],
    }
}

/// `IsSmall`: `if n < 10 { return true }; return false`.
/// Line 12 is `CMPQ AX, $10; JL then; JMP else`.
fn is_small(jcc: &str) -> Vec<Insn> {
    insns(&[
        (0x5d2360, "55", 11),
        (0x5d2361, "4889e5", 11),
        (0x5d2364, "4883ec08", 11),
        (0x5d2368, "4889442418", 11),
        (0x5d236d, "c644240700", 11),
        (0x5d2372, "4883f80a", 12),
        (0x5d2376, jcc, 12),
        (0x5d2378, "eb10", 12),
        (0x5d237a, "c644240701", 13),
        (0x5d237f, "b801000000", 13),
        (0x5d2384, "4883c408", 13),
        (0x5d2388, "5d", 13),
        (0x5d2389, "c3", 13),
        (0x5d238a, "c644240700", 15),
        (0x5d238f, "31c0", 15),
        (0x5d2391, "4883c408", 15),
        (0x5d2395, "5d", 15),
        (0x5d2396, "c3", 15),
    ])
}

/// `CountUp`: `for i < n { i = i + 1 }`. The compiler swapped the operands:
/// line 20 is `CMPQ n, i; JG body; JMP after`.
fn count_up() -> Vec<Insn> {
    insns(&[
        (0x5d23c0, "eb00", 20),
        (0x5d23c2, "488b4c2408", 20),
        (0x5d23c7, "48394c2420", 20),
        (0x5d23cc, "7f02", 20),
        (0x5d23ce, "eb09", 20),
        (0x5d23d0, "48ff442408", 21),
        (0x5d23d5, "eb00", 21),
        (0x5d23d7, "ebe9", 20),
        (0x5d23d9, "488b442408", 23),
    ])
}

/// `Total`: `s := a + b; log(s); return s`. Line 7 is one 5-byte CALL.
fn total() -> Vec<Insn> {
    insns(&[
        (0x5d2321, "4801d8", 6),
        (0x5d2324, "4889442410", 6),
        (0x5d2329, "e8b2ffffff", 7),
        (0x5d232e, "488b442410", 8),
        (0x5d2333, "4889442408", 8),
    ])
}

fn isa() -> Isa {
    Isa::amd64()
}

// --- Condition negation: invert the condition code, whatever the polarity.

#[test]
fn test_negate_condition_inverts_the_one_conditional_jump() {
    let got = derive(
        &plan("negate-condition", 12),
        FILE,
        &is_small("7c02"),
        &isa(),
    );
    assert_eq!(got, Ok(patch(0x5d2376, "7c02", "7d02")));
}

#[test]
fn test_negate_condition_on_swapped_loop_condition_inverts_jg() {
    let got = derive(&plan("negate-condition", 20), FILE, &count_up(), &isa());
    assert_eq!(got, Ok(patch(0x5d23cc, "7f02", "7e02")));
}

#[test]
fn test_negate_condition_when_jump_is_taken_on_false() {
    // JGE to the else-block (line 15) implements `n < 10` with inverted polarity.
    let got = derive(
        &plan("negate-condition", 12),
        FILE,
        &is_small("7d12"),
        &isa(),
    );
    assert_eq!(got, Ok(patch(0x5d2376, "7d12", "7c12")));
}

// --- Relational boundary swap: needs polarity, signedness and operand order.

#[test]
fn test_ror_lt_to_le_on_direct_signed_jump_taken_when_true() {
    let got = derive(&plan("ror", 12), FILE, &is_small("7c02"), &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7c02", "7e02")));
}

#[test]
fn test_ror_lt_to_le_on_swapped_operands_becomes_jge() {
    // `i < n` compiled as `n > i`: `i <= n` is `n >= i`.
    let got = derive(&plan("ror", 20), FILE, &count_up(), &isa());
    assert_eq!(got, Ok(patch(0x5d23cc, "7f02", "7d02")));
}

#[test]
fn test_ror_lt_to_le_when_jump_is_taken_on_false_becomes_jg() {
    // JGE else == `!(n < 10)`; the mutant `!(n <= 10)` is JG else.
    let got = derive(&plan("ror", 12), FILE, &is_small("7d12"), &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7d12", "7f12")));
}

#[test]
fn test_ror_keeps_unsigned_comparisons_unsigned() {
    let got = derive(&plan("ror", 12), FILE, &is_small("7202"), &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7202", "7602")));
}

#[test]
fn test_ror_patches_near_conditional_jump_in_its_second_opcode_byte() {
    let near = insns(&[
        (0x1000, "4883f80a", 12),
        (0x1004, "0f8c00000000", 12),
        (0x100a, "c644240701", 13),
    ]);
    let got = derive(&plan("ror", 12), FILE, &near, &isa());
    assert_eq!(got, Ok(patch(0x1004, "0f8c00000000", "0f8e00000000")));
}

#[test]
fn test_ror_rejects_condition_code_that_does_not_implement_the_operator() {
    // JE cannot implement `<` in either operand order.
    let got = derive(&plan("ror", 12), FILE, &is_small("7402"), &isa());
    assert_eq!(
        got,
        Err(Unpatchable::ConditionMismatch {
            cc: "e".into(),
            token: "lt".into()
        })
    );
}

// --- Conditions the patcher refuses rather than guessing.

#[test]
fn test_condition_with_two_conditional_jumps_is_unpatchable() {
    let mut fx = is_small("7c02");
    fx[7] = Insn {
        bytes: hex_bytes("7510"),
        ..fx[7].clone()
    }; // JNE in place of the JMP
    let got = derive(&plan("negate-condition", 12), FILE, &fx, &isa());
    assert_eq!(
        got,
        Err(Unpatchable::ConditionalJumps { line: 12, found: 2 })
    );
}

#[test]
fn test_condition_without_conditional_jump_is_unpatchable() {
    let got = derive(
        &plan("negate-condition", 12),
        FILE,
        &is_small("eb02"),
        &isa(),
    );
    assert_eq!(
        got,
        Err(Unpatchable::ConditionalJumps { line: 12, found: 0 })
    );
}

#[test]
fn test_jump_landing_on_the_condition_line_has_ambiguous_polarity() {
    let got = derive(
        &plan("negate-condition", 12),
        FILE,
        &is_small("7c00"),
        &isa(),
    );
    assert_eq!(got, Err(Unpatchable::AmbiguousPolarity { line: 12 }));
}

#[test]
fn test_jump_to_an_address_outside_the_function_is_unpatchable() {
    let got = derive(
        &plan("negate-condition", 12),
        FILE,
        &is_small("7c7f"),
        &isa(),
    );
    assert_eq!(got, Err(Unpatchable::UnknownDestination(0x5d2378 + 0x7f)));
}

#[test]
fn test_instructions_from_other_files_are_ignored() {
    let mut fx = is_small("7c02");
    fx.push(Insn {
        pc: 0x9000,
        bytes: hex_bytes("7402"),
        file: "/src/other.go".into(),
        line: 12,
        column: 0,
    });
    let got = derive(&plan("negate-condition", 12), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7c02", "7d02")));
}

// --- Statement deletion: jump from the statement's first byte to the next statement.

#[test]
fn test_delete_statement_jumps_over_the_call_with_a_short_jump() {
    let got = derive(&plan("delete-statement", 7), FILE, &total(), &isa());
    assert_eq!(got, Ok(patch(0x5d2329, "e8b2", "eb03")));
}

#[test]
fn test_delete_long_statement_uses_a_near_jump() {
    let nops = "90".repeat(195);
    let fx = insns(&[
        (0x2000, &nops, 7),
        (0x20c3, "e800000000", 7),
        (0x20c8, "488b442410", 8),
    ]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x2000, "9090909090", "e9c3000000")));
}

#[test]
fn test_delete_is_too_short_when_no_listed_jump_reaches() {
    // A table with only the 2-byte jump cannot cross 198 bytes.
    let short_only = Isa::parse(
        &include_str!("../../../../../isa/amd64.toml")
            .replace("[[jmp]]\nopcode = 0xe9\nrel = 4\n", ""),
    )
    .unwrap();
    let nops = "90".repeat(195);
    let fx = insns(&[
        (0x2000, &nops, 7),
        (0x20c3, "e800000000", 7),
        (0x20c8, "488b442410", 8),
    ]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &short_only);
    assert_eq!(
        got,
        Err(Unpatchable::TooShort {
            line: 7,
            bytes: 200
        })
    );
}

#[test]
fn test_delete_statement_ignores_out_of_line_blocks_of_its_line() {
    // The second line-7 block is not reached from the statement (a panic or
    // unwinding path); deletion follows only the statement's own path.
    let fx = insns(&[
        (0x2000, "e8b2ffffff", 7),
        (0x2005, "90", 8),
        (0x2006, "e800000000", 7),
        (0x200b, "90", 8),
    ]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x2000, "e8b2", "eb03")));
}

/// angzarr-embedded-host-config `from_yaml`'s closure, line 217
/// `doc.remove(KEY);`: the call, a jump over landing pads (line 219) to the
/// drop of its result, then line 218's argument setup, still on line 217.
fn rust_remove_and_drop() -> Vec<Insn> {
    insns(&[
        (0xb9ae1, "488b742410", 216),
        (0xb9ae6, "488d1559c4f6ff", 217),
        (0xb9aed, "488b0584670f00", 217),
        (0xb9af4, "488d7c2438", 217),
        (0xb9af9, "b90e000000", 217),
        (0xb9afe, "ffd0", 217),
        (0xb9b00, "eb20", 217),
        (0xb9b02, "488b0547630f00", 219),
        (0xb9b09, "488d7c2418", 219),
        (0xb9b0e, "ffd0", 219),
        (0xb9b10, "eb4b", 219),
        (0xb9b12, "4889c1", 219),
        (0xb9b15, "89d0", 219),
        (0xb9b17, "48894c2468", 219),
        (0xb9b1c, "89442470", 219),
        (0xb9b20, "ebe0", 219),
        (0xb9b22, "488b0557670f00", 217),
        (0xb9b29, "488d7c2438", 217),
        (0xb9b2e, "ffd0", 217),
        (0xb9b30, "eb00", 217),
        (0xb9b32, "488b3c24", 217),
        (0xb9b36, "488d742418", 217),
        (0xb9b3b, "e830fdfbff", 218),
    ])
}

#[test]
fn test_delete_statement_follows_its_jump_to_drop_its_result_too() {
    // Skipping only `remove` would run `drop_in_place` on the unwritten
    // result. The jump lands after the drop call, so line 218's argument
    // setup still runs.
    let p = retarget(plan("delete-statement", 7), 217);
    let got = derive(&p, FILE, &rust_remove_and_drop(), &isa());
    assert_eq!(got, Ok(patch(0xb9ae6, "488d", "eb48")));
}

#[test]
fn test_delete_statement_whose_path_loops_back_is_bounded() {
    // jmp to itself on the statement's line must not loop the walk.
    let fx = insns(&[
        (0x100, "e800000000", 7),
        (0x105, "ebfe", 7),
        (0x107, "90", 8),
    ]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x100, "e800", "eb03")));
}

#[test]
fn test_delete_last_statement_of_function_has_no_resume_point() {
    let fx = insns(&[(0x2000, "e8b2ffffff", 7)]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Err(Unpatchable::NoResumePoint { line: 7 }));
}

#[test]
fn test_statement_with_no_instructions_is_unpatchable() {
    let got = derive(
        &plan("delete-statement", 7),
        FILE,
        &is_small("7c02"),
        &isa(),
    );
    assert_eq!(got, Err(Unpatchable::NoInstructions { line: 7 }));
}

// --- Deletion ends after the statement's last call: the line table can
// attribute following code (Rust: loading the return value) to the call.

/// Rust `total`: line 5 is `call log` then `mov rax, [rsp+8]`, which
/// loads `s` for `return s` (docs/rust-gdb-findings.md).
fn rust_total() -> Vec<Insn> {
    insns(&[
        (0x8e6e, "4801f7", 4),
        (0x8e85, "e866ffffff", 5),
        (0x8e8a, "488b442408", 5),
        (0x8e8f, "4883c428", 7),
        (0x8e93, "c3", 7),
    ])
}

#[test]
fn test_delete_call_statement_keeps_code_after_the_call_on_its_line() {
    let rust_plan = retarget(plan("delete-statement", 7), 5);
    let got = derive(&rust_plan, FILE, &rust_total(), &isa());
    assert_eq!(got, Ok(patch(0x8e85, "e866", "eb03")));
}

#[test]
fn test_delete_call_statement_skips_argument_setup_through_the_last_call() {
    // mov rdi, [rsp+8]; call f; call drop_in_place; mov [rsp], rax
    let fx = insns(&[
        (0x100, "488b7c2408", 7),
        (0x105, "e800000000", 7),
        (0x10a, "ff15f0000000", 7),
        (0x110, "48890424", 7),
        (0x114, "c3", 8),
    ]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x100, "488b", "eb0e")));
}

#[test]
fn test_delete_call_statement_without_a_call_instruction_is_unpatchable() {
    let fx = insns(&[(0x100, "488b7c2408", 7), (0x105, "c3", 8)]);
    let got = derive(&plan("delete-statement", 7), FILE, &fx, &isa());
    assert_eq!(got, Err(Unpatchable::NoCall { line: 7 }));
}

/// The fixture's Go plan for `log(s)` with its lines moved to `line`.
fn retarget(mut p: MutantPlan, line: u32) -> MutantPlan {
    use cherenkov_mutation::plan::{Action, Target};
    if let Action::JumpOnly {
        resume: Target::AfterStatement(span),
    } = &mut p.action
    {
        span.start.line = line;
        span.end.line = line;
    }
    p.stop_at.line = line;
    p
}

// --- Arithmetic swap: the one add/sub-family instruction on the statement.

#[test]
fn test_aor_swaps_add_for_sub_in_a_go_assignment() {
    // Total line 6: `s := a + b` is `ADDQ BX, AX`.
    let fx = insns(&[
        (0x5d2321, "4801d8", 6),
        (0x5d2324, "4889442410", 6),
        (0x5d2329, "e8b2ffffff", 7),
    ]);
    let got = derive(&plan("aor", 6), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x5d2321, "4801d8", "4829d8")));
}

#[test]
fn test_aor_swaps_add_and_leaves_rusts_overflow_check_alone() {
    // Rust line 4: `add rdi, rsi; mov; seto al; jo panic`.
    let fx = insns(&[
        (0x8e6e, "4801f7", 6),
        (0x8e71, "48897c2408", 6),
        (0x8e76, "0f90c0", 6),
        (0x8e79, "7019", 6),
        (0x8e7b, "e800000000", 7),
    ]);
    let got = derive(&plan("aor", 6), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x8e6e, "4801f7", "4829f7")));
}

#[test]
fn test_aor_turns_inc_into_dec() {
    // Go CountUp line 21: `i = i + 1` is `INCQ 0x8(SP)`; Rust uses `inc rax`.
    let go = insns(&[
        (0x5d23d0, "48ff442408", 21),
        (0x5d23d5, "eb00", 21),
        (0x5d23d7, "ebe9", 20),
    ]);
    assert_eq!(
        derive(&plan("aor", 21), FILE, &go, &isa()),
        Ok(patch(0x5d23d0, "48ff442408", "48ff4c2408"))
    );
    let rust = insns(&[
        (0x100, "488b442418", 21),
        (0x105, "48ffc0", 21),
        (0x108, "c3", 22),
    ]);
    assert_eq!(
        derive(&plan("aor", 21), FILE, &rust, &isa()),
        Ok(patch(0x105, "48ffc0", "48ffc8"))
    );
}

#[test]
fn test_aor_swaps_immediate_forms_through_the_modrm_reg_field() {
    // add rax, 1 (83 /0 ib) -> sub rax, 1 (83 /5 ib)
    let fx = insns(&[(0x100, "4883c001", 6), (0x104, "c3", 7)]);
    assert_eq!(
        derive(&plan("aor", 6), FILE, &fx, &isa()),
        Ok(patch(0x100, "4883c001", "4883e801"))
    );
}

#[test]
fn test_aor_requires_exactly_one_matching_instruction() {
    let two = insns(&[(0x100, "4801d8", 6), (0x103, "4801c8", 6), (0x106, "c3", 7)]);
    assert_eq!(
        derive(&plan("aor", 6), FILE, &two, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 6, found: 2 })
    );
    // `+` in the source but only a `sub` in the code.
    let sub = insns(&[(0x100, "4829d8", 6), (0x103, "c3", 7)]);
    assert_eq!(
        derive(&plan("aor", 6), FILE, &sub, &isa()),
        Err(Unpatchable::ArithmeticInstructions { line: 6, found: 0 })
    );
}

// --- Return flip: every boolean constant the return statement stores.

#[test]
fn test_return_true_flips_the_slot_store_and_the_result_register_in_go() {
    // IsSmall line 13: MOVB $1, 0x7(SP); MOVL $1, AX
    let got = derive(
        &plan("return-bool-flip", 13),
        FILE,
        &is_small("7c02"),
        &isa(),
    );
    assert_eq!(
        got,
        Ok(Patch {
            regions: vec![
                region(0x5d237a, "c644240701", "c644240700"),
                region(0x5d237f, "b801000000", "b800000000")
            ]
        })
    );
}

#[test]
fn test_return_false_turns_the_zeroing_xor_into_mov_al_1() {
    // IsSmall line 15: MOVB $0, 0x7(SP); XORL AX, AX
    let got = derive(
        &plan("return-bool-flip", 15),
        FILE,
        &is_small("7c02"),
        &isa(),
    );
    assert_eq!(
        got,
        Ok(Patch {
            regions: vec![
                region(0x5d238a, "c644240700", "c644240701"),
                region(0x5d238f, "31c0", "b001")
            ]
        })
    );
}

#[test]
fn test_return_flip_patches_rusts_stack_slot_store() {
    // Rust is_small line 11: mov byte ptr [rsp-9], 1
    let fx = insns(&[(0x8f22, "c64424f701", 13), (0x8f27, "8a4424f7", 14)]);
    let got = derive(&plan("return-bool-flip", 13), FILE, &fx, &isa());
    assert_eq!(got, Ok(patch(0x8f22, "c64424f701", "c64424f700")));
}

#[test]
fn test_return_flip_without_a_boolean_constant_is_unpatchable() {
    let fx = insns(&[(0x100, "488b442408", 13), (0x105, "c3", 14)]);
    assert_eq!(
        derive(&plan("return-bool-flip", 13), FILE, &fx, &isa()),
        Err(Unpatchable::NoConstant { line: 13 })
    );
}

// --- Body replaced by the default return: a stub at the true entry.

#[test]
fn test_body_default_writes_the_go_stub_over_the_function_entry() {
    let go = isa().with_abi("go").unwrap();
    let got = derive(&plan("body-default", 11), FILE, &is_small("7c02"), &go);
    // XORL AX,AX; XORL BX,BX; XORL CX,CX; XORL DI,DI; XORL SI,SI; XORPS X0,X0; RET
    assert_eq!(
        got,
        Ok(patch(
            0x5d2360,
            "554889e54883ec084889442418c6",
            "31c031db31c931ff31f60f57c0c3"
        ))
    );
}

#[test]
fn test_body_default_writes_the_sysv_stub_without_touching_callee_saved_registers() {
    let sysv = isa().with_abi("sysv").unwrap();
    let got = derive(&plan("body-default", 11), FILE, &is_small("7c02"), &sysv);
    // xor eax,eax; xorps xmm0,xmm0; ret
    assert_eq!(got, Ok(patch(0x5d2360, "554889e54883", "31c00f57c0c3")));
}

#[test]
fn test_body_default_needs_a_known_default_and_an_abi() {
    let mut p = plan("body-default", 11);
    p.action = cherenkov_mutation::plan::Action::ReturnDefault {
        value_text: None,
        zero_bits: false,
        async_fn: false,
    };
    let sysv = isa().with_abi("sysv").unwrap();
    assert_eq!(
        derive(&p, FILE, &is_small("7c02"), &sysv),
        Err(Unpatchable::NoDefault)
    );
    assert_eq!(
        derive(&plan("body-default", 11), FILE, &is_small("7c02"), &isa()),
        Err(Unpatchable::NoReturnStub)
    );
    assert!(isa().with_abi("vax").is_err());
}

#[test]
fn test_body_value_other_than_zero_is_not_patched_by_the_stub() {
    let mut p = plan("body-default", 11);
    p.action = cherenkov_mutation::plan::Action::ReturnDefault {
        value_text: Some("true".into()),
        zero_bits: false,
        async_fn: false,
    };
    let sysv = isa().with_abi("sysv").unwrap();
    assert_eq!(
        derive(&p, FILE, &is_small("7c02"), &sysv),
        Err(Unpatchable::NonZeroValue("true".into()))
    );
}

#[test]
fn test_body_default_refuses_a_function_with_a_jump_into_the_stub() {
    // A loop back to entry+1 would land inside the stub's bytes.
    let fx = insns(&[
        (0x100, "55", 11),
        (0x101, "4889e5", 11),
        (0x104, "ebfb", 12),
        (0x106, "c3", 12),
    ]);
    let sysv = isa().with_abi("sysv").unwrap();
    assert_eq!(
        derive(&plan("body-default", 11), FILE, &fx, &sysv),
        Err(Unpatchable::JumpIntoStub(0x101))
    );
}

#[test]
fn test_body_default_refuses_a_function_shorter_than_the_stub() {
    let fx = insns(&[(0x100, "b001", 11), (0x102, "c3", 11)]);
    let sysv = isa().with_abi("sysv").unwrap();
    assert_eq!(
        derive(&plan("body-default", 11), FILE, &fx, &sysv),
        Err(Unpatchable::FunctionTooShort { bytes: 3, stub: 6 })
    );
}

// --- Operators and contexts without tier P rules.

#[test]
fn test_mul_div_swap_and_token_edits_outside_conditions_or_assignments_have_no_rule() {
    let mut p = plan("aor", 6);
    p.edit = cherenkov_mutation::edit::Edit::ReplaceToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        from: cherenkov_mutation::token::Token::Mul,
        with: cherenkov_mutation::token::Token::Div,
    };
    assert_eq!(
        derive(&p, FILE, &is_small("7c02"), &isa()),
        Err(Unpatchable::UnsupportedEdit("aor mul -> div".into()))
    );
}

// --- A condition replaced by a constant (a match guard -> true/false): the
// jump always or never goes its way, at the jump's own length.

fn constant(value: bool) -> MutantPlan {
    let mut p = plan("negate-condition", 12);
    p.edit = cherenkov_mutation::edit::Edit::ReplaceWithLiteral {
        capture: cherenkov_mutation::concept::Capture::Cond,
        lit: cherenkov_mutation::edit::Lit::Bool(value),
    };
    p
}

#[test]
fn test_constant_true_makes_a_jump_taken_when_true_unconditional() {
    let got = derive(&constant(true), FILE, &is_small("7c02"), &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7c02", "eb02")));
}

#[test]
fn test_constant_false_makes_a_jump_taken_when_true_fall_through() {
    let got = derive(&constant(false), FILE, &is_small("7c02"), &isa());
    assert_eq!(got, Ok(patch(0x5d2376, "7c02", "eb00")));
}

// --- The two exits of a condition's code, for evaluating a mutated
// condition under the debugger and continuing at the exit it selects.
// `logic` from the rust-ops fidelity fixture, as GDB and the DWARF line table
// give it: GDB's line, the row's column, or 0 on a compiler-generated row.

fn rust_plan(mutation: &str) -> MutantPlan {
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
    let ops = cherenkov_mutation::operator_set::named("cargo-mutants")
        .unwrap()
        .operators();
    plan_file(&file, &profile, &ops)
        .plans
        .into_iter()
        .find(|p| p.mutation == Some(mutation) && p.function.as_deref() == Some("logic"))
        .unwrap()
}

fn logic() -> Vec<Insn> {
    [
        (0x7952e, "a801", 28, 8),
        (0x79530, "750a", 28, 8),
        (0x79532, "8a4424ef", 28, 0),
        (0x79536, "a801", 31, 8),
        (0x79538, "7521", 31, 8),
        (0x7953a, "eb15", 31, 8),
        (0x7953c, "8a4424ee", 31, 0),
        (0x79540, "a801", 28, 13),
        (0x79542, "7502", 28, 13),
        (0x79544, "ebec", 28, 13),
        (0x79546, "48c74424f001000000", 29, 16),
        (0x7954f, "eb1e", 29, 0),
        (0x79551, "8a4424ee", 31, 0),
        (0x79555, "a801", 31, 13),
        (0x79557, "7502", 31, 13),
        (0x79559, "eb0b", 31, 13),
        (0x7955b, "48c74424f002000000", 32, 16),
        (0x79564, "eb09", 32, 0),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/lib.rs".into(),
        line,
        column,
    })
    .collect()
}

#[test]
fn test_and_condition_exits_are_the_then_block_and_the_next_statements_lead_in() {
    use cherenkov_exec_dap::tierp::branch_exits;
    let exits = branch_exits(&rust_plan("lcr/and->or"), "src/lib.rs", &logic(), &isa()).unwrap();
    assert_eq!(exits.stop, 0x7952e);
    assert_eq!(exits.if_true, 0x79546, "return 1");
    // Compiler-generated code leading into `if p || q`, on the false path.
    assert_eq!(exits.if_false, 0x79532);
}

#[test]
fn test_or_condition_exits() {
    use cherenkov_exec_dap::tierp::branch_exits;
    let exits = branch_exits(&rust_plan("lcr/or->and"), "src/lib.rs", &logic(), &isa()).unwrap();
    assert_eq!(exits.stop, 0x79536);
    assert_eq!(exits.if_true, 0x7955b, "return 2");
    assert_eq!(exits.if_false, 0x79566);
}

// --- Swapping && and || without evaluating anything: at each jump out of
// the left operand (the program has computed it), go where the mutated
// operator goes. In a condition, R's own exits say where; as a value, the
// constant block of a short circuit is reused with the other constant.

use cherenkov_exec_dap::tierp::{Goto, SlotStore, Steer, swap_logical};

const JNE: Option<u8> = Some(5);

fn to(pc: u64) -> Goto {
    Goto {
        to: pc,
        store: None,
    }
}

#[test]
fn test_and_to_or_in_a_condition_short_circuits_into_the_then_block_when_the_left_holds() {
    let got = swap_logical(&rust_plan("lcr/and->or"), "src/lib.rs", &logic()).unwrap();
    // `p && q`: `jne 0x7953c` goes on into `q` when `p` holds, else falls
    // to the else path. As `p || q`: p holds -> the then-block (where q's
    // own true exit goes); p fails -> evaluate q.
    assert_eq!(
        got,
        vec![Steer {
            pc: 0x79530,
            cc: JNE,
            taken: to(0x79546),
            fall: to(0x7953c),
        }]
    );
}

#[test]
fn test_or_to_and_in_a_condition_evaluates_the_right_when_the_left_holds() {
    let got = swap_logical(&rust_plan("lcr/or->and"), "src/lib.rs", &logic()).unwrap();
    // `p || q`: `jne` to the then-block when p holds; else `jmp` into q.
    // As `p && q`: p holds -> evaluate q; p fails -> q's false exit.
    assert_eq!(
        got,
        vec![
            Steer {
                pc: 0x79538,
                cc: JNE,
                taken: to(0x79551),
                fall: to(0x7953a),
            },
            Steer {
                pc: 0x7953a,
                cc: None,
                taken: to(0x79566),
                fall: to(0x79566),
            },
        ]
    );
}

/// `is_local_path`'s chain of `||`, mutated at the operator before `line`,
/// whose left operand starts the chain and whose right ends on `line`.
fn chain_swap(line: u32, rhs_end: u32) -> MutantPlan {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |line: u32, col: u32| Pos { byte: 0, line, col };
    let mut p = rust_plan("lcr/or->and");
    p.stop_at = at(129, 5);
    p.site = Span {
        start: at(line, 9),
        end: at(line, 11),
    };
    p.operands = vec![
        Span {
            start: at(129, 5),
            end: at(line - 1, 99),
        },
        Span {
            start: at(line, 12),
            end: at(line, rhs_end),
        },
    ];
    p
}

#[test]
fn test_or_to_and_first_in_a_value_chain_goes_to_the_next_operand_when_the_left_fails() {
    // `a || b || ...` as `(a && b) || ...`: a holds -> evaluate b; a fails
    // -> where b's false exit goes, the next operand. The block storing
    // `true` is positioned in `a` but belongs to no operand.
    let got = swap_logical(
        &chain_swap(130, 32),
        "src/address.rs",
        &is_local_path_whole(),
    )
    .unwrap();
    assert_eq!(
        got,
        vec![Steer {
            pc: 0x16d173,
            cc: JNE,
            taken: to(0x16d175),
            fall: to(0x16d19d),
        }]
    );
}

#[test]
fn test_or_to_and_last_in_a_value_chain_stores_false_when_the_left_fails() {
    // `(a || b || c || d) && e`: any operand holding -> evaluate e; d
    // failing -> the result is false: the `movb $1` block's slot gets 0.
    let got = swap_logical(
        &chain_swap(133, 23),
        "src/address.rs",
        &is_local_path_whole(),
    )
    .unwrap();
    let right = to(0x16d1d0);
    let fals = Goto {
        to: 0x16d1e6,
        store: Some(SlotStore {
            disp: 0x17,
            size: 1,
            value: 0,
        }),
    };
    let steer = |pc, fall| Steer {
        pc,
        cc: JNE,
        taken: right,
        fall,
    };
    assert_eq!(
        got,
        vec![
            steer(0x16d173, to(0x16d175)),
            steer(0x16d192, to(0x16d194)),
            steer(0x16d1ba, to(0x16d1bc)),
            steer(0x16d1ce, fals),
        ]
    );
}

/// ctxloom `expand`, line 36: `if a || b || c { return ...; }`. The
/// then-block ends in an unpositioned `jmp` just before `c`'s code.
fn expand_line_36() -> Vec<Insn> {
    [
        (0x16d54b, "488b7c2458", 36, 8),      // mov    0x58(%rsp),%rdi
        (0x16d550, "488b742460", 36, 8),      // mov    0x60(%rsp),%rsi
        (0x16d555, "488d156541f2ff", 36, 12), // lea    -0xdbe9b(%rip),%rdx
        (0x16d55c, "b907000000", 36, 12),     // mov    $0x7,%ecx
        (0x16d561, "ff15595b2c00", 36, 12),   // call   *0x2c5b59(%rip)
        (0x16d567, "a801", 36, 8),            // test   $0x1,%al
        (0x16d569, "7522", 36, 8),            // jne    16d58d <+0x5d>
        (0x16d56b, "488b7c2458", 36, 38),     // mov    0x58(%rsp),%rdi
        (0x16d570, "488b742460", 36, 38),     // mov    0x60(%rsp),%rsi
        (0x16d575, "488d15d4f6f1ff", 36, 42), // lea    -0xe092c(%rip),%rdx
        (0x16d57c, "b908000000", 36, 42),     // mov    $0x8,%ecx
        (0x16d581, "ff15395b2c00", 36, 42),   // call   *0x2c5b39(%rip)
        (0x16d587, "a801", 36, 38),           // test   $0x1,%al
        (0x16d589, "7502", 36, 38),           // jne    16d58d <+0x5d>
        (0x16d58b, "eb39", 36, 38),           // jmp    16d5c6 <+0x96>
        (0x16d58d, "488b742458", 37, 21),     // mov    0x58(%rsp),%rsi
        (0x16d592, "488b542460", 37, 21),     // mov    0x60(%rsp),%rdx
        (0x16d597, "488d7c2468", 37, 25),     // lea    0x68(%rsp),%rdi
        (0x16d59c, "ff150e5a2c00", 37, 25),   // call   *0x2c5a0e(%rip)
        (0x16d5a2, "488b442448", 37, 25),     // mov    0x48(%rsp),%rax
        (0x16d5a7, "488b4c2468", 37, 16),     // mov    0x68(%rsp),%rcx
        (0x16d5ac, "488908", 37, 16),         // mov    %rcx,(%rax)
        (0x16d5af, "488b4c2470", 37, 16),     // mov    0x70(%rsp),%rcx
        (0x16d5b4, "48894808", 37, 16),       // mov    %rcx,0x8(%rax)
        (0x16d5b8, "488b4c2478", 37, 16),     // mov    0x78(%rsp),%rcx
        (0x16d5bd, "48894810", 37, 16),       // mov    %rcx,0x10(%rax)
        (0x16d5c1, "e97f020000", 0, 0),       // jmp    16d845 <+0x315>
        (0x16d5c6, "488b7c2458", 36, 69),     // mov    0x58(%rsp),%rdi
        (0x16d5cb, "488b742460", 36, 69),     // mov    0x60(%rsp),%rsi
        (0x16d5d0, "488d15f940f2ff", 36, 73), // lea    -0xdbf07(%rip),%rdx
        (0x16d5d7, "b907000000", 36, 73),     // mov    $0x7,%ecx
        (0x16d5dc, "ff15de5a2c00", 36, 73),   // call   *0x2c5ade(%rip)
        (0x16d5e2, "a801", 36, 69),           // test   $0x1,%al
        (0x16d5e4, "75a7", 36, 69),           // jne    16d58d <+0x5d>
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/address.rs".into(),
        line,
        column,
    })
    .collect()
}

#[test]
fn test_or_to_and_after_a_compound_left_steers_every_jump_out_of_it() {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos {
        byte: 0,
        line: 36,
        col,
    };
    let mut plan = rust_plan("lcr/or->and");
    plan.stop_at = at(8);
    plan.operands = vec![
        Span {
            start: at(8),
            end: at(65),
        },
        Span {
            start: at(69),
            end: at(95),
        },
    ];
    // `(a || b) && c`: a or b holding -> evaluate c; b failing -> c's
    // false exit, the next statement. The then-block's closing `jmp` has
    // no position and is not part of `c`.
    let got = swap_logical(&plan, "src/address.rs", &expand_line_36()).unwrap();
    let c = to(0x16d5c6);
    assert_eq!(
        got,
        vec![
            Steer {
                pc: 0x16d569,
                cc: JNE,
                taken: c,
                fall: to(0x16d56b),
            },
            Steer {
                pc: 0x16d589,
                cc: JNE,
                taken: c,
                fall: to(0x16d58b),
            },
            Steer {
                pc: 0x16d58b,
                cc: None,
                taken: to(0x16d5e6),
                fall: to(0x16d5e6),
            },
        ]
    );
}

#[test]
fn test_and_or_swap_refuses_other_edits() {
    let got = swap_logical(&comparison(132), "src/address.rs", &is_local_path());
    assert!(
        matches!(got, Err(Unpatchable::UnsupportedEdit(_))),
        "{got:?}"
    );
}

// --- Negating a boolean the program computes (`==` <-> `!=`, deleting a
// `!`): patch the instruction that consumes the value, in any context.
// `is_local_path` from ctxloom: `... || raw == "." || raw == ".."`, each
// comparison a call returning its bool in al.

fn is_local_path() -> Vec<Insn> {
    [
        (0x16d1bc, "4889e7", 132, 12),
        (0x16d1bf, "488d35ea452a00", 132, 19),
        (0x16d1c6, "ff15945d2c00", 132, 12),
        (0x16d1cc, "a801", 132, 9),
        (0x16d1ce, "75c6", 132, 9),
        (0x16d1d0, "4889e7", 133, 12),
        (0x16d1d3, "488d35e6452a00", 133, 19),
        (0x16d1da, "ff15805d2c00", 133, 12),
        (0x16d1e0, "2401", 133, 9),
        (0x16d1e2, "88442417", 133, 9),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/address.rs".into(),
        line,
        column,
    })
    .collect()
}

/// All of `is_local_path` as the line table gives it (one row per line):
/// the chain's short-circuit block `movb $1, 0x17(%rsp); jmp join` is
/// positioned at the first operand.
fn is_local_path_whole() -> Vec<Insn> {
    [
        (0x16d150, "4883ec18", 128, 0),        // sub    $0x18,%rsp
        (0x16d154, "48893c24", 128, 0),        // mov    %rdi,(%rsp)
        (0x16d158, "4889742408", 128, 0),      // mov    %rsi,0x8(%rsp)
        (0x16d15d, "488b3c24", 129, 5),        // mov    (%rsp),%rdi
        (0x16d161, "488b742408", 129, 5),      // mov    0x8(%rsp),%rsi
        (0x16d166, "ba2f000000", 129, 9),      // mov    $0x2f,%edx
        (0x16d16b, "ff15275f2c00", 129, 9),    // call   *0x2c5f27(%rip)
        (0x16d171, "a801", 129, 5),            // test   $0x1,%al
        (0x16d173, "7521", 129, 5),            // jne    16d196 <+0x46>
        (0x16d175, "488b3c24", 130, 12),       // mov    (%rsp),%rdi
        (0x16d179, "488b742408", 130, 12),     // mov    0x8(%rsp),%rsi
        (0x16d17e, "488d154345f2ff", 130, 16), // lea    -0xdbabd(%rip),%rdx
        (0x16d185, "b902000000", 130, 16),     // mov    $0x2,%ecx
        (0x16d18a, "ff15305f2c00", 130, 16),   // call   *0x2c5f30(%rip)
        (0x16d190, "a801", 130, 12),           // test   $0x1,%al
        (0x16d192, "7502", 130, 12),           // jne    16d196 <+0x46>
        (0x16d194, "eb07", 130, 12),           // jmp    16d19d <+0x4d>
        (0x16d196, "c644241701", 129, 5),      // movb   $0x1,0x17(%rsp)
        (0x16d19b, "eb49", 129, 5),            // jmp    16d1e6 <+0x96>
        (0x16d19d, "488b3c24", 131, 12),       // mov    (%rsp),%rdi
        (0x16d1a1, "488b742408", 131, 12),     // mov    0x8(%rsp),%rsi
        (0x16d1a6, "488d151d45f2ff", 131, 16), // lea    -0xdbae3(%rip),%rdx
        (0x16d1ad, "b903000000", 131, 16),     // mov    $0x3,%ecx
        (0x16d1b2, "ff15085f2c00", 131, 16),   // call   *0x2c5f08(%rip)
        (0x16d1b8, "a801", 131, 12),           // test   $0x1,%al
        (0x16d1ba, "75da", 131, 12),           // jne    16d196 <+0x46>
        (0x16d1bc, "4889e7", 132, 12),         // mov    %rsp,%rdi
        (0x16d1bf, "488d35ea452a00", 132, 12), // lea    0x2a45ea(%rip),%rsi
        (0x16d1c6, "ff15945d2c00", 132, 12),   // call   *0x2c5d94(%rip)
        (0x16d1cc, "a801", 132, 12),           // test   $0x1,%al
        (0x16d1ce, "75c6", 132, 12),           // jne    16d196 <+0x46>
        (0x16d1d0, "4889e7", 133, 12),         // mov    %rsp,%rdi
        (0x16d1d3, "488d35e6452a00", 133, 12), // lea    0x2a45e6(%rip),%rsi
        (0x16d1da, "ff15805d2c00", 133, 12),   // call   *0x2c5d80(%rip)
        (0x16d1e0, "2401", 133, 12),           // and    $0x1,%al
        (0x16d1e2, "88442417", 133, 12),       // mov    %al,0x17(%rsp)
        (0x16d1e6, "8a442417", 134, 2),        // mov    0x17(%rsp),%al
        (0x16d1ea, "2401", 134, 2),            // and    $0x1,%al
        (0x16d1ec, "4883c418", 134, 2),        // add    $0x18,%rsp
        (0x16d1f0, "c3", 134, 2),              // ret
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/address.rs".into(),
        line,
        column,
    })
    .collect()
}

fn comparison(line: u32) -> MutantPlan {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos { byte: 0, line, col };
    let mut p = rust_plan("lcr/and->or");
    p.edit = cherenkov_mutation::edit::Edit::ReplaceToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        from: cherenkov_mutation::token::Token::Eq,
        with: cherenkov_mutation::token::Token::Ne,
    };
    p.stop_at = at(12);
    p.site = Span {
        start: at(16),
        end: at(18),
    };
    p.operands = vec![
        Span {
            start: at(12),
            end: at(15),
        },
        Span {
            start: at(19),
            end: at(22),
        },
    ];
    p.action = cherenkov_mutation::plan::Action::InPlace {
        expr_text: String::new(),
    };
    p
}

#[test]
fn test_negating_a_comparison_whose_value_feeds_a_jump_inverts_the_jump() {
    let got = derive(&comparison(132), "src/address.rs", &is_local_path(), &isa());
    assert_eq!(got, Ok(patch(0x16d1ce, "75c6", "74c6")));
}

#[test]
fn test_negating_a_comparison_whose_value_is_stored_flips_its_normalising_and() {
    let got = derive(&comparison(133), "src/address.rs", &is_local_path(), &isa());
    assert_eq!(got, Ok(patch(0x16d1e0, "2401", "3401")));
}

// --- Deleting a `!` whose operand is a call: rustc's `xor al, 0xff` (with
// the `and al, 1` after it) becomes the plain normalisation. ctxloom
// `expand`, line 45: `... && !user_host.contains('/'))`.

#[test]
fn test_deleting_a_not_compiled_as_xor_ff_normalises_the_bool_instead() {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos {
        byte: 0,
        line: 45,
        col,
    };
    let mut plan = comparison(45);
    plan.edit = cherenkov_mutation::edit::Edit::DeleteToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        token: cherenkov_mutation::token::Token::Not,
    };
    plan.stop_at = at(16);
    plan.site = Span {
        start: at(57),
        end: at(58),
    };
    plan.operands = vec![Span {
        start: at(58),
        end: at(81),
    }];
    let insns: Vec<Insn> = [
        (0x16d7ff, "488b742430", 0, 16),
        (0x16d804, "488b7c2428", 0, 16),
        (0x16d809, "ba2f000000", 45, 68),
        (0x16d80e, "ff1504572c00", 45, 68),
        (0x16d814, "34ff", 45, 57),
        (0x16d816, "2401", 45, 57),
        (0x16d818, "88842427010000", 45, 57),
        (0x16d81f, "488b7c2448", 0, 57),
        (0x16d824, "8a842427010000", 45, 16),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/address.rs".into(),
        line,
        column,
    })
    .collect();
    let got = derive(&plan, "src/address.rs", &insns, &isa());
    assert_eq!(got, Ok(patch(0x16d814, "34ff", "2401")));
}

// --- A closure's body shares its line with the function around it: of
// the functions with code on the line, the mutant's is the one with code
// positioned inside the mutated site.

#[test]
fn test_the_function_with_code_at_the_site_is_chosen_among_those_on_its_line() {
    use cherenkov_exec_dap::tierp::at_site;
    use cherenkov_mutation::syntax::{Pos, Span};
    let insn = |pc, column| Insn {
        pc,
        bytes: hex_bytes("90"),
        file: "src/lib.rs".into(),
        line: 177,
        column,
    };
    // `v.iter().filter(|x| *x % 2 == 0).count()`: the function's code is
    // at `v`, `.filter` and `.count`; the closure's at `*x % 2 == 0`.
    let function = vec![insn(0x10, 5), insn(0x11, 14), insn(0x12, 39)];
    let closure = vec![insn(0x20, 25), insn(0x21, 30), insn(0x22, 0)];
    let at = |col| Pos {
        byte: 0,
        line: 177,
        col,
    };
    // The mutated expression, `*x % 2 == 0`.
    let site = Span {
        start: at(25),
        end: at(36),
    };
    assert_eq!(at_site(&[&function, &closure], &site), 1);
    // Nothing positioned in the site anywhere: the first, as before.
    let elsewhere = Span {
        start: at(50),
        end: at(52),
    };
    assert_eq!(at_site(&[&function, &closure], &elsewhere), 0);
}

// --- Deleting a `!` that rustc folded into the branch it decides: ctxloom
// `remotes_mut`, `if !matches!(remotes, Value::Map(_)) {`. The match
// stores its bool, then `testb $1, slot; jne` inside the operand's own
// code consumes it: that jump is inverted.

#[test]
fn test_deleting_a_not_folded_into_its_branch_inverts_the_branch() {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos {
        byte: 0,
        line: 384,
        col,
    };
    let mut plan = comparison(384);
    plan.edit = cherenkov_mutation::edit::Edit::DeleteToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        token: cherenkov_mutation::token::Token::Not,
    };
    plan.stop_at = at(12);
    plan.site = Span {
        start: at(12),
        end: at(13),
    };
    plan.operands = vec![Span {
        start: at(13),
        end: at(41),
    }];
    let insns: Vec<Insn> = [
        (0x14e515, "48833803", 384, 13),
        (0x14e519, "7507", 384, 13),
        (0x14e51b, "c644241701", 384, 13),
        (0x14e520, "eb05", 384, 13),
        (0x14e522, "c644241700", 384, 13),
        (0x14e527, "f644241701", 384, 13),
        (0x14e52c, "753b", 384, 13),
        (0x14e52e, "488b05934d2e00", 385, 35),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/registry.rs".into(),
        line,
        column,
    })
    .collect();
    let got = derive(&plan, "src/registry.rs", &insns, &isa());
    assert_eq!(got, Ok(patch(0x14e52c, "753b", "743b")));
}

// --- A comparison whose result is a value: `setcc` makes the bool, and its
// condition code is swapped as a branch's would be. ctxloom
// `Closure::conflicts`: `.filter(|(_, hashes)| hashes.len() > 1)`, an
// unsigned `>` (`seta`).

fn value_compare(with: cherenkov_mutation::token::Token) -> MutantPlan {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos {
        byte: 0,
        line: 96,
        col,
    };
    let mut p = comparison(96);
    p.edit = cherenkov_mutation::edit::Edit::ReplaceToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        from: cherenkov_mutation::token::Token::Gt,
        with,
    };
    p.stop_at = at(35);
    p.site = Span {
        start: at(48),
        end: at(49),
    };
    p.operands = vec![
        Span {
            start: at(35),
            end: at(47),
        },
        Span {
            start: at(50),
            end: at(51),
        },
    ];
    p.action = cherenkov_mutation::plan::Action::EvalReturn {
        value_text: String::new(),
    };
    p
}

fn conflicts_closure() -> Vec<Insn> {
    [
        (0x1d87d0, "4883ec18", 96, 0),
        (0x1d87dd, "4889f0", 96, 26),
        (0x1d87e9, "488b7e08", 96, 35),
        (0x1d87ed, "e8de37ffff", 96, 42),
        (0x1d87f2, "4883f801", 96, 35),
        (0x1d87f6, "0f97c0", 96, 35),
        (0x1d87f9, "2401", 96, 51),
        (0x1d87fb, "4883c418", 96, 51),
        (0x1d87ff, "c3", 96, 51),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/plan.rs".into(),
        line,
        column,
    })
    .collect()
}

#[test]
fn test_a_comparison_made_into_a_value_has_its_setcc_condition_swapped() {
    use cherenkov_mutation::token::Token;
    let at = |with| {
        derive(
            &value_compare(with),
            "src/plan.rs",
            &conflicts_closure(),
            &isa(),
        )
    };
    // Unsigned: `<` is setb, `==` sete, `>=` setae.
    assert_eq!(at(Token::Lt), Ok(patch(0x1d87f6, "0f97c0", "0f92c0")));
    assert_eq!(at(Token::Eq), Ok(patch(0x1d87f6, "0f97c0", "0f94c0")));
    assert_eq!(at(Token::Ge), Ok(patch(0x1d87f6, "0f97c0", "0f93c0")));
}

// --- `!` on an integer is a bitwise not: `xor al, 0xff` with no `and al, 1`
// after it (a bool's `!` is normalised). Never undone as a bool's.

#[test]
fn test_an_integer_not_is_not_undone_as_a_bool_s() {
    use cherenkov_mutation::syntax::{Pos, Span};
    let at = |col: u32| Pos {
        byte: 0,
        line: 250,
        col,
    };
    let mut plan = comparison(250);
    plan.edit = cherenkov_mutation::edit::Edit::DeleteToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        token: cherenkov_mutation::token::Token::Not,
    };
    plan.stop_at = at(5);
    plan.site = Span {
        start: at(5),
        end: at(6),
    };
    plan.operands = vec![Span {
        start: at(6),
        end: at(7),
    }];
    let insns: Vec<Insn> = [
        (0x7fd50, "4088f8", 249, 0),
        (0x7fd53, "884424ff", 249, 0),
        (0x7fd57, "34ff", 250, 5),
        (0x7fd59, "c3", 251, 2),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/lib.rs".into(),
        line,
        column,
    })
    .collect();
    let got = derive(&plan, "src/lib.rs", &insns, &isa());
    assert!(got.is_err(), "{got:?}");
}

#[test]
fn test_aor_refuses_a_swap_rust_checks_by_comparing_the_result() {
    // Rust `let y = x + 1` on u32: `add edi, 1; mov; cmp edi, eax; jb panic`.
    // As `x - 1` the check would fire for every nonzero x.
    let fx = insns(&[
        (0x7f37c, "83c701", 6),
        (0x7f37f, "897c240c", 6),
        (0x7f383, "39c7", 6),
        (0x7f385, "720d", 6),
        (0x7f387, "8b44240c", 7),
    ]);
    assert!(derive(&plan("aor", 6), FILE, &fx, &isa()).is_err());
}

#[test]
fn test_a_checked_operation_s_overflow_panic_is_its_check_s_target() {
    use cherenkov_exec_dap::tierp::overflow_panic;
    use cherenkov_mutation::syntax::{Pos, Span};
    // `a + b` on u32: `add edi, esi; mov; cmp edi, eax; jb panic`, and the
    // panic block loads its location and calls.
    let at = |col: u32| Pos {
        byte: 0,
        line: 311,
        col,
    };
    let mut plan = rust_plan("lcr/and->or");
    plan.operands = vec![
        Span {
            start: at(5),
            end: at(6),
        },
        Span {
            start: at(9),
            end: at(10),
        },
    ];
    let insns: Vec<Insn> = [
        (0x7ff9e, "89c7", 311, 5),
        (0x7ffa0, "01f7", 311, 5),
        (0x7ffa2, "897c240c", 311, 5),
        (0x7ffa6, "39c7", 311, 5),
        (0x7ffa8, "7209", 311, 5),
        (0x7ffaa, "8b44240c", 0, 5),
        (0x7ffae, "4883c418", 312, 2),
        (0x7ffb2, "c3", 312, 2),
        (0x7ffb3, "488d3d86fa0500", 311, 5),
        (0x7ffba, "ff15202c0600", 311, 5),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/lib.rs".into(),
        line,
        column,
    })
    .collect();
    assert_eq!(overflow_panic(&plan, "src/lib.rs", &insns), Some(0x7ffb3));
    // Unchecked arithmetic has none.
    assert_eq!(overflow_panic(&plan, "src/lib.rs", &insns[..3]), None);
}

#[test]
fn test_every_copy_of_a_generic_with_code_at_the_site_is_kept() {
    use cherenkov_exec_dap::tierp::at_site_all;
    use cherenkov_mutation::syntax::{Pos, Span};
    let insn = |pc, column| Insn {
        pc,
        bytes: hex_bytes("90"),
        file: "src/lib.rs".into(),
        line: 9,
        column,
    };
    // `pick<u8>` and `pick<u32>`: the same code at the same columns; a
    // closure on the line has less there, and its own entry.
    let u8_copy = vec![insn(0x10, 8), insn(0x11, 12)];
    let u32_copy = vec![insn(0x20, 8), insn(0x21, 12)];
    let other = vec![insn(0x30, 14)];
    let at = |col| Pos {
        byte: 0,
        line: 9,
        col,
    };
    let site = Span {
        start: at(8),
        end: at(20),
    };
    assert_eq!(at_site_all(&[&u8_copy, &other, &u32_copy], &site), [0, 2]);
}

#[test]
fn test_copies_of_a_generic_with_different_code_at_the_site_are_all_kept() {
    use cherenkov_exec_dap::tierp::at_site_all;
    use cherenkov_mutation::syntax::{Pos, Span};
    let insn = |pc, column| Insn {
        pc,
        bytes: hex_bytes("90"),
        file: "src/lib.rs".into(),
        line: 9,
        column,
    };
    // `check<u8>` compares inline; `check<&str>` calls the comparison and
    // has more code in the same expression. Both start at the function's
    // entry; the closure inside starts elsewhere.
    let inline = vec![insn(0x10, 8), insn(0x11, 12)];
    let calls = vec![
        insn(0x20, 8),
        insn(0x21, 12),
        insn(0x22, 13),
        insn(0x23, 15),
    ];
    let closure = vec![insn(0x30, 14), insn(0x31, 15)];
    let at = |col| Pos {
        byte: 0,
        line: 9,
        col,
    };
    let site = Span {
        start: at(8),
        end: at(20),
    };
    assert_eq!(at_site_all(&[&inline, &calls, &closure], &site), [0, 1]);
}

// --- Deleting a statement that branches: `note(s.parse()?);` decides at run
// time whether it returns early, so no straight path through it skips it.
// A jump to a panic block (an overflow check) is no such branch.

#[test]
fn test_deleting_a_statement_that_may_return_early_is_refused() {
    // call parse; test; je ok; call from_residual (the Err path, falling
    // through); jmp out; ok: call note.
    let fx = insns(&[
        (0x100, "e800000000", 6),
        (0x105, "a801", 6),
        (0x107, "740a", 6),
        (0x109, "e800000000", 6),
        (0x10e, "eb05", 6),
        (0x113, "e800000000", 6),
        (0x118, "c3", 7),
    ]);
    assert!(derive(&retarget(plan("delete-statement", 7), 6), FILE, &fx, &isa()).is_err());
}

#[test]
fn test_deleting_a_statement_through_an_overflow_check_still_resumes_after_its_call() {
    // add rdi, 1; jo panic; call f; (next statement); panic: lea; call.
    let fx = insns(&[
        (0x200, "4883c701", 6),
        (0x204, "700a", 6),
        (0x206, "e800000000", 6),
        (0x20b, "4889c0", 7),
        (0x20e, "c3", 7),
        (0x210, "488d3d00000000", 6),
        (0x217, "e800000000", 6),
    ]);
    assert!(derive(&retarget(plan("delete-statement", 7), 6), FILE, &fx, &isa()).is_ok());
}

#[test]
fn test_the_last_branch_of_a_negated_and_is_not_taken_for_the_not_s() {
    use cherenkov_mutation::syntax::{Pos, Span};
    // `if !(p() && q())`: the last `test al, 1; jne` decides q, not the
    // whole negation; inverting it is no deleted `!`.
    let at = |col: u32| Pos {
        byte: 0,
        line: 384,
        col,
    };
    let mut plan = comparison(384);
    plan.edit = cherenkov_mutation::edit::Edit::DeleteToken {
        capture: cherenkov_mutation::concept::Capture::Op,
        token: cherenkov_mutation::token::Token::Not,
    };
    plan.stop_at = at(8);
    plan.site = Span {
        start: at(8),
        end: at(9),
    };
    plan.operands = vec![Span {
        start: at(9),
        end: at(24),
    }];
    let insns: Vec<Insn> = [
        (0x100, "e800000000", 384, 11),
        (0x105, "a801", 384, 11),
        (0x107, "7510", 384, 11),
        (0x109, "e800000000", 384, 19),
        (0x10e, "a801", 384, 19),
        (0x110, "7507", 384, 19),
        (0x112, "488b05934d2e00", 385, 35),
    ]
    .into_iter()
    .map(|(pc, hex, line, column)| Insn {
        pc,
        bytes: hex_bytes(hex),
        file: "src/registry.rs".into(),
        line,
        column,
    })
    .collect();
    let got = derive(&plan, "src/registry.rs", &insns, &isa());
    assert_ne!(got, Ok(patch(0x110, "7507", "7407")), "{got:?}");
}
