//! The shapes of arithmetic patches tier P has been shown to get right.
//!
//! A shape is what a patch is made of, as far as its correctness goes: the
//! kind of operation, the operand width, how the compiler checked it for
//! overflow, the swap, the encoding of the instruction and whether its
//! second operand is in memory. `isa/tierp-shapes.toml` lists the shapes
//! that passed the gate (`tests/tierp_gate`: every one compiled with the
//! pinned rustc, patched, run and compared with the compiled mutant) and
//! the ones that did not; tier P patches arithmetic of Rust code only for
//! the first.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cherenkov_mutation::token::Token;
use serde::Deserialize;

use crate::isa::IsaError;
use crate::stubs::StubShape;

/// How the overflow of an integer operation is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Check {
    /// `seto; jo`: the overflow flag, which a swapped operation sets itself.
    Overflow,
    /// `cmp; jb`, or `jb` on the carry flag after the operation.
    Carry,
    /// No check (wrapping arithmetic, Go).
    Unchecked,
}

impl Check {
    fn name(self) -> &'static str {
        match self {
            Check::Overflow => "of",
            Check::Carry => "carry",
            Check::Unchecked => "none",
        }
    }
}

/// What reads the flags of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Consumer {
    /// A conditional jump.
    Jump,
    /// A `setcc`, which makes the bool.
    Set,
}

/// What a comparison compares: integers (`cmp`) or floats (`ucomis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Domain {
    Int,
    Float,
}

/// The kind of operation and what it is checked by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Int(Check),
    Float,
    Bitwise,
    /// A comparison operator swapped by the condition code of the
    /// instruction that reads its flags.
    Compare(Consumer, Domain),
}

/// One shape of arithmetic patch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Shape {
    pub kind: Kind,
    /// Operand width in bits.
    pub width: u8,
    pub from: Token,
    pub with: Token,
    /// The source instruction's opcode (and ModRM reg field, where the
    /// encoding has one), in hex.
    pub encoding: String,
    /// The second operand is in memory.
    pub memory: bool,
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, check) = match self.kind {
            Kind::Int(c) => ("int", c.name()),
            Kind::Float => ("float", "-"),
            Kind::Bitwise => ("bitwise", "-"),
            Kind::Compare(consumer, domain) => {
                let consumer = match consumer {
                    Consumer::Jump => "jcc",
                    Consumer::Set => "setcc",
                };
                let domain = match domain {
                    Domain::Int => "int",
                    Domain::Float => "float",
                };
                return write!(
                    f,
                    "cmp {consumer} {domain} {}->{} {}",
                    self.from.name(),
                    self.with.name(),
                    self.encoding
                );
            }
        };
        write!(
            f,
            "{kind} w{} {check} {}->{} {} {}",
            self.width,
            self.from.name(),
            self.with.name(),
            self.encoding,
            if self.memory { "mem" } else { "reg" }
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableToml {
    rustc: String,
    panic_call: String,
    pass: Vec<String>,
    declined: Vec<String>,
    #[serde(default)]
    stubs: Vec<String>,
    #[serde(default)]
    stubs_declined: Vec<String>,
}

/// The committed table: the shapes that passed the gate and the ones that
/// did not, harvested with one rustc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The `rustc -vV` release and commit the table was harvested with.
    pub rustc: String,
    /// What a patched overflow check does with the panic it calls:
    /// `kept` (the original operation's panic function), or `retargeted`
    /// (the mutated operation's, where the binary has it).
    pub panic_call: String,
    pass: BTreeSet<String>,
    declined: BTreeSet<String>,
    /// The bytes of each function-value stub shape that passed.
    stubs: BTreeMap<String, Vec<u8>>,
    stubs_declined: BTreeSet<String>,
}

/// A stub's line in the table: its shape, `=`, its bytes in hex.
fn stub_line(shape: &str, bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{shape} = {}", hex.join(" "))
}

fn parse_stub(line: &str) -> Result<(String, Vec<u8>), IsaError> {
    let bad = || IsaError(format!("stub line {line:?} is not `shape = hex bytes`"));
    let (shape, hex) = line.split_once(" = ").ok_or_else(bad)?;
    let bytes = hex
        .split(' ')
        .map(|b| u8::from_str_radix(b, 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| bad())?;
    Ok((shape.to_string(), bytes))
}

impl Table {
    pub fn parse(src: &str) -> Result<Table, IsaError> {
        let t: TableToml = toml::from_str(src).map_err(|e| IsaError(e.to_string()))?;
        Ok(Table {
            rustc: t.rustc,
            panic_call: t.panic_call,
            pass: t.pass.into_iter().collect(),
            declined: t.declined.into_iter().collect(),
            stubs: t
                .stubs
                .iter()
                .map(|l| parse_stub(l))
                .collect::<Result<_, _>>()?,
            stubs_declined: t.stubs_declined.into_iter().collect(),
        })
    }

    /// A table of nothing but the stubs a gate harvested.
    pub fn harvested(stubs: BTreeMap<String, Vec<u8>>) -> Table {
        Table {
            rustc: String::new(),
            panic_call: "kept".into(),
            pass: BTreeSet::new(),
            declined: BTreeSet::new(),
            stubs,
            stubs_declined: BTreeSet::new(),
        }
    }

    /// The bytes that return the value of `shape`, when it passed the gate.
    pub fn stub(&self, shape: &StubShape) -> Option<&[u8]> {
        self.stubs.get(&shape.to_string()).map(Vec::as_slice)
    }

    /// The table as a file's text: sorted, one shape per line.
    pub fn render(
        rustc: &str,
        pass: &BTreeSet<String>,
        declined: &BTreeSet<String>,
        stubs: &BTreeMap<String, Vec<u8>>,
        stubs_declined: &BTreeSet<String>,
    ) -> String {
        let list = |name: &str, items: &BTreeSet<String>| {
            let mut s = format!("{name} = [\n");
            for i in items {
                s.push_str(&format!("    {i:?},\n"));
            }
            s.push_str("]\n");
            s
        };
        let lines: BTreeSet<String> = stubs.iter().map(|(s, b)| stub_line(s, b)).collect();
        format!(
            "# Written by `just tierp-harvest` (tests/tierp_gate): do not edit.\n\
             # The shapes of arithmetic patches that passed, and those that did not,\n\
             # against the mutants rustc compiles. Tier P patches arithmetic of Rust\n\
             # code for a shape in `pass` only.\n\
             rustc = {rustc:?}\n\
             # The panic of an overflow check keeps calling the original operation's\n\
             # function (\"attempt to add with overflow\" after an add -> sub patch):\n\
             # the test's verdict is the compiled mutant's, its message may differ.\n\
             panic_call = \"kept\"\n\n{}\n{}\n\
             # Function-value stubs: the bytes rustc makes of a trivial value, written\n\
             # over a function's entry, per result layout class (`src/stubs.rs`).\n\
             {}\n{}",
            list("pass", pass),
            list("declined", declined),
            list("stubs", &lines),
            list("stubs_declined", stubs_declined)
        )
    }

    pub fn admits(&self, shape: &Shape) -> bool {
        self.pass.contains(&shape.to_string())
    }

    /// Whether the table has decided `shape`, either way.
    pub fn knows(&self, shape: &Shape) -> bool {
        let key = shape.to_string();
        self.pass.contains(&key) || self.declined.contains(&key)
    }

    pub fn passed(&self) -> &BTreeSet<String> {
        &self.pass
    }

    pub fn declined(&self) -> &BTreeSet<String> {
        &self.declined
    }
}
