use angzarr_embedded::angzarr_ports::memory::MemoryStore;

use super::*;
use crate::core::{new_run, run_cover};
use cherenkov_core::execution::{RawOutcome, Tier};
use cherenkov_run_domain::observation;
use cherenkov_run_domain::{PlanRun, PreparationReported, Selection};

fn preparation() -> PreparationReported {
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

fn plan() -> PlanRun {
    PlanRun {
        mutants: vec![crate::test_plans::executable("a")],
        ..PlanRun::default()
    }
}

#[tokio::test]
async fn test_following_a_stream_reads_each_page_once_in_sequence_order() {
    let core = RunCore::new(MemoryStore::new()).unwrap();
    let run = new_run();
    let mut follow = Follow::new(run_cover(&run));
    assert!(follow.next(&core).await.unwrap().is_empty());
    core.command(run_cover(&run), &plan()).await.unwrap();
    let first: Vec<u64> = follow
        .next(&core)
        .await
        .unwrap()
        .iter()
        .map(|p| p.sequence)
        .collect();
    assert_eq!(first, [0, 1]);
    assert!(follow.next(&core).await.unwrap().is_empty(), "nothing new");
    core.report(run_cover(&run), &preparation(), "p")
        .await
        .unwrap();
    let then: Vec<u64> = follow
        .next(&core)
        .await
        .unwrap()
        .iter()
        .map(|p| p.sequence)
        .collect();
    assert_eq!(then, [2]);
    assert_eq!(follow.cursor(), 3);
}

#[tokio::test]
async fn test_following_one_stream_reads_nothing_of_another() {
    let core = RunCore::new(MemoryStore::new()).unwrap();
    let (ours, theirs) = (new_run(), new_run());
    let mut follow = Follow::new(run_cover(&ours));
    core.command(run_cover(&theirs), &plan()).await.unwrap();
    assert!(follow.next(&core).await.unwrap().is_empty());
}
