//! `cherenkov run <dir>`: run every mutant (see `run.rs`); `cherenkov plan
//! <dir>`: the dry run, planned and never run. Both go through the run
//! layer and print its events (`output.rs`): JSON Lines, or text on a
//! terminal (`--format`). The composition root: wires the filesystem,
//! tree-sitter and debugger adapters into the core use cases.
//!
//! A plan's per-language summary (mutants, concept coverage, problems)
//! goes to stderr. An error is `{code, args, message}` on stderr under
//! JSON, `cherenkov: message` under text, with the exit-code convention's
//! code (10 failed, 20 declined).

mod error;
mod output;
mod run;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angzarr_presentation::{Catalog, CatalogError, Format};

use cherenkov_core::concept::Concept;
use cherenkov_core::operator_set::{OperatorSet, named, registry};
use cherenkov_core::plan::{Diagnostics, SkipReason};
use cherenkov_core::profile::Profile;
use cherenkov_core::session::{FileProblem, ProjectPlan, plan_project};
use cherenkov_fs::FsSourceReader;
use cherenkov_run_application::layer::{self, Settings};
use cherenkov_syntax::TreeSitterSyntax;
use clap::{Parser, Subcommand, ValueEnum};
use cli_support::{Failure, usage_exit};

use crate::error::CliError;
use crate::output::Listed;

/// The program's name, as errors are prefixed with it.
const PROG: &str = "cherenkov";

#[derive(Parser)]
#[command(
    name = "cherenkov",
    version,
    about = "Debugger-driven mutation testing"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the mutants Cherenkov would run for a project directory: the
    /// run planned, never run.
    Plan {
        dir: PathBuf,
        /// json: one event per line; text: one line per mutant. Default:
        /// text on a terminal, json otherwise.
        #[arg(long, value_enum)]
        format: Option<FormatArg>,
        /// Also skip files matching this glob, relative to DIR (repeatable),
        /// e.g. `**/*.test.rs` for tests kept beside the code.
        #[arg(long)]
        exclude: Vec<String>,
        /// The named operator set to use (see `cherenkov sets`).
        #[arg(long, default_value = "breadth")]
        operators: String,
        /// Judge only the lines a unified diff adds (as cargo-mutants'
        /// `--in-diff`): a mutant stays when its span touches one.
        #[arg(long = "in-diff", value_name = "FILE")]
        in_diff: Option<PathBuf>,
    },
    /// Run every mutant of a Go package (Delve) or Cargo project (GDB) and
    /// print each verdict as it is decided.
    Run {
        dir: PathBuf,
        /// json: one event per line; text: each mutant's verdict and
        /// details. Default: text on a terminal, json otherwise.
        #[arg(long, value_enum)]
        format: Option<FormatArg>,
        /// Path to dlv (default: PATH, then $HOME/go/bin).
        #[arg(long)]
        dlv: Option<PathBuf>,
        /// Path to go (default: PATH, then $HOME/go/bin).
        #[arg(long)]
        go: Option<PathBuf>,
        /// Path to gdb, 14 or later (default: PATH).
        #[arg(long)]
        gdb: Option<PathBuf>,
        /// Path to cargo (default: PATH).
        #[arg(long)]
        cargo: Option<PathBuf>,
        /// Also skip files matching this glob, relative to DIR (repeatable).
        #[arg(long)]
        exclude: Vec<String>,
        /// The named operator set to use (see `cherenkov sets`).
        #[arg(long, default_value = "breadth")]
        operators: String,
        /// Run only this test, by the runner's own name for it (repeatable).
        /// Rust: the libtest name (`module::test_name`); Go: `TestName` or
        /// `TestName/subtest`. Default: every test.
        #[arg(long = "test", value_name = "NAME")]
        tests: Vec<String>,
        /// Workers running mutants at once, and test programs prepared at
        /// once. Each worker has its own debugger sessions; a project whose
        /// test programs share files or ports may need 1. Default: as many
        /// as the machine's cores and memory allow.
        #[arg(long)]
        jobs: Option<usize>,
        /// An argument for cargo when it builds and checks the tests, as the
        /// project's own mutation runs pass them (repeatable): `--lib`,
        /// `--features=test-utils`.
        #[arg(long = "cargo-arg", value_name = "ARG", allow_hyphen_values = true)]
        cargo_args: Vec<String>,
        /// Write each mutant that could not be run (unsupported) to FILE,
        /// one JSON document a line, with a `reason_code` and its reason:
        /// what the run accepted. The same lines are in the event stream.
        #[arg(long = "deficiency-log", value_name = "FILE")]
        deficiency_log: Option<PathBuf>,
        /// Judge only the lines a unified diff adds (as cargo-mutants'
        /// `--in-diff`): a mutant stays when its span touches one.
        #[arg(long = "in-diff", value_name = "FILE")]
        in_diff: Option<PathBuf>,
    },
    /// List the named operator sets, their sources, and what each does not cover yet.
    Sets,
    /// List every named mutation, where its definition came from, and peers
    /// that have the same or a similar mutation.
    Mutations,
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Text,
    Json,
}

/// The format asked for, else text on a terminal and JSON off one.
fn chosen(asked: Option<FormatArg>) -> Format {
    let asked = asked.map(|f| match f {
        FormatArg::Text => Format::Text,
        FormatArg::Json => Format::Json,
    });
    Format::choose(false, asked, std::io::stdout().is_terminal())
}

/// The messages of every crate the program's errors come from.
fn catalog() -> Result<Catalog, CatalogError> {
    let mut sources = vec![error::EN_US];
    sources.extend(cherenkov_run_application::error::CATALOG_SOURCES);
    Catalog::en_us(&sources)
}

fn main() -> ExitCode {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(e) => {
            eprintln!("{PROG}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(usage_exit(&e).code(false));
        }
    };
    let (format, result) = match cli.command {
        Command::Plan {
            dir,
            format,
            exclude,
            operators,
            in_diff,
        } => {
            let format = chosen(format);
            let result = operator_set(&operators)
                .and_then(|set| plan(&dir, format, &exclude, &set, in_diff.as_deref()));
            (format, result)
        }
        Command::Run {
            dir,
            format,
            dlv,
            go,
            gdb,
            cargo,
            exclude,
            operators,
            tests,
            jobs,
            cargo_args,
            in_diff,
            deficiency_log,
        } => {
            let format = chosen(format);
            let result = operator_set(&operators).and_then(|set| {
                run::run(
                    &dir,
                    &run::Tools {
                        dlv,
                        go,
                        gdb,
                        cargo,
                        jobs,
                        cargo_args,
                        in_diff,
                        deficiency_log,
                    },
                    &exclude,
                    &set,
                    &tests,
                    format,
                )
            });
            (format, result)
        }
        Command::Sets => {
            sets();
            (Format::Text, Ok(()))
        }
        Command::Mutations => {
            mutations();
            (Format::Text, Ok(()))
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let failure = Failure::of(error);
            let _ = failure.say(PROG, format, &catalog, &mut std::io::stderr());
            ExitCode::from(failure.exit.code(false))
        }
    }
}

/// The set called `name`, or an error naming the known sets.
pub(crate) fn operator_set(name: &str) -> Result<OperatorSet, CliError> {
    named(name).ok_or_else(|| {
        let known: Vec<&str> = registry().iter().map(|s| s.name).collect();
        CliError::UnknownOperatorSet {
            name: name.to_string(),
            known: known.join(", "),
        }
    })
}

fn sets() {
    for (i, s) in registry().iter().enumerate() {
        let default = if i == 0 { " (default)" } else { "" };
        println!("{}{default}: {}", s.name, s.summary);
        println!("  source: {}", s.source);
        let ids: Vec<&str> = s.operators().iter().map(|o| o.id()).collect();
        println!("  operators: {}", ids.join(", "));
        for p in s.pending {
            println!("  pending: {p}");
        }
    }
}

fn mutations() {
    use cherenkov_core::operator_set::{Relation, mutations};
    for m in mutations() {
        println!("{}", m.name);
        for o in m.origins {
            let name = o.name.map(|n| format!(" {n}")).unwrap_or_default();
            match o.relation {
                Relation::Derived => println!("  derived from {}{name}", o.source),
                Relation::Same => println!("  same as {}{name}", o.source),
                Relation::Similar(note) => {
                    println!("  similar to {}{name}: {note}", o.source)
                }
            }
        }
    }
}

/// The dry run: `dir`'s mutants planned as a run with `set`, its events
/// printed in `format`, and nothing run.
fn plan(
    dir: &Path,
    format: Format,
    exclude: &[String],
    set: &OperatorSet,
    in_diff: Option<&Path>,
) -> Result<(), CliError> {
    // Under JSON, run.started names the set.
    if format == Format::Text {
        eprintln!("operators: {}", set.name);
    }
    let syntax = TreeSitterSyntax::builtin().map_err(|e| CliError::Syntax {
        reason: e.to_string(),
    })?;
    let profiles = with_excludes(syntax.profiles(), exclude);
    let unreadable = |e: &dyn std::fmt::Display| CliError::Unreadable {
        path: dir.display().to_string(),
        reason: e.to_string(),
    };
    let reader = FsSourceReader::new(dir, &profiles).map_err(|e| unreadable(&e))?;
    let project = plan_project(&reader, &syntax, &profiles, &set.operators()).map_err(|e| {
        CliError::Planning {
            reason: format!("{}: {e}", dir.display()),
        }
    })?;
    let plans = match in_diff {
        Some(file) => run::in_diff(project.plans.clone(), file, dir)?,
        None => project.plans.clone(),
    };
    let languages: Vec<&str> = project.diagnostics.keys().map(|l| l.as_str()).collect();
    let settings = Settings {
        root: dir.display().to_string(),
        language: languages.join(","),
        operators: set.name.to_string(),
        jobs: 0,
    };
    let mut sink = output::sink(format, Listed::Planned, None);
    layer::run(
        cherenkov_run_store::open(),
        &plans,
        &settings,
        None,
        sink.as_mut(),
    )
    .map_err(|e| CliError::run(&e))?;
    summarize(&project);
    Ok(())
}

/// Profiles with `exclude` globs added to every language's file rules.
pub(crate) fn with_excludes(mut profiles: Vec<Profile>, exclude: &[String]) -> Vec<Profile> {
    for p in &mut profiles {
        p.files.exclude.extend(exclude.iter().cloned());
    }
    profiles
}

fn summarize(project: &ProjectPlan) {
    for (lang, d) in &project.diagnostics {
        eprintln!("{lang}: {}", summary_line(d));
    }
    for (path, problem) in &project.problems {
        let what = match problem {
            FileProblem::Unreadable(m) => format!("unreadable: {m}"),
            FileProblem::Analysis(m) => format!("analysis failed: {m}"),
            FileProblem::SyntaxErrors => "syntax errors; not planned".to_string(),
        };
        eprintln!("problem: {}: {what}", path.display());
    }
}

fn summary_line(d: &Diagnostics) -> String {
    let mapped = Concept::ALL.len() - d.unmapped.len();
    let mut parts = vec![
        format!("{} mutants", d.planned),
        format!("concepts mapped {mapped}/{}", Concept::ALL.len()),
    ];
    if !d.unmapped.is_empty() {
        let names: Vec<&str> = d.unmapped.iter().map(|c| c.name()).collect();
        parts.push(format!("unmapped: {}", names.join(", ")));
    }
    if !d.unrepresentable.is_empty() {
        let items: Vec<String> = d
            .unrepresentable
            .iter()
            .map(|(c, n)| format!("{} {n}", c.name()))
            .collect();
        parts.push(format!("unrepresentable: {}", items.join(", ")));
    }
    let no_action: usize = d
        .skipped
        .iter()
        .filter(|((_, r), _)| *r == SkipReason::NoActionForContext)
        .map(|(_, n)| n)
        .sum();
    if no_action > 0 {
        parts.push(format!("skipped (no action for context): {no_action}"));
    }
    let mut regions: BTreeMap<&str, usize> = BTreeMap::new();
    for ((_, reason), n) in &d.skipped {
        if let SkipReason::Region(kind) = reason {
            *regions.entry(kind.name()).or_default() += n;
        }
    }
    if !regions.is_empty() {
        let items: Vec<String> = regions.iter().map(|(k, n)| format!("{k} {n}")).collect();
        parts.push(format!("skipped regions: {}", items.join(", ")));
    }
    if !d.disabled.is_empty() {
        let n: usize = d.disabled.values().sum();
        parts.push(format!("disabled: {n}"));
    }
    if d.deduplicated > 0 {
        parts.push(format!("deduplicated: {}", d.deduplicated));
    }
    parts.join("; ")
}
