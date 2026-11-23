//! Reading an executable the way the executor does: the line table's rows
//! (`cherenkov_exec_dap::lines::rows`), a function's bytes decoded into
//! instructions that carry the row's line and column, and the bytes
//! patched in a copy of the file.

use std::path::Path;

use cherenkov_exec_dap::lines::{self, Row};
use cherenkov_exec_dap::tierp::{Insn, Patch};
use iced_x86::{Decoder, DecoderOptions};
use object::{Object, ObjectSection};

/// An executable's bytes and the line table.
pub struct Exe {
    pub bytes: Vec<u8>,
    rows: Vec<Row>,
    /// The indices of the rows of each source file, by its last path part.
    by_module: std::collections::HashMap<String, Vec<usize>>,
    /// (address, size, file offset) of each section.
    sections: Vec<(u64, u64, usize)>,
}

/// A function's instructions, with the file the line table names.
pub struct Function {
    pub file: String,
    pub insns: Vec<Insn>,
}

fn is_module(file: &str, module: &str) -> bool {
    file.rsplit('/').next() == Some(module)
}

impl Function {
    /// The instructions with their line:column, for `TIERP_GATE_DUMP`.
    pub fn listing(&self) -> String {
        let mut out = String::new();
        for i in &self.insns {
            let insn = Decoder::with_ip(64, &i.bytes, i.pc, DecoderOptions::NONE).decode();
            let hex: String = i.bytes.iter().map(|b| format!("{b:02x}")).collect();
            out.push_str(&format!(
                "{:x} {}:{:<3} {:?} {hex}\n",
                i.pc,
                i.line,
                i.column,
                insn.mnemonic()
            ));
        }
        out
    }
}

impl Exe {
    pub fn read(path: &Path) -> Exe {
        let bytes = std::fs::read(path).expect("read the executable");
        let sections = object::File::parse(&*bytes)
            .expect("an object file")
            .sections()
            .filter_map(|s| Some((s.address(), s.size(), s.file_range()?.0 as usize)))
            .collect();
        let rows = lines::rows(path).expect("a line table");
        let mut by_module: std::collections::HashMap<String, Vec<usize>> = Default::default();
        for (k, row) in rows.iter().enumerate() {
            let name = row.file.rsplit('/').next().unwrap_or_default();
            by_module.entry(name.to_string()).or_default().push(k);
        }
        Exe {
            bytes,
            rows,
            by_module,
            sections,
        }
    }

    /// The file offset of the virtual address `addr`.
    fn offset(&self, addr: u64) -> Option<usize> {
        self.sections
            .iter()
            .find(|&&(from, size, _)| addr >= from && addr < from + size)
            .map(|&(from, _, start)| start + (addr - from) as usize)
    }

    /// The code of module `module` (`s_3.rs`): from its first row to the
    /// row that follows its last.
    pub fn function(&self, module: &str) -> Option<Function> {
        let mine = self.by_module.get(module)?;
        let (first, last) = (*mine.first()?, *mine.last()?);
        let (start, end) = (self.rows[first].addr, self.rows.get(last + 1)?.addr);
        let at = self.offset(start)?;
        let code = self.bytes.get(at..at + (end - start) as usize)?;
        let mut decoder = Decoder::with_ip(64, code, start, DecoderOptions::NONE);
        let mut insns = Vec::new();
        while decoder.can_decode() {
            let from = decoder.position();
            let insn = decoder.decode();
            let row = &self.rows[self.rows.partition_point(|r| r.addr <= insn.ip()) - 1];
            if is_module(&row.file, module) {
                insns.push(Insn {
                    pc: insn.ip(),
                    bytes: code[from..from + insn.len()].to_vec(),
                    file: row.file.clone(),
                    line: row.line,
                    column: row.column,
                });
            }
        }
        let file = self.rows[first].file.clone();
        Some(Function { file, insns })
    }

    /// Write `patch`'s replacements over the originals; false when the
    /// bytes there are not the ones the patch expects.
    pub fn apply(&mut self, patch: &Patch) -> bool {
        for r in &patch.regions {
            let Some(at) = self.offset(r.addr) else {
                return false;
            };
            if self.bytes[at..at + r.original.len()] != r.original[..] {
                return false;
            }
            self.bytes[at..at + r.replacement.len()].copy_from_slice(&r.replacement);
        }
        true
    }
}
