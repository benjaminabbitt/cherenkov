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

mod config;
mod error;
mod names;
mod output;
mod render;
mod run;
#[cfg(test)]
#[path = "run_output.test.rs"]
mod run_output;
mod selection;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angzarr_presentation::{Catalog, CatalogError, Format};

use cherenkov_core::concept::Concept;
use cherenkov_core::operator_set::{CustomSet, OperatorSet, SetError, registry, resolve_sets};
use cherenkov_core::plan::{Diagnostics, MutantPlan, Provenance, SkipReason};
use cherenkov_core::profile::Profile;
use cherenkov_core::session::{FileProblem, ProjectPlan};
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::Settings;
use cherenkov_run_domain::OperatorSet as LanguageSet;
use cherenkov_syntax::TreeSitterSyntax;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use cli_support::{
    DETAILED_EXITCODE, Exit, ExitStatuses, Failure, detailed_exitcode_arg, exit_statuses,
    usage_exit,
};

use crate::error::CliError;
use crate::output::Listed;
use crate::selection::{SelectArgs, Selection, plan_selected};

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
    Plan(Box<PlanArgs>),
    /// Run every mutant of a Go package (Delve) or Cargo project (GDB) and
    /// print each verdict as it is decided.
    Run(Box<RunArgs>),
    /// List the named operator sets, their sources, and what each does not cover yet.
    Sets,
    /// List every named mutation, where its definition came from, and peers
    /// that have the same or a similar mutation.
    Mutations,
    /// Dump the operator inventory (operators, their replacements,
    /// definitions, sources, other tools' names, per-language spellings)
    /// as Markdown or JSON, for documentation.
    Operators(OperatorsArgs),
}

/// What `cherenkov operators` is asked.
#[derive(Args)]
struct OperatorsArgs {
    /// markdown (default) or json (schema 1: the keys are documented in
    /// docs/mutation-operators.md).
    #[arg(long, value_enum, default_value = "markdown")]
    format: DumpFormat,
    /// Only this registered set's operators, each with its table.
    #[arg(long, value_name = "SET")]
    set: Option<String>,
    /// Leave out each language's spellings of the replacements.
    #[arg(long = "no-spellings")]
    no_spellings: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum DumpFormat {
    Markdown,
    Json,
}

/// What `cherenkov plan` is asked.
#[derive(Args)]
struct PlanArgs {
    dir: PathBuf,
    /// json: one event per line; text: one line per mutant. Default:
    /// text on a terminal, json otherwise.
    #[arg(long, value_enum)]
    format: Option<FormatArg>,
    /// Also skip files matching this glob (repeatable), e.g.
    /// `**/*.test.rs` for tests kept beside the code. A glob without a
    /// `/` matches at any depth, as in cargo-mutants.
    #[arg(short = 'e', long)]
    exclude: Vec<String>,
    /// The named operator set to use, `a,b,file.toml` for several (see
    /// `cherenkov sets`). Default: `cargo-mutants` for Rust, `breadth` for
    /// the other languages.
    #[arg(long)]
    operators: Option<String>,
    #[command(flatten)]
    select: SelectArgs,
    /// Print each mutant's cargo-mutants name (`path:line:col: replace
    /// X with Y in fn`), one a line, instead of the run's events.
    #[arg(long)]
    names: bool,
    /// Judge only the lines a unified diff adds (as cargo-mutants'
    /// `--in-diff`): a mutant stays when its span touches one.
    #[arg(long = "in-diff", value_name = "FILE")]
    in_diff: Option<PathBuf>,
}

/// What `cherenkov run` is asked: boxed in the command, which would
/// otherwise be as large as it.
#[derive(Args)]
struct RunArgs {
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
    /// Also skip files matching this glob (repeatable); without a `/` it
    /// matches at any depth, as in cargo-mutants.
    #[arg(short = 'e', long)]
    exclude: Vec<String>,
    /// The named operator set to use, `a,b,file.toml` for several (see
    /// `cherenkov sets`). Default: `cargo-mutants` for Rust, `breadth` for
    /// the other languages.
    #[arg(long)]
    operators: Option<String>,
    #[command(flatten)]
    select: SelectArgs,
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
    /// Write the finished run's verdicts to FILE (conventionally
    /// `.cherenkov/verdicts.jsonl`, committed): one JSON line for each
    /// mutant, sorted by id, holding no time or run id, so it diffs as
    /// one line for each changed verdict. A run that fails writes nothing.
    #[arg(long = "verdicts", value_name = "FILE")]
    verdict_file: Option<PathBuf>,
    /// Judge only the lines a unified diff adds (as cargo-mutants'
    /// `--in-diff`): a mutant stays when its span touches one.
    #[arg(long = "in-diff", value_name = "FILE")]
    in_diff: Option<PathBuf>,
    /// Do not ask rustc which function-body replacements do not compile
    /// (Rust): those the debugger cannot run stay unsupported instead of
    /// unviable. The check compiles the project in a scratch copy under its
    /// target directory, a few times for the whole run.
    #[arg(long = "no-viability-check")]
    no_viability_check: bool,
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

/// The statuses of the raw command line, for the exits that come before it is
/// parsed.
fn raw_statuses() -> ExitStatuses {
    let flag = format!("--{DETAILED_EXITCODE}");
    ExitStatuses::new(std::env::args().any(|a| a == flag))
}

/// The command line, and the statuses its `--detailed-exitcode` flag binds.
fn parse() -> Result<(Cli, ExitStatuses), clap::Error> {
    let matches = Cli::command()
        .arg(detailed_exitcode_arg())
        .try_get_matches()?;
    let statuses = exit_statuses(&matches);
    Ok((Cli::from_arg_matches(&matches)?, statuses))
}

fn main() -> ExitCode {
    let catalog = match catalog() {
        Ok(catalog) => catalog,
        Err(e) => {
            eprintln!("{PROG}: {e}");
            return raw_statuses().process(Exit::FAILED);
        }
    };
    let (cli, statuses) = match parse() {
        Ok(parsed) => parsed,
        Err(e) => {
            let _ = e.print();
            return raw_statuses().process(usage_exit(&e));
        }
    };
    let (format, result) = match cli.command {
        Command::Plan(args) => {
            let PlanArgs {
                dir,
                format,
                exclude,
                operators,
                select,
                names,
                in_diff,
            } = *args;
            let format = chosen(format);
            let result = Selection::load(&dir, &select, &exclude).and_then(|selection| {
                plan(
                    &dir,
                    format,
                    operators.as_deref(),
                    &selection,
                    in_diff.as_deref(),
                    names,
                )
            });
            (format, result)
        }
        Command::Run(args) => {
            let RunArgs {
                dir,
                format,
                dlv,
                go,
                gdb,
                cargo,
                exclude,
                operators,
                select,
                tests,
                jobs,
                cargo_args,
                in_diff,
                deficiency_log,
                verdict_file,
                no_viability_check,
            } = *args;
            let format = chosen(format);
            let result = Selection::load(&dir, &select, &exclude).and_then(|selection| {
                let cargo_args = [cargo_args, selection.cargo_args.clone()].concat();
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
                        verdict_file,
                        viability: !no_viability_check,
                        selection,
                    },
                    operators.as_deref(),
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
        Command::Operators(args) => (Format::Text, operators(&args)),
    };
    match result {
        Ok(()) => statuses.process(Exit::OK),
        Err(error) => {
            let failure = Failure::of(error);
            let _ = failure.say(PROG, format, &catalog, &mut std::io::stderr());
            statuses.process(failure.exit)
        }
    }
}

/// The sets `spec` asks for (`a,b,file.toml`), or the error that says why not.
pub(crate) fn operator_sets(spec: &str) -> Result<Vec<OperatorSet>, CliError> {
    resolve_sets(spec, load_set).map_err(|e| match e {
        SetError::Unknown(name) => {
            let known: Vec<&str> = registry().iter().map(|s| s.name).collect();
            CliError::UnknownOperatorSet {
                name,
                known: known.join(", "),
            }
        }
        SetError::Unreadable { path, reason } => CliError::Unreadable { path, reason },
        SetError::Invalid { path, reason } => CliError::BadOperatorSet { path, reason },
        other => CliError::BadOperatorSet {
            path: spec.to_string(),
            reason: other.reason(),
        },
    })
}

/// A custom set's file, read and parsed.
fn load_set(path: &str) -> Result<CustomSet, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

/// The sets a run uses for `lang`: those asked for, else its default.
pub(crate) fn operator_sets_for(
    asked: Option<&str>,
    lang: &str,
) -> Result<Vec<OperatorSet>, CliError> {
    operator_sets(asked.unwrap_or(cherenkov_core::operator_set::default_set_name(lang)))
}

/// The sets' names, as `run.started` and the operators line show them.
pub(crate) fn set_names(sets: &[OperatorSet]) -> String {
    let names: Vec<&str> = sets.iter().map(|s| s.name).collect();
    names.join(",")
}

/// Each of `sets`, for each of `languages`: what `run.started` names.
pub(crate) fn language_sets<'a>(
    languages: impl IntoIterator<Item = &'a str>,
    sets: &[OperatorSet],
) -> Vec<LanguageSet> {
    languages
        .into_iter()
        .flat_map(|language| {
            sets.iter().map(move |s| LanguageSet {
                language: language.to_string(),
                set: s.name.to_string(),
            })
        })
        .collect()
}

/// With several sets, how many mutants each produced and how many more than
/// one did: the provenance of the plan, summarized. Nothing for one set.
pub(crate) fn provenance_line(
    sets: &[OperatorSet],
    plans: &[MutantPlan],
    provenance: &Provenance,
) -> Option<String> {
    if sets.len() < 2 {
        return None;
    }
    let of: Vec<&Vec<&str>> = plans.iter().filter_map(|p| provenance.get(&p.id)).collect();
    let each: Vec<String> = sets
        .iter()
        .map(|s| {
            format!(
                "{} {}",
                s.name,
                of.iter().filter(|v| v.contains(&s.name)).count()
            )
        })
        .collect();
    let shared = of.iter().filter(|v| v.len() > 1).count();
    Some(format!(
        "provenance: {}; in more than one set: {shared}",
        each.join(", ")
    ))
}

fn sets() {
    use cherenkov_core::inventory::{Inventory, Spellings};
    use cherenkov_core::operator_set::default_set_name;
    for s in registry() {
        let default = if s.name == default_set_name("rust") {
            " (default for rust)"
        } else if s.name == default_set_name("") {
            " (default for the other languages)"
        } else {
            ""
        };
        println!("{}{default}: {}", s.name, s.summary);
        println!("  source: {}", s.source);
        let inventory = Inventory::of_set(&s, &Spellings::none());
        println!("  operators ({}):", inventory.operators.len());
        for op in &inventory.operators {
            let aliases = if op.aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", op.aliases.join(", "))
            };
            let details: Vec<String> = op
                .replacements
                .iter()
                .map(|r| {
                    let own: Vec<&str> = r
                        .aliases
                        .iter()
                        .filter(|a| !op.aliases.contains(a))
                        .map(String::as_str)
                        .collect();
                    if own.is_empty() {
                        r.detail.to_string()
                    } else {
                        format!("{} ({})", r.detail, own.join(", "))
                    }
                })
                .collect();
            println!("    {}{aliases}: {}", op.name, details.join(" "));
        }
        for p in s.pending {
            println!("  pending: {p}");
        }
    }
}

/// The inventory as the options ask, printed.
fn operators(args: &OperatorsArgs) -> Result<(), CliError> {
    use cherenkov_core::inventory::{Inventory, Spellings};
    let syntax = TreeSitterSyntax::builtin().map_err(|e| CliError::Syntax {
        reason: e.to_string(),
    })?;
    let profiles = syntax.profiles();
    let tables: Vec<(&str, &cherenkov_core::token::TokenTable)> = if args.no_spellings {
        Vec::new()
    } else {
        profiles
            .iter()
            .map(|p| (p.lang.as_str(), &p.tokens))
            .collect()
    };
    let spellings = Spellings::of(&tables);
    let inventory = match &args.set {
        None => Inventory::of_all(&spellings),
        Some(name) => {
            let set = operator_sets(name)?.remove(0);
            Inventory::of_set(&set, &spellings)
        }
    };
    match args.format {
        DumpFormat::Markdown => print!("{}", inventory.markdown()),
        DumpFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&inventory).unwrap_or_default()
        ),
    }
    Ok(())
}

/// The operators and, under each, its replacements with their origins.
fn mutations() {
    use cherenkov_core::inventory::{Inventory, Spellings};
    use cherenkov_core::operator_set::{Relation, families};
    let all = families();
    let inventory = Inventory::of_all(&Spellings::none());
    println!("operators: {}", all.len());
    for family in &all {
        println!("\n{}: {}", family.id, family.definition);
        let family_aliases = inventory
            .operators
            .iter()
            .find(|o| o.name == family.id)
            .map(|o| o.aliases.clone())
            .unwrap_or_default();
        if !family_aliases.is_empty() {
            println!("  also called: {}", family_aliases.join(", "));
        }
        for m in &family.replacements {
            println!("{}", m.name);
            let aliases = inventory
                .operators
                .iter()
                .flat_map(|o| &o.replacements)
                .find(|r| r.name == m.name)
                .map(|r| r.aliases.clone())
                .unwrap_or_default();
            if !aliases.is_empty() {
                println!("  also called: {}", aliases.join(", "));
            }
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
}

/// The dry run: `dir`'s mutants planned as a run with `set`, its events
/// printed in `format`, and nothing run.
/// `dir` planned with the sets asked for, or with each language's default
/// when none were: the project, the sets used when one list was asked for,
/// and the label that names what was used.
fn plan_with(
    dir: &Path,
    syntax: &TreeSitterSyntax,
    profiles: &[Profile],
    operators: Option<&str>,
    selection: &Selection,
) -> Result<Planned, CliError> {
    if let Some(spec) = operators {
        let sets = operator_sets(spec)?;
        let project = plan_selected(dir, syntax, profiles, &sets, selection)?;
        let label = set_names(&sets);
        let languages = language_sets(project.diagnostics.keys().map(|l| l.as_str()), &sets);
        return Ok((project, sets, label, languages));
    }
    let mut project = ProjectPlan::default();
    let mut used: Vec<(String, String)> = Vec::new();
    let mut languages: Vec<LanguageSet> = Vec::new();
    for profile in profiles {
        let lang = profile.lang.as_str();
        let sets = operator_sets_for(None, lang)?;
        let part = plan_selected(dir, syntax, std::slice::from_ref(profile), &sets, selection)?;
        if !part.diagnostics.is_empty() {
            used.push((lang.to_string(), set_names(&sets)));
            languages.extend(language_sets([lang], &sets));
        }
        project.plans.extend(part.plans);
        project.diagnostics.extend(part.diagnostics);
        project.problems.extend(part.problems);
        project.provenance.extend(part.provenance);
    }
    project.plans.sort_by(|a, b| a.file.cmp(&b.file));
    Ok((project, Vec::new(), default_label(&used), languages))
}

/// A planned project, the sets used when one list was asked for, the label
/// that names what was used, and the set of each language.
type Planned = (ProjectPlan, Vec<OperatorSet>, String, Vec<LanguageSet>);

/// The default sets in use, named by language unless one set serves all.
fn default_label(used: &[(String, String)]) -> String {
    let mut sets: Vec<&str> = used.iter().map(|(_, set)| set.as_str()).collect();
    sets.dedup();
    if sets.len() == 1 {
        return sets[0].to_string();
    }
    let pairs: Vec<String> = used.iter().map(|(l, s)| format!("{l}={s}")).collect();
    pairs.join(",")
}

fn plan(
    dir: &Path,
    format: Format,
    operators: Option<&str>,
    selection: &Selection,
    in_diff: Option<&Path>,
    names: bool,
) -> Result<(), CliError> {
    let syntax = TreeSitterSyntax::builtin()
        .map_err(|e| CliError::Syntax {
            reason: e.to_string(),
        })?
        .with_skip_calls(selection.skip_calls.clone());
    let profiles = with_excludes(syntax.profiles(), &selection.exclude_globs);
    let (project, sets, label, languages) =
        plan_with(dir, &syntax, &profiles, operators, selection)?;
    // Under JSON, run.started names the set.
    if format == Format::Text && !names {
        eprintln!("operators: {label}");
    }
    let plans = match in_diff {
        Some(file) => run::in_diff(project.plans.clone(), file, dir)?,
        None => project.plans.clone(),
    };
    if names {
        for name in crate::names::name_all(dir, &plans) {
            println!("{name}");
        }
        summarize(&project);
        return Ok(());
    }
    let settings = Settings {
        root: dir.display().to_string(),
        operators: languages,
        jobs: 0,
    };
    let sink = output::sink(format, Listed::Planned, None);
    cherenkov_wiring::run(
        cherenkov_run_store::open(),
        &plans,
        &settings,
        None,
        Readers::printing(output::printing(sink)),
    )
    .map_err(|e| CliError::run(&e))?;
    summarize(&project);
    if let Some(line) = provenance_line(&sets, &plans, &project.provenance) {
        eprintln!("{line}");
    }
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
