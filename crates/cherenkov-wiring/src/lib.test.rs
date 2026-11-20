use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use angzarr_embedded::OnRetryable;
use angzarr_ports::memory::MemoryStore;
use cherenkov_core::operator::catalog;
use cherenkov_core::plan::plan_file;
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_run_application::covers::run_cover;
use cherenkov_run_application::layer::{JsonLines, Settings};
use cherenkov_run_domain::PlanRun;
use cherenkov_run_ports::Edge;
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
    let core = core(MemoryStore::new(), OnRetryable::default())
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

#[test]
fn a_dry_run_plans_and_does_not_finish() {
    let settings = Settings {
        root: ".".to_string(),
        language: "rust".to_string(),
        operators: "default".to_string(),
        jobs: 1,
    };
    let mut out = JsonLines(Vec::new());
    let status = run(MemoryStore::new(), &plans(), &settings, None, &mut out).unwrap();
    assert_eq!(status, None);
    let text = String::from_utf8(out.0).unwrap();
    assert!(text.starts_with(r#"{"event":"run.started""#), "{text}");
    assert!(text.contains(r#""event":"mutant.planned""#), "{text}");
}
