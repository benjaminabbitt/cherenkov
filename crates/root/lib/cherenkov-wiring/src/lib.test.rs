use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::{OnRetryable, ProjectorHandler};
use angzarr_ports::memory::MemoryStore;
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::plan_file;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_application::covers::run_cover;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::Settings;
use cherenkov_run_domain::events::{RunEvent, run_event};
use cherenkov_run_domain::{OperatorSet, PlanRun};
use cherenkov_run_ports::Edge;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

use super::*;

/// The mutants of `fn a(x: i32) -> i32 { x + 1 }`.
fn plans() -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = "fn a(x: i32) -> i32 { x + 1 }\n";
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
        .unwrap();
    let plans = plan_file(&file, &profile, &catalog()).plans;
    assert!(!plans.is_empty());
    plans
}

fn now_seconds() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

#[test]
fn a_sample_is_at_the_instant_it_is_taken_under_a_command_id_of_its_own() {
    let before = now_seconds();
    let (first, second) = (sample(), sample());
    assert!(first.now.seconds >= before && first.now.seconds <= now_seconds());
    assert_ne!(first.command_id, second.command_id);
}

#[test]
fn each_run_has_an_id_of_its_own() {
    assert_ne!(new_run(), new_run());
}

#[tokio::test]
async fn the_core_stamps_what_it_records_at_the_clock() {
    let before = now_seconds();
    let core = core(
        MemoryStore::new(),
        OnRetryable::default(),
        &Sampling::production(),
    )
    .await
    .unwrap();
    let run = new_run();
    core.command(run_cover(&run), &PlanRun::default())
        .await
        .unwrap();
    let events = core.events(&run_cover(&run), 0).await.unwrap();
    let at = events[0]
        .meta
        .as_ref()
        .unwrap()
        .recorded_at
        .as_ref()
        .unwrap();
    assert!(at.seconds >= before && at.seconds <= now_seconds());
}

/// An output that keeps the events of the run it is handed.
#[derive(Clone, Default)]
struct Kept(Arc<Mutex<Vec<RunEvent>>>);

impl ProjectorHandler for Kept {
    fn name(&self) -> &str {
        "output"
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let pages = request.events.iter().flat_map(|book| &book.pages);
        self.0.lock().unwrap().extend(pages.filter_map(run_event));
        Ok(ProjectResponse::default())
    }
}

#[test]
fn a_dry_run_plans_and_does_not_finish() {
    let settings = Settings {
        root: ".".to_string(),
        operators: vec![OperatorSet {
            language: "rust".to_string(),
            set: "default".to_string(),
        }],
        jobs: 1,
    };
    let kept = Kept::default();
    let status = run(
        MemoryStore::new(),
        &plans(),
        &settings,
        None,
        Readers::printing(Box::new(kept.clone())),
        Sampling::production(),
    )
    .unwrap();
    assert_eq!(status, None);
    let events = kept.0.lock().unwrap();
    assert!(matches!(events[0], RunEvent::RunStarted(_)), "{events:?}");
    assert!(
        matches!(events[1], RunEvent::MutantsPlanned(_)),
        "{events:?}"
    );
}

#[test]
fn a_run_on_an_injected_sampling_is_named_and_stamped_by_it() {
    let settings = Settings {
        root: ".".to_string(),
        operators: vec![OperatorSet {
            language: "rust".to_string(),
            set: "default".to_string(),
        }],
        jobs: 1,
    };
    let kept = Kept::default();
    let started = start(
        MemoryStore::new(),
        &plans(),
        &settings,
        None,
        Readers::printing(Box::new(kept.clone())),
        crate::testing::sampling(),
    )
    .unwrap();
    assert_eq!(started.run, Uuid::from_bytes([1; 16]));
    let events = kept.0.lock().unwrap();
    let RunEvent::RunStarted(run) = &events[0] else {
        panic!("{events:?}");
    };
    assert!(
        run.started_at
            .as_ref()
            .is_some_and(|at| at.seconds > 1_000_000 && at.seconds < 2_000_000)
    );
}
