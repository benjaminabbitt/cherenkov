//! The function-value half of the gate: each function and the mutant that
//! replaces its body with a value are compiled with the pinned rustc, the
//! real tier P patch is derived from the original's code and applied, and
//! the patched original must do what the compiled mutant does.

use std::collections::BTreeMap;
use std::path::Path;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::tierp::{Patch, derive_shaped};

use crate::compile::{self, Unit};
use crate::exe::Exe;
use crate::run;
use crate::value_driver;
use crate::values::ValueCase;
use crate::{Scratch, Verdict, judge, write_patched};

/// What the gate found for the function-value cases.
pub struct ValueFindings {
    pub verdicts: BTreeMap<usize, Verdict>,
    /// The bytes rustc makes of each value alone, where they can be a stub.
    pub harvested: BTreeMap<usize, Result<Vec<u8>, String>>,
}

fn units(cases: &[ValueCase]) -> Vec<Unit> {
    let mut out = Vec::new();
    for c in cases {
        out.push(Unit {
            name: format!("s_{}", c.id),
            original: c.original.clone(),
            mutated: c.mutated.clone(),
        });
        out.push(Unit {
            name: format!("h_{}", c.id),
            original: c.bare.clone(),
            mutated: c.bare.clone(),
        });
    }
    out
}

fn harvest(exe: &Exe, cases: &[ValueCase]) -> BTreeMap<usize, Result<Vec<u8>, String>> {
    cases
        .iter()
        .map(|c| {
            let bytes = exe
                .function(&format!("h_{}.rs", c.id))
                .ok_or_else(|| "no code in the line table".to_string())
                .and_then(|f| crate::harvest::stub_bytes(&f));
            (c.id, bytes)
        })
        .collect()
}

/// The patch the executor would derive for `case` from the original's code.
fn derived(exe: &Exe, case: &ValueCase, isa: &Isa) -> Result<Patch, String> {
    let function = exe
        .function(&format!("s_{}.rs", case.id))
        .ok_or("no code in the line table")?;
    derive_shaped(&case.plan, &function.file, &function.insns, isa)
        .map(|(patch, _)| patch)
        .map_err(|e| e.to_string())
}

fn patch_all(
    exe: &mut Exe,
    cases: &[ValueCase],
    isa: &Isa,
) -> (BTreeMap<usize, Verdict>, Vec<usize>) {
    let mut verdicts = BTreeMap::new();
    let mut patched = Vec::new();
    for case in cases {
        match derived(exe, case, isa) {
            Err(why) => {
                verdicts.insert(case.id, Verdict::Declined(why));
            }
            Ok(patch) if exe.apply(&patch) => patched.push(case.id),
            Ok(_) => {
                verdicts.insert(case.id, Verdict::NotApplied);
            }
        }
    }
    (verdicts, patched)
}

fn explain(mutant: &Path, patched: &Path, id: usize) {
    let (a, b) = (run::dump(mutant, id), run::dump(patched, id));
    for (m, p) in a.iter().zip(&b).filter(|(m, p)| m != p).take(3) {
        eprintln!("  case {id}: mutant `{m}`, patched `{p}`");
    }
}

pub fn gate(cases: &[ValueCase], isa: &Isa) -> ValueFindings {
    let work = Scratch::new("values");
    let main = value_driver::main_source(cases);
    let units = units(cases);
    let krate = compile::Crate {
        support: value_driver::SUPPORT,
        main: &main,
        units: &units,
    };
    let built = compile::build(&work.0, &krate);
    let mut patched_exe = Exe::read(&built.original);
    let harvested = harvest(&patched_exe, cases);
    let (mut verdicts, patched_ids) = patch_all(&mut patched_exe, cases, isa);
    let patched_path = work.0.join("patched.exe");
    write_patched(&built.original, &patched_exe, &patched_path);
    let all: Vec<usize> = cases.iter().map(|c| c.id).collect();
    let orig = run::sweep(&built.original, &all);
    let mutant = run::sweep(&built.mutant, &all);
    let patched = run::sweep(&patched_path, &patched_ids);
    for &id in &patched_ids {
        let v = judge(orig[&id], mutant[&id], patched[&id]);
        if v == Verdict::Mismatch {
            explain(&built.mutant, &patched_path, id);
        }
        verdicts.insert(id, v);
    }
    ValueFindings {
        verdicts,
        harvested,
    }
}
