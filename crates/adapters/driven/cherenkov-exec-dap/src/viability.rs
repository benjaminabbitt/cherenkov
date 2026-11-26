//! Whether a mutant's source compiles, decided by rustc and never by a table
//! of types: the replacement is applied to a scratch copy of the workspace
//! and `cargo check` says. Many mutants share one compile: those that do not
//! overlap are applied together, rustc's errors are attributed to the mutant
//! whose text they lie in, the blamed are set aside and the rest checked
//! again, and an error that is nobody's bisects the batch down to its cause.
//! Only a compile that fails is believed once the unmutated crate is known
//! to pass. cargo-mutants' `--check` decides the same way (a mutant whose
//! `cargo check` fails is unviable); this is written from its documented
//! behaviour, not from its code.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Toolchain;
use crate::reason::Reason;
use cherenkov_mutation::execution::{Budget, Observed, RawOutcome};
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_ports::executor::{Driven, ExecError, MutantExecutor, Recording};
use coded_diagnostic::Report;
use serde_json::Value;

/// A mutant to check: its text replaces `start..end` of `file` (relative to
/// the project's root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: MutantId,
    pub file: PathBuf,
    pub start: usize,
    pub end: usize,
    pub with: String,
}

/// The plans to check: those that replace a function's body and whose
/// original text is still where the plan found it in the file under `root`.
pub fn candidates(root: &Path, plans: &[MutantPlan]) -> Vec<Candidate> {
    let mut sources: HashMap<&Path, Option<String>> = HashMap::new();
    let mut found = Vec::new();
    for plan in plans.iter().filter(|p| p.operator == "fn-value") {
        let Some(with) = &plan.mutated else { continue };
        let source = sources
            .entry(plan.file.as_path())
            .or_insert_with(|| std::fs::read_to_string(root.join(&plan.file)).ok());
        let (start, end) = (plan.site.start.byte, plan.site.end.byte);
        if source.as_deref().and_then(|s| s.get(start..end)) == Some(plan.original.as_str()) {
            found.push(Candidate {
                id: plan.id.clone(),
                file: plan.file.clone(),
                start,
                end,
                with: with.clone(),
            });
        }
    }
    found
}

/// `source` with each edit `(start, end, text)` applied, the edits sorted
/// and not overlapping; and where each edit's text lies in the result.
pub fn apply_edits(source: &str, edits: &[(usize, usize, &str)]) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(source.len());
    let mut at = Vec::with_capacity(edits.len());
    let mut done = 0;
    for &(start, end, with) in edits {
        text.push_str(&source[done..start]);
        at.push(text.len()..text.len() + with.len());
        text.push_str(with);
        done = end;
    }
    text.push_str(&source[done..]);
    (text, at)
}

/// The candidates (by index) in as few batches as hold them with none
/// overlapping another in its batch.
pub fn batches(candidates: &[Candidate]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by_key(|&i| (&candidates[i].file, candidates[i].start));
    // Each batch: the members and, per file, where the last one ends.
    let mut all: Vec<(Vec<usize>, HashMap<&Path, usize>)> = Vec::new();
    for i in order {
        let c = &candidates[i];
        let free = |ends: &HashMap<&Path, usize>| {
            ends.get(c.file.as_path()).is_none_or(|&end| end <= c.start)
        };
        let at = all.iter().position(|(_, ends)| free(ends));
        let (members, ends) = match at {
            Some(at) => &mut all[at],
            None => {
                all.push((Vec::new(), HashMap::new()));
                all.last_mut().expect("just pushed")
            }
        };
        members.push(i);
        ends.insert(c.file.as_path(), c.end);
    }
    all.into_iter()
        .map(|(mut members, _)| {
            members.sort_unstable();
            members
        })
        .collect()
}

/// One error of rustc: its words and where it points (the primary span and
/// the call sites of the macros that made it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub message: String,
    pub spans: Vec<(PathBuf, Range<usize>)>,
}

/// The errors in `cargo check --message-format=json`'s output.
pub fn diagnostics(stdout: &str) -> Vec<Diag> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-message" && m["message"]["level"] == "error")
        .filter_map(|m| diag_of(&m["message"]))
        .collect()
}

fn diag_of(message: &Value) -> Option<Diag> {
    let primary = message["spans"]
        .as_array()?
        .iter()
        .find(|s| s["is_primary"] == true)?;
    let code = message["code"]["code"]
        .as_str()
        .map(|c| format!("[{c}]"))
        .unwrap_or_default();
    Some(Diag {
        message: format!("error{code}: {}", message["message"].as_str()?),
        spans: span_chain(primary),
    })
}

/// A span, then the call site that expanded it, and so on outwards.
fn span_chain(span: &Value) -> Vec<(PathBuf, Range<usize>)> {
    let mut chain = Vec::new();
    let mut at = Some(span);
    while let Some(s) = at {
        let place = (
            s["file_name"].as_str(),
            s["byte_start"].as_u64(),
            s["byte_end"].as_u64(),
        );
        if let (Some(file), Some(start), Some(end)) = place {
            chain.push((PathBuf::from(file), start as usize..end as usize));
        }
        at = Some(&s["expansion"]["span"]).filter(|s| s.is_object());
    }
    chain
}

/// Whose errors a compile reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blamed {
    /// The mutants (by index) an error lies inside the text of, and the
    /// first error of each.
    pub by_mutant: Vec<(usize, String)>,
    /// The first error that lies inside no mutant's text.
    pub elsewhere: Option<String>,
}

impl Blamed {
    fn is_clean(&self) -> bool {
        self.by_mutant.is_empty() && self.elsewhere.is_none()
    }
}

/// Each error assigned to the mutant whose text (`regions`, per mutant, in
/// the file as rustc names it) holds one of the places it points at whole:
/// an error that reaches outside a mutant's text is nobody's.
pub fn blame(diags: &[Diag], regions: &[(PathBuf, Range<usize>)]) -> Blamed {
    let mut blamed = Blamed::default();
    for diag in diags {
        let owner = regions.iter().position(|(file, text)| {
            diag.spans
                .iter()
                .any(|(f, s)| f == file && text.start <= s.start && s.end <= text.end)
        });
        match owner {
            Some(i) if !blamed.by_mutant.iter().any(|(m, _)| *m == i) => {
                blamed.by_mutant.push((i, diag.message.clone()));
            }
            Some(_) => {}
            None => {
                blamed.elsewhere.get_or_insert_with(|| diag.message.clone());
            }
        }
    }
    blamed
}

/// Which of `n` mutants do not compile, and why, by `check`ing subsets: a
/// subset's blamed mutants are set aside and the rest checked again; errors
/// that are nobody's split the subset, and a mutant that breaks the crate
/// alone is rejected whatever the error says.
pub fn settle(
    n: usize,
    check: &mut dyn FnMut(&[usize]) -> Result<Blamed, ExecError>,
) -> Result<Vec<(usize, String)>, ExecError> {
    let mut rejected = Vec::new();
    let mut work = if n == 0 {
        vec![]
    } else {
        vec![(0..n).collect::<Vec<_>>()]
    };
    while let Some(subset) = work.pop() {
        settle_one(subset, check, &mut rejected, &mut work)?;
    }
    rejected.sort();
    Ok(rejected)
}

fn settle_one(
    mut subset: Vec<usize>,
    check: &mut dyn FnMut(&[usize]) -> Result<Blamed, ExecError>,
    rejected: &mut Vec<(usize, String)>,
    work: &mut Vec<Vec<usize>>,
) -> Result<(), ExecError> {
    while !subset.is_empty() {
        let blamed = check(&subset)?;
        if blamed.is_clean() {
            return Ok(());
        }
        if blamed.by_mutant.is_empty() {
            let why = blamed.elsewhere.unwrap_or_default();
            if let [only] = subset[..] {
                rejected.push((only, why));
            } else {
                let second = subset.split_off(subset.len() / 2);
                work.push(second);
                work.push(subset);
            }
            return Ok(());
        }
        subset.retain(|i| !blamed.by_mutant.iter().any(|(m, _)| m == i));
        rejected.extend(blamed.by_mutant);
    }
    Ok(())
}

/// What a check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    /// The mutants rustc rejects, with the first error it gives.
    pub rejected: HashMap<MutantId, String>,
    /// How many `cargo check` runs it took.
    pub compiles: usize,
    pub took: Duration,
}

/// Where the scratch copy of a workspace and its build live: in its target
/// directory, so that a second run starts from the first's dependencies.
fn scratch_dir(workspace: &Path) -> PathBuf {
    scratch_dir_in(
        std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from),
        workspace,
    )
}

/// [`scratch_dir`] for a given `CARGO_TARGET_DIR`. A target directory the
/// caller shares between projects is shared by their checks too, so each
/// workspace's scratch copy is named for the workspace: a copy is only
/// brought up to date, never emptied, and another project's files left in
/// it would be compiled with this one's.
pub fn scratch_dir_in(shared_target: Option<PathBuf>, workspace: &Path) -> PathBuf {
    match shared_target {
        Some(target) => target
            .join("cherenkov-viability")
            .join(workspace.to_string_lossy().replace(['/', '\\', ':'], "%")),
        None => workspace.join("target").join("cherenkov-viability"),
    }
}

/// Copy what differs of `from` into `to`, leaving out `.git` and `skip`.
fn sync_tree(from: &Path, to: &Path, skip: &[PathBuf]) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" || skip.contains(&path) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue; // a dangling link
        };
        let dest = to.join(entry.file_name());
        if meta.is_dir() {
            sync_tree(&path, &dest, skip)?;
        } else if std::fs::read(&dest).ok() != std::fs::read(&path).ok() {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

/// Mutants written into the scratch workspace.
struct Applied {
    /// Each mutant's text, as rustc will name its place.
    regions: Vec<(PathBuf, Range<usize>)>,
    /// The files written (relative to the project's root) and what to put
    /// back in them.
    written: Vec<(PathBuf, String)>,
}

/// Where the check is, said before each `cargo check` it runs: the run's
/// number (the baseline's included) and the batch of candidates it serves.
/// How many runs there will be is not known in advance (a failing batch is
/// bisected); the batches are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub compile: usize,
    pub batch: usize,
    pub batches: usize,
}

/// A scratch workspace to compile mutants in.
struct Rustc<'a> {
    cargo: &'a Path,
    /// The project's own root, whose files the mutants edit.
    root: &'a Path,
    tree: PathBuf,
    /// The project's root as the workspace's root sees it.
    below: PathBuf,
    target: PathBuf,
    args: &'a [String],
    baseline_passes: bool,
    compiles: usize,
    /// The batch being settled, from one, and how many there are.
    batch: (usize, usize),
    progress: &'a mut dyn FnMut(Progress),
}

impl Rustc<'_> {
    fn cargo_check(&mut self) -> Result<(bool, String, String), ExecError> {
        self.compiles += 1;
        (self.progress)(Progress {
            compile: self.compiles,
            batch: self.batch.0,
            batches: self.batch.1,
        });
        let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
        let out = Command::new(self.cargo)
            .args([
                "check",
                "--all-targets",
                "--offline",
                "--message-format=json",
            ])
            .args(self.args)
            .env("CARGO_TARGET_DIR", &self.target)
            // A lint that denies warnings makes no mutant unviable.
            .env("RUSTFLAGS", format!("{flags} --cap-lints=warn"))
            .current_dir(self.tree.join(&self.below))
            .output()
            .map_err(|e| {
                ExecError::of(&Toolchain::Spawn {
                    tool: self.cargo.display().to_string(),
                    detail: e.to_string(),
                })
            })?;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }

    /// The scratch copy of the project's `file`.
    fn copy_of(&self, file: &Path) -> PathBuf {
        self.tree.join(&self.below).join(file)
    }

    /// Compile with `subset` applied; who the errors are.
    fn blame(&mut self, subset: &[&Candidate]) -> Result<Blamed, ExecError> {
        let Applied { regions, written } = self.apply(subset)?;
        let result = self.cargo_check();
        for (file, original) in &written {
            std::fs::write(self.copy_of(file), original).map_err(io_error)?;
        }
        let (passes, stdout, stderr) = result?;
        if passes {
            return Ok(Blamed::default());
        }
        self.require_baseline()?;
        let mut blamed = blame(&diagnostics(&stdout), &regions);
        if blamed.is_clean() {
            blamed.elsewhere = Some(last_line(&stderr));
        }
        Ok(blamed)
    }

    /// Write the subset's mutated files; the mutants' text as rustc will
    /// name its place, and the originals to put back.
    fn apply(&self, subset: &[&Candidate]) -> Result<Applied, ExecError> {
        let mut regions = vec![(PathBuf::new(), 0..0); subset.len()];
        let mut written = Vec::new();
        let mut files: Vec<&Path> = subset.iter().map(|c| c.file.as_path()).collect();
        files.sort_unstable();
        files.dedup();
        for file in files {
            let mut here: Vec<usize> = (0..subset.len())
                .filter(|&i| subset[i].file == file)
                .collect();
            here.sort_by_key(|&i| subset[i].start);
            let original = std::fs::read_to_string(self.root.join(file)).map_err(io_error)?;
            let edits: Vec<(usize, usize, &str)> = here
                .iter()
                .map(|&i| (subset[i].start, subset[i].end, subset[i].with.as_str()))
                .collect();
            let (text, at) = apply_edits(&original, &edits);
            std::fs::write(self.copy_of(file), text).map_err(io_error)?;
            for (&i, range) in here.iter().zip(at) {
                regions[i] = (self.below.join(file), range);
            }
            written.push((file.to_path_buf(), original));
        }
        Ok(Applied { regions, written })
    }

    /// Believe a failing compile only if the unmutated crate passes.
    fn require_baseline(&mut self) -> Result<(), ExecError> {
        if self.baseline_passes {
            return Ok(());
        }
        let (passes, _, stderr) = self.cargo_check()?;
        if !passes {
            return Err(ExecError::of(&Toolchain::BaselineUnchecked {
                detail: last_line(&stderr),
            }));
        }
        self.baseline_passes = true;
        Ok(())
    }
}

fn io_error(e: std::io::Error) -> ExecError {
    ExecError::of(&Toolchain::Viability {
        detail: e.to_string(),
    })
}

fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("cargo check failed")
        .trim()
        .to_string()
}

/// Which of `candidates` rustc rejects, checking them in a scratch copy of
/// `workspace` (the project's `root` is inside it) with `cargo check` and
/// `cargo_args`, telling `progress` before each `cargo check`. An error for
/// a project that cannot be checked at all.
pub fn check(
    cargo: &Path,
    workspace: &Path,
    root: &Path,
    cargo_args: &[String],
    candidates: &[Candidate],
    progress: &mut dyn FnMut(Progress),
) -> Result<Checked, ExecError> {
    let started = Instant::now();
    let scratch = scratch_dir(workspace);
    let skip = [workspace.join("target"), scratch.clone()];
    let tree = scratch.join("tree");
    sync_tree(workspace, &tree, &skip).map_err(io_error)?;
    let mut rustc = Rustc {
        cargo,
        root,
        tree,
        below: root.strip_prefix(workspace).unwrap_or(Path::new("")).into(),
        target: scratch.join("target"),
        args: cargo_args,
        baseline_passes: false,
        compiles: 0,
        batch: (0, 0),
        progress,
    };
    let mut rejected = HashMap::new();
    let all = batches(candidates);
    for (at, batch) in all.iter().enumerate() {
        rustc.batch = (at + 1, all.len());
        let members: Vec<&Candidate> = batch.iter().map(|&i| &candidates[i]).collect();
        let found = settle(members.len(), &mut |subset| {
            let chosen: Vec<&Candidate> = subset.iter().map(|&i| members[i]).collect();
            rustc.blame(&chosen)
        })?;
        for (i, why) in found {
            rejected.insert(members[i].id.clone(), why);
        }
    }
    Ok(Checked {
        rejected,
        compiles: rustc.compiles,
        took: started.elapsed(),
    })
}

/// An executor that calls a mutant unviable, without running it, when rustc
/// rejects its source.
pub struct RustcRejects<E> {
    inner: E,
    rejected: Arc<HashMap<MutantId, String>>,
}

impl<E> RustcRejects<E> {
    pub fn new(inner: E, rejected: Arc<HashMap<MutantId, String>>) -> Self {
        RustcRejects { inner, rejected }
    }

    /// The outcome of a mutant rustc rejects: no program can be built from
    /// its source, so no debugger runs it.
    fn unviable(&self, plan: &MutantPlan) -> Option<RawOutcome> {
        self.rejected
            .get(&plan.id)
            .map(|detail| RawOutcome::Unviable {
                why: Report::of(&Reason::RustcRejects {
                    detail: detail.clone(),
                }),
            })
    }
}

impl<E: MutantExecutor> MutantExecutor for RustcRejects<E> {
    fn run(&mut self, plan: &MutantPlan, budget: Budget) -> Result<Observed, ExecError> {
        match self.unviable(plan) {
            Some(raw) => Ok(Observed::bare(raw)),
            None => self.inner.run(plan, budget),
        }
    }
    fn baseline(&mut self, budget: Budget) -> Result<Observed, ExecError> {
        self.inner.baseline(budget)
    }
    fn driven(
        &mut self,
        plans: &[MutantPlan],
        budget: Budget,
    ) -> Result<Option<Vec<Driven>>, ExecError> {
        self.inner.driven(plans, budget)
    }
    fn recording(&mut self, plans: &[MutantPlan], budget: Budget) -> Result<Recording, ExecError> {
        self.inner.recording(plans, budget)
    }
    fn select_tests(&mut self, tests: Option<&[String]>) {
        self.inner.select_tests(tests);
    }
    fn describe(&self) -> Option<String> {
        self.inner.describe()
    }
    fn close_session(&mut self) {
        self.inner.close_session();
    }
    fn canceller(&self) -> std::sync::Arc<dyn cherenkov_run_ports::executor::Cancel> {
        self.inner.canceller()
    }
}
