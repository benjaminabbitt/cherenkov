//! The function-value half of the gate: each function and the mutant that
//! replaces its body with a value are compiled with the pinned rustc, the
//! real tier P patch is derived from the original's code and applied, and
//! the patched original must do what the compiled mutant does.

use std::collections::BTreeMap;
use std::path::Path;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::layouts::{Layout, Layouts};
use cherenkov_exec_dap::shapes::Table;
use cherenkov_exec_dap::stubs::{StubShape, shape_of};
use cherenkov_exec_dap::tierp::{Patch, derive_returning};

use crate::compile::{self, Unit};
use crate::exe::Exe;
use crate::run;
use crate::value_driver;
use crate::values::ValueCase;
use crate::{Scratch, Verdict, judge, write_patched};

/// What the gate found for the function-value cases.
pub struct ValueFindings {
    pub verdicts: BTreeMap<usize, Verdict>,
    /// The shape of each case that has one.
    pub shapes: BTreeMap<usize, StubShape>,
    /// The bytes rustc makes of each shape, where every sample agrees and
    /// they can be a stub.
    pub harvested: BTreeMap<String, Vec<u8>>,
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

/// The bytes of each case's value alone.
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

/// The layout of the result of each case's function, from the original's
/// DWARF, as the executor reads it.
fn layouts_of(original: &Path, exe: &Exe, cases: &[ValueCase]) -> BTreeMap<usize, Layout> {
    let layouts = Layouts::read(original).expect("the DWARF of the gate executable");
    cases
        .iter()
        .filter_map(|c| {
            let function = exe.function(&format!("s_{}.rs", c.id))?;
            let entry = function.insns.iter().map(|i| i.pc).min()?;
            Some((c.id, layouts.result_of(entry)?))
        })
        .collect()
}

/// The shape of each case, and the bytes of each shape whose samples all
/// gave the same ones.
fn stubs_of(
    cases: &[ValueCase],
    layouts: &BTreeMap<usize, Layout>,
    harvested: &BTreeMap<usize, Result<Vec<u8>, String>>,
) -> (BTreeMap<usize, StubShape>, BTreeMap<String, Vec<u8>>) {
    let mut shapes = BTreeMap::new();
    let mut samples: BTreeMap<String, Vec<&Result<Vec<u8>, String>>> = BTreeMap::new();
    for c in cases {
        let Some(shape) = layouts.get(&c.id).and_then(|l| shape_of(l, &c.value)) else {
            continue;
        };
        samples
            .entry(shape.to_string())
            .or_default()
            .push(&harvested[&c.id]);
        shapes.insert(c.id, shape);
    }
    let agreed = samples.into_iter().filter_map(|(shape, got)| {
        let first = got.first()?.as_ref().ok()?;
        got.iter()
            .all(|g| g.as_ref().ok() == Some(first))
            .then(|| (shape, first.clone()))
    });
    (shapes, agreed.collect())
}

/// The patch the executor would derive for `case` from the original's code.
fn derived(
    exe: &Exe,
    case: &ValueCase,
    layout: Option<&Layout>,
    isa: &Isa,
) -> Result<Patch, String> {
    let function = exe
        .function(&format!("s_{}.rs", case.id))
        .ok_or("no code in the line table")?;
    derive_returning(&case.plan, &function.file, &function.insns, isa, layout)
        .map_err(|e| e.to_string())
}

fn patch_all(
    exe: &mut Exe,
    cases: &[ValueCase],
    layouts: &BTreeMap<usize, Layout>,
    isa: &Isa,
) -> (BTreeMap<usize, Verdict>, Vec<usize>) {
    let mut verdicts = BTreeMap::new();
    let mut patched = Vec::new();
    for case in cases {
        match derived(exe, case, layouts.get(&case.id), isa) {
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
    gate_with(cases, isa, &|_, bytes| bytes)
}

/// [`gate`], with each shape's harvested bytes passed through `corrupt`
/// before the stubs are patched in: the gate must then say so.
pub fn gate_with(
    cases: &[ValueCase],
    isa: &Isa,
    corrupt: &dyn Fn(&str, Vec<u8>) -> Vec<u8>,
) -> ValueFindings {
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
    let sampled = harvest(&patched_exe, cases);
    let layouts = layouts_of(&built.original, &patched_exe, cases);
    let (shapes, harvested) = stubs_of(cases, &layouts, &sampled);
    let harvested: BTreeMap<String, Vec<u8>> = harvested
        .into_iter()
        .map(|(shape, bytes)| {
            let bytes = corrupt(&shape, bytes);
            (shape, bytes)
        })
        .collect();
    let isa = isa.with_shapes(Table::harvested(harvested.clone()));
    let (mut verdicts, patched_ids) = patch_all(&mut patched_exe, cases, &layouts, &isa);
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
        shapes,
        harvested,
    }
}
