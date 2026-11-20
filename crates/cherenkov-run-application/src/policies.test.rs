//! The two policies, served by the bus as they are in a run: a policy has
//! no way to be called but to be handed an event, so these tests hand it
//! events by serving a bus over a store that holds them.

use crate::testing::{Core, new_core, new_run};
use cherenkov_run_ports::Edge;

use angzarr_embedded::v1::Uuid;
use cherenkov_core::execution::{RawOutcome, Tier};
use cherenkov_run_domain::events::{MutantEvent, RunEvent, mutant_event, run_event};
use cherenkov_run_domain::observation;
use cherenkov_run_domain::{
    PlanRun, ProgramRun, ProgramRunReported, ProgramRunResult, Ran, RunId, Verdict,
};

use crate::covers::{mutant_cover, run_cover};
use crate::test_plans::{prepared, serve_policies};

/// A plan of `ids` for a run of one test program.
fn plan(ids: &[&str]) -> PlanRun {
    PlanRun {
        programs: 1,
        mutants: ids
            .iter()
            .map(|id| crate::test_plans::executable(id))
            .collect(),
        ..PlanRun::default()
    }
}

async fn mutant_events(core: &Core, id: &str) -> Vec<MutantEvent> {
    core.events(&mutant_cover(id).unwrap(), 0)
        .await
        .unwrap()
        .iter()
        .filter_map(mutant_event)
        .collect()
}

async fn ready_run(core: &Core) -> Uuid {
    let run = new_run();
    core.command(run_cover(&run), &plan(&["a", "b"]))
        .await
        .unwrap();
    core.report(run_cover(&run), &prepared(), "program 0")
        .await
        .unwrap();
    run
}

#[tokio::test]
async fn test_a_run_ready_plans_each_mutant_on_its_own_stream_naming_the_run_once_served() {
    let core = new_core().await;
    let run = ready_run(&core).await;
    for id in ["a", "b"] {
        assert!(
            mutant_events(&core, id).await.is_empty(),
            "no policy is inline: nothing is planned until the bus serves it"
        );
    }
    serve_policies(&core).await;
    for id in ["a", "b"] {
        let events = mutant_events(&core, id).await;
        let [MutantEvent::MutantPlanned(planned)] = &events[..] else {
            panic!("{id}: {events:?}");
        };
        assert_eq!(planned.run.as_ref(), Some(&RunId::from(run.clone())));
        let summary = planned.mutant.as_ref().and_then(|m| m.mutant.as_ref());
        assert_eq!(summary.map(|m| m.id.as_str()), Some(id));
    }
}

#[tokio::test]
async fn test_a_bus_served_again_plans_nothing_twice() {
    // The positions are in the store: a new bus is handed only what the
    // last did not finish, here nothing.
    let core = new_core().await;
    ready_run(&core).await;
    serve_policies(&core).await;
    serve_policies(&core).await;
    assert_eq!(mutant_events(&core, "a").await.len(), 1);
}

/// `a`'s one program told to have run it and failed a test.
async fn killed_a(core: &Core, run: &Uuid) {
    let fact = ProgramRunReported {
        run: Some(RunId::from(run.clone())),
        ran: Some(ProgramRun {
            program: 0,
            program_name: "calc-1".to_string(),
            took_ms: 0,
            tests: Vec::new(),
            result: ProgramRunResult::Ran(Ran {
                observation: Some(observation::encode(&RawOutcome::Exited {
                    code: 101,
                    tier: Tier::P,
                    wall: std::time::Duration::ZERO,
                })),
                killed_by: Vec::new(),
            }),
        }),
    };
    core.report(mutant_cover("a").unwrap(), &fact, "a")
        .await
        .unwrap();
}

async fn decisions(core: &Core, run: &Uuid) -> Vec<(String, Verdict)> {
    core.events(&run_cover(run), 0)
        .await
        .unwrap()
        .iter()
        .filter_map(|p| match run_event(p) {
            Some(RunEvent::DecisionRecorded(d)) => Some((d.mutant, d.verdict)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn test_a_mutant_s_decision_is_recorded_on_the_run_that_asked_once_however_often_served() {
    let core = new_core().await;
    let run = ready_run(&core).await;
    serve_policies(&core).await;
    killed_a(&core, &run).await;
    for _ in 0..2 {
        serve_policies(&core).await;
    }
    assert_eq!(
        decisions(&core, &run).await,
        [("a".to_string(), Verdict::Killed)]
    );
}

#[tokio::test]
async fn test_the_fan_in_passes_over_a_mutant_planned() {
    let core = new_core().await;
    let run = ready_run(&core).await;
    serve_policies(&core).await;
    assert!(decisions(&core, &run).await.is_empty());
}
