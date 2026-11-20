//! A mutation-test run's two aggregates: the run, one stream per run rooted at its id,
//! and the mutant, one stream per mutant rooted at its content hash, which
//! outlives the runs that plan it.
//!
//! A run is planned once, by one command, on a stream with no history: its
//! start and its mutants in plan order. It then records what it is told as
//! facts, never refused: each program prepared, a program that did not
//! prepare (which finishes it, failed), and each of its own mutants'
//! decisions, once per mutant (another's, or one already decided, records
//! nothing), counted in its tallies; the last of them finishes it, complete.
//! Nothing is recorded after it finishes.
//!
//! A mutant is planned once per asking run, and decided once per asking
//! run: its decision carries the mutant as planned, its outcome, the run
//! and what the verdict depended on.
//!
//! Each is snapshotted whole after every write, and loads from its newest
//! snapshot and the events after it, so handling an input folds only the
//! events since the last write, not the stream. A snapshot restores to
//! the state its events replay to, and one holding what no stream folds to
//! is refused. A snapshot is used only under rules of the same major and
//! minor version as this crate's: a change to what an aggregate's state holds or
//! how its events fold bumps the minor version, so a durable store's older
//! snapshots are passed over and their streams replayed.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use angzarr_embedded::Timestamp;
use angzarr_embedded::ids;
use angzarr_embedded::markup::{
    Bug, Context, Emit, FactEmit, Failed, Refuses, Report, Snapshots, pack_message,
};
use angzarr_embedded::v1::Uuid;
use angzarr_embedded_macros::{aggregate, applies, handles_cmd, handles_fact};
use cherenkov_core::execution::{Budget, RawOutcome};

pub mod events;
mod judge;
pub mod observation;
pub mod plan;
mod refusal;
mod rejection;

use judge::{Decision, judge};
use refusal::refuse;

pub use refusal::NotOurs;
#[cfg(test)]
pub use rejection::diagnostic_samples;
pub use rejection::{
    EN_US, MutantRejection, RunRejection, SnapshotRejection, UnderivableRoot, Unreadable,
};

/// The run's domain layer (`cherenkov.run.v1`), generated from the protos.
// The generated layer holds what the aggregates here do not use (the
// interface traits, the output enums) and a command enum much larger in
// some variants than others.
#[allow(dead_code, clippy::large_enum_variant, clippy::result_large_err)]
pub mod run {
    include!(concat!(env!("OUT_DIR"), "/cherenkov.run.v1.domain.rs"));
}

/// The mutant's domain layer (`cherenkov.mutant.v1`), generated from the
/// protos.
#[allow(dead_code, clippy::large_enum_variant, clippy::result_large_err)]
pub mod mutant {
    include!(concat!(env!("OUT_DIR"), "/cherenkov.mutant.v1.domain.rs"));
}

pub use mutant::{
    DebuggerFailed, ExecutableMutant, KilledBy, MutantDecided, MutantPlanned, MutantRun,
    MutantSnapshot, NoMutant, NoRun, Observation, Outcome, PlanMutant, PlannedForRun,
    PlannedMutant, ProgramRan, ProgramRun, ProgramRunReported, ProgramRunResult, Ran, RunId, Tier,
    Unit, Verdict,
};
pub use run::{
    AlreadyPlanned, DecisionRecorded, DecisionReported, Drive, Driven, DrivenKind, Drives,
    MutantsPlanned, PlanRun, PreparationFailed, PreparationReported, ProgramFailed,
    ProgramPrepared, RunFinished, RunReady, RunSnapshot, RunStarted, RunStatus, Selection,
    TestList, VerdictCount,
};

/// The run domain: a stream per run.
pub const RUN_DOMAIN: &str = "cherenkov-run";
/// The mutant domain: a stream per mutant.
pub const MUTANT_DOMAIN: &str = "cherenkov-mutant";

/// The root of mutant `id`'s stream: derived from the id alone, so every
/// run that plans the mutant finds the same stream.
pub fn mutant_root(id: &str) -> Result<Uuid, UnderivableRoot> {
    root_of(MUTANT_DOMAIN, id)
}

/// The root of `domain`'s stream for `key`, which the derivation refuses a
/// domain that holds its separator.
fn root_of(domain: &str, key: &str) -> Result<Uuid, UnderivableRoot> {
    ids::compute_root(domain, key).map_err(|_| UnderivableRoot {
        domain: domain.to_string(),
        key: key.to_string(),
    })
}

/// Whether `keys` are in strictly ascending order: sorted, none twice.
fn ascending<K: Ord>(keys: impl IntoIterator<Item = K>) -> bool {
    let keys: Vec<K> = keys.into_iter().collect();
    keys.windows(2).all(|pair| pair[0] < pair[1])
}

/// One run, folded from its stream.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Run {
    started_at: Option<Timestamp>,
    /// How many test programs are to be prepared.
    programs: u32,
    /// The indexes of the programs prepared.
    prepared: BTreeSet<u32>,
    /// The mutants as planned, until the run is ready: what it hands on.
    plans: Vec<ExecutableMutant>,
    ready: bool,
    /// The planned mutants with no decision yet, by id.
    undecided: BTreeSet<String>,
    /// How many mutants got each verdict.
    counts: BTreeMap<Verdict, u32>,
    finished: bool,
}

/// Milliseconds from `from` to `to`; none when `to` is earlier.
fn millis_between(from: &Timestamp, to: &Timestamp) -> u64 {
    let ms = |t: &Timestamp| i128::from(t.seconds) * 1000 + i128::from(t.nanos) / 1_000_000;
    u64::try_from(ms(to) - ms(from)).unwrap_or(0)
}

impl Run {
    /// Whether the run is planned and not yet finished.
    pub fn live(&self) -> bool {
        self.started_at.is_some() && !self.finished
    }

    /// Whether the run has finished.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Whether every test program is prepared: the mutants can run.
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// The run finishing at `at` with `counts` and `status`.
    fn finish(
        &self,
        counts: &BTreeMap<Verdict, u32>,
        at: &Timestamp,
        status: RunStatus,
    ) -> RunFinished {
        RunFinished {
            counts: tallies(counts),
            took_ms: self
                .started_at
                .map_or(0, |started| millis_between(&started, at)),
            status,
        }
    }

    /// A program that did not prepare, and the run finished, failed.
    fn failed(&self, program: Option<String>, reason: String, ctx: &Context) -> FactEmit<Self> {
        FactEmit::of([
            pack_message(&ProgramFailed { program, reason }),
            pack_message(&self.finish(&self.counts, &ctx.recorded_at(), RunStatus::Failed)),
        ])
    }

    /// Its finish, when it is live and nothing is left to decide.
    fn done(&self, ctx: &Context) -> Option<RunFinished> {
        (self.live() && self.undecided.is_empty())
            .then(|| self.finish(&self.counts, &ctx.recorded_at(), RunStatus::Complete))
    }
}

/// `counts` as the tallies an event or a snapshot holds: in ascending
/// order of verdict.
fn tallies(counts: &BTreeMap<Verdict, u32>) -> Vec<VerdictCount> {
    counts
        .iter()
        .map(|(&verdict, &count)| VerdictCount { verdict, count })
        .collect()
}

/// The unmutated run's wall time, when it passed; else why the program
/// cannot run mutants. Unreadable when the reporter named no baseline.
fn baseline_of(fact: &PreparationReported) -> Result<Result<Duration, String>, Unreadable> {
    let observed = fact
        .baseline
        .clone()
        .ok_or(Unreadable::Missing { field: "baseline" })?;
    let raw = observation::decode(observed)?;
    Ok(match raw {
        RawOutcome::Exited { code: 0, wall, .. } => Ok(wall),
        raw => {
            let named = match fact.program.as_str() {
                "" => String::new(),
                program => format!(" ({program})"),
            };
            let failing: String = fact
                .baseline_failures
                .iter()
                .map(|line| format!("\n  {line}"))
                .collect();
            Err(format!(
                "unmutated tests do not pass under the debugger{named}: {raw:?}{failing}"
            ))
        }
    })
}

/// The event for a program whose baseline passed in `wall`.
fn program_prepared(fact: PreparationReported, wall: Duration) -> ProgramPrepared {
    ProgramPrepared {
        program: fact.program,
        baseline_ms: millis(wall),
        prepared_ms: fact.prepared_ms,
        tests: fact.tests,
        selection: fact.selection,
        failed_tests: fact.failed_tests,
        selection_reason: fact.selection_reason,
        index: fact.index,
        budget_ms: millis(Budget::for_mutant(wall).timeout),
        drives: fact.drives,
    }
}

fn millis(wall: Duration) -> u64 {
    u64::try_from(wall.as_millis()).unwrap_or(u64::MAX)
}

impl Snapshots for Run {
    type Anchor = RunSnapshot;

    fn snapshot(&self) -> Option<RunSnapshot> {
        Some(RunSnapshot {
            started_at: self.started_at,
            undecided: self.undecided.iter().cloned().collect(),
            counts: tallies(&self.counts),
            finished: self.finished,
            programs: self.programs,
            prepared: self.prepared.iter().copied().collect(),
            plans: self.plans.clone(),
            ready: self.ready,
        })
    }

    /// Resume from `snapshot`, refused when it holds what no stream folds
    /// to; refused, the run is unchanged.
    fn restore(&mut self, snapshot: RunSnapshot) -> Result<(), Report> {
        if !ascending(&snapshot.undecided) {
            return Err(Report::of(&SnapshotRejection::UndecidedUnordered));
        }
        if !ascending(snapshot.counts.iter().map(|tally| tally.verdict)) {
            return Err(Report::of(&SnapshotRejection::TalliesUnordered));
        }
        if !ascending(&snapshot.prepared) {
            return Err(Report::of(&SnapshotRejection::PreparedUnordered));
        }
        let holds = !snapshot.undecided.is_empty()
            || !snapshot.counts.is_empty()
            || snapshot.finished
            || snapshot.programs > 0
            || !snapshot.plans.is_empty();
        if snapshot.started_at.is_none() && holds {
            return Err(Report::of(&SnapshotRejection::Unstarted));
        }
        *self = Run {
            started_at: snapshot.started_at,
            undecided: snapshot.undecided.into_iter().collect(),
            counts: snapshot
                .counts
                .into_iter()
                .map(|tally| (tally.verdict, tally.count))
                .collect(),
            finished: snapshot.finished,
            programs: snapshot.programs,
            prepared: snapshot.prepared.into_iter().collect(),
            plans: snapshot.plans,
            ready: snapshot.ready,
        };
        Ok(())
    }
}

#[aggregate(domain = "cherenkov-run")]
impl Run {
    /// Plan the run: once, on a stream with no history. A run of no
    /// mutants has nothing to decide, and finishes with its plan.
    #[handles_cmd(PlanRun, refuses(AlreadyPlanned))]
    fn plan(
        &self,
        cmd: PlanRun,
        ctx: &Context,
    ) -> Result<Emit<Self>, Failed<Refuses<Self, PlanRun>>> {
        if ctx.command.had_prior_events {
            return Err(refuse!(
                RunRejection,
                RunRejection::AlreadyPlanned,
                Refuses<Self, PlanRun>,
                run,
                [AlreadyPlanned => AlreadyPlanned]
            ));
        }
        let started = RunStarted {
            root: cmd.root,
            language: cmd.language,
            operators: cmd.operators,
            jobs: cmd.jobs,
            fast: cmd.fast,
            mutants: u32::try_from(cmd.mutants.len()).unwrap_or(u32::MAX),
            started_at: Some(ctx.recorded_at()),
            programs: cmd.programs,
        };
        let planned = MutantsPlanned {
            mutants: cmd.mutants,
        };
        Ok(Emit::of((started, planned))
            .then(|run: &Run, ctx: &Context| Ok::<_, Failed<Refuses<Run, PlanRun>>>(run.done(ctx))))
    }

    /// A test program's preparation, judged: its baseline must have passed,
    /// and the run records the program prepared, with the time one mutant
    /// may run in it; with the last program prepared the run is ready. A
    /// baseline that did not pass finishes the run, failed. Recorded while
    /// the run is live and not yet ready; a program told twice counts once.
    #[handles_fact(PreparationReported)]
    fn prepared(&self, fact: PreparationReported, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        if !self.live() || self.ready || self.prepared.contains(&fact.index) {
            return Ok(FactEmit::none());
        }
        let baseline = baseline_of(&fact).map_err(|e| Bug::new(&e))?;
        let wall = match baseline {
            Ok(wall) => wall,
            Err(reason) => return Ok(self.failed(Some(fact.program), reason, ctx)),
        };
        let last = self.programs > 0 && self.prepared.len() + 1 == self.programs as usize;
        let ready = RunReady {
            programs: self.programs,
            mutants: self.plans.clone(),
        };
        let prepared = pack_message(&program_prepared(fact, wall));
        Ok(match last {
            true => FactEmit::of([prepared, pack_message(&ready)]),
            false => FactEmit::of([prepared]),
        })
    }

    /// A test program that did not prepare: recorded while the run is
    /// live, and the run finishes, failed.
    #[handles_fact(PreparationFailed)]
    fn preparation_failed(
        &self,
        fact: PreparationFailed,
        ctx: &Context,
    ) -> Result<FactEmit<Self>, Bug> {
        Ok(match self.live() {
            true => self.failed(fact.program, fact.reason, ctx),
            false => FactEmit::none(),
        })
    }

    /// A mutant's decision: recorded once for each of the run's undecided
    /// mutants while it is live, and the last finishes the run.
    #[handles_fact(DecisionReported)]
    fn decided(&self, fact: DecisionReported, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        if !self.live() || !self.undecided.contains(&fact.mutant) {
            return Ok(FactEmit::none());
        }
        let mut counts = self.counts.clone();
        *counts.entry(fact.verdict).or_default() += 1;
        let last = self.undecided.len() == 1;
        let decided = pack_message(&DecisionRecorded {
            mutant: fact.mutant,
            verdict: fact.verdict,
        });
        Ok(match last {
            true => FactEmit::of([
                decided,
                pack_message(&self.finish(&counts, &ctx.recorded_at(), RunStatus::Complete)),
            ]),
            false => FactEmit::of([decided]),
        })
    }

    #[applies(RunStarted)]
    fn started(&mut self, event: RunStarted) {
        self.started_at = event.started_at;
        self.programs = event.programs;
    }

    #[applies(MutantsPlanned)]
    fn planned(&mut self, event: MutantsPlanned) {
        self.undecided = event
            .mutants
            .iter()
            .filter_map(|m| m.mutant.as_ref())
            .map(|m| m.id.clone())
            .collect();
        self.plans = event.mutants;
    }

    #[applies(ProgramPrepared)]
    fn program_prepared(&mut self, event: ProgramPrepared) {
        self.prepared.insert(event.index);
    }

    #[applies(RunReady)]
    fn run_ready(&mut self, event: RunReady) {
        let _ = event;
        self.ready = true;
        self.plans.clear();
    }

    #[applies(ProgramFailed)]
    fn program_failed(&mut self, event: ProgramFailed) {
        let _ = event;
    }

    #[applies(DecisionRecorded)]
    fn decision_recorded(&mut self, event: DecisionRecorded) {
        self.undecided.remove(&event.mutant);
        *self.counts.entry(event.verdict).or_default() += 1;
    }

    #[applies(RunFinished)]
    fn run_finished(&mut self, event: RunFinished) {
        let _ = event;
        self.finished = true;
    }
}

/// One mutant, folded from its stream: for each run that planned it, the
/// mutant as planned, how many programs the run has, what each that has run
/// it made of it, and whether it is decided for that run.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Mutant {
    runs: BTreeMap<String, Asked>,
}

/// What one run that planned a mutant has of it.
#[derive(Clone, PartialEq, Debug, Default)]
struct Asked {
    mutant: PlannedMutant,
    programs: u32,
    ran: Vec<ProgramRun>,
    decided: bool,
}

/// A run's key among a mutant's runs.
fn run_key(run: Option<&RunId>) -> String {
    run.map(ToString::to_string).unwrap_or_default()
}

impl Snapshots for Mutant {
    type Anchor = MutantSnapshot;

    fn snapshot(&self) -> Option<MutantSnapshot> {
        Some(MutantSnapshot {
            runs: self
                .runs
                .iter()
                .map(|(run, asked)| MutantRun {
                    run: run.clone(),
                    mutant: Some(asked.mutant.clone()),
                    decided: asked.decided,
                    programs: asked.programs,
                    ran: asked.ran.clone(),
                })
                .collect(),
        })
    }

    /// Resume from `snapshot`, refused when it holds what no stream folds
    /// to; refused, the mutant is unchanged.
    fn restore(&mut self, snapshot: MutantSnapshot) -> Result<(), Report> {
        if !ascending(snapshot.runs.iter().map(|run| &run.run)) {
            return Err(Report::of(&SnapshotRejection::RunsUnordered));
        }
        let runs = snapshot
            .runs
            .into_iter()
            .map(|run| match run.mutant {
                Some(mutant) if distinct(&run.ran) => Ok((
                    run.run,
                    Asked {
                        mutant,
                        programs: run.programs,
                        ran: run.ran,
                        decided: run.decided,
                    },
                )),
                Some(_) => Err(Report::of(&SnapshotRejection::RanRepeated)),
                None => Err(Report::of(&SnapshotRejection::Unplanned)),
            })
            .collect::<Result<_, _>>()?;
        *self = Mutant { runs };
        Ok(())
    }
}

/// Whether no program is among `ran` twice.
fn distinct(ran: &[ProgramRun]) -> bool {
    let programs: BTreeSet<u32> = ran.iter().map(|run| run.program).collect();
    programs.len() == ran.len()
}

#[aggregate(domain = "cherenkov-mutant")]
impl Mutant {
    /// Plan the mutant for a run: once per run, for a run and a mutant with
    /// its plan.
    #[handles_cmd(PlanMutant, refuses(NoRun, NoMutant, PlannedForRun))]
    fn plan(
        &self,
        cmd: PlanMutant,
        ctx: &Context,
    ) -> Result<MutantPlanned, Failed<Refuses<Self, PlanMutant>>> {
        let _ = ctx;
        let refused = |rejection| {
            refuse!(
                MutantRejection,
                rejection,
                Refuses<Self, PlanMutant>,
                mutant,
                [NoRun => NoRun, NoMutant => NoMutant, PlannedForRun => PlannedForRun]
            )
        };
        let Some(run) = cmd.run else {
            return Err(refused(MutantRejection::NoRun));
        };
        // A mutant with no plan could be decided, never run.
        let Some(mutant) = cmd
            .mutant
            .filter(|executable| executable.mutant.is_some() && executable.plan.is_some())
        else {
            return Err(refused(MutantRejection::NoMutant));
        };
        if self.runs.contains_key(&run_key(Some(&run))) {
            return Err(refused(MutantRejection::PlannedForRun));
        }
        Ok(MutantPlanned {
            run: Some(run),
            mutant: Some(mutant),
            programs: cmd.programs,
        })
    }

    /// What a test program made of the mutant for a run that planned it:
    /// recorded once for each program, and the mutant decides with the run
    /// that settles it (a detection, a mutant no executor can apply, a
    /// debugger that failed) or the last program's. Once it is decided, or
    /// for a run that did not plan it, a run told records nothing.
    #[handles_fact(ProgramRunReported)]
    fn program_ran(&self, fact: ProgramRunReported, ctx: &Context) -> Result<FactEmit<Self>, Bug> {
        let _ = ctx;
        let Some(asked) = self.runs.get(&run_key(fact.run.as_ref())) else {
            return Ok(FactEmit::none());
        };
        let Some(ran) = fact.ran else {
            return Ok(FactEmit::none());
        };
        if asked.decided || asked.ran.iter().any(|told| told.program == ran.program) {
            return Ok(FactEmit::none());
        }
        let all = [asked.ran.clone(), vec![ran.clone()]].concat();
        let decision = judge(&all, asked.programs).map_err(|e| Bug::new(&e))?;
        let recorded = pack_message(&ProgramRan {
            run: fact.run.clone(),
            ran: Some(ran),
        });
        Ok(match decision {
            None => FactEmit::of([recorded]),
            Some(Decision { outcome, programs }) => FactEmit::of([
                recorded,
                pack_message(&MutantDecided {
                    mutant: Some(asked.mutant.clone()),
                    verdict: Some(outcome),
                    run: fact.run,
                    programs,
                }),
            ]),
        })
    }

    #[applies(MutantPlanned)]
    fn planned(&mut self, event: MutantPlanned) {
        let mutant = event
            .mutant
            .and_then(|executable| executable.mutant)
            .unwrap_or_default();
        self.runs.insert(
            run_key(event.run.as_ref()),
            Asked {
                mutant,
                programs: event.programs,
                ..Asked::default()
            },
        );
    }

    #[applies(ProgramRan)]
    fn program_was_run(&mut self, event: ProgramRan) {
        if let (Some(asked), Some(ran)) =
            (self.runs.get_mut(&run_key(event.run.as_ref())), event.ran)
        {
            asked.ran.push(ran);
        }
    }

    #[applies(MutantDecided)]
    fn decided(&mut self, event: MutantDecided) {
        if let Some(asked) = self.runs.get_mut(&run_key(event.run.as_ref())) {
            asked.decided = true;
        }
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

#[cfg(test)]
#[path = "verdict.test.rs"]
mod verdict_tests;
