//! The tier P gate: every arithmetic or bitwise operator replacement the
//! planner makes in a snippet, for every operand form, is compiled twice
//! with the pinned rustc (the original and the mutated source), the real
//! tier P patch derived from the original's line table is applied to the
//! original's code, and both are run natively over edge values and seeded
//! random inputs (all values, for 8-bit types): the patched and the
//! compiled mutant must give the same result, and panic on the same inputs.

mod cases;
mod compile;
mod conds;
mod driver;
mod exe;
mod run;
mod snippets;
mod table;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cases::Case;
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::shapes::Shape;
use cherenkov_exec_dap::tierp::{Patch, Unpatchable, derive_shaped};
use exe::Exe;
use run::Outcome;
use snippets::Form;

/// What the gate found for one case.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Declined(String),
    /// The original and the mutant gave the same results: no evidence.
    Vacuous,
    Passed,
    Mismatch,
    Crashed,
    NotApplied,
}

impl Verdict {
    fn wrong(&self) -> bool {
        matches!(
            self,
            Verdict::Mismatch | Verdict::Crashed | Verdict::NotApplied
        )
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let dir = std::env::temp_dir().join(format!("cherenkov-tierp-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The patch the executor would derive for `case` from the original's code.
fn derived(exe: &Exe, case: &Case, isa: &Isa) -> Result<(Patch, Option<Shape>), String> {
    let module = format!("s_{}.rs", case.id);
    let function = exe.function(&module).ok_or("no code in the line table")?;
    derive_shaped(&case.plan, &function.file, &function.insns, isa)
        .map_err(|e: Unpatchable| e.to_string())
}

fn write_patched(original: &Path, exe: &Exe, to: &Path) {
    std::fs::write(to, &exe.bytes).expect("write the patched executable");
    let perms = std::fs::metadata(original).expect("metadata").permissions();
    std::fs::set_permissions(to, perms).expect("permissions");
}

fn judge(orig: Outcome, mutant: Outcome, patched: Outcome) -> Verdict {
    if matches!(patched, Outcome::Crashed) {
        return Verdict::Crashed;
    }
    if patched != mutant {
        return Verdict::Mismatch;
    }
    if orig == mutant {
        Verdict::Vacuous
    } else {
        Verdict::Passed
    }
}

/// What the gate found, per case and per shape.
struct Findings {
    verdicts: BTreeMap<usize, Verdict>,
    shapes: BTreeMap<usize, Shape>,
}

/// `TIERP_GATE_DUMP=3,7`: those cases' source and original code, then stop.
fn dump_requested(cases: &[Case], exe: &Exe) {
    let Ok(ids) = std::env::var("TIERP_GATE_DUMP") else {
        return;
    };
    // Each item is a case number or text of "label type mutation".
    for item in ids.split(',') {
        let named = |c: &&Case| {
            let name = format!(
                "{} {} {}",
                c.snippet.label,
                c.snippet.ty,
                c.plan.mutation.unwrap_or("?")
            );
            item.parse::<usize>()
                .map_or(name.contains(item), |n| n == c.id)
        };
        let Some(c) = cases.iter().find(named) else {
            continue;
        };
        let id = c.id;
        let code = exe.function(&format!("s_{id}.rs")).map(|f| f.listing());
        eprintln!(
            "== case {id} {} {}\n{}{}",
            c.plan.mutation.unwrap_or("?"),
            c.plan.operator,
            c.snippet.source,
            code.unwrap_or_default()
        );
    }
    std::process::exit(0);
}

fn gate(cases: &[Case]) -> Findings {
    let work = Scratch::new();
    let started = std::time::Instant::now();
    let built = compile::build(&work.0, cases);
    eprintln!("compiled both executables in {:?}", started.elapsed());
    let isa = Isa::amd64_unfiltered();
    let mut patched_exe = Exe::read(&built.original);
    let mut found = Findings {
        verdicts: BTreeMap::new(),
        shapes: BTreeMap::new(),
    };
    let mut patched_ids = Vec::new();
    let derive_started = std::time::Instant::now();
    dump_requested(cases, &patched_exe);
    for case in cases {
        match derived(&patched_exe, case, &isa) {
            Err(why) => {
                found.verdicts.insert(case.id, Verdict::Declined(why));
            }
            Ok((patch, shape)) => {
                found.shapes.extend(shape.map(|s| (case.id, s)));
                if patched_exe.apply(&patch) {
                    patched_ids.push(case.id);
                } else {
                    found.verdicts.insert(case.id, Verdict::NotApplied);
                }
            }
        }
    }
    eprintln!(
        "derived and applied the patches in {:?}",
        derive_started.elapsed()
    );
    let patched_path = work.0.join("patched.exe");
    write_patched(&built.original, &patched_exe, &patched_path);
    let all: Vec<usize> = cases.iter().map(|c| c.id).collect();
    let started = std::time::Instant::now();
    let orig = run::sweep(&built.original, &all);
    let mutant = run::sweep(&built.mutant, &all);
    let patched = run::sweep(&patched_path, &patched_ids);
    eprintln!("ran the three executables in {:?}", started.elapsed());
    for &id in &patched_ids {
        let v = judge(orig[&id], mutant[&id], patched[&id]);
        if v == Verdict::Mismatch {
            explain(&built.mutant, &patched_path, id);
        }
        found.verdicts.insert(id, v);
    }
    found
}

/// The first inputs on which the patched code and the mutant differ.
fn explain(mutant: &Path, patched: &Path, id: usize) {
    let (a, b) = (run::dump(mutant, id), run::dump(patched, id));
    for (m, p) in a.iter().zip(&b).filter(|(m, p)| m != p).take(3) {
        eprintln!("  case {id}: mutant `{m}`, patched `{p}`");
    }
}

fn report(cases: &[Case], found: &Findings) -> BTreeMap<String, usize> {
    let mut summary: BTreeMap<String, usize> = BTreeMap::new();
    for c in cases {
        let v = &found.verdicts[&c.id];
        let kind = if c.snippet.form == Form::Cond {
            "cond"
        } else {
            "arith"
        };
        let outcome = match v {
            Verdict::Declined(_) => "declined".to_string(),
            other => format!("{other:?}"),
        };
        *summary.entry(format!("{kind} {outcome}")).or_default() += 1;
        if let Verdict::Declined(why) = v {
            eprintln!(
                "declined {kind} {} {} {} #{}: {why}",
                c.snippet.label,
                c.snippet.ty,
                c.plan.mutation.unwrap_or("?"),
                c.id
            );
        }
        if v.wrong() {
            eprintln!(
                "{v:?} case {} {} {} {} {:?}",
                c.id,
                c.snippet.ty,
                c.snippet.label,
                c.plan.mutation.unwrap_or("?"),
                c.snippet.source.lines().nth(2).unwrap_or("")
            );
        }
    }
    summary
}

fn evidence(found: &Findings) -> table::Evidence {
    let mut by_shape: table::Evidence = BTreeMap::new();
    for (id, shape) in &found.shapes {
        by_shape
            .entry(shape.clone())
            .or_default()
            .push(found.verdicts[id].clone());
    }
    by_shape
}

/// Every snippet and operator replacement is run: `TIERP_HARVEST=write`
/// writes the shapes table (`just tierp-harvest`), otherwise the table the
/// gate finds must be the committed one.
#[test]
fn test_tier_p_patches_behave_like_the_rustc_mutants_they_stand_for() {
    let cases = cases::all();
    // `TIERP_GATE_ONLY=cond` (or `arith`): a faster look at one family; the
    // table is not compared then.
    let only = std::env::var("TIERP_GATE_ONLY").ok();
    let cases: Vec<Case> = match only.as_deref() {
        Some(family) => cases
            .into_iter()
            .filter(|c| (c.snippet.form == Form::Cond) == (family == "cond"))
            .collect(),
        None => cases,
    };
    let started = std::time::Instant::now();
    let found = gate(&cases);
    let summary = report(&cases, &found);
    let by_shape = evidence(&found);
    eprintln!(
        "{} cases, {summary:?}, {} shapes seen, {:?}",
        cases.len(),
        by_shape.len(),
        started.elapsed()
    );
    assert!(
        found.verdicts.values().all(|v| !v.wrong()),
        "patches that do not behave like their mutants"
    );
    if only.is_some() {
        return;
    }
    assert_eq!(table::unlisted(&by_shape), Vec::<String>::new());
    let text = table::render(&compile::rustc_identity(), &by_shape);
    if std::env::var("TIERP_HARVEST").is_ok_and(|v| v == "write") {
        std::fs::write(table::path(), text).expect("write the shapes table");
    } else {
        let committed = std::fs::read_to_string(table::path()).expect("the shapes table");
        assert!(
            committed == text,
            "isa/tierp-shapes.toml is not what the gate finds: run `just tierp-harvest`"
        );
    }
}
