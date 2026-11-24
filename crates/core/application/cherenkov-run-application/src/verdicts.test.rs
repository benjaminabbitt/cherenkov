use std::sync::{Arc, Mutex};

use angzarr_embedded::angzarr_router::hpb::ProjectRequest;
use angzarr_embedded::v1::{EventBook, EventPage, Uuid};
use cherenkov_run_domain::{
    DecisionReported, KilledBy, MutantDecided, Outcome, PlanRun, PlannedMutant, Tier, Verdict,
};
use cherenkov_run_ports::Edge;

use super::*;
use crate::covers::run_cover;
use crate::test_plans::{executable, prepared};
use crate::testing::{new_core, new_run};

/// What the file was written with, each time.
#[derive(Clone, Default)]
struct Written {
    writes: Arc<Mutex<Vec<Vec<VerdictLine>>>>,
    broken: bool,
}

impl VerdictFile for Written {
    fn write(&self, lines: &[VerdictLine]) -> Result<(), Report> {
        if self.broken {
            return Err(Report::new("test-disk-full"));
        }
        self.writes.lock().unwrap().push(lines.to_vec());
        Ok(())
    }
}

fn killed_by(test: &str) -> KilledBy {
    KilledBy {
        test: test.to_string(),
        location: None,
        message: String::new(),
        got: None,
        expected: None,
    }
}

/// Mutant `id` decided, killed by `killers` or, with none, survived.
fn decision(run: &Uuid, id: &str, killers: &[&str]) -> DecisionReported {
    let verdict = match killers.is_empty() {
        true => Verdict::Survived,
        false => Verdict::Killed,
    };
    DecisionReported {
        mutant: id.to_string(),
        verdict,
        decision: Some(MutantDecided {
            mutant: Some(PlannedMutant {
                id: id.to_string(),
                file: "src/lib.rs".to_string(),
                line: 4,
                column: 5,
                end_line: 4,
                end_column: 6,
                function: Some("total".to_string()),
                operator: "aor".to_string(),
                description: "`+` becomes `-`".to_string(),
                original: "+".to_string(),
                mutated: Some("-".to_string()),
            }),
            verdict: Some(Outcome {
                verdict,
                tier: Some(Tier::P),
                took_ms: 31,
                tests_run: Vec::new(),
                killed_by: killers.iter().map(|t| killed_by(t)).collect(),
                reason: None,
            }),
            run: Some(cherenkov_run_domain::RunId::from(run.clone())),
            programs: vec!["calc".to_string()],
        }),
    }
}

/// The pages of a run of `b` and `a` (in that order of planning) that
/// decides `b` as killed by `z` and `y` and `z` again, and `a` as survived.
async fn finished_run() -> (Uuid, Vec<EventPage>) {
    let core = new_core().await;
    let run = new_run();
    let plan = PlanRun {
        programs: 1,
        mutants: vec![executable("b"), executable("a")],
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    core.report(run_cover(&run), &prepared(), "program 0")
        .await
        .unwrap();
    for fact in [
        decision(&run, "b", &["z", "y", "z"]),
        decision(&run, "a", &[]),
    ] {
        let id = fact.mutant.clone();
        core.report(run_cover(&run), &fact, &id).await.unwrap();
    }
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    (run, pages)
}

fn request(run: &Uuid, pages: &[EventPage]) -> ProjectRequest {
    ProjectRequest {
        events: Some(EventBook {
            cover: Some(run_cover(run)),
            next_sequence: pages.last().map_or(0, |page| page.sequence + 1),
            pages: pages.to_vec(),
            snapshot: None,
        }),
    }
}

async fn feed(projection: &VerdictProjection, run: &Uuid, pages: &[EventPage]) {
    for page in pages {
        projection
            .dispatch(&request(run, std::slice::from_ref(page)))
            .await
            .unwrap();
    }
}

fn ids(lines: &[VerdictLine]) -> Vec<&str> {
    lines.iter().map(|line| line.id.as_str()).collect()
}

#[tokio::test]
async fn test_the_file_is_written_when_the_run_finishes_sorted_by_id_with_its_killers_sorted() {
    let (run, pages) = finished_run().await;
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    feed(&projection, &run, &pages).await;
    let writes = file.writes.lock().unwrap();
    assert_eq!(writes.len(), 1, "once, at the run's end");
    assert_eq!(ids(&writes[0]), ["a", "b"]);
    let (a, b) = (&writes[0][0], &writes[0][1]);
    assert_eq!(a.verdict, i32::from(Verdict::Survived));
    assert!(a.killed_by.is_empty());
    assert_eq!(b.verdict, i32::from(Verdict::Killed));
    assert_eq!(b.killed_by, ["y", "z"], "sorted, each once");
    assert_eq!(
        (b.file.as_str(), b.line, b.column, b.operator.as_str()),
        ("src/lib.rs", 4, 5, "aor")
    );
    assert_eq!(b.function.as_deref(), Some("total"));
    assert_eq!(
        (b.original.as_str(), b.mutated.as_deref()),
        ("+", Some("-"))
    );
}

#[tokio::test]
async fn test_nothing_is_written_before_the_run_finishes() {
    let (run, pages) = finished_run().await;
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    feed(&projection, &run, &pages[..pages.len() - 1]).await;
    assert!(file.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_a_run_handed_again_writes_the_same_file_again() {
    let (run, pages) = finished_run().await;
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    feed(&projection, &run, &pages).await;
    feed(&projection, &run, &pages).await;
    let writes = file.writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        writes[0], writes[1],
        "one line for each mutant, however often"
    );
}

#[tokio::test]
async fn test_a_run_that_failed_writes_nothing_so_the_last_good_file_stays() {
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
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    feed(&projection, &run, &pages).await;
    assert!(file.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_a_decision_holding_no_mutant_or_no_outcome_is_not_a_line() {
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
    let nothing = |id: &str, decision| DecisionReported {
        mutant: id.to_string(),
        verdict: Verdict::Survived,
        decision,
    };
    let no_outcome = MutantDecided {
        verdict: None,
        ..decision(&run, "b", &[]).decision.unwrap()
    };
    for fact in [nothing("a", None), nothing("b", Some(no_outcome))] {
        let id = fact.mutant.clone();
        core.report(run_cover(&run), &fact, &id).await.unwrap();
    }
    let pages = core.events(&run_cover(&run), 0).await.unwrap();
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    feed(&projection, &run, &pages).await;
    assert_eq!(file.writes.lock().unwrap().as_slice(), [Vec::new()]);
}

#[tokio::test]
async fn test_a_file_that_cannot_be_written_is_a_failure_the_bus_serves_again() {
    let (run, pages) = finished_run().await;
    let file = Written {
        broken: true,
        ..Written::default()
    };
    let projection = VerdictProjection::new(Arc::new(file));
    let (last, before) = pages.split_last().unwrap();
    feed(&projection, &run, before).await;
    let error = projection
        .dispatch(&request(&run, std::slice::from_ref(last)))
        .await
        .unwrap_err();
    assert_eq!(
        angzarr_embedded::class(error.grpc as i32),
        angzarr_embedded::Class::BugRetryable
    );
}

#[tokio::test]
async fn test_a_stream_with_no_root_is_passed_over() {
    let file = Written::default();
    let projection = VerdictProjection::new(Arc::new(file.clone()));
    let rootless = ProjectRequest {
        events: Some(EventBook::default()),
    };
    projection.dispatch(&rootless).await.unwrap();
    projection
        .dispatch(&ProjectRequest { events: None })
        .await
        .unwrap();
    assert!(file.writes.lock().unwrap().is_empty());
}

#[test]
fn test_the_projection_is_named_for_its_subscription() {
    let projection = VerdictProjection::new(Arc::new(Written::default()));
    assert_eq!(projection.name(), VERDICTS);
}
