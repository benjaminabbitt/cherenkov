//! `cherenkov run <dir>`: run every mutant of a Go package (Delve) or a Cargo
//! package or workspace (GDB) on the run layer, printing each event as it
//! is recorded (`output.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use angzarr_presentation::Format;

use cherenkov_core::diff::AddedLines;
use cherenkov_core::execution::MutantExecutor;
use cherenkov_core::id::MutantId;
use cherenkov_core::operator_set::OperatorSet;
use cherenkov_core::plan::MutantPlan;
use cherenkov_core::types::DefaultFacts;
use cherenkov_exec_dap::delve::{DelveConfig, DelveExecutor};
use cherenkov_exec_dap::gdb::{GdbConfig, GdbExecutor};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::toolchain::{
    build_go_test_binary, build_rust_test_programs_with, cargo_test_env, cargo_workspace,
    cargo_workspace_prefix, go_test_args, rust_code_files_with, rust_test_args, rustup_toolchain,
};
use cherenkov_exec_dap::viability::{self, RustcRejects};
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::{Settings, Workers};
use cherenkov_run_application::verdicts::VerdictFile;
use cherenkov_run_application::workers::{Killer, SESSIONS_PER_WORKER};
use cherenkov_syntax::TreeSitterSyntax;

use crate::error::CliError;
use crate::output::{self, Listed};
use crate::selection::{Selection, plan_selected};

pub struct Tools {
    pub dlv: Option<PathBuf>,
    pub go: Option<PathBuf>,
    pub gdb: Option<PathBuf>,
    pub cargo: Option<PathBuf>,
    /// Test programs prepared at once.
    pub jobs: Option<usize>,
    /// Arguments for cargo when it builds and checks the tests.
    pub cargo_args: Vec<String>,
    /// A unified diff: only mutants touching a line it adds are run.
    pub in_diff: Option<PathBuf>,
    /// A file the mutants that could not be run are written to.
    pub deficiency_log: Option<PathBuf>,
    /// A file the finished run's verdicts are written to, one JSON line for
    /// each mutant, sorted by id.
    pub verdict_file: Option<PathBuf>,
    /// Whether rustc is asked which function-body replacements compile.
    pub viability: bool,
    /// Which mutants are planned (test code, files, names).
    pub selection: Selection,
}

/// The plans whose spans touch a line the unified diff in `file` adds. The
/// diff's paths are from the repository's root; `root` is where the run
/// started, which `git` places below it.
pub fn in_diff(
    plans: Vec<MutantPlan>,
    file: &Path,
    root: &Path,
) -> Result<Vec<MutantPlan>, CliError> {
    let text = std::fs::read_to_string(file).map_err(|e| unreadable(file, &e))?;
    let added = AddedLines::parse(&text);
    let below = repository_prefix(root);
    Ok(plans
        .into_iter()
        .filter(|p| added.touches_below(&below, &p.file, p.site.start.line, p.site.end.line))
        .collect())
}

/// `root`'s path from its git repository's root (empty at the root, or
/// outside a repository).
fn repository_prefix(root: &Path) -> PathBuf {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-prefix"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_default()
}

/// What every run is given: where it is, what it plans with, and how it
/// prints.
struct Asked<'a> {
    root: &'a Path,
    tools: &'a Tools,
    sets: &'a [OperatorSet],
    tests: &'a [String],
    format: Format,
}

pub fn run(
    dir: &Path,
    tools: &Tools,
    operators: Option<&str>,
    tests: &[String],
    format: Format,
) -> Result<(), CliError> {
    let root = dir.canonicalize().map_err(|e| unreadable(dir, &e))?;
    let lang = if root.join("go.mod").is_file() {
        "go"
    } else if root.join("Cargo.toml").is_file() {
        "rust"
    } else {
        return Err(CliError::NoProject {
            dir: dir.display().to_string(),
        });
    };
    let sets = crate::operator_sets_for(operators, lang)?;
    // Under JSON, run.started names the set.
    if format == Format::Text {
        eprintln!("operators: {}", crate::set_names(&sets));
    }
    let asked = Asked {
        root: &root,
        tools,
        sets: &sets,
        tests,
        format,
    };
    if lang == "go" {
        run_go(&asked)
    } else {
        run_rust(&asked)
    }
}

/// `plans`, narrowed to the diff when one was given.
fn narrowed(plans: Vec<MutantPlan>, asked: &Asked) -> Result<Vec<MutantPlan>, CliError> {
    match &asked.tools.in_diff {
        Some(file) => in_diff(plans, file, asked.root),
        None => Ok(plans),
    }
}

fn unreadable(path: &Path, e: &dyn std::fmt::Display) -> CliError {
    CliError::Unreadable {
        path: path.display().to_string(),
        reason: e.to_string(),
    }
}

fn toolchain(e: impl std::fmt::Display) -> CliError {
    CliError::Toolchain {
        reason: e.to_string(),
    }
}

fn run_go(asked: &Asked) -> Result<(), CliError> {
    let (root, tools) = (asked.root, asked.tools);
    let plans = narrowed(
        plan_language(root, "go", asked.sets, &tools.selection)?,
        asked,
    )?;
    let jobs = tools.jobs.unwrap_or(1);
    let dlv = find_tool(tools.dlv.as_deref(), "dlv")?;
    let go = find_tool(tools.go.as_deref(), "go")?;
    // The toolchain writes the test binary into a scratch directory outside
    // the project; nothing in the working tree is written.
    let scratch = std::env::temp_dir().join(format!("cherenkov-run-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|e| unreadable(&scratch, &e))?;
    let binary = scratch.join("pkg.test");
    let result = build_go_test_binary(&go, root, &binary)
        .map_err(toolchain)
        .and_then(|()| {
            let config = DelveConfig {
                dlv,
                test_binary: binary,
                test_args: go_test_args(asked.tests),
            };
            let executor: Box<dyn MutantExecutor + Send> =
                Box::new(DelveExecutor::new(config, Isa::amd64()));
            // One Delve session: its server's port is fixed per run.
            on_run_layer(asked, "go", &plans, workers_of(vec![vec![executor]], jobs))
        });
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn run_rust(asked: &Asked) -> Result<(), CliError> {
    let (root, tools) = (asked.root, asked.tools);
    let gdb = find_tool(tools.gdb.as_deref(), "gdb")?;
    let cargo = find_tool(tools.cargo.as_deref(), "cargo")?;
    // The code under test is what a non-test build compiles: test modules,
    // inline or in their own files, are never mutated.
    let code = rust_code_files_with(&cargo, root, &tools.cargo_args).map_err(toolchain)?;
    let canonical = root.canonicalize().map_err(|e| unreadable(root, &e))?;
    let plans: Vec<MutantPlan> = plan_language(root, "rust", asked.sets, &tools.selection)?
        .into_iter()
        .filter(|p| code.binary_search(&canonical.join(&p.file)).is_ok())
        .collect();
    let plans = narrowed(plans, asked)?;
    // cargo builds into the project's own target directory, as for any test run.
    let mut programs =
        build_rust_test_programs_with(&cargo, root, &tools.cargo_args).map_err(toolchain)?;
    programs.sort();
    let prefix = cargo_workspace_prefix(&cargo, root).map_err(toolchain)?;
    let facts = default_facts(&cargo, root, &tools.cargo_args)?;
    let (workspace_root, _) = cargo_workspace(&cargo, root).map_err(toolchain)?;
    let rejected = rustc_rejects(asked, &cargo, &workspace_root, &plans);
    // One executor per test program for each worker; every worker has its
    // own GDB sessions.
    let set = || -> Vec<Box<dyn MutantExecutor + Send>> {
        programs
            .iter()
            .cloned()
            .map(|program| {
                // In its package's directory, as `cargo test` runs it.
                // With cargo's environment too, as `cargo test` runs it.
                let toolchain = rustup_toolchain(&program.package_dir);
                let env = cargo_test_env(&program, &cargo, toolchain.as_deref());
                let support_toolchain = toolchain.clone();
                let config = GdbConfig {
                    gdb: gdb.clone(),
                    test_binary: program.binary,
                    test_args: rust_test_args(asked.tests),
                    cwd: Some(program.package_dir),
                    env,
                    source_root: Some(workspace_root.clone()),
                };
                let executor = GdbExecutor::new(config, Isa::amd64())
                    .with_source_prefix(prefix.clone())
                    .with_scope_check(
                        workspace_root.join(&prefix),
                        cherenkov_syntax::rust_scope::unresolved_names,
                    )
                    .with_default_facts(facts.clone())
                    .with_toolchain(support_toolchain);
                Box::new(RustcRejects::new(executor, rejected.clone()))
                    as Box<dyn MutantExecutor + Send>
            })
            .collect()
    };
    let jobs = tools.jobs.unwrap_or_else(|| machine_jobs(&programs));
    let sets = (0..jobs.max(1)).map(|_| set()).collect();
    let result = on_run_layer(asked, "rust", &plans, workers_of(sets, jobs));
    cherenkov_exec_dap::support_lib::remove_scratch();
    result
}

/// The mutants rustc rejects, by id, with its error: asked once at the start
/// of the run for every function-body replacement, in a few compiles of a
/// scratch copy of the workspace. None when the check is off or the project
/// cannot be checked; the diagnostic says which.
fn rustc_rejects(
    asked: &Asked,
    cargo: &Path,
    workspace: &Path,
    plans: &[MutantPlan],
) -> Arc<HashMap<MutantId, String>> {
    let tools = asked.tools;
    if !tools.viability {
        eprintln!(
            "cherenkov: viability check off (--no-viability-check): mutants rustc would reject stay unsupported"
        );
        return Arc::default();
    }
    let candidates = viability::candidates(asked.root, plans);
    if candidates.is_empty() {
        return Arc::default();
    }
    match viability::check(cargo, workspace, asked.root, &tools.cargo_args, &candidates) {
        Ok(found) => {
            eprintln!("{}", viability_summary(candidates.len(), &found));
            Arc::new(found.rejected)
        }
        Err(e) => {
            eprintln!(
                "cherenkov: viability check skipped ({e}): mutants rustc would reject stay unsupported; --no-viability-check turns it off"
            );
            Arc::default()
        }
    }
}

/// The diagnostic line of a finished viability check.
fn viability_summary(checked: usize, found: &viability::Checked) -> String {
    format!(
        "cherenkov: viability check: {} of {checked} function-body replacements rejected by rustc \
         (unviable when the debugger cannot run them), {} cargo check runs, {:.1} s",
        found.rejected.len(),
        found.compiles,
        found.took.as_secs_f64()
    )
}

/// The run layer's workers over `sets`, `jobs` programs prepared at once.
fn workers_of(sets: Vec<Vec<Box<dyn MutantExecutor + Send>>>, jobs: usize) -> Workers {
    Workers {
        sets,
        jobs,
        explain,
    }
}

/// Run `plans` on the run layer with `workers`, printing its events as
/// they are recorded.
fn on_run_layer(
    asked: &Asked,
    lang: &str,
    plans: &[MutantPlan],
    workers: Workers,
) -> Result<(), CliError> {
    let settings = Settings {
        root: asked.root.display().to_string(),
        operators: crate::language_sets([lang], asked.sets),
        jobs: workers.jobs,
    };
    let log = match &asked.tools.deficiency_log {
        Some(path) => {
            let file = std::fs::File::create(path).map_err(|e| unreadable(path, &e))?;
            Some(Box::new(file) as Box<dyn std::io::Write + Send>)
        }
        None => None,
    };
    let sink = output::sink(asked.format, Listed::Decided, log);
    cherenkov_wiring::run(
        cherenkov_run_store::open(),
        plans,
        &settings,
        Some(workers),
        Readers {
            output: output::printing(sink),
            verdicts: asked
                .tools
                .verdict_file
                .clone()
                .map(|path| Arc::new(output::JsonVerdictFile::new(path)) as Arc<dyn VerdictFile>),
        },
    )
    .map(drop)
    .map_err(|e| CliError::run(&e))
}

/// What the workspace's own source says about its types' defaults: every
/// non-test source file of every member, and the members' crate names.
fn default_facts(
    cargo: &Path,
    root: &Path,
    cargo_args: &[String],
) -> Result<DefaultFacts, CliError> {
    let (workspace, crates) = cargo_workspace(cargo, root).map_err(toolchain)?;
    let mut facts = DefaultFacts::default();
    for name in &crates {
        facts.project_crate(name);
    }
    for file in rust_code_files_with(cargo, &workspace, cargo_args).map_err(toolchain)? {
        if let Ok(src) = std::fs::read_to_string(&file) {
            facts.merge(cherenkov_syntax::rust_types::default_facts(&src));
        }
    }
    Ok(facts)
}

fn plan_language(
    root: &Path,
    lang: &str,
    sets: &[OperatorSet],
    selection: &Selection,
) -> Result<Vec<MutantPlan>, CliError> {
    let syntax = TreeSitterSyntax::builtin()
        .map_err(|e| CliError::Syntax {
            reason: e.to_string(),
        })?
        .with_skip_calls(selection.skip_calls.clone());
    let profiles: Vec<_> = crate::with_excludes(syntax.profiles(), &selection.exclude_globs)
        .into_iter()
        .filter(|p| p.lang.as_str() == lang)
        .collect();
    Ok(plan_selected(root, &syntax, &profiles, sets, selection)?.plans)
}

/// Workers for a machine with `cpus` and `available` bytes of memory, each
/// needing `per_worker` bytes: as many as both allow, and at least one.
fn auto_jobs(cpus: usize, available: u64, per_worker: u64) -> usize {
    let by_memory = available / per_worker.max(1);
    cpus.min(usize::try_from(by_memory).unwrap_or(usize::MAX))
        .max(1)
}

/// Workers for this machine: its cores, and its available memory over what
/// a worker's sessions need (four times the largest test binary each). Says
/// what it chose and why.
fn machine_jobs(programs: &[cherenkov_exec_dap::toolchain::TestProgram]) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let largest = programs
        .iter()
        .filter_map(|p| std::fs::metadata(&p.binary).ok())
        .map(|m| m.len())
        .max()
        .unwrap_or(0);
    let per_worker = SESSIONS_PER_WORKER as u64 * 4 * largest;
    let available = sysinfo::System::new().ok().map(|mut system| {
        system.refresh_memory();
        system.available_memory()
    });
    let Some(available) = available else {
        eprintln!("cherenkov: {cpus} workers (cores; available memory unknown)");
        return cpus;
    };
    let jobs = auto_jobs(cpus, available, per_worker);
    const MIB: u64 = 1 << 20;
    eprintln!(
        "cherenkov: {jobs} workers ({cpus} cores; {} MiB available, about {} MiB per worker)",
        available / MIB,
        per_worker / MIB
    );
    jobs
}

/// What caught a mutant: each failing test with where it failed and, for an
/// equality assertion, the value the mutant produced against the value the
/// test expects (`assert_eq!(actual, expected)`).
fn explain(output: &str) -> Vec<Killer> {
    cherenkov_exec_dap::libtest::failures(output)
        .into_iter()
        .map(|(test, f)| Killer {
            test,
            location: f.location,
            message: f.message,
            got: f.left,
            expected: f.right,
        })
        .collect()
}

/// An explicit path, else `name` on PATH, else `$HOME/go/bin/name`.
fn find_tool(explicit: Option<&Path>, name: &str) -> Result<PathBuf, CliError> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    let on_path = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|c| c.is_file())
    });
    let in_gobin = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("go/bin").join(name))
        .filter(|c| c.is_file());
    on_path.or(in_gobin).ok_or_else(|| CliError::ToolMissing {
        tool: name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_viability_diagnostic_says_what_was_rejected_and_what_it_cost() {
        let found = viability::Checked {
            rejected: HashMap::from([(MutantId("a".into()), "e".into())]),
            compiles: 3,
            took: std::time::Duration::from_millis(12_340),
        };
        let line = viability_summary(40, &found);
        assert!(line.contains("1 of 40"), "{line}");
        assert!(line.contains("3 cargo check runs"), "{line}");
        assert!(line.contains("12.3 s"), "{line}");
    }

    #[test]
    fn test_auto_jobs_is_bound_by_memory_and_by_cores_and_is_at_least_one() {
        const GIB: u64 = 1 << 30;
        assert_eq!(auto_jobs(20, 22 * GIB, 2 * GIB), 11, "memory-bound");
        assert_eq!(auto_jobs(4, 100 * GIB, GIB), 4, "core-bound");
        assert_eq!(auto_jobs(20, GIB, 8 * GIB), 1, "never none");
    }
}
