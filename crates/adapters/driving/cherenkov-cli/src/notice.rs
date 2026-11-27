//! What a command noticed on its way and says on the error stream without
//! failing: coded, with named data (the messages are
//! `locales/en-US/messages.ftl`'s), and said through [`Say`] in the format
//! the command prints in (`cli-support`'s `say_warning`: the diagnostic's
//! envelope under JSON, `cherenkov: message` under text).

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex};

use cherenkov_mutation::concept::Concept;
use cherenkov_mutation::plan::{Diagnostics, SkipReason};
use cli_support::say_warning;
use coded_diagnostic::{Arg, Diagnostic, Report, code};
use presentation::{Catalog, Format};

/// The program's name, as a notice is prefixed with it under text.
const PROG: &str = "cherenkov";

/// A plan's summary for one language: how much was planned and what was
/// left out, the optional parts said only when there is something to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub lang: String,
    pub mutants: u64,
    pub mapped: u64,
    pub total: u64,
    /// The concepts the language's profile does not map.
    pub unmapped: Vec<String>,
    /// `concept count`, one for each concept with matches that do not fit
    /// the model.
    pub unrepresentable: Vec<String>,
    /// Edits with no action for their context.
    pub no_action: u64,
    /// `kind count`, one for each kind of region skipped.
    pub regions: Vec<String>,
    pub disabled: u64,
    pub deduplicated: u64,
}

impl Planned {
    /// `lang`'s plan, summarized from what the planner counted.
    pub fn of(lang: &str, d: &Diagnostics) -> Planned {
        Planned {
            lang: lang.to_string(),
            mutants: d.planned as u64,
            mapped: (Concept::ALL.len() - d.unmapped.len()) as u64,
            total: Concept::ALL.len() as u64,
            unmapped: d.unmapped.iter().map(|c| c.name().to_string()).collect(),
            unrepresentable: d
                .unrepresentable
                .iter()
                .map(|(c, n)| format!("{} {n}", c.name()))
                .collect(),
            no_action: no_action(d),
            regions: regions(d),
            disabled: d.disabled.values().sum::<usize>() as u64,
            deduplicated: d.deduplicated as u64,
        }
    }
}

/// The edits skipped for want of an action in their context.
fn no_action(d: &Diagnostics) -> u64 {
    d.skipped
        .iter()
        .filter(|((_, reason), _)| *reason == SkipReason::NoActionForContext)
        .map(|(_, n)| *n as u64)
        .sum()
}

/// `kind count` for each kind of region the plan skipped.
fn regions(d: &Diagnostics) -> Vec<String> {
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for ((_, reason), n) in &d.skipped {
        if let SkipReason::Region(kind) = reason {
            *by_kind.entry(kind.name()).or_default() += n;
        }
    }
    by_kind
        .iter()
        .map(|(kind, n)| format!("{kind} {n}"))
        .collect()
}

/// Time as the notices say it: whole seconds and tenths of one.
fn seconds(tenths: u64) -> [(&'static str, Arg); 2] {
    [
        ("whole", (tenths / 10).into()),
        ("tenth", (tenths % 10).into()),
    ]
}

/// The tenths of a second in `elapsed`, rounded.
pub fn tenths(elapsed: std::time::Duration) -> u64 {
    (elapsed.as_secs_f64() * 10.0).round() as u64
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The operator sets a plan or a run uses.
    Operators { label: String },
    /// What was planned for a language.
    Planned(Box<Planned>),
    /// A file that was not planned, and why (`cause` is the run
    /// application's file problem).
    FileProblem { path: String, cause: Report },
    /// How many mutants each of several operator sets produced, and how
    /// many more than one did.
    Provenance { each: String, shared: u64 },
    /// A phase before the run began, said when it starts so that a long
    /// one shows the run alive.
    PhaseStarted { phase: String },
    /// How long a phase before the run took.
    Phase { phase: String, tenths: u64 },
    /// How a run's time split between preparation and the mutants.
    Phases {
        preparation_tenths: u64,
        mutants_tenths: u64,
    },
    /// The viability check was turned off.
    ViabilityOff,
    /// What the viability check found.
    Viability {
        rejected: u64,
        checked: u64,
        compiles: u64,
        tenths: u64,
    },
    /// The viability check is about to run its `compile`th `cargo check`,
    /// while checking batch `batch` of `batches`.
    ViabilityProgress {
        compile: u64,
        batch: u64,
        batches: u64,
    },
    /// Cargo has finished compiling `done` crates, `last` the latest; said
    /// while a build or a viability check's `cargo check` runs.
    Compiling { done: u64, last: String },
    /// The viability check could not run: `cause` is what stopped it.
    ViabilitySkipped { cause: Report },
    /// A run carried on a run an earlier process began.
    CarriedOn { run: String },
    /// The workers chosen, one for each core, when the machine's memory is not
    /// known.
    Workers { cpus: u64 },
    /// The workers chosen from the machine's cores and memory.
    WorkersSized {
        jobs: u64,
        cpus: u64,
        available_mib: u64,
        per_worker_mib: u64,
    },
    /// What an `--in-diff` run did not list.
    InDiff { skipped: u64 },
}

impl Notice {
    /// Whether a run printing JSON leaves the notice out: the stream says
    /// it already (`run.started` names the sets and the run carried on),
    /// or it is a note for a person at a terminal.
    pub fn quiet_under_json(&self) -> bool {
        matches!(
            self,
            Notice::Operators { .. } | Notice::CarriedOn { .. } | Notice::InDiff { .. }
        )
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl Diagnostic for Notice {
    fn code(&self) -> &str {
        match self {
            Notice::Operators { .. } => code!("notice-operators"),
            Notice::Planned(_) => code!("notice-planned"),
            Notice::FileProblem { .. } => code!("notice-file-problem"),
            Notice::Provenance { .. } => code!("notice-provenance"),
            Notice::PhaseStarted { .. } => code!("notice-phase-started"),
            Notice::Phase { .. } => code!("notice-phase"),
            Notice::Phases { .. } => code!("notice-phases"),
            Notice::ViabilityOff => code!("notice-viability-off"),
            Notice::Viability { .. } => code!("notice-viability"),
            Notice::ViabilityProgress { .. } => code!("notice-viability-progress"),
            Notice::Compiling { .. } => code!("notice-compiling"),
            Notice::ViabilitySkipped { .. } => code!("notice-viability-skipped"),
            Notice::CarriedOn { .. } => code!("notice-carried-on"),
            Notice::Workers { .. } => code!("notice-workers"),
            Notice::WorkersSized { .. } => code!("notice-workers-sized"),
            Notice::InDiff { .. } => code!("notice-in-diff"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        match self {
            Notice::Operators { label } => vec![("label", label.as_str().into())],
            Notice::Planned(planned) => planned_args(planned),
            Notice::FileProblem { path, cause } => {
                vec![
                    ("path", path.as_str().into()),
                    ("cause", Arg::nested(cause)),
                ]
            }
            Notice::Provenance { each, shared } => {
                vec![("each", each.as_str().into()), ("shared", (*shared).into())]
            }
            Notice::PhaseStarted { phase } => vec![("phase", phase.as_str().into())],
            Notice::Phase { phase, tenths } => {
                let mut args = vec![("phase", phase.as_str().into())];
                args.extend(seconds(*tenths));
                args
            }
            Notice::Phases {
                preparation_tenths,
                mutants_tenths,
            } => vec![
                ("preparation_whole", (preparation_tenths / 10).into()),
                ("preparation_tenth", (preparation_tenths % 10).into()),
                ("mutants_whole", (mutants_tenths / 10).into()),
                ("mutants_tenth", (mutants_tenths % 10).into()),
            ],
            Notice::ViabilityOff => Vec::new(),
            Notice::Viability {
                rejected,
                checked,
                compiles,
                tenths,
            } => {
                let mut args = vec![
                    ("rejected", (*rejected).into()),
                    ("checked", (*checked).into()),
                    ("compiles", (*compiles).into()),
                ];
                args.extend(seconds(*tenths));
                args
            }
            Notice::ViabilityProgress {
                compile,
                batch,
                batches,
            } => vec![
                ("compile", (*compile).into()),
                ("batch", (*batch).into()),
                ("batches", (*batches).into()),
            ],
            Notice::Compiling { done, last } => {
                vec![("done", (*done).into()), ("last", last.as_str().into())]
            }
            Notice::ViabilitySkipped { cause } => vec![("cause", Arg::nested(cause))],
            Notice::CarriedOn { run } => vec![("run", run.as_str().into())],
            Notice::Workers { cpus } => vec![("cpus", (*cpus).into())],
            Notice::WorkersSized {
                jobs,
                cpus,
                available_mib,
                per_worker_mib,
            } => vec![
                ("jobs", (*jobs).into()),
                ("cpus", (*cpus).into()),
                ("available_mib", (*available_mib).into()),
                ("per_worker_mib", (*per_worker_mib).into()),
            ],
            Notice::InDiff { skipped } => vec![("skipped", (*skipped).into())],
        }
    }
}

/// A plan's summary args: each optional part's count (the message's
/// selector) and its items.
fn planned_args(p: &Planned) -> Vec<(&'static str, Arg)> {
    vec![
        ("lang", p.lang.as_str().into()),
        ("mutants", p.mutants.into()),
        ("mapped", p.mapped.into()),
        ("total", p.total.into()),
        ("unmapped_count", p.unmapped.len().into()),
        ("unmapped", p.unmapped.join(", ").into()),
        ("unrepresentable_count", p.unrepresentable.len().into()),
        ("unrepresentable", p.unrepresentable.join(", ").into()),
        ("no_action", p.no_action.into()),
        ("regions_count", p.regions.len().into()),
        ("regions", p.regions.join(", ").into()),
        ("disabled", p.disabled.into()),
        ("deduplicated", p.deduplicated.into()),
    ]
}

/// Where and how notices are said: the stream they go to (the error
/// stream, unless a test gives another), in the command's format, worded
/// by the program's catalog. Cloned into whatever says a notice later, as a
/// sink of the run's documents does.
#[derive(Clone)]
pub struct Say {
    format: Format,
    catalog: Arc<Catalog>,
    to: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Say {
    /// Notices on the process's error stream.
    pub fn stderr(format: Format, catalog: Arc<Catalog>) -> Say {
        Say::to(format, catalog, std::io::stderr())
    }

    /// Notices on `to`.
    pub fn to(format: Format, catalog: Arc<Catalog>, to: impl Write + Send + 'static) -> Say {
        Say {
            format,
            catalog,
            to: Arc::new(Mutex::new(Box::new(to))),
        }
    }

    /// The catalog this says its notices in, for what else of the program's
    /// speaks in words (a run's printed reasons).
    pub fn catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.catalog)
    }

    /// Say `notice`, unless this format leaves it out ([`Notice::quiet_under_json`]).
    pub fn say(&self, notice: &Notice) {
        if self.format == Format::Json && notice.quiet_under_json() {
            return;
        }
        let Ok(mut to) = self.to.lock() else { return };
        say_warning(notice, PROG, self.format, &self.catalog, &mut *to);
    }

    /// The format notices are said in.
    pub fn format(&self) -> Format {
        self.format
    }
}

impl fmt::Debug for Say {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Say").field("format", &self.format).finish()
    }
}

/// One value of every notice, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    vec![
        Box::new(Notice::Operators { label: "l".into() }),
        Box::new(Notice::Planned(Box::new(sample_planned()))),
        Box::new(Notice::FileProblem {
            path: "p".into(),
            cause: Report::new("cherenkov-run-application-file-syntax-errors"),
        }),
        Box::new(Notice::Provenance {
            each: "a 1".into(),
            shared: 2,
        }),
        Box::new(Notice::PhaseStarted {
            phase: "build".into(),
        }),
        Box::new(Notice::Phase {
            phase: "build".into(),
            tenths: 15,
        }),
        Box::new(Notice::ViabilityProgress {
            compile: 2,
            batch: 1,
            batches: 3,
        }),
        Box::new(Notice::Compiling {
            done: 12,
            last: "serde".into(),
        }),
        Box::new(Notice::Phases {
            preparation_tenths: 25,
            mutants_tenths: 75,
        }),
        Box::new(Notice::ViabilityOff),
        Box::new(Notice::Viability {
            rejected: 3,
            checked: 10,
            compiles: 2,
            tenths: 12,
        }),
        Box::new(Notice::ViabilitySkipped {
            cause: Report::new("cherenkov-run-application-no-workers"),
        }),
        Box::new(Notice::CarriedOn { run: "r".into() }),
        Box::new(Notice::Workers { cpus: 4 }),
        Box::new(Notice::WorkersSized {
            jobs: 3,
            cpus: 8,
            available_mib: 2048,
            per_worker_mib: 512,
        }),
        Box::new(Notice::InDiff { skipped: 3 }),
    ]
}

/// A plan with every optional part there is.
#[cfg(test)]
pub fn sample_planned() -> Planned {
    Planned {
        lang: "python".into(),
        mutants: 12,
        mapped: 13,
        total: 16,
        unmapped: vec!["expr".into(), "name".into()],
        unrepresentable: vec!["binary 1".into()],
        no_action: 2,
        regions: vec!["test 1".into()],
        disabled: 3,
        deduplicated: 4,
    }
}

#[cfg(test)]
#[path = "notice.test.rs"]
mod tests;
