//! x86-64 instructions by meaning, decoded by iced-x86, instead of byte
//! patterns: the questions the patching rules ask of the program's code,
//! and the few rewrites tier P makes (an inverted or retargeted jump, a
//! negated bool), written by hand at the instruction's own length. Bytes are
//! from ctxloom's `is_local_path` and the rust-ops fixture.

use cherenkov_exec_dap::x86::{Jump, decode};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn test_a_conditional_jump_has_its_condition_and_absolute_target() {
    let i = decode(0x16d1ce, &hex("75c6")).unwrap();
    let Some(Jump {
        conditional: true,
        target,
    }) = i.jump()
    else {
        panic!("{i:?}")
    };
    assert_eq!(target, 0x16d196);
    assert!(!i.is_call());
}

#[test]
fn test_a_conditional_jump_names_its_condition_as_x86_encodes_it() {
    // jne (75), short and near (0f 85); jl (7c); jmp has none.
    assert_eq!(decode(0, &hex("75c6")).unwrap().condition(), Some(5));
    assert_eq!(
        decode(0, &hex("0f8510000000")).unwrap().condition(),
        Some(5)
    );
    assert_eq!(decode(0, &hex("7c02")).unwrap().condition(), Some(0xc));
    assert_eq!(decode(0, &hex("eb07")).unwrap().condition(), None);
}

#[test]
fn test_a_bool_returned_in_al_is_read_by_test_and_or_mov_of_al() {
    for b in ["a801", "2401", "3401", "88442417"] {
        assert!(decode(0, &hex(b)).unwrap().reads_bool_al(), "{b}");
    }
    // A load into al and a ud2 after a panic call do not read a result.
    for b in ["8a442417", "0f0b"] {
        assert!(!decode(0, &hex(b)).unwrap().reads_bool_al(), "{b}");
    }
}

#[test]
fn test_negating_a_bool_normalisation_swaps_and_with_xor() {
    assert_eq!(
        decode(0, &hex("2401")).unwrap().negated_bool(),
        Some(hex("3401"))
    );
    assert_eq!(
        decode(0, &hex("3401")).unwrap().negated_bool(),
        Some(hex("2401"))
    );
    // sete al -> setne al
    assert_eq!(
        decode(0, &hex("0f94c0")).unwrap().negated_bool(),
        Some(hex("0f95c0"))
    );
}

#[test]
fn test_a_not_on_a_bool_in_al_is_undone_by_normalising_it_instead() {
    // rustc's `!b`: `xor al, 0xff` (then `and al, 1`), or `xor al, 1`.
    assert_eq!(
        decode(0, &hex("34ff")).unwrap().undone_not(),
        Some(hex("2401"))
    );
    assert_eq!(
        decode(0, &hex("3401")).unwrap().undone_not(),
        Some(hex("2401"))
    );
    // Not a `!`: other constants, other registers, a normalisation.
    assert_eq!(decode(0, &hex("3402")).unwrap().undone_not(), None);
    assert_eq!(decode(0, &hex("80f1ff")).unwrap().undone_not(), None);
    assert_eq!(decode(0, &hex("2401")).unwrap().undone_not(), None);
}

#[test]
fn test_a_jump_reencodes_inverted_or_retargeted_at_its_own_length() {
    let i = decode(0x16d1ce, &hex("75c6")).unwrap();
    assert_eq!(i.inverted_jump(), Some(hex("74c6")));
    assert_eq!(i.retargeted(0x16d1d0), Some(hex("7500")));
}

#[test]
fn test_a_constant_store_to_a_stack_slot_names_slot_and_value() {
    // movb $0x1,0x17(%rsp): the `||` chain's result true.
    let i = decode(0, &hex("c644241701")).unwrap();
    assert_eq!(i.stack_store(), Some((0x17, Some(1), 1)));
    // mov %al,0x17(%rsp): the last operand's result.
    let i = decode(0, &hex("88442417")).unwrap();
    assert_eq!(i.stack_store(), Some((0x17, None, 1)));
}

#[test]
fn test_a_near_jump_reencodes_with_a_32_bit_displacement() {
    // jne rel32 (0f 85): inverted to je, retargeted 0x1000 ahead.
    let i = decode(0x1000, &hex("0f8510000000")).unwrap();
    assert_eq!(i.inverted_jump(), Some(hex("0f8410000000")));
    assert_eq!(i.retargeted(0x2006), Some(hex("0f8500100000")));
}

#[test]
fn test_a_short_jump_that_cannot_reach_is_not_retargeted() {
    let i = decode(0x1000, &hex("75c6")).unwrap();
    assert_eq!(i.retargeted(0x9000), None);
}

#[test]
fn test_a_setcc_names_its_condition_and_reencodes_with_another() {
    // setl al (0f 9c c0) -> setge al (0f 9d c0); setcc to memory keeps its ModRM.
    let setl = decode(0, &hex("0f9cc0")).unwrap();
    assert_eq!(setl.setcc_condition(), Some(0xc));
    assert_eq!(setl.with_setcc_condition(0xd), Some(hex("0f9dc0")));
    let to_mem = decode(0, &hex("0f94442417")).unwrap();
    assert_eq!(to_mem.with_setcc_condition(0x5), Some(hex("0f95442417")));
    assert_eq!(decode(0, &hex("7c02")).unwrap().setcc_condition(), None);
}
