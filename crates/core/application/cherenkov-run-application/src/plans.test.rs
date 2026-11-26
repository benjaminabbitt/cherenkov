//! Plans the tests run: two `aor` mutants, in `a` and in `b`.

use crate::testing::Core;

use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use angzarr_embedded::BusPolicy;
use angzarr_embedded::angzarr_bus::Subscription;
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_ports::Serving;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

use crate::policies::{FanIn, FanOut};

pub fn plans() -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = "fn a(x: i32) -> i32 { x + 1 }\nfn b(x: i32) -> i32 { x - 1 }\n";
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
        .unwrap();
    let mut plans: Vec<MutantPlan> = plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .filter(|p| p.operator == "aor")
        .collect();
    plans.sort_by_key(|p| p.stop_at.line);
    plans.dedup_by_key(|p| p.stop_at.line);
    assert_eq!(plans.len(), 2);
    plans
}

/// A mutant named only by its id, with an empty plan: what the run records
/// of a mutant for a test that never runs it.
pub fn executable(id: &str) -> cherenkov_run_domain::ExecutableMutant {
    cherenkov_run_domain::ExecutableMutant {
        mutant: Some(cherenkov_run_domain::PlannedMutant {
            id: id.to_string(),
            ..Default::default()
        }),
        plan: Some(cherenkov_run_domain::mutant::MutantPlan {
            id: id.to_string(),
            ..Default::default()
        }),
    }
}

/// `plans` as the run is planned with them: each mutant as a reader needs
/// it, and its plan as a runner does.
pub fn executables(plans: &[MutantPlan]) -> Vec<cherenkov_run_domain::ExecutableMutant> {
    plans
        .iter()
        .map(|p| cherenkov_run_domain::ExecutableMutant {
            mutant: Some(cherenkov_run_domain::PlannedMutant {
                id: p.id.0.clone(),
                ..Default::default()
            }),
            plan: Some(cherenkov_run_domain::plan::encode(p)),
        })
        .collect()
}

/// Serve both policies on a bus over `core`'s store until it is idle: one
/// pass of what they owe. A bus built here is a process starting.
pub async fn serve_policies(core: &Core) {
    let policies = vec![
        Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanOut)),
        Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanIn)),
    ];
    core.serve(policies, &mut |_| false).await.unwrap();
}

/// `n` mutants, each a copy of one of the two plans under an id of its own:
/// even ones in `a`, odd ones in `b`.
pub fn many(n: usize) -> Vec<MutantPlan> {
    let two = plans();
    (0..n)
        .map(|i| MutantPlan {
            id: cherenkov_mutation::id::MutantId(format!("{i:016x}")),
            ..two[i % 2].clone()
        })
        .collect()
}

/// The fact that a run's one program is prepared, which makes the run
/// ready.
pub fn prepared() -> cherenkov_run_domain::PreparationReported {
    cherenkov_run_domain::PreparationReported {
        program: "calc-1".to_string(),
        prepared_ms: 2,
        tests: None,
        selection: cherenkov_run_domain::Selection::Recorded,
        failed_tests: Vec::new(),
        selection_reason: None,
        selection_why: None,
        index: 0,
        baseline: Some(cherenkov_run_domain::observation::encode(
            &cherenkov_mutation::execution::RawOutcome::Exited {
                code: 0,
                tier: cherenkov_mutation::execution::Tier::P,
                wall: std::time::Duration::from_millis(1),
            },
        )),
        baseline_failures: Vec::new(),
        drives: None,
        baseline_output_tail: None,
    }
}
