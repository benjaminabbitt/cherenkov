//! A test program's preparation as the run records it, and the run's
//! record of it as a runner reads it: the two directions of the same
//! facts, which carry what a runner needs of a program (the time a mutant
//! may run in it, which tests call a mutant's function) so that it needs
//! nothing from the process that prepared it.

use std::collections::BTreeMap;
use std::time::Duration;

use cherenkov_mutation::execution::Budget;
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_domain::observation;
use cherenkov_run_domain::{
    Drive, Driven, DrivenKind, Drives, PreparationReported, ProgramPrepared, Selection, TestList,
};
use cherenkov_run_ports::executor::{self as core, Selection as Chosen};

use crate::reason::Reason;
use crate::workers::{Preparation, Programs};
use cherenkov_run_domain::why;
use coded_diagnostic::Report;

/// `preparation` of a program, as the fact the run is told: its baseline
/// as observed, and what recording found, for each of `plans`.
pub fn report(preparation: Preparation, plans: &[MutantPlan]) -> PreparationReported {
    let Preparation {
        program,
        describe,
        baseline,
        baseline_failures,
        baseline_output_tail,
        took,
        recording,
    } = preparation;
    let mut fact = PreparationReported {
        program: describe.unwrap_or_default(),
        prepared_ms: millis(took),
        tests: None,
        selection: Selection::EveryTest,
        failed_tests: Vec::new(),
        selection_reason: None,
        selection_why: Some(why::encode(&Report::of(&Reason::BaselineFailed))),
        index: u32::try_from(program).unwrap_or(u32::MAX),
        baseline: Some(observation::encode(&baseline)),
        baseline_failures,
        baseline_output_tail,
        drives: None,
    };
    if let Some(recording) = recording {
        fact.tests = recording
            .tests
            .map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        (fact.selection, fact.failed_tests, fact.selection_why) = match recording.selection {
            Chosen::Recorded => (Selection::Recorded, Vec::new(), None),
            Chosen::RecordedWithFailures { failed_tests } => {
                (Selection::RecordedWithFailures, failed_tests, None)
            }
            Chosen::EveryTest { why } => {
                (Selection::EveryTest, Vec::new(), Some(why::encode(&why)))
            }
        };
        fact.drives = recording.driven.map(|driven| drives(plans, &driven));
    }
    fact
}

/// For each plan, which tests call its function.
fn drives(plans: &[MutantPlan], driven: &[core::Driven]) -> Drives {
    Drives {
        drives: plans
            .iter()
            .zip(driven)
            .map(|(plan, driven)| Drive {
                mutant: plan.id.0.clone(),
                driven: Some(Driven {
                    kind: match driven {
                        core::Driven::NotCalled => DrivenKind::NotCalled(Default::default()),
                        core::Driven::Tests(tests) => DrivenKind::Tests(TestList {
                            tests: tests.clone(),
                        }),
                        core::Driven::Every => DrivenKind::Every(Default::default()),
                    },
                }),
            })
            .collect(),
    }
}

/// The programs the run prepared, in their order, as the workers run
/// mutants with them.
pub fn programs(mut prepared: Vec<ProgramPrepared>) -> Programs {
    prepared.sort_by_key(|program| program.index);
    Programs::new(
        prepared
            .iter()
            .map(|program| Budget {
                timeout: Duration::from_millis(program.budget_ms),
            })
            .collect(),
        prepared
            .iter()
            .map(|program| program.drives.as_ref().map(driven))
            .collect(),
        prepared
            .iter()
            .map(|program| (!program.program.is_empty()).then(|| program.program.clone()))
            .collect(),
    )
}

/// Which tests call each mutant's function, by the mutant's id.
fn driven(drives: &Drives) -> BTreeMap<String, core::Driven> {
    drives
        .drives
        .iter()
        .filter_map(|drive| {
            let kind = &drive.driven.as_ref()?.kind;
            Some((
                drive.mutant.clone(),
                match kind {
                    DrivenKind::NotCalled(_) => core::Driven::NotCalled,
                    DrivenKind::Tests(list) => core::Driven::Tests(list.tests.clone()),
                    DrivenKind::Every(_) => core::Driven::Every,
                },
            ))
        })
        .collect()
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "programs.test.rs"]
mod tests;
