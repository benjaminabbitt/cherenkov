//! Instruction shapes and patch rules, loaded from `isa/<arch>.toml`.

use std::collections::BTreeMap;

use angzarr_diagnostic::{Arg, Diagnostic, code};
use cherenkov_mutation::token::Token;
use serde::Deserialize;

use crate::shapes::{Check, Kind, Shape, Table};

/// Why an instruction table does not load or answer: coded, with named
/// data (the messages are `locales/en-US/messages.ftl`'s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsaError {
    /// The table is not TOML of the right shape; `detail` is what the
    /// parser said.
    Toml { detail: String },
    /// The table names a token that does not exist.
    UnknownToken { name: String },
    /// The table names a condition code it does not define.
    UnknownCondition { name: String },
    /// A conditional jump's displacement is not 1 or 4 bytes.
    BadDisplacement { rel: usize },
    /// The table has no return stub for the calling convention.
    NoStub { abi: String },
    /// A stub's line is not `shape = hex bytes`.
    BadStubLine { line: String },
}

impl std::fmt::Display for IsaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        angzarr_diagnostic::display(self, f)
    }
}

impl std::error::Error for IsaError {}

impl Diagnostic for IsaError {
    fn code(&self) -> &str {
        match self {
            IsaError::Toml { .. } => code!("isa-toml"),
            IsaError::UnknownToken { .. } => code!("isa-unknown-token"),
            IsaError::UnknownCondition { .. } => code!("isa-unknown-condition"),
            IsaError::BadDisplacement { .. } => code!("isa-bad-displacement"),
            IsaError::NoStub { .. } => code!("isa-no-stub"),
            IsaError::BadStubLine { .. } => code!("isa-bad-stub-line"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            IsaError::Toml { detail } => vec![("detail", detail.as_str().into())],
            IsaError::UnknownToken { name } | IsaError::UnknownCondition { name } => {
                vec![("name", name.as_str().into())]
            }
            IsaError::BadDisplacement { rel } => vec![("rel", (*rel).into())],
            IsaError::NoStub { abi } => vec![("abi", abi.as_str().into())],
            IsaError::BadStubLine { line } => vec![("line", line.as_str().into())],
        }
    }
}

/// A decoded conditional jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jcc {
    pub cc: u8,
    /// Byte offset of the opcode byte that carries the condition code.
    pub cc_offset: usize,
    pub len: usize,
    /// Displacement, relative to the end of the instruction.
    pub rel: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signedness {
    Signed,
    Unsigned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isa {
    cc_names: BTreeMap<u8, String>,
    inverse_mask: u8,
    jcc: Vec<(Vec<u8>, u8, usize)>,
    jmp: Vec<(u8, usize)>,
    rex: (u8, u8),
    /// (opcode, exact length, ModRM reg field)
    call: Vec<(u8, Option<usize>, Option<u8>)>,
    arith: Vec<(Form, Form)>,
    bitwise: Vec<BitwiseToml>,
    float: FloatToml,
    bool_store: Vec<BoolStoreToml>,
    bool_zero: Vec<BoolZeroToml>,
    return_stubs: BTreeMap<String, Vec<u8>>,
    /// The calling convention [`Isa::return_stub`] answers for.
    abi: Option<String>,
    relational: BTreeMap<(Signedness, Token), u8>,
    swapped: BTreeMap<Token, Token>,
    /// The arithmetic patch shapes known to be right (Rust code only).
    shapes: Option<Table>,
}

/// An instruction swapped to another operation, and what kind of
/// instruction it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    pub replacement: Vec<u8>,
    /// The source instruction's opcode (and ModRM reg field), in hex.
    pub encoding: String,
    /// The operand width in bits.
    pub width: u8,
    /// The second operand is in memory.
    pub memory: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsaToml {
    inverse_mask: u8,
    rex: [u8; 2],
    call: Vec<CallToml>,
    arith: Vec<ArithToml>,
    bitwise: Vec<BitwiseToml>,
    float_arith: FloatToml,
    bool_store: Vec<BoolStoreToml>,
    bool_zero: Vec<BoolZeroToml>,
    return_stub: BTreeMap<String, Vec<u8>>,
    condition_codes: BTreeMap<String, u8>,
    jcc: Vec<JccToml>,
    jmp: Vec<JmpToml>,
    relational: RelationalToml,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JccToml {
    prefix: Vec<u8>,
    opcode: u8,
    rel: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallToml {
    opcode: u8,
    length: Option<usize>,
    modrm_reg: Option<u8>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct Form {
    opcode: u8,
    modrm_reg: Option<u8>,
    /// The opcode is followed by a ModRM byte (not the `al/eax, imm` forms).
    #[serde(default = "yes")]
    modrm: bool,
    /// The operands are bytes.
    #[serde(default)]
    byte: bool,
}

fn yes() -> bool {
    true
}

/// The bitwise operations' encodings of one shape.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct BitwiseToml {
    and: Form,
    or: Form,
    xor: Form,
}

/// Scalar floating-point arithmetic: `<prefix> [REX] 0F <opcode> ModRM`,
/// the operations differing in the opcode alone.
#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct FloatToml {
    prefixes: Vec<u8>,
    escape: u8,
    add: u8,
    sub: u8,
    mul: u8,
    div: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArithToml {
    add: Form,
    sub: Form,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BoolStoreToml {
    opcode: u8,
    modrm_reg: Option<u8>,
    #[serde(default)]
    reg_in_opcode: bool,
    imm: usize,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BoolZeroToml {
    bytes: Vec<u8>,
    one: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JmpToml {
    opcode: u8,
    rel: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationalToml {
    signed: BTreeMap<String, String>,
    unsigned: BTreeMap<String, String>,
    swapped: BTreeMap<String, String>,
}

/// A form's opcode, and its ModRM reg field where the encoding has one, in hex.
fn encoding(form: &Form) -> String {
    match form.modrm_reg {
        Some(r) => format!("{:02x}/{r}", form.opcode),
        None => format!("{:02x}", form.opcode),
    }
}

fn bitwise_form(row: &BitwiseToml, t: Token) -> Option<Form> {
    match t {
        Token::BitAnd => Some(row.and),
        Token::BitOr => Some(row.or),
        Token::BitXor => Some(row.xor),
        _ => None,
    }
}

fn ordered_pairs(tokens: &[Token]) -> Vec<(Token, Token)> {
    tokens
        .iter()
        .flat_map(|&a| {
            tokens
                .iter()
                .filter(move |&&b| b != a)
                .map(move |&b| (a, b))
        })
        .collect()
}

/// The shapes of one encoding: each width it can have, its second operand
/// in a register and (where it has a ModRM byte) in memory.
fn form_shapes(kind: Kind, form: &Form, from: Token, with: Token) -> Vec<Shape> {
    let widths: &[u8] = if form.byte { &[8] } else { &[16, 32, 64] };
    let memories: &[bool] = if form.modrm { &[false, true] } else { &[false] };
    widths
        .iter()
        .flat_map(|&width| {
            memories.iter().map(move |&memory| Shape {
                kind,
                width,
                from,
                with,
                encoding: encoding(form),
                memory,
            })
        })
        .collect()
}

fn token(name: &str) -> Result<Token, IsaError> {
    Token::from_name(name).ok_or_else(|| IsaError::UnknownToken {
        name: name.to_string(),
    })
}

impl Isa {
    pub fn parse(toml_src: &str) -> Result<Isa, IsaError> {
        let t: IsaToml = toml::from_str(toml_src).map_err(|e| IsaError::Toml {
            detail: e.to_string(),
        })?;
        let by_name: BTreeMap<&str, u8> = t
            .condition_codes
            .iter()
            .map(|(n, &c)| (n.as_str(), c))
            .collect();
        let cc = |name: &str| {
            by_name
                .get(name)
                .copied()
                .ok_or_else(|| IsaError::UnknownCondition {
                    name: name.to_string(),
                })
        };
        let mut relational = BTreeMap::new();
        for (signedness, table) in [
            (Signedness::Signed, &t.relational.signed),
            (Signedness::Unsigned, &t.relational.unsigned),
        ] {
            for (tok, name) in table {
                relational.insert((signedness, token(tok)?), cc(name)?);
            }
        }
        let mut swapped = BTreeMap::new();
        for (a, b) in &t.relational.swapped {
            swapped.insert(token(a)?, token(b)?);
        }
        for j in &t.jcc {
            if j.rel != 1 && j.rel != 4 {
                return Err(IsaError::BadDisplacement { rel: j.rel });
            }
        }
        Ok(Isa {
            cc_names: t.condition_codes.into_iter().map(|(n, c)| (c, n)).collect(),
            inverse_mask: t.inverse_mask,
            jcc: t
                .jcc
                .into_iter()
                .map(|j| (j.prefix, j.opcode, j.rel))
                .collect(),
            jmp: t.jmp.into_iter().map(|j| (j.opcode, j.rel)).collect(),
            rex: (t.rex[0], t.rex[1]),
            call: t
                .call
                .into_iter()
                .map(|c| (c.opcode, c.length, c.modrm_reg))
                .collect(),
            arith: t.arith.into_iter().map(|a| (a.add, a.sub)).collect(),
            bitwise: t.bitwise,
            float: t.float_arith,
            bool_store: t.bool_store,
            bool_zero: t.bool_zero,
            return_stubs: t.return_stub,
            abi: None,
            relational,
            swapped,
            shapes: None,
        })
    }

    /// The table shipped in `isa/amd64.toml`, with the arithmetic shapes
    /// of `isa/tierp-shapes.toml`: Rust code's arithmetic is patched only
    /// where they say it is right.
    pub fn amd64() -> Isa {
        Isa {
            shapes: Some(
                Table::parse(include_str!("../../../../../isa/tierp-shapes.toml"))
                    .expect("isa/tierp-shapes.toml is valid"),
            ),
            ..Isa::amd64_unfiltered()
        }
    }

    /// The same without the shapes table: what the gate that writes the
    /// table patches with.
    pub fn amd64_unfiltered() -> Isa {
        Isa::parse(include_str!("../../../../../isa/amd64.toml")).expect("isa/amd64.toml is valid")
    }

    /// The same, deciding arithmetic shapes by `table`.
    pub fn with_shapes(&self, table: Table) -> Isa {
        Isa {
            shapes: Some(table),
            ..self.clone()
        }
    }

    /// The table of arithmetic shapes, when this ISA has one.
    pub fn shapes(&self) -> Option<&Table> {
        self.shapes.as_ref()
    }

    /// The bytes that return the trivial value of `shape`, when the shapes
    /// table says they are right (none without a table).
    pub fn stub(&self, shape: &crate::stubs::StubShape) -> Option<&[u8]> {
        self.shapes.as_ref()?.stub(shape)
    }

    /// The calling convention chosen with [`Isa::with_abi`].
    pub fn abi(&self) -> Option<&str> {
        self.abi.as_deref()
    }

    /// Whether arithmetic of `shape` may be patched (any shape, without a
    /// table).
    pub fn admits(&self, shape: &Shape) -> bool {
        self.shapes.as_ref().is_none_or(|t| t.admits(shape))
    }

    pub fn cc_name(&self, cc: u8) -> Option<&str> {
        self.cc_names.get(&cc).map(String::as_str)
    }

    pub fn inverse(&self, cc: u8) -> u8 {
        cc ^ self.inverse_mask
    }

    /// `bytes` must be exactly one instruction.
    pub fn decode_jcc(&self, bytes: &[u8]) -> Option<Jcc> {
        self.jcc.iter().find_map(|(prefix, opcode, rel)| {
            let cc_offset = prefix.len();
            let len = cc_offset + 1 + rel;
            if bytes.len() != len
                || !bytes.starts_with(prefix)
                || bytes[cc_offset] & 0xf0 != *opcode
            {
                return None;
            }
            let disp = &bytes[cc_offset + 1..];
            let rel = match disp.len() {
                1 => i64::from(disp[0] as i8),
                _ => i64::from(i32::from_le_bytes(disp.try_into().ok()?)),
            };
            Some(Jcc {
                cc: bytes[cc_offset] & 0x0f,
                cc_offset,
                len,
                rel,
            })
        })
    }

    /// The same table, answering [`Isa::return_stub`] for calling convention `abi`.
    pub fn with_abi(&self, abi: &str) -> Result<Isa, IsaError> {
        if !self.return_stubs.contains_key(abi) {
            return Err(IsaError::NoStub {
                abi: abi.to_string(),
            });
        }
        Ok(Isa {
            abi: Some(abi.to_string()),
            ..self.clone()
        })
    }

    /// Bytes that return the zero value of a register-returned result,
    /// for the ABI chosen with [`Isa::with_abi`].
    pub fn return_stub(&self) -> Option<&[u8]> {
        self.abi
            .as_ref()
            .and_then(|a| self.return_stubs.get(a))
            .map(Vec::as_slice)
    }

    /// Split an optional REX prefix from the instruction.
    fn rex_split<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        match bytes.first() {
            Some(&b) if (self.rex.0..=self.rex.1).contains(&b) => bytes.split_at(1),
            _ => bytes.split_at(0),
        }
    }

    /// Split the operand-size prefix (`66`) and a REX prefix, each
    /// optional, from the instruction.
    fn legacy_split<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        let size = usize::from(bytes.first() == Some(&0x66));
        let rex = bytes
            .get(size)
            .is_some_and(|b| (self.rex.0..=self.rex.1).contains(b));
        bytes.split_at(size + usize::from(rex))
    }

    /// The operand width a prefix and the form's own size give.
    fn width(&self, prefix: &[u8], byte: bool) -> u8 {
        let wide = prefix
            .last()
            .is_some_and(|&b| (self.rex.0..=self.rex.1).contains(&b) && b & 0x8 != 0);
        match (byte, wide, prefix.contains(&0x66)) {
            (true, _, _) => 8,
            (_, true, _) => 64,
            (_, _, true) => 16,
            _ => 32,
        }
    }

    /// `bytes` with an add-family instruction turned into the matching sub
    /// (or back, when `to_add`); `None` if it is not of the source family.
    pub fn arith_swap(&self, bytes: &[u8], to_add: bool) -> Option<Vec<u8>> {
        self.arith_match(bytes, to_add).map(|m| m.replacement)
    }

    /// [`Isa::arith_swap`], with what kind of instruction was swapped.
    pub fn arith_match(&self, bytes: &[u8], to_add: bool) -> Option<Matched> {
        let (prefix, body) = self.legacy_split(bytes);
        let (&opcode, rest) = body.split_first()?;
        let modrm_reg = rest.first().map(|m| (m >> 3) & 0x7);
        let matches =
            |f: &Form| f.opcode == opcode && f.modrm_reg.is_none_or(|r| Some(r) == modrm_reg);
        let (from, to) = self.arith.iter().find_map(|(add, sub)| {
            let (from, to) = if to_add { (sub, add) } else { (add, sub) };
            matches(from).then_some((*from, *to))
        })?;
        let mut out = prefix.to_vec();
        out.push(to.opcode);
        out.extend_from_slice(rest);
        if let (Some(r), Some(_)) = (to.modrm_reg, from.modrm_reg) {
            let m = prefix.len() + 1;
            out[m] = (out[m] & !0x38) | (r << 3);
        }
        Some(Matched {
            replacement: out,
            encoding: encoding(&from),
            width: self.width(prefix, from.byte),
            memory: from.modrm && rest.first().is_some_and(|m| m >> 6 != 3),
        })
    }

    /// `bytes`, a bitwise instruction (`and`, `or` or `xor`, any shape of
    /// the table) of the operation `from`, as the one of `with`; `None` if
    /// it is not of `from`'s. The registers-only forms that name one
    /// register twice are skipped: `xor r, r` zeroes, `and r, r` tests.
    pub fn bitwise_swap(&self, bytes: &[u8], from: Token, with: Token) -> Option<Vec<u8>> {
        self.bitwise_match(bytes, from, with).map(|m| m.replacement)
    }

    /// [`Isa::bitwise_swap`], with what kind of instruction was swapped.
    pub fn bitwise_match(&self, bytes: &[u8], from: Token, with: Token) -> Option<Matched> {
        let (prefix, body) = self.legacy_split(bytes);
        let (&opcode, rest) = body.split_first()?;
        let modrm = rest.first().copied();
        let (source, target) = self.bitwise.iter().find_map(|row| {
            let source = bitwise_form(row, from)?;
            let hit = source.opcode == opcode
                && source
                    .modrm_reg
                    .is_none_or(|r| modrm.map(|m| (m >> 3) & 0x7) == Some(r));
            if !hit {
                return None;
            }
            Some((source, bitwise_form(row, with)?))
        })?;
        if source.modrm
            && source.modrm_reg.is_none()
            && modrm.is_some_and(|m| m >> 6 == 3 && (m >> 3) & 0x7 == m & 0x7)
        {
            return None;
        }
        let mut out = prefix.to_vec();
        out.push(target.opcode);
        out.extend_from_slice(rest);
        if let (Some(r), Some(_)) = (target.modrm_reg, source.modrm_reg) {
            let m = prefix.len() + 1;
            out[m] = (out[m] & !0x38) | (r << 3);
        }
        Some(Matched {
            replacement: out,
            encoding: encoding(&source),
            width: self.width(prefix, source.byte),
            memory: source.modrm && modrm.is_some_and(|m| m >> 6 != 3),
        })
    }

    /// `bytes`, a scalar floating-point `from` (`+`, `-`, `*`, `/`), as the
    /// instruction of `with`; `None` if it is not one of `from`'s.
    pub fn float_swap(&self, bytes: &[u8], from: Token, with: Token) -> Option<Vec<u8>> {
        self.float_match(bytes, from, with).map(|m| m.replacement)
    }

    /// [`Isa::float_swap`], with what kind of instruction was swapped.
    pub fn float_match(&self, bytes: &[u8], from: Token, with: Token) -> Option<Matched> {
        let opcode = |t: Token| match t {
            Token::Add => Some(self.float.add),
            Token::Sub => Some(self.float.sub),
            Token::Mul => Some(self.float.mul),
            Token::Div => Some(self.float.div),
            _ => None,
        };
        let (&prefix, after) = bytes.split_first()?;
        if !self.float.prefixes.contains(&prefix) {
            return None;
        }
        let (rex, body) = self.rex_split(after);
        // The opcode byte: after the prefix, the REX and the escape.
        let at = 1 + rex.len() + 1;
        match body {
            [escape, op, modrm, ..] if *escape == self.float.escape && *op == opcode(from)? => {
                let mut out = bytes.to_vec();
                out[at] = opcode(with)?;
                Some(Matched {
                    replacement: out,
                    encoding: format!("{op:02x}"),
                    width: if prefix == 0xf3 { 32 } else { 64 },
                    memory: modrm >> 6 != 3,
                })
            }
            _ => None,
        }
    }

    /// Every shape of arithmetic patch these rules can make: each swap of
    /// each encoding at each width, each check, with the second operand in a
    /// register and in memory. The shapes table decides every one of them.
    pub fn shape_universe(&self) -> Vec<Shape> {
        let mut out = Vec::new();
        for (add, sub) in &self.arith {
            for (source, from, with) in
                [(add, Token::Add, Token::Sub), (sub, Token::Sub, Token::Add)]
            {
                for check in [Check::Overflow, Check::Carry, Check::Unchecked] {
                    out.extend(form_shapes(Kind::Int(check), source, from, with));
                }
            }
        }
        for row in &self.bitwise {
            for (from, with) in ordered_pairs(&[Token::BitAnd, Token::BitOr, Token::BitXor]) {
                let source = bitwise_form(row, from).expect("a bitwise token");
                out.extend(form_shapes(Kind::Bitwise, &source, from, with));
            }
        }
        out.extend(crate::tierp::compare_universe(self));
        for (from, with) in ordered_pairs(&[Token::Add, Token::Sub, Token::Mul, Token::Div]) {
            for width in [32, 64] {
                for memory in [false, true] {
                    out.push(Shape {
                        kind: Kind::Float,
                        width,
                        from,
                        with,
                        encoding: format!("{:02x}", self.float_opcode(from)),
                        memory,
                    });
                }
            }
        }
        out
    }

    fn float_opcode(&self, t: Token) -> u8 {
        match t {
            Token::Add => self.float.add,
            Token::Sub => self.float.sub,
            Token::Mul => self.float.mul,
            _ => self.float.div,
        }
    }

    /// `bytes` storing the other boolean, if it stores the constant `value`.
    pub fn bool_flip(&self, bytes: &[u8], value: bool) -> Option<Vec<u8>> {
        if !value && let Some(z) = self.bool_zero.iter().find(|z| z.bytes == bytes) {
            return Some(z.one.clone());
        }
        let (rex, body) = self.rex_split(bytes);
        let (&opcode, rest) = body.split_first()?;
        let store = self.bool_store.iter().find(|s| {
            let op_ok = if s.reg_in_opcode {
                opcode & !0x7 == s.opcode
            } else {
                opcode == s.opcode
            };
            op_ok
                && s.modrm_reg
                    .is_none_or(|r| rest.first().is_some_and(|m| (m >> 3) & 0x7 == r))
        })?;
        let imm_at = bytes.len().checked_sub(store.imm)?;
        if imm_at < rex.len() + 1 {
            return None;
        }
        let imm = &bytes[imm_at..];
        if imm[0] != u8::from(value) || imm[1..].iter().any(|&b| b != 0) {
            return None;
        }
        let mut out = bytes.to_vec();
        out[imm_at] = u8::from(!value);
        Some(out)
    }

    /// The destination offset of a conditional or unconditional jump,
    /// relative to the end of the instruction.
    pub fn jump_rel(&self, bytes: &[u8]) -> Option<i64> {
        if let Some(j) = self.decode_jcc(bytes) {
            return Some(j.rel);
        }
        let (&opcode, disp) = bytes.split_first()?;
        let &(_, rel) = self
            .jmp
            .iter()
            .find(|&&(op, rel)| op == opcode && disp.len() == rel)?;
        match rel {
            1 => Some(i64::from(disp[0] as i8)),
            4 => Some(i64::from(i32::from_le_bytes(disp.try_into().ok()?))),
            _ => None,
        }
    }

    /// Whether `bytes` (exactly one instruction) is a call.
    pub fn is_call(&self, bytes: &[u8]) -> bool {
        let body = match bytes.first() {
            Some(&b) if (self.rex.0..=self.rex.1).contains(&b) => &bytes[1..],
            _ => bytes,
        };
        self.call.iter().any(|&(opcode, length, modrm_reg)| {
            body.first() == Some(&opcode)
                && length.is_none_or(|n| body.len() == n)
                && modrm_reg.is_none_or(|r| body.get(1).is_some_and(|m| (m >> 3) & 0x7 == r))
        })
    }

    /// Re-encode `jcc` (decoded from `bytes`) with a new condition code.
    pub fn with_cc(&self, bytes: &[u8], jcc: &Jcc, cc: u8) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out[jcc.cc_offset] = (out[jcc.cc_offset] & 0xf0) | (cc & 0x0f);
        out
    }

    /// The shortest unconditional jump from `from` to `to` that fits in
    /// `room` bytes.
    pub fn encode_jmp(&self, from: u64, to: u64, room: usize) -> Option<Vec<u8>> {
        self.jmp.iter().find_map(|&(opcode, rel)| {
            let len = 1 + rel;
            if len > room {
                return None;
            }
            let disp = i128::from(to) - i128::from(from) - len as i128;
            let mut out = vec![opcode];
            match rel {
                1 => out.push(i8::try_from(disp).ok()? as u8),
                4 => out.extend_from_slice(&i32::try_from(disp).ok()?.to_le_bytes()),
                _ => return None,
            }
            Some(out)
        })
    }

    /// The condition code true when `lhs <token> rhs` holds.
    pub fn relational(&self, signedness: Signedness, token: Token) -> Option<u8> {
        self.relational.get(&(signedness, token)).copied()
    }

    /// The token for the same comparison with operands swapped.
    pub fn swapped(&self, token: Token) -> Option<Token> {
        self.swapped.get(&token).copied()
    }
}
