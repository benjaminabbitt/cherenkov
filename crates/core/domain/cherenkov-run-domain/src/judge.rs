//! A mutant's verdict, decided from what each test program made of it.
//!
//! The runner executes and reports, one fact for each program's run; the
//! mutant decides. It walks the runs in the order of the programs and stops
//! at the first that settles the mutant: a detection (a failing test, a
//! crash, a timeout), a mutant no executor can apply (unsupported, or one
//! that would not compile), or a debugger that failed on it. Otherwise it
//! waits for the last program: then every run that reached the code passed
//! (survived), or none did (not reached), or no program calls its function
//! (not called). What a run observed becomes a verdict in the core
//! (`classify` and `combine`), the one place that says what an exit code or
//! a crash means.
//!
//! A pure function of the runs, so the same facts decide the same way
//! wherever they are folded.

use cherenkov_mutation::execution::{RawOutcome, classify, combine};
use cherenkov_mutation::verdict::Verdict as Judged;

use crate::mutant::{
    DebuggerFailed, KilledBy, Outcome, ProgramRun, ProgramRunResult, Ran, Tier, Verdict,
};
use crate::observation;
use crate::rejection::Unreadable;

/// A mutant decided: its outcome, and the test programs it ran in.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub outcome: Outcome,
    pub programs: Vec<String>,
}

/// The decision `ran` (the runs of a mutant's programs so far, in any
/// order) comes to, if it comes to one; `programs` is how many programs the
/// run has. Refused where a run's observation is unreadable.
pub fn judge(ran: &[ProgramRun], programs: u32) -> Result<Option<Decision>, Unreadable> {
    let mut ordered: Vec<&ProgramRun> = ran.iter().collect();
    ordered.sort_by_key(|run| run.program);
    let mut folded = Folded::default();
    for run in ordered {
        if let Some(decision) = folded.add(run)? {
            return Ok(Some(decision));
        }
    }
    Ok(folded.last(programs))
}

/// What the runs before the next one came to.
#[derive(Default)]
struct Folded {
    /// How many programs have run (or been skipped as not calling it).
    seen: u32,
    outcomes: Vec<RawOutcome>,
    tests_run: Vec<String>,
    programs: Vec<String>,
    took_ms: u64,
}

/// What a mutant's verdict is, before what it took and ran in is added.
struct Concluded {
    verdict: Verdict,
    tier: Option<Tier>,
    detail: Option<String>,
}

impl Folded {
    /// `run` too: the decision it settles the mutant with, if it does.
    fn add(&mut self, run: &ProgramRun) -> Result<Option<Decision>, Unreadable> {
        self.seen += 1;
        self.took_ms += run.took_ms;
        match &run.result {
            ProgramRunResult::NotCalled(_) => Ok(None),
            ProgramRunResult::Failed(failed) => {
                self.ran_in(run);
                let reason = format!(
                    "the debugger failed on this mutant: {}",
                    debugger_failure(failed)
                );
                let concluded = Concluded {
                    verdict: Verdict::Unsupported,
                    tier: None,
                    detail: Some(reason),
                };
                Ok(Some(self.decided(concluded, &[])))
            }
            ProgramRunResult::Ran(ran) => {
                self.ran_in(run);
                self.observed(ran)
            }
        }
    }

    /// The program `run` ran in, with the tests it ran with.
    fn ran_in(&mut self, run: &ProgramRun) {
        self.tests_run.extend(run.tests.iter().cloned());
        if !run.program_name.is_empty() {
            self.programs.push(run.program_name.clone());
        }
    }

    /// A run observed: the decision, when it settles the mutant.
    fn observed(&mut self, ran: &Ran) -> Result<Option<Decision>, Unreadable> {
        let observation = ran.observation.clone().ok_or(Unreadable::Missing {
            field: "observation",
        })?;
        let raw = observation::decode(observation)?;
        let detected = classify(&raw).verdict.detected();
        let unusable = matches!(
            raw,
            RawOutcome::Unsupported { .. } | RawOutcome::Unviable { .. }
        );
        self.outcomes.push(raw);
        if !(detected || unusable) {
            return Ok(None);
        }
        let killers = if detected { &ran.killed_by[..] } else { &[] };
        let concluded = self.conclude();
        Ok(Some(self.decided(concluded, killers)))
    }

    /// The decision once every program has run, if they have.
    fn last(&mut self, programs: u32) -> Option<Decision> {
        if self.seen < programs {
            return None;
        }
        let concluded = self.conclude();
        Some(self.decided(concluded, &[]))
    }

    /// The verdict of the outcomes so far: of all of them combined, or, with
    /// none (no program calls the function), that it is not called.
    fn conclude(&mut self) -> Concluded {
        if self.outcomes.is_empty() {
            return Concluded {
                verdict: Verdict::NotCalled,
                tier: None,
                detail: None,
            };
        }
        let outcome = classify(&combine(std::mem::take(&mut self.outcomes)));
        Concluded {
            verdict: verdict(outcome.verdict),
            tier: outcome.tier.map(observation::tier_of),
            detail: outcome.detail,
        }
    }

    fn decided(&self, concluded: Concluded, killers: &[KilledBy]) -> Decision {
        let Concluded {
            verdict,
            tier,
            detail,
        } = concluded;
        // A reason is for a verdict that is not a test's: an error, a
        // refusal, code no test reached.
        let reason = match verdict {
            Verdict::Killed | Verdict::Survived | Verdict::Timeout => None,
            _ => detail,
        };
        Decision {
            outcome: Outcome {
                verdict,
                tier,
                took_ms: self.took_ms,
                tests_run: self.tests_run.clone(),
                killed_by: killers.to_vec(),
                reason,
            },
            programs: self.programs.clone(),
        }
    }
}

/// What the debugger's failure says, for the mutant's outcome reason (a
/// text, which the verdict file and the output hold): a failure recorded as
/// text says it; a coded one is named by its keyed form, whose English is
/// the output's catalog's, and not the domain's.
fn debugger_failure(failed: &DebuggerFailed) -> String {
    match &failed.why {
        Some(why) => crate::why::decode(Some(why), &failed.reason).to_string(),
        None => failed.reason.clone(),
    }
}

/// `judged`, the core's verdict, as the run records it.
fn verdict(judged: Judged) -> Verdict {
    match judged {
        Judged::Killed => Verdict::Killed,
        Judged::Survived => Verdict::Survived,
        Judged::Timeout => Verdict::Timeout,
        Judged::NotReached => Verdict::NotReached,
        Judged::Unevaluable => Verdict::Unevaluable,
        Judged::EvaluatorRaised => Verdict::EvaluatorRaised,
        Judged::UnsupportedByExecutor => Verdict::Unsupported,
        Judged::Unviable => Verdict::Unviable,
    }
}
