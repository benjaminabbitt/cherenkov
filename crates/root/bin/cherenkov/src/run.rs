//! `cherenkov run <dir>`: run every mutant of a Go package (Delve) or a Cargo
//! package or workspace (GDB) on the run layer, printing each event as it
//! is recorded (`output.rs`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use coded_diagnostic::Report;
use presentation::Format;

use cherenkov_exec_dap::delve::{DelveConfig, DelveExecutor};
use cherenkov_exec_dap::gdb::{GdbConfig, GdbExecutor};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::libtest::list_tests;
use cherenkov_exec_dap::progress::Compiling;
use cherenkov_exec_dap::routing::OnlyJudged;
use cherenkov_exec_dap::toolchain::{
    Package, TestProgram, build_go_test_binary, build_rust_test_programs_of,
    build_rust_test_programs_with, cargo_test_env, cargo_workspace, cargo_workspace_prefix,
    go_test_args, package_args, package_of, rust_code_files_with, rust_test_args, rustup_toolchain,
    workspace_packages,
};
use cherenkov_exec_dap::viability::{self, RustcRejects};
use cherenkov_mutation::diff::AddedLines;
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::operator_set::OperatorSet;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_mutation::types::DefaultFacts;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::{Settings, Workers};
use cherenkov_run_application::verdicts::VerdictFile;
use cherenkov_run_application::workers::SESSIONS_PER_WORKER;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use cherenkov_syntax::TreeSitterSyntax;

use crate::changed_tests::{ChangedTest, changed_tests, libtest_names};
use crate::phases::Phases;
use crate::selection::{Selection, plan_selected};
use crate::test_scope::{Scope, records_changed_tests_only};
use cherenkov_cli::error::CliError;
use cherenkov_cli::notice::{Notice, Say, tenths};
use cherenkov_cli::output::{self, Listed};

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
    /// Where the run is kept so a killed run can be resumed: a file of the
    /// SQLite store. Default: `.cherenkov/store/runs.sqlite` under the project.
    pub store: Option<PathBuf>,
    /// Keep the run in memory: it is lost with the process and cannot be
    /// resumed. Wins over `store`.
    pub ephemeral: bool,
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
    say: &'a Say,
}

pub fn run(
    dir: &Path,
    tools: &Tools,
    operators: Option<&str>,
    tests: &[String],
    format: Format,
    say: &Say,
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
    say.say(&Notice::Operators {
        label: crate::set_names(&sets),
    });
    // A run killed with SIGKILL could not remove its scratch space.
    cherenkov_exec_dap::support_lib::sweep_stale_scratch();
    say_skipped_tests(say, &tools.selection.skip_tests);
    let asked = Asked {
        root: &root,
        tools,
        sets: &sets,
        tests,
        format,
        say,
    };
    // One run at a time keeps a store: a second is refused before any work.
    let _held = store_file(&asked)?
        .map(|path| crate::store::lock(&path))
        .transpose()?;
    if lang == "go" {
        run_go(&asked)
    } else {
        run_rust(&asked)
    }
}

/// Say which tests are left out of judging, when any are.
fn say_skipped_tests(say: &Say, skipped: &[String]) {
    if !skipped.is_empty() {
        say.say(&Notice::SkippedTests {
            tests: skipped.join(", "),
        });
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
        detail: e.to_string(),
    }
}

fn toolchain(e: ExecError) -> CliError {
    CliError::toolchain(&e)
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
    let path = std::env::temp_dir().join(format!("cherenkov-run-{}", std::process::id()));
    let scratch = GoScratch::create(path.clone()).map_err(|e| unreadable(&path, &e))?;
    let binary = scratch.0.join("pkg.test");
    build_go_test_binary(&go, root, &binary)
        .map_err(toolchain)
        .and_then(|()| {
            let config = DelveConfig {
                dlv,
                test_binary: binary,
                test_args: go_test_args(asked.tests, &tools.selection.skip_tests),
            };
            let executor: Box<dyn MutantExecutor + Send> =
                Box::new(DelveExecutor::new(config, Isa::amd64()));
            // One Delve session: its server's port is fixed per run.
            on_run_layer(asked, "go", &plans, workers_of(vec![vec![executor]], jobs))
        })
}

/// The Go run's scratch directory. Removed when the run ends, however it
/// ends (a panic included); a killed run's is swept by the next run
/// (`support_lib::sweep_stale_scratch`).
struct GoScratch(PathBuf);

impl GoScratch {
    fn create(path: PathBuf) -> std::io::Result<GoScratch> {
        std::fs::create_dir_all(&path)?;
        Ok(GoScratch(path))
    }
}

impl Drop for GoScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_rust(asked: &Asked) -> Result<(), CliError> {
    let (root, tools) = (asked.root, asked.tools);
    let gdb = find_tool(tools.gdb.as_deref(), "gdb")?;
    let cargo = find_tool(tools.cargo.as_deref(), "cargo")?;
    // The code under test is what a non-test build compiles: test modules,
    // inline or in their own files, are never mutated.
    let code = rust_code_files_with(&cargo, root, &tools.cargo_args, &mut |c| {
        say_compiling(asked.say, c)
    })
    .map_err(toolchain)?;
    let canonical = root.canonicalize().map_err(|e| unreadable(root, &e))?;
    let every: Vec<MutantPlan> = plan_language(root, "rust", asked.sets, &tools.selection)?
        .into_iter()
        .filter(|p| code.binary_search(&canonical.join(&p.file)).is_ok())
        .collect();
    // Changed application code: the mutants in the diff.
    let changed_code = narrowed(every.clone(), asked)?;
    let packages = workspace_packages(&cargo, root).map_err(toolchain)?;
    let scope = Scope::of(
        &tools.selection.test_packages,
        &changed_code,
        &packages,
        &canonical,
    );
    // Changed tests: the mutants of what they can reach, judged by them.
    let changed_tests = diff_tests(asked)?;
    let scope = scope.with_changed_tests(&changed_tests, &every, &packages, &canonical);
    let plans = scope.plans_to_run(changed_code, &every);
    let mut programs = timed(asked.say, "build", || {
        build_programs(&cargo, root, tools, &scope, &mut |c| {
            say_compiling(asked.say, c)
        })
    })?;
    programs.sort();
    let prefix = cargo_workspace_prefix(&cargo, root).map_err(toolchain)?;
    let facts = timed(asked.say, "source facts", || {
        default_facts(&cargo, root, &tools.cargo_args, &mut |c| {
            say_compiling(asked.say, c)
        })
    })?;
    let (workspace_root, _) = cargo_workspace(&cargo, root).map_err(toolchain)?;
    let rejected = timed(asked.say, "viability check", || {
        rustc_rejects(asked, &cargo, &workspace_root, &plans, &scope)
    });
    let routed: Vec<Routed> = programs
        .iter()
        .map(|program| routed(asked, &packages, &scope, program))
        .collect();
    // One executor per test program for each worker; every worker has its
    // own GDB sessions. A program judges the mutants routed to it.
    let set = || -> Vec<Box<dyn MutantExecutor + Send>> {
        programs
            .iter()
            .cloned()
            .zip(&routed)
            .map(|(program, routed)| {
                // In its package's directory, as `cargo test` runs it.
                // With cargo's environment too, as `cargo test` runs it.
                let toolchain = rustup_toolchain(&program.package_dir);
                let env = cargo_test_env(&program, &cargo, toolchain.as_deref());
                let support_toolchain = toolchain.clone();
                let config = GdbConfig {
                    gdb: gdb.clone(),
                    test_binary: program.binary,
                    test_args: routed.test_args.clone(),
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
                let executor = RustcRejects::new(executor, rejected.clone());
                Box::new(
                    OnlyJudged::new(executor, routed.judged.clone())
                        .with_changed(routed.only_changed.clone(), routed.changed.clone()),
                ) as Box<dyn MutantExecutor + Send>
            })
            .collect()
    };
    let jobs = tools
        .jobs
        .unwrap_or_else(|| machine_jobs(asked.say, &programs));
    let sets = (0..jobs.max(1)).map(|_| set()).collect();
    on_run_layer(asked, "rust", &plans, workers_of(sets, jobs))
}

/// What one test program is given: the mutants it judges, those among them
/// that only the changed tests judge, those tests by libtest name, and the
/// arguments it runs with.
struct Routed {
    judged: Arc<BTreeSet<MutantId>>,
    only_changed: Arc<BTreeSet<MutantId>>,
    changed: Vec<String>,
    test_args: Vec<String>,
}

/// `program`'s routing. When nothing but changed tests judge here, the
/// program is baselined and recorded with those tests alone.
fn routed(asked: &Asked, packages: &[Package], scope: &Scope, program: &TestProgram) -> Routed {
    let package = package_of(packages, &program.package_dir).unwrap_or_default();
    let (judged, only_changed) = (scope.judged_by(package), scope.only_changed(package));
    let listed = list_tests(&program.binary).unwrap_or_default();
    let changed = libtest_names(&listed, &scope.changed_in(package));
    let skips = &asked.tools.selection.skip_tests;
    let test_args = if records_changed_tests_only(&judged, &only_changed, &changed, asked.tests) {
        rust_test_args(&changed, skips)
    } else {
        rust_test_args(asked.tests, skips)
    };
    Routed {
        judged,
        only_changed,
        changed,
        test_args,
    }
}

/// The tests the `--in-diff` diff changes (none without one).
fn diff_tests(asked: &Asked) -> Result<Vec<ChangedTest>, CliError> {
    let Some(file) = &asked.tools.in_diff else {
        return Ok(Vec::new());
    };
    let text = std::fs::read_to_string(file).map_err(|e| unreadable(file, &e))?;
    Ok(tests_changed_by(&text, asked.root))
}

/// The tests the unified diff `text` changes in the project at `root`.
fn tests_changed_by(text: &str, root: &Path) -> Vec<ChangedTest> {
    let below = repository_prefix(root);
    let read = |relative: &Path| std::fs::read_to_string(root.join(relative)).ok();
    changed_tests(&AddedLines::parse(text), &below, &read)
}

/// The test programs of the packages that judge the planned mutants: the
/// whole workspace's under `test_workspace`, or when a mutant is in no
/// package.
fn build_programs(
    cargo: &Path,
    root: &Path,
    tools: &Tools,
    scope: &Scope,
    compiling: &mut dyn FnMut(Compiling),
) -> Result<Vec<TestProgram>, CliError> {
    // cargo builds into the project's own target directory, as for any test run.
    match scope.build() {
        None => build_rust_test_programs_with(cargo, root, &tools.cargo_args, compiling),
        Some(packages) if packages.is_empty() => Ok(Vec::new()),
        Some(packages) => {
            build_rust_test_programs_of(cargo, root, &tools.cargo_args, &packages, compiling)
        }
    }
    .map_err(toolchain)
}

/// `work`, and what it took on stderr.
fn timed<T>(say: &Say, phase: &str, work: impl FnOnce() -> T) -> T {
    say.say(&Notice::PhaseStarted {
        phase: phase.to_string(),
    });
    let started = std::time::Instant::now();
    let done = work();
    say.say(&Notice::Phase {
        phase: phase.to_string(),
        tenths: tenths(started.elapsed()),
    });
    done
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
    scope: &Scope,
) -> Arc<HashMap<MutantId, String>> {
    let tools = asked.tools;
    if !tools.viability {
        asked.say.say(&Notice::ViabilityOff);
        return Arc::default();
    }
    let candidates = viability::candidates(asked.root, plans);
    if candidates.is_empty() {
        return Arc::default();
    }
    // Only the packages the mutants are in are compiled.
    let args = [tools.cargo_args.clone(), package_args(&scope.mutated())].concat();
    let mut progress = |at| say_progress(asked.say, at);
    let mut compiling = |c| say_compiling(asked.say, c);
    match viability::check(
        cargo,
        workspace,
        asked.root,
        &args,
        &candidates,
        &mut progress,
        &mut compiling,
    ) {
        Ok(found) => {
            asked.say.say(&viability_summary(candidates.len(), &found));
            Arc::new(found.rejected)
        }
        Err(e) => {
            asked.say.say(&Notice::ViabilitySkipped {
                cause: Report::of(&e),
            });
            Arc::default()
        }
    }
}

/// The notice that the viability check is about to run another `cargo check`.
fn say_progress(say: &Say, at: viability::Progress) {
    say.say(&Notice::ViabilityProgress {
        compile: at.compile as u64,
        batch: at.batch as u64,
        batches: at.batches as u64,
    });
}

/// The notice that cargo has finished compiling another crate.
fn say_compiling(say: &Say, at: Compiling) {
    say.say(&Notice::Compiling {
        done: at.done as u64,
        last: at.last,
    });
}

/// The notice of a finished viability check.
fn viability_summary(checked: usize, found: &viability::Checked) -> Notice {
    Notice::Viability {
        rejected: found.rejected.len() as u64,
        checked: checked as u64,
        compiles: found.compiles as u64,
        tenths: tenths(found.took),
    }
}

/// The run layer's workers over `sets`, `jobs` programs prepared at once.
fn workers_of(sets: Vec<Vec<Box<dyn MutantExecutor + Send>>>, jobs: usize) -> Workers {
    Workers { sets, jobs }
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
    let readers = run_readers(asked, log);
    let sampling = cherenkov_wiring::Sampling::production();
    let started = match store_file(asked)? {
        Some(path) => cherenkov_wiring::start_in_file(
            &path,
            plans,
            &settings,
            Some(workers),
            readers,
            sampling,
        ),
        None => cherenkov_wiring::start(
            cherenkov_run_store::memory(),
            plans,
            &settings,
            Some(workers),
            readers,
            sampling,
        ),
    }
    .map_err(|e| CliError::run(&e))?;
    // Under JSON the stream says so: the run it prints begins again with
    // `run.started`, under the id of the run that was carried on.
    if started.resumed {
        asked.say.say(&Notice::CarriedOn {
            run: started.run.to_string(),
        });
    }
    Ok(())
}

/// The readers of a run: its output through the sinks the options ask for,
/// and the verdict file if there is one.
fn run_readers(asked: &Asked, log: Option<Box<dyn std::io::Write + Send>>) -> Readers {
    let in_diff = asked.tools.in_diff.is_some();
    let mut sink = output::sink(asked.format, Listed::Decided, log);
    if in_diff {
        sink = Box::new(output::InDiff::new(sink, asked.say.clone()));
    }
    let skipped = asked.tools.selection.skip_tests.clone();
    let sink = Box::new(output::SkippedTests::new(sink, skipped));
    let sink = Box::new(Phases::new(sink, asked.say.clone()));
    Readers::new(
        output::printing(sink, asked.say.catalog()),
        asked.tools.verdict_file.clone().map(|path| {
            let file = output::JsonVerdictFile::new(path);
            let file = if in_diff {
                file.without_not_called()
            } else {
                file
            };
            Arc::new(file) as Arc<dyn VerdictFile>
        }),
    )
}

/// The file the run is kept in, none for a run kept in memory. Its
/// directory is made.
fn store_file(asked: &Asked) -> Result<Option<PathBuf>, CliError> {
    if asked.tools.ephemeral {
        return Ok(None);
    }
    let Some(path) = &asked.tools.store else {
        return cherenkov_run_store::in_project(asked.root)
            .map(Some)
            .map_err(|e| unreadable(&asked.root.join(".cherenkov"), &e));
    };
    if let Some(directory) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(directory).map_err(|e| unreadable(directory, &e))?;
    }
    Ok(Some(path.clone()))
}

/// What the workspace's own source says about its types' defaults: every
/// non-test source file of every member, and the members' crate names.
fn default_facts(
    cargo: &Path,
    root: &Path,
    cargo_args: &[String],
    compiling: &mut dyn FnMut(Compiling),
) -> Result<DefaultFacts, CliError> {
    let (workspace, crates) = cargo_workspace(cargo, root).map_err(toolchain)?;
    let mut facts = DefaultFacts::default();
    for name in &crates {
        facts.project_crate(name);
    }
    for file in rust_code_files_with(cargo, &workspace, cargo_args, compiling).map_err(toolchain)? {
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
        .map_err(|e| CliError::syntax(&e))?
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
fn machine_jobs(say: &Say, programs: &[cherenkov_exec_dap::toolchain::TestProgram]) -> usize {
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
        say.say(&Notice::Workers { cpus: cpus as u64 });
        return cpus;
    };
    let jobs = auto_jobs(cpus, available, per_worker);
    const MIB: u64 = 1 << 20;
    say.say(&Notice::WorkersSized {
        jobs: jobs as u64,
        cpus: cpus as u64,
        available_mib: available / MIB,
        per_worker_mib: per_worker / MIB,
    });
    jobs
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
    use crate::test_scope::TestPackages;
    use crate::test_scope::tests::plan_in;

    #[test]
    fn test_the_viability_diagnostic_says_what_was_rejected_and_what_it_cost() {
        let found = viability::Checked {
            rejected: HashMap::from([(MutantId("a".into()), "e".into())]),
            compiles: 3,
            took: std::time::Duration::from_millis(12_340),
        };
        assert_eq!(
            viability_summary(40, &found),
            Notice::Viability {
                rejected: 1,
                checked: 40,
                compiles: 3,
                tenths: 123,
            }
        );
    }

    #[derive(Clone, Default)]
    struct Heard(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Heard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Heard {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn say_into(heard: &Heard) -> Say {
        let catalog = presentation::Catalog::en_us(&[cherenkov_cli::error::EN_US]).unwrap();
        Say::to(Format::Text, Arc::new(catalog), heard.clone())
    }

    #[test]
    fn test_a_phase_says_it_started_before_its_work_runs_and_its_time_after() {
        let heard = Heard::default();
        let say = say_into(&heard);
        let during = timed(&say, "build", || heard.text());
        assert_eq!(during, "cherenkov: phase build started\n");
        let after = heard.text();
        assert!(
            after.ends_with("cherenkov: phase build: 0.0 s\n"),
            "{after}"
        );
    }

    #[test]
    fn test_a_viability_progress_is_said_as_its_notice() {
        let heard = Heard::default();
        say_progress(
            &say_into(&heard),
            viability::Progress {
                compile: 4,
                batch: 2,
                batches: 5,
            },
        );
        assert_eq!(
            heard.text(),
            "cherenkov: viability check: cargo check run 4 (batch 2 of 5)\n"
        );
    }

    #[test]
    fn test_auto_jobs_is_bound_by_memory_and_by_cores_and_is_at_least_one() {
        const GIB: u64 = 1 << 30;
        assert_eq!(auto_jobs(20, 22 * GIB, 2 * GIB), 11, "memory-bound");
        assert_eq!(auto_jobs(4, 100 * GIB, GIB), 4, "core-bound");
        assert_eq!(auto_jobs(20, GIB, 8 * GIB), 1, "never none");
    }

    /// A stand-in for cargo: logs its arguments, answers `test` with one
    /// test program.
    fn fake_cargo(tag: &str) -> (test_dir::TestDir, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = test_dir::TestDir::new(&format!("cherenkov-run-fake-{tag}"));
        let log = dir.join("log");
        let test = r#"{"reason":"compiler-artifact","manifest_path":"/w/a/Cargo.toml","profile":{"test":true},"executable":"/t/a-1"}"#;
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\necho '{test}'\n",
            log.display()
        );
        let cargo = dir.join("cargo");
        std::fs::write(&cargo, script).unwrap();
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, cargo, log)
    }

    fn tools() -> Tools {
        Tools {
            dlv: None,
            go: None,
            gdb: None,
            cargo: None,
            jobs: None,
            cargo_args: vec!["--locked".to_string()],
            in_diff: None,
            deficiency_log: None,
            verdict_file: None,
            viability: true,
            selection: Selection::default(),
            store: None,
            ephemeral: true,
        }
    }

    fn package(name: &str) -> cherenkov_exec_dap::toolchain::Package {
        cherenkov_exec_dap::toolchain::Package {
            name: name.to_string(),
            dir: PathBuf::from(format!("/w/{name}")),
            deps: Vec::new(),
        }
    }

    #[test]
    fn test_a_program_runs_with_the_skipped_tests_left_out() {
        let mut tools = tools();
        tools.selection.skip_tests = vec!["bus::slow".to_string()];
        let heard = Heard::default();
        let say = say_into(&heard);
        let asked = Asked {
            root: Path::new("/w"),
            tools: &tools,
            sets: &[],
            tests: &[],
            format: Format::Text,
            say: &say,
        };
        let program = TestProgram {
            binary: PathBuf::from("/t/missing"),
            package_dir: PathBuf::from("/w/a"),
        };
        let scope = Scope::of(
            &TestPackages::Mutated,
            &[],
            &[package("a")],
            Path::new("/w"),
        );
        let routed = routed(&asked, &[package("a")], &scope, &program);
        assert_eq!(routed.test_args, ["--skip", "bus::slow", "--exact"]);
    }

    #[test]
    fn test_the_go_scratch_directory_of_a_run_that_panics_is_removed() {
        let base = test_dir::TestDir::new("cherenkov-go-scratch");
        let path = base.join("cherenkov-run-1");
        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _scratch = GoScratch::create(path.clone()).unwrap();
            assert!(path.exists());
            panic!("the run fails");
        }));
        assert!(ended.is_err());
        assert!(!path.exists(), "the scratch outlived its run");
    }

    #[test]
    fn test_the_build_is_of_the_packages_that_judge_and_of_nothing_when_none_does() {
        let (_dir, cargo, log) = fake_cargo("build");
        let packages = [package("a"), package("b")];
        let plans = [plan_in("m1", "a/src/lib.rs")];
        let root = Path::new("/w");
        let scope = Scope::of(&TestPackages::Mutated, &plans, &packages, root);
        let built = build_programs(&cargo, Path::new("/"), &tools(), &scope, &mut |_| {}).unwrap();
        assert_eq!(built.len(), 1);
        assert_eq!(built[0].binary, PathBuf::from("/t/a-1"));
        let nothing = Scope::of(&TestPackages::Mutated, &[], &packages, root);
        assert!(
            build_programs(&cargo, Path::new("/"), &tools(), &nothing, &mut |_| {})
                .unwrap()
                .is_empty()
        );
        let everything = Scope::of(&TestPackages::Workspace, &plans, &packages, root);
        assert_eq!(
            build_programs(&cargo, Path::new("/"), &tools(), &everything, &mut |_| {})
                .unwrap()
                .len(),
            1
        );
        let logged = std::fs::read_to_string(&log).unwrap();
        // The second build never ran cargo: two runs, the package build and the whole.
        assert_eq!(
            logged.lines().collect::<Vec<_>>(),
            [
                "test --no-run --message-format=json --locked --package a",
                "test --no-run --message-format=json --locked"
            ]
        );
    }

    #[test]
    fn test_the_tests_a_diff_changes_are_found_in_the_project_s_files() {
        let root = test_dir::TestDir::new("cherenkov-diff");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "pub fn f() {}\n\n#[test]\nfn test_f() {\n    f();\n}\n",
        )
        .unwrap();
        let diff = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -5,1 +5,1 @@\n-    g();\n+    f();\n";
        let found = tests_changed_by(diff, &root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "test_f");
        assert_eq!(found[0].file, Path::new("src/lib.rs"));
    }
}
