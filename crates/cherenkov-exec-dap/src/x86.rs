//! x86-64 instructions by meaning: decoded by iced-x86, so patching rules
//! ask what an instruction does instead of matching bytes. The rewrites tier
//! P makes are few and written by hand at the instruction's own length:
//! a jump inverted or retargeted, a bool normalisation negated.

use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, Mnemonic, OpKind, Register};

/// One decoded instruction, with its bytes.
#[derive(Debug, Clone)]
pub struct Instr {
    pub pc: u64,
    pub bytes: Vec<u8>,
    inner: Instruction,
}

/// A near jump: conditional or not, and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jump {
    pub conditional: bool,
    pub target: u64,
}

/// The first instruction of `bytes`, at address `pc`.
pub fn decode(pc: u64, bytes: &[u8]) -> Option<Instr> {
    let mut decoder = Decoder::with_ip(64, bytes, pc, DecoderOptions::NONE);
    let inner = decoder.decode();
    if inner.is_invalid() {
        return None;
    }
    Some(Instr {
        pc,
        bytes: bytes[..inner.len()].to_vec(),
        inner,
    })
}

impl Instr {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn next(&self) -> u64 {
        self.pc + self.bytes.len() as u64
    }

    pub fn is_call(&self) -> bool {
        matches!(
            self.inner.flow_control(),
            FlowControl::Call | FlowControl::IndirectCall
        )
    }

    pub fn jump(&self) -> Option<Jump> {
        let conditional = self.inner.is_jcc_short_or_near();
        if !(conditional || self.inner.is_jmp_short_or_near()) {
            return None;
        }
        Some(Jump {
            conditional,
            target: self.inner.near_branch_target(),
        })
    }

    /// A conditional jump's condition code, as x86 encodes it (`70+cc`):
    /// 0 is `o`, 5 is `ne`, 15 is `g`.
    pub fn condition(&self) -> Option<u8> {
        if !self.inner.is_jcc_short_or_near() {
            return None;
        }
        // iced numbers them from 1, after `None`.
        (self.inner.condition_code() as u8).checked_sub(1)
    }

    /// Reads a bool from `al`, as the instruction after a call returning
    /// `bool` does: `test al, 1`, `and al, 1`, `xor al, 1`, `test r/m8, al`,
    /// `mov r/m8, al`.
    pub fn reads_bool_al(&self) -> bool {
        let i = &self.inner;
        let al_imm1 = i.op0_kind() == OpKind::Register
            && i.op0_register() == Register::AL
            && i.op1_kind() == OpKind::Immediate8
            && i.immediate8() == 1;
        let from_al = i.op_count() == 2
            && i.op1_kind() == OpKind::Register
            && i.op1_register() == Register::AL;
        match i.mnemonic() {
            Mnemonic::Test | Mnemonic::And | Mnemonic::Xor => {
                al_imm1 || (i.mnemonic() == Mnemonic::Test && from_al)
            }
            Mnemonic::Mov => from_al,
            _ => false,
        }
    }

    /// Tests the low bit of a byte in memory (`testb $1, slot`): a bool
    /// the expression stored, as opposed to a call's result in `al`.
    pub fn tests_low_bit_in_memory(&self) -> bool {
        self.tests_low_bit() && self.inner.op0_kind() == OpKind::Memory
    }

    /// A `cmp`: it only sets flags, for the jump that follows.
    pub fn is_compare(&self) -> bool {
        self.inner.mnemonic() == Mnemonic::Cmp
    }

    /// Reads its second operand from memory (`add r, [m]`, `addsd x, [m]`):
    /// rustc puts the left operand of a commutative operation there when it
    /// has it in memory, and the right one in the register.
    pub fn reads_memory_source(&self) -> bool {
        self.inner.op_count() >= 2 && self.inner.op1_kind() == OpKind::Memory
    }

    /// A `mov`, which leaves the flags as they were.
    pub fn is_move(&self) -> bool {
        self.inner.mnemonic() == Mnemonic::Mov
    }

    /// Loads an address (`lea`): how rustc passes a panic's location.
    pub fn is_lea(&self) -> bool {
        self.inner.mnemonic() == Mnemonic::Lea
    }

    /// Tests the low bit of a byte (`test r/m8, 1`): how a bool stored to a
    /// slot or register is read for a branch.
    pub fn tests_low_bit(&self) -> bool {
        let i = &self.inner;
        i.mnemonic() == Mnemonic::Test && i.op1_kind() == OpKind::Immediate8 && i.immediate8() == 1
    }

    /// The same-length instruction computing the opposite bool: `and al, 1`
    /// (normalises) and `xor al, 1` (negates) swap; `setcc` inverts.
    pub fn negated_bool(&self) -> Option<Vec<u8>> {
        let i = &self.inner;
        let al_imm1 = i.op0_kind() == OpKind::Register
            && i.op0_register() == Register::AL
            && i.op1_kind() == OpKind::Immediate8
            && i.immediate8() == 1;
        let mut out = self.bytes.clone();
        match (i.mnemonic(), &self.bytes[..]) {
            // The short `al, imm8` forms: 24 (and), 34 (xor).
            (Mnemonic::And, [0x24, 0x01]) if al_imm1 => out[0] = 0x34,
            (Mnemonic::Xor, [0x34, 0x01]) if al_imm1 => out[0] = 0x24,
            // setcc is 0f 90+cc: the condition's low bit inverts it.
            (m, [0x0f, op, ..]) if is_setcc(m) && (0x90..=0x9f).contains(op) => out[1] ^= 1,
            _ => return None,
        }
        Some(out)
    }

    /// A `!` on the bool in `al` (`xor al, 1`, or rustc's `xor al, 0xff`
    /// before `and al, 1`) undone: `and al, 1`, which keeps the bool.
    pub fn undone_not(&self) -> Option<Vec<u8>> {
        // The short `al, imm8` form: 34 ib.
        match self.bytes[..] {
            [0x34, 0x01 | 0xff] if self.inner.mnemonic() == Mnemonic::Xor => Some(vec![0x24, 0x01]),
            _ => None,
        }
    }

    /// A `setcc`'s condition code, as x86 encodes it (`0f 90+cc`).
    pub fn setcc_condition(&self) -> Option<u8> {
        match self.bytes[..] {
            [0x0f, op, ..] if is_setcc(self.inner.mnemonic()) && (0x90..=0x9f).contains(&op) => {
                Some(op & 0x0f)
            }
            _ => None,
        }
    }

    /// This `setcc` with condition code `cc` instead, at its own length.
    pub fn with_setcc_condition(&self, cc: u8) -> Option<Vec<u8>> {
        self.setcc_condition()?;
        let mut out = self.bytes.clone();
        out[1] = 0x90 | (cc & 0x0f);
        Some(out)
    }

    /// This conditional jump with its condition inverted: the condition's
    /// low bit, in `70+cc` (short) or `0f 80+cc` (near).
    pub fn inverted_jump(&self) -> Option<Vec<u8>> {
        if !self.inner.is_jcc_short_or_near() {
            return None;
        }
        let mut out = self.bytes.clone();
        let at = usize::from(out.first() == Some(&0x0f));
        out[at] ^= 1;
        Some(out)
    }

    /// This jump sent to `target` instead, at its own length; `None` when
    /// the displacement does not fit.
    pub fn retargeted(&self, target: u64) -> Option<Vec<u8>> {
        self.jump()?;
        let rel = i128::from(target) - i128::from(self.next());
        let mut out = self.bytes.clone();
        // The displacement is the instruction's last 1 or 4 bytes.
        let short = matches!(out[..], [0x70..=0x7f | 0xeb, _]);
        if short {
            out[1] = i8::try_from(rel).ok()? as u8;
        } else {
            let n = out.len();
            out[n - 4..].copy_from_slice(&i32::try_from(rel).ok()?.to_le_bytes());
        }
        Some(out)
    }

    /// A store to a stack slot (`[rsp+disp]`): its displacement, the stored
    /// constant (or `None` for a register), and its size in bytes.
    pub fn stack_store(&self) -> Option<(i64, Option<u64>, u64)> {
        let i = &self.inner;
        if i.mnemonic() != Mnemonic::Mov
            || i.op0_kind() != OpKind::Memory
            || i.memory_base() != Register::RSP
        {
            return None;
        }
        let size = i.memory_size().size() as u64;
        let value = match i.op1_kind() {
            OpKind::Immediate8 => Some(u64::from(i.immediate8())),
            OpKind::Immediate16 => Some(u64::from(i.immediate16())),
            OpKind::Immediate32 => Some(u64::from(i.immediate32())),
            OpKind::Immediate32to64 => Some(i.immediate32to64() as u64),
            OpKind::Register => None,
            _ => return None,
        };
        Some((i.memory_displacement64() as i64, value, size))
    }
}

fn is_setcc(m: Mnemonic) -> bool {
    use Mnemonic::*;
    matches!(
        m,
        Seto | Setno
            | Setb
            | Setae
            | Sete
            | Setne
            | Setbe
            | Seta
            | Sets
            | Setns
            | Setp
            | Setnp
            | Setl
            | Setge
            | Setle
            | Setg
    )
}
