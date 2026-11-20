//! The line table with columns, read from a test binary's DWARF: GDB drops
//! columns, but a match guard shares its line with its pattern and body, so
//! only a column tells them apart. Slow: builds the fidelity fixture.

use std::path::{Path, PathBuf};

use cherenkov_exec_dap::lines::{Row, rows};
use cherenkov_exec_dap::toolchain::build_rust_test_binaries;

fn ops_binary() -> PathBuf {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../cherenkov-fidelity/fixtures/rust-ops");
    build_rust_test_binaries(Path::new("cargo"), &root)
        .unwrap()
        .into_iter()
        .find(|b| {
            let out = std::process::Command::new(b)
                .arg("--list")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).contains("test_classify")
        })
        .unwrap()
}

fn on(rows: &[Row], line: u32) -> Vec<&Row> {
    rows.iter()
        .filter(|r| r.file.ends_with("src/lib.rs") && r.line == line)
        .collect()
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_rows_carry_columns_that_separate_a_guard_from_its_body() {
    // `        Some(n) if n > 10 => 1,` : guard at column 20, body at 30.
    let all = rows(&ops_binary()).unwrap();
    let cols: Vec<u32> = on(&all, 91).iter().map(|r| r.column).collect();
    assert!(cols.contains(&20), "{cols:?}");
    assert!(cols.contains(&30), "{cols:?}");
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_rows_mark_statement_starts() {
    let all = rows(&ops_binary()).unwrap();
    assert!(
        on(&all, 99).iter().any(|r| r.is_stmt),
        "arm body `0 => 10` starts a statement"
    );
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_rows_are_only_of_code_the_binary_holds() {
    // The linker drops unused functions but leaves their line rows, at
    // addresses near zero: no code is there, and GDB places no line on them.
    use object::{Object, ObjectSection, SectionKind};
    let binary = ops_binary();
    let data = std::fs::read(&binary).unwrap();
    let file = object::File::parse(&*data).unwrap();
    let code: Vec<(u64, u64)> = file
        .sections()
        .filter(|s| s.kind() == SectionKind::Text)
        .map(|s| (s.address(), s.address() + s.size()))
        .collect();
    let all = rows(&binary).unwrap();
    assert!(!all.is_empty());
    let outside: Vec<&Row> = all
        .iter()
        .filter(|r| !code.iter().any(|&(lo, hi)| r.addr >= lo && r.addr < hi))
        .collect();
    assert!(
        outside.is_empty(),
        "{} rows outside code, e.g. {:?}",
        outside.len(),
        outside.first()
    );
}
