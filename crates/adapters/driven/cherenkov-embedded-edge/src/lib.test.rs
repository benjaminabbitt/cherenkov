use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::angzarr_bus::{Credit, Scope, Subscription};
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::coded_diagnostic::Report;
use angzarr_embedded::testing::Samples;
use angzarr_embedded::v1::{Cover, Uuid};
use angzarr_embedded::{BusPolicy, ProjectorHandler, Registry};
use cherenkov_run_domain::{ExecutableMutant, PlanRun, PlannedMutant, RUN_DOMAIN, register};
use cherenkov_run_ports::{Delivery, Edge, Listing, Recovery, Serving};

use super::*;

type Edged = EmbeddedEdge<MemoryStore>;

/// An edge on the run domain over an empty store, sampled from `Samples`:
/// the n-th call is at second n.
async fn edge() -> Edged {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let samples = Mutex::new(Samples::starting_at(1000));
    EmbeddedEdge::start(
        MemoryStore::new(),
        registry,
        OnRetryable::default(),
        move || samples.lock().unwrap().next_sample(),
        Arc::new(TokioWait),
    )
    .await
}

fn run() -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: Some(Uuid::from_bytes([7; 16])),
        edition: None,
    }
}

#[tokio::test]
async fn a_command_is_recorded_and_the_stream_read_back() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let events = edge.events(&run(), 0).await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(edge.events(&run(), 1).await.unwrap().len(), 2);
}

#[tokio::test]
async fn what_a_call_records_is_stamped_at_the_roots_sample() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let events = edge.events(&run(), 0).await.unwrap();
    let at = events[0]
        .meta
        .as_ref()
        .unwrap()
        .recorded_at
        .as_ref()
        .unwrap();
    assert_eq!(at.seconds, 1001);
}

#[tokio::test]
async fn a_fact_reported_twice_under_one_id_is_a_duplicate() {
    use cherenkov_run_domain::PreparationFailed;
    let edge = edge().await;
    let plan = PlanRun {
        programs: 1,
        mutants: vec![ExecutableMutant {
            mutant: Some(PlannedMutant {
                id: "m".to_string(),
                ..PlannedMutant::default()
            }),
            plan: None,
        }],
        ..PlanRun::default()
    };
    edge.command(run(), &plan).await.unwrap();
    let fact = PreparationFailed {
        program: Some("p".to_string()),
        reason: "r".to_string(),
        why: None,
    };
    let first = edge.report(run(), &fact, "one").await.unwrap();
    assert_eq!(first.delivery, Delivery::Applied);
    assert!(!first.events.is_empty());
    let again = edge.report(run(), &fact, "one").await.unwrap();
    assert_eq!(again.delivery, Delivery::Duplicate);
    assert_eq!(again.events.len(), first.events.len());
}

#[tokio::test]
async fn a_refused_command_is_a_business_failure() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let again = edge.command(run(), &PlanRun::default()).await.unwrap_err();
    assert_eq!(again.recovery, Recovery::Business);
    assert!(!again.retryable());
}

#[tokio::test]
async fn the_next_commit_resolves_after_a_call() {
    let edge = edge().await;
    let commit = edge.next_commit();
    edge.command(run(), &PlanRun::default()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), commit)
        .await
        .unwrap();
}

struct Nothing;

impl ProjectorHandler for Nothing {
    fn name(&self) -> &str {
        "nothing"
    }

    async fn dispatch(&self, _: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        Ok(ProjectResponse::default())
    }
}

fn nothing() -> Subscription<Box<dyn DynProjectorHandler>, BusPolicy> {
    Subscription::projector(
        Scope::domain(RUN_DOMAIN),
        Credit::new(std::num::NonZeroUsize::MIN),
        Box::new(Nothing),
    )
}

#[tokio::test]
async fn serving_nothing_returns_when_told_to_stop() {
    let edge = edge().await;
    edge.serve(vec![nothing()], &mut |_| false).await.unwrap();
}

#[tokio::test]
async fn two_subscriptions_of_one_name_are_misconfigured() {
    let edge = edge().await;
    let both = vec![nothing(), nothing()];
    assert!(edge.serve(both, &mut |_| false).await.is_err());
}

#[tokio::test]
async fn a_stream_with_no_root_cannot_be_read() {
    let edge = edge().await;
    let rootless = Cover {
        root: None,
        ..run()
    };
    let error = edge.events(&rootless, 0).await.unwrap_err();
    assert_eq!(error.recovery, Recovery::Invalid);
}

#[test]
fn a_failure_is_captured_with_its_code_and_its_recovery() {
    let error = failed(&Report::new("angzarr-x"), Recovery::Retry);
    assert_eq!(error.cause, Report::new("angzarr-x"));
    assert!(error.retryable());
}

#[tokio::test]
async fn events_are_read_from_the_sequence_asked() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let from: Vec<u64> = edge
        .events(&run(), 2)
        .await
        .unwrap()
        .iter()
        .map(|p| p.sequence)
        .collect();
    assert_eq!(from, [2]);
}

#[tokio::test]
async fn the_streams_of_a_domain_are_listed_with_their_heads() {
    let edge = edge().await;
    assert!(edge.streams(RUN_DOMAIN).await.unwrap().is_empty());
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let listed = edge.streams(RUN_DOMAIN).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].cover, run());
    assert_eq!(listed[0].head, 3);
    assert!(edge.streams("another-domain").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_window_of_a_stream_is_the_pages_between_two_sequences() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let all = edge.events(&run(), 0).await.unwrap();
    let window = edge.window(&run(), 1, 2).await.unwrap();
    assert_eq!(window.len(), 1);
    assert_eq!(window[0].sequence, all[1].sequence);
    assert!(edge.window(&run(), 3, 9).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_stream_is_purged_when_it_is_still_as_long_as_the_caller_read_it() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let head = edge.streams(RUN_DOMAIN).await.unwrap()[0].head;
    let removed = edge.purge(&run(), head).await.unwrap();
    assert_eq!(removed, head);
    assert!(edge.events(&run(), 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_purge_of_a_stream_written_since_it_was_read_is_refused_and_removes_nothing() {
    let edge = edge().await;
    edge.command(run(), &PlanRun::default()).await.unwrap();
    let head = edge.streams(RUN_DOMAIN).await.unwrap()[0].head;
    let error = edge.purge(&run(), head - 1).await.unwrap_err();
    assert!(error.retryable(), "a lost race is worth reading again");
    assert_eq!(edge.events(&run(), 0).await.unwrap().len() as u64, head);
}
