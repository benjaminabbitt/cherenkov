//! Several operator sets planned as one: a mutant with the same span and
//! replacement is planned once, remembering every set that produced it.

use std::collections::HashMap;

use crate::operator_set::OperatorSet;
use crate::plan::{MutantPlan, PlanOutput, plan_file};
use crate::profile::Profile;
use crate::syntax::{FileSyntax, Span};

/// What a plan changes: where, and to what. A plan without a source form is
/// told apart by its edit.
type Change = (Span, String);

fn change(plan: &MutantPlan) -> Change {
    let to = plan
        .mutated
        .clone()
        .unwrap_or_else(|| format!("edit {}", plan.edit.describe()));
    (plan.site, to)
}

/// Plan one file with each of `sets` in turn and merge: the first set's plans
/// keep their order, a later set adds only the changes not yet planned and
/// its name to those already planned. Each plan is tagged with its sets.
/// Test code is declined unless `include_tests`.
pub fn plan_file_sets(
    file: &FileSyntax,
    profile: &Profile,
    sets: &[OperatorSet],
    include_tests: bool,
) -> PlanOutput {
    let mut merged = PlanOutput::default();
    let mut index: HashMap<Change, usize> = HashMap::new();
    for set in sets {
        let planned = plan_file(file, profile, &set.operators_for(include_tests));
        merged.diagnostics.merge(&planned.diagnostics);
        for plan in planned.plans {
            match index.get(&change(&plan)) {
                Some(&at) => tag(&mut merged, at, set.name),
                None => {
                    merged.provenance.insert(plan.id.clone(), vec![set.name]);
                    index.insert(change(&plan), merged.plans.len());
                    merged.plans.push(plan);
                }
            }
        }
    }
    if sets.len() > 1 {
        // One list in the order a single set's plans come in.
        merged.plans.sort_by(|a, b| {
            (a.stop_at.byte, a.operator, &a.edit, a.site.start.byte).cmp(&(
                b.stop_at.byte,
                b.operator,
                &b.edit,
                b.site.start.byte,
            ))
        });
    }
    merged.diagnostics.planned = merged.plans.len();
    merged
}

/// Plan `at` of `merged` also came from `set`: counted as deduplicated.
fn tag(merged: &mut PlanOutput, at: usize, set: &'static str) {
    let sets = merged
        .provenance
        .entry(merged.plans[at].id.clone())
        .or_default();
    if !sets.contains(&set) {
        sets.push(set);
        merged.diagnostics.deduplicated += 1;
    }
}
