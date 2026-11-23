use cherenkov_core::token::Token;
use cherenkov_exec_dap::isa::{Isa, Jcc, Signedness};

#[test]
fn test_amd64_table_names_condition_codes_and_inverts_by_low_bit() {
    let isa = Isa::amd64();
    assert_eq!(isa.cc_name(0xc), Some("l"));
    assert_eq!(isa.cc_name(isa.inverse(0xc)), Some("ge"));
    assert_eq!(isa.cc_name(isa.inverse(0x7)), Some("be"));
}

#[test]
fn test_decode_short_and_near_conditional_jumps() {
    let isa = Isa::amd64();
    assert_eq!(
        isa.decode_jcc(&[0x7c, 0x02]),
        Some(Jcc {
            cc: 0xc,
            cc_offset: 0,
            len: 2,
            rel: 2
        })
    );
    assert_eq!(
        isa.decode_jcc(&[0x7d, 0xfe]),
        Some(Jcc {
            cc: 0xd,
            cc_offset: 0,
            len: 2,
            rel: -2
        })
    );
    assert_eq!(
        isa.decode_jcc(&[0x0f, 0x8e, 0xfb, 0xff, 0xff, 0xff]),
        Some(Jcc {
            cc: 0xe,
            cc_offset: 1,
            len: 6,
            rel: -5
        })
    );
}

#[test]
fn test_decode_rejects_non_conditional_and_truncated_instructions() {
    let isa = Isa::amd64();
    for bytes in [
        &[0xeb, 0x10][..],
        &[0xe8, 0xb2, 0xff, 0xff, 0xff],
        &[0x7c],
        &[0x0f, 0x8c, 0x00],
        &[0x0f, 0x1f, 0x00],
        &[],
    ] {
        assert_eq!(isa.decode_jcc(bytes), None, "{bytes:02x?}");
    }
    // Trailing bytes beyond the instruction are not part of it.
    assert_eq!(isa.decode_jcc(&[0x7c, 0x02, 0x90]), None);
}

#[test]
fn test_with_cc_rewrites_only_the_condition_nibble() {
    let isa = Isa::amd64();
    let short = [0x7c, 0x02];
    assert_eq!(
        isa.with_cc(&short, &isa.decode_jcc(&short).unwrap(), 0xe),
        [0x7e, 0x02]
    );
    let near = [0x0f, 0x8c, 0x10, 0x00, 0x00, 0x00];
    assert_eq!(
        isa.with_cc(&near, &isa.decode_jcc(&near).unwrap(), 0xd),
        [0x0f, 0x8d, 0x10, 0x00, 0x00, 0x00]
    );
}

#[test]
fn test_encode_jmp_prefers_short_form_when_it_fits() {
    let isa = Isa::amd64();
    assert_eq!(isa.encode_jmp(0x100, 0x105, 2), Some(vec![0xeb, 0x03]));
    assert_eq!(isa.encode_jmp(0x100, 0x100, 2), Some(vec![0xeb, 0xfe]));
    assert_eq!(isa.encode_jmp(0x100, 0x181, 5), Some(vec![0xeb, 0x7f]));
}

#[test]
fn test_encode_jmp_falls_back_to_near_form_or_none() {
    let isa = Isa::amd64();
    assert_eq!(
        isa.encode_jmp(0x100, 0x182, 5),
        Some(vec![0xe9, 0x7d, 0x00, 0x00, 0x00])
    );
    assert_eq!(
        isa.encode_jmp(0x200, 0x100, 5),
        Some(vec![0xe9, 0xfb, 0xfe, 0xff, 0xff])
    );
    assert_eq!(isa.encode_jmp(0x100, 0x182, 4), None);
    assert_eq!(isa.encode_jmp(0x100, 0x101, 1), None);
}

#[test]
fn test_relational_condition_codes_by_signedness_and_swap() {
    let isa = Isa::amd64();
    let name = |cc: Option<u8>| cc.and_then(|c| isa.cc_name(c).map(String::from));
    assert_eq!(
        name(isa.relational(Signedness::Signed, Token::Lt)).as_deref(),
        Some("l")
    );
    assert_eq!(
        name(isa.relational(Signedness::Unsigned, Token::Le)).as_deref(),
        Some("be")
    );
    assert_eq!(name(isa.relational(Signedness::Signed, Token::Add)), None);
    assert_eq!(isa.swapped(Token::Lt), Some(Token::Gt));
    assert_eq!(isa.swapped(Token::Eq), Some(Token::Eq));
}

#[test]
fn test_parse_rejects_unknown_condition_code_and_token_names() {
    let good = include_str!("../../../isa/amd64.toml");
    let bad_cc = good.replace("lt = \"l\"", "lt = \"less\"");
    assert!(Isa::parse(&bad_cc).unwrap_err().0.contains("less"));
    let bad_token = good.replace("lt = \"gt\"", "lt = \"greater\"");
    assert!(Isa::parse(&bad_token).unwrap_err().0.contains("greater"));
    assert!(Isa::parse("inverse_mask = 1").is_err());
}

#[test]
fn test_decode_call_recognizes_direct_and_indirect_calls() {
    let isa = Isa::amd64();
    for call in [
        &[0xe8, 0x66, 0xff, 0xff, 0xff][..],
        &[0xff, 0x15, 0x17, 0xd1, 0x05, 0x00],
        &[0xff, 0xd0],
        &[0x41, 0xff, 0xd3],
    ] {
        assert!(isa.is_call(call), "{call:02x?}");
    }
    for other in [
        &[0xeb, 0x03][..],
        &[0xff, 0xe0],
        &[0xff, 0x25, 0, 0, 0, 0],
        &[0x48, 0x8b, 0x44, 0x24, 0x08],
        &[0xe8, 0x00],
        &[],
    ] {
        assert!(!isa.is_call(other), "{other:02x?}");
    }
}

#[test]
fn test_abi_selects_a_return_stub_and_unknown_abis_are_rejected() {
    let isa = Isa::amd64();
    assert_eq!(isa.return_stub(), None);
    assert_eq!(
        isa.with_abi("sysv").unwrap().return_stub(),
        Some(&[0x31, 0xc0, 0x0f, 0x57, 0xc0, 0xc3][..])
    );
    assert!(
        isa.with_abi("go")
            .unwrap()
            .return_stub()
            .unwrap()
            .ends_with(&[0xc3])
    );
    assert!(isa.with_abi("vax").unwrap_err().0.contains("vax"));
}

// --- Floating-point and bitwise swaps.

#[test]
fn test_float_swap_changes_only_the_opcode_of_double_and_single_scalar_arithmetic() {
    let isa = Isa::amd64();
    // addsd xmm0, xmm1 / addss, and with a REX prefix after the F2.
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x58, 0xc1], Token::Add, Token::Sub),
        Some(vec![0xf2, 0x0f, 0x5c, 0xc1])
    );
    assert_eq!(
        isa.float_swap(&[0xf3, 0x0f, 0x59, 0xc1], Token::Mul, Token::Div),
        Some(vec![0xf3, 0x0f, 0x5e, 0xc1])
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x44, 0x0f, 0x5e, 0xc1], Token::Div, Token::Add),
        Some(vec![0xf2, 0x44, 0x0f, 0x58, 0xc1])
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x5c, 0x45, 0xf8], Token::Sub, Token::Mul),
        Some(vec![0xf2, 0x0f, 0x59, 0x45, 0xf8])
    );
}

#[test]
fn test_float_swap_leaves_other_instructions_and_operators_alone() {
    let isa = Isa::amd64();
    // movsd, packed addpd, an addsd read as a sub, and non-arithmetic tokens.
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x10, 0xc1], Token::Add, Token::Sub),
        None
    );
    assert_eq!(
        isa.float_swap(&[0x66, 0x0f, 0x58, 0xc1], Token::Add, Token::Sub),
        None
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x58, 0xc1], Token::Sub, Token::Add),
        None
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x58, 0xc1], Token::Add, Token::Rem),
        None
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x58, 0xc1], Token::Rem, Token::Add),
        None
    );
    assert_eq!(
        isa.float_swap(&[0xf2, 0x0f, 0x58], Token::Add, Token::Sub),
        None
    );
    assert_eq!(isa.float_swap(&[], Token::Add, Token::Sub), None);
}

#[test]
fn test_bitwise_swap_changes_the_opcode_or_the_modrm_reg_field() {
    let isa = Isa::amd64();
    // or rax, rcx (09 /r) as and and xor.
    assert_eq!(
        isa.bitwise_swap(&[0x48, 0x09, 0xc8], Token::BitOr, Token::BitAnd),
        Some(vec![0x48, 0x21, 0xc8])
    );
    assert_eq!(
        isa.bitwise_swap(&[0x48, 0x09, 0xc8], Token::BitOr, Token::BitXor),
        Some(vec![0x48, 0x31, 0xc8])
    );
    // or dword [rsp+8], 4 (83 /1 ib) as and (83 /4 ib).
    assert_eq!(
        isa.bitwise_swap(&[0x83, 0x4c, 0x24, 0x08, 0x04], Token::BitOr, Token::BitAnd),
        Some(vec![0x83, 0x64, 0x24, 0x08, 0x04])
    );
    // or eax, imm32 (0d id) as xor (35 id): no ModRM byte.
    assert_eq!(
        isa.bitwise_swap(&[0x0d, 0x00, 0x01, 0x00, 0x00], Token::BitOr, Token::BitXor),
        Some(vec![0x35, 0x00, 0x01, 0x00, 0x00])
    );
}

#[test]
fn test_bitwise_swap_is_only_for_the_source_operation_and_not_for_zeroing() {
    let isa = Isa::amd64();
    assert_eq!(
        isa.bitwise_swap(&[0x48, 0x21, 0xc8], Token::BitOr, Token::BitAnd),
        None
    );
    // xor eax, eax zeroes: it is no operation of the program's.
    assert_eq!(
        isa.bitwise_swap(&[0x31, 0xc0], Token::BitXor, Token::BitOr),
        None
    );
    // xor with two registers is one.
    assert_eq!(
        isa.bitwise_swap(&[0x31, 0xc8], Token::BitXor, Token::BitOr),
        Some(vec![0x09, 0xc8])
    );
    // xor esi, 1 (83 /6): the reg field is the opcode's extension, the rm
    // field the register, and they may be equal.
    assert_eq!(
        isa.bitwise_swap(&[0x83, 0xf6, 0x01], Token::BitXor, Token::BitOr),
        Some(vec![0x83, 0xce, 0x01])
    );
    assert_eq!(
        isa.bitwise_swap(&[0x48, 0x09, 0xc8], Token::BitOr, Token::Add),
        None
    );
    assert_eq!(isa.bitwise_swap(&[], Token::BitOr, Token::BitAnd), None);
}
