//! Plans the tests run: two `aor` mutants, in `a` and in `b`.

use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use angzarr_embedded::angzarr_bus::{Bus as _, Following, Subscription};
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::{Bus, BusPolicy, Host};
use cherenkov_core::operator::catalog;
use cherenkov_core::plan::{MutantPlan, plan_file};
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

use crate::core::{RunCore, now};
use crate::host::{POLL, TokioWait};
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
pub async fn serve_policies(core: &RunCore<MemoryStore>) {
    let kernel = core.kernel();
    let host = Host::new(
        core.store(),
        &kernel,
        &now,
        &TokioWait,
        POLL,
        Following::Changes(core.store()),
    )
    .pushed_by(core.store());
    let mut bus = Bus::new(host);
    for policy in [BusPolicy::new(FanOut), BusPolicy::new(FanIn)] {
        bus.subscribe(Subscription::policy(NonZeroUsize::MIN, policy))
            .unwrap();
    }
    bus.serve(|_| false).await.unwrap();
}

/// `n` mutants, each a copy of one of the two plans under an id of its own:
/// even ones in `a`, odd ones in `b`.
pub fn many(n: usize) -> Vec<MutantPlan> {
    let two = plans();
    (0..n)
        .map(|i| MutantPlan {
            id: cherenkov_core::id::MutantId(format!("{i:016x}")),
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
        index: 0,
        baseline: Some(cherenkov_run_domain::observation::encode(
            &cherenkov_core::execution::RawOutcome::Exited {
                code: 0,
                tier: cherenkov_core::execution::Tier::P,
                wall: std::time::Duration::from_millis(1),
            },
        )),
        baseline_failures: Vec::new(),
        drives: None,
    }
}
