//! The test binary's DWARF line table, with the columns and statement flags
//! GDB does not keep: a match guard shares its line with its pattern and
//! body, so only the column tells which instructions are the guard's.

use std::borrow::Cow;
use std::path::Path;

use crate::error::Toolchain;
use cherenkov_run_ports::executor::ExecError;
use gimli::{EndianSlice, RunTimeEndian};
use object::{Object, ObjectSection};

/// One row of the line table: the code from `addr` up to the next row's
/// address comes from `file:line:column`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub addr: u64,
    pub file: String,
    pub line: u32,
    /// 1-based; 0 when the row has no column.
    pub column: u32,
    /// The row begins a statement (where a line breakpoint would stop).
    pub is_stmt: bool,
}

/// Every row of every compilation unit whose code the binary holds,
/// sorted by address. The linker drops unused functions but leaves their
/// rows, at addresses near zero where no code is: those are left out.
pub fn rows(binary: &Path) -> Result<Vec<Row>, ExecError> {
    let err = |what: &str, e: &dyn std::fmt::Display| {
        ExecError::of(&Toolchain::Binary {
            binary: binary.display().to_string(),
            step: what.to_string(),
            detail: e.to_string(),
        })
    };
    let data = std::fs::read(binary).map_err(|e| err("read", &e))?;
    let file = object::File::parse(&*data).map_err(|e| err("parse", &e))?;
    let endian = if file.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    let section = |id: gimli::SectionId| -> Result<Cow<'_, [u8]>, gimli::Error> {
        Ok(file
            .section_by_name(id.name())
            .and_then(|s| s.uncompressed_data().ok())
            .unwrap_or(Cow::Borrowed(&[])))
    };
    let code: Vec<(u64, u64)> = file
        .sections()
        .filter(|s| s.kind() == object::SectionKind::Text)
        .map(|s| (s.address(), s.address() + s.size()))
        .collect();
    let in_code = |addr: u64| code.iter().any(|&(lo, hi)| addr >= lo && addr < hi);
    let owned = gimli::DwarfSections::load(section).map_err(|e| err("dwarf", &e))?;
    let dwarf = owned.borrow(|s| EndianSlice::new(s, endian));
    let mut out = Vec::new();
    let mut units = dwarf.units();
    while let Some(header) = units.next().map_err(|e| err("unit", &e))? {
        let unit = dwarf.unit(header).map_err(|e| err("unit", &e))?;
        let Some(program) = unit.line_program.clone() else {
            continue;
        };
        let mut lines = program.rows();
        while let Some((header, row)) = lines.next_row().map_err(|e| err("line row", &e))? {
            if row.end_sequence() || !in_code(row.address()) {
                continue;
            }
            let Some(entry) = row.file(header) else {
                continue;
            };
            let name = dwarf
                .attr_string(&unit, entry.path_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let dir = entry
                .directory(header)
                .and_then(|d| dwarf.attr_string(&unit, d).ok())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let path = if name.starts_with('/') || dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            out.push(Row {
                addr: row.address(),
                file: path,
                line: row.line().map_or(0, |l| l.get() as u32),
                column: match row.column() {
                    gimli::ColumnType::LeftEdge => 0,
                    gimli::ColumnType::Column(c) => c.get() as u32,
                },
                is_stmt: row.is_stmt(),
            });
        }
    }
    out.sort_by_key(|r| r.addr);
    Ok(out)
}

/// Whether the binary carries a DWARF line table at all (`.debug_line`,
/// compressed or not), whatever its producer: a test binary built without
/// debug info has none.
pub fn has_line_table(binary: &Path) -> bool {
    let Ok(data) = std::fs::read(binary) else {
        return false;
    };
    let Ok(file) = object::File::parse(&*data) else {
        return false;
    };
    [".debug_line", ".zdebug_line"]
        .iter()
        .any(|name| file.section_by_name(name).is_some_and(|s| s.size() > 0))
}
