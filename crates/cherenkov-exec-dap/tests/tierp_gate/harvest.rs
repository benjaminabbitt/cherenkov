//! What rustc makes of a value alone: the bytes of `fn f() -> T { value }`,
//! kept as a return stub only when they are made of register moves and the
//! return, touching nothing but the caller-saved result registers (no
//! relocation, no memory, no jump).

use iced_x86::{Decoder, DecoderOptions, Mnemonic, OpKind, Register};

use crate::exe::Function;

/// The registers a stub may write: result registers and scratch.
const SCRATCH: [Register; 3] = [Register::RAX, Register::RCX, Register::RDX];

/// The bytes of the function up to its return (the padding after it is not
/// the function's) when they can be written over another's entry, or why
/// they cannot.
pub fn stub_bytes(function: &Function) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut last = Mnemonic::INVALID;
    for insn in &function.insns {
        let decoded = Decoder::new(64, &insn.bytes, DecoderOptions::NONE).decode();
        check(&decoded)?;
        last = decoded.mnemonic();
        bytes.extend_from_slice(&insn.bytes);
        if last == Mnemonic::Ret {
            break;
        }
    }
    if last != Mnemonic::Ret {
        return Err("the body does not end in a return".into());
    }
    Ok(bytes)
}

fn check(insn: &iced_x86::Instruction) -> Result<(), String> {
    if !matches!(
        insn.mnemonic(),
        Mnemonic::Mov | Mnemonic::Xor | Mnemonic::And | Mnemonic::Ret
    ) {
        return Err(format!("{:?} is not a register move", insn.mnemonic()));
    }
    (0..insn.op_count()).try_for_each(|i| match insn.op_kind(i) {
        OpKind::Register if SCRATCH.contains(&insn.op_register(i).full_register()) => Ok(()),
        OpKind::Register => Err(format!("writes {:?}", insn.op_register(i))),
        OpKind::Immediate8
        | OpKind::Immediate16
        | OpKind::Immediate32
        | OpKind::Immediate64
        | OpKind::Immediate8to16
        | OpKind::Immediate8to32
        | OpKind::Immediate8to64
        | OpKind::Immediate32to64 => Ok(()),
        other => Err(format!("operand {other:?}")),
    })
}
