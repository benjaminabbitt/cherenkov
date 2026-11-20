use angzarr_embedded::v1::{EventBook, EventPage};
use cherenkov_core::execution::{RawOutcome, Tier};
use cherenkov_run_domain::observation;
use cherenkov_run_domain::{DecisionReported, PlanRun, PreparationReported, Selection, Verdict};

use super::*;
use crate::covers::run_cover;
use crate::test_plans::executable;
use crate::testing::{Core, new_core, new_run};
use cherenkov_run_ports::Edge;

fn request(root: &Uuid, pages: Vec<EventPage>) -> ProjectRequest {
    ProjectRequest {
        events: Some(EventBook {
            cover: Some(run_cover(root)),
            next_sequence: pages.last().map_or(0, |page| page.sequence + 1),
            pages,
            snapshot: None,
        }),
    }
}

fn prepared() -> PreparationReported {
    PreparationReported {
        program: "calc-1".to_string(),
        prepared_ms: 2,
        tests: None,
        selection: Selection::Recorded,
        failed_tests: Vec::new(),
        selection_reason: None,
        index: 0,
        baseline: Some(observation::encode(&RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: std::time::Duration::from_millis(1),
        })),
        baseline_failures: Vec::new(),
        drives: None,
    }
}

/// A run of `a` and `b` planned and ready, and its stream's pages. No
/// policy is served, so no mutant is planned on its stream.
async fn ready_run() -> (Uuid, Vec<EventPage>, Core) {
    let core = new_core().await;
    let run = new_run();
    let plan = PlanRun {
        programs: 1,
        mutants: vec![executable("a"), executable("b")],
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    core.report(run_cover(&run), &prepared(), "program 0")
        .await
        .unwrap();
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    (run, pages, core)
}

async fn decide(core: &Core, run: &Uuid, id: &str) {
    let fact = DecisionReported {
        mutant: id.to_string(),
        verdict: Verdict::Killed,
    };
    core.report(run_cover(run), &fact, id).await.unwrap();
}

#[tokio::test]
async fn test_a_run_owes_the_mutants_it_planned_until_each_is_decided_and_ends_with_the_last() {
    let (run, pages, core) = ready_run().await;
    let live = LiveRuns::default();
    assert_eq!(live.status(&run), None, "not seen");
    assert_eq!(live.owed(&run), 0, "not seen");
    live.dispatch(&request(&run, pages)).await.unwrap();
    assert!(live.ready(&run));
    assert_eq!((live.owed(&run), live.status(&run)), (2, None));
    assert_eq!(live.unfinished(), [run.to_string()]);
    decide(&core, &run, "a").await;
    let next = core.events(&run_cover(&run), 4).await.unwrap();
    live.dispatch(&request(&run, next)).await.unwrap();
    assert_eq!((live.owed(&run), live.status(&run)), (1, None));
    decide(&core, &run, "b").await;
    let last = core.events(&run_cover(&run), 5).await.unwrap();
    live.dispatch(&request(&run, last)).await.unwrap();
    assert_eq!(live.owed(&run), 0);
    assert_eq!(live.status(&run), Some(RunStatus::Complete));
    assert!(live.unfinished().is_empty());
}

#[tokio::test]
async fn test_an_event_handed_again_changes_nothing_because_the_whole_history_is_after_a_restart() {
    let (run, pages, core) = ready_run().await;
    decide(&core, &run, "a").await;
    let all = core.events(&run_cover(&run), 0).await.unwrap();
    let live = LiveRuns::default();
    live.dispatch(&request(&run, pages)).await.unwrap();
    for _ in 0..2 {
        live.dispatch(&request(&run, all.clone())).await.unwrap();
    }
    assert_eq!((live.owed(&run), live.status(&run)), (1, None));
}

#[tokio::test]
async fn test_a_run_whose_stream_is_gone_is_forgotten() {
    let (run, pages, _core) = ready_run().await;
    let live = LiveRuns::default();
    live.dispatch(&request(&run, pages)).await.unwrap();
    live.dispatch(&request(&run, Vec::new())).await.unwrap();
    assert!(live.unfinished().is_empty());
    assert_eq!(live.owed(&run), 0);
}

#[tokio::test]
async fn test_a_run_whose_program_does_not_prepare_ends_failed() {
    let core = new_core().await;
    let run = new_run();
    let plan = PlanRun {
        programs: 1,
        mutants: vec![executable("a")],
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    let failed = cherenkov_run_domain::PreparationFailed {
        program: None,
        reason: "no debugger".to_string(),
    };
    core.report(run_cover(&run), &failed, "failed")
        .await
        .unwrap();
    let live = LiveRuns::default();
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    live.dispatch(&request(&run, pages)).await.unwrap();
    assert_eq!(live.status(&run), Some(RunStatus::Failed));
    assert!(!live.ready(&run));
}
