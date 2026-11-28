use angzarr_embedded::v1::{EventBook, EventPage};
use cherenkov_mutation::execution::{RawOutcome, Tier};
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
        selection_why: None,
        index: 0,
        baseline: Some(observation::encode(&RawOutcome::Exited {
            code: 0,
            tier: Tier::P,
            wall: std::time::Duration::from_millis(1),
        })),
        baseline_failures: Vec::new(),
        drives: None,
        baseline_output_tail: None,
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
        decision: None,
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
        why: None,
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

#[tokio::test]
async fn test_the_programs_a_run_prepared_are_known_once_it_is_ready_and_not_before() {
    let (run, pages, _core) = ready_run().await;
    let live = LiveRuns::default();
    assert!(live.programs(&run).is_none(), "not seen");
    // The run's start and its mutants planned: no program prepared yet.
    live.dispatch(&request(&run, pages[..2].to_vec()))
        .await
        .unwrap();
    assert!(live.programs(&run).is_none(), "not ready");
    // The preparation, and readiness.
    live.dispatch(&request(&run, pages[2..].to_vec()))
        .await
        .unwrap();
    let programs = live.programs(&run).unwrap();
    assert_eq!(programs.len(), 1);
    assert_eq!(programs.name(0), Some("calc-1"));
    // Handed again: still the one program.
    live.dispatch(&request(&run, pages)).await.unwrap();
    assert_eq!(live.programs(&run).unwrap().len(), 1);
}

/// A reader that fails every dispatch.
struct Failing;

impl ProjectorHandler for Failing {
    fn name(&self) -> &str {
        "failing"
    }

    async fn dispatch(&self, _: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        Err(CodedError::unhandled())
    }
}

/// A finished run's whole stream, and the same run's stream before its end.
async fn finished_run() -> (Uuid, Vec<EventPage>, Vec<EventPage>) {
    let (run, _, core) = ready_run().await;
    decide(&core, &run, "a").await;
    decide(&core, &run, "b").await;
    let all = core.events(&run_cover(&run), 0).await.unwrap();
    let before = core.events(&run_cover(&run), 0).await.unwrap();
    let before = before[..before.len() - 1].to_vec();
    (run, all, before)
}

#[tokio::test]
async fn test_a_run_has_not_ended_for_the_bus_until_every_watched_reader_has_handled_its_finish() {
    let (run, all, before) = finished_run().await;
    let live = LiveRuns::default();
    let slow = crate::host::watching(&live, crate::testing::silent());
    let quick = crate::host::watching(&live, crate::testing::silent());
    live.dispatch(&request(&run, all.clone())).await.unwrap();
    assert_eq!(live.status(&run), Some(RunStatus::Complete));
    assert!(!live.ended(&run), "no reader has handled the finish");
    quick.dispatch(&request(&run, all.clone())).await.unwrap();
    assert!(!live.ended(&run), "one reader has not");
    slow.dispatch(&request(&run, before)).await.unwrap();
    assert!(!live.ended(&run), "the events before the finish are not it");
    slow.dispatch(&request(&run, all)).await.unwrap();
    assert!(live.ended(&run));
}

#[tokio::test]
async fn test_a_reader_whose_handling_of_the_finish_failed_has_not_handled_it() {
    let (run, all, _) = finished_run().await;
    let live = LiveRuns::default();
    let failing = crate::host::watching(&live, Box::new(Failing));
    live.dispatch(&request(&run, all.clone())).await.unwrap();
    assert!(failing.dispatch(&request(&run, all)).await.is_err());
    assert!(!live.ended(&run));
}

#[tokio::test]
async fn test_a_reader_that_handled_another_runs_finish_has_not_handled_this_one() {
    let (run, all, _) = finished_run().await;
    let (other, other_all, _) = finished_run().await;
    let live = LiveRuns::default();
    let reader = crate::host::watching(&live, crate::testing::silent());
    live.dispatch(&request(&run, all)).await.unwrap();
    reader.dispatch(&request(&other, other_all)).await.unwrap();
    assert!(!live.ended(&run));
}

#[tokio::test]
async fn test_a_run_with_no_watched_readers_has_ended_with_its_finish() {
    let (run, all, _) = finished_run().await;
    let live = LiveRuns::default();
    assert!(!live.ended(&run), "not seen");
    live.dispatch(&request(&run, all)).await.unwrap();
    assert!(live.ended(&run));
}
