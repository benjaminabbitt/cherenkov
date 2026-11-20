use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::v1::{Cover, EventBook, EventPage};
use cherenkov_core::execution::{Budget, ExecError, MutantExecutor, RawOutcome, Tier};

use super::*;
use crate::core::{mutant_cover, new_run, run_cover};
use crate::test_plans::plans;
use crate::workers::{self, Killer};
use cherenkov_run_domain::{PlanMutant, PlanRun, ProgramRunReported, Verdict};

type Core = RunCore<MemoryStore>;

/// Kills `a`'s mutant; `b`'s survives. Counts its runs.
struct Fake {
    runs: Arc<AtomicUsize>,
}

fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(1),
    }
}

impl MutantExecutor for Fake {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<RawOutcome, ExecError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101),
            _ => exited(0),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(exited(0))
    }

    fn describe(&self) -> Option<String> {
        Some("calc-test".to_string())
    }
}

fn explain(_: &str) -> Vec<Killer> {
    Vec::new()
}

fn request(stream: &Cover, page: EventPage) -> ProjectRequest {
    ProjectRequest {
        events: Some(EventBook {
            cover: Some(stream.clone()),
            next_sequence: page.sequence + 1,
            pages: vec![page],
            snapshot: None,
        }),
    }
}

/// A run of the two plans, planned and ready (its `programs` programs
/// prepared), and a projector on one worker that holds nothing but the
/// store. The planned mutants are on their streams.
async fn projector(
    runs: &Arc<AtomicUsize>,
    programs: usize,
) -> (Arc<Core>, Uuid, Runner<MemoryStore>) {
    let plans = plans();
    let core = Arc::new(RunCore::new(MemoryStore::new()).unwrap());
    let run = new_run();
    let plan = PlanRun {
        programs: u32::try_from(programs).unwrap(),
        mutants: crate::test_plans::executables(&plans),
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    let mut set: Vec<Box<dyn MutantExecutor + Send>> = (0..programs)
        .map(|_| Box::new(Fake { runs: runs.clone() }) as Box<dyn MutantExecutor + Send>)
        .collect();
    let mut prepared = Vec::new();
    workers::prepare(&plans, &mut set, 1, explain, &mut |p| {
        prepared.push(p.unwrap())
    });
    for preparation in prepared {
        let external_id = format!("program {}", preparation.program);
        let fact = crate::programs::report(preparation, &plans);
        core.report(run_cover(&run), &fact, &external_id)
            .await
            .unwrap();
    }
    crate::test_plans::serve_policies(&core).await;
    let projector = Runner::new(core.clone(), explain, vec![Worker::new(set)]);
    (core, run, projector)
}

/// Hand `projector` the event that planned mutant `id`, as the bus does.
async fn handed_planned(projector: &Runner<MemoryStore>, core: &Core, id: &str) {
    let planned = pages(core, id).await;
    projector
        .dispatch(&request(&mutant_cover(id).unwrap(), planned[0].clone()))
        .await
        .unwrap();
}

async fn pages(core: &Core, id: &str) -> Vec<EventPage> {
    core.events(&mutant_cover(id).unwrap(), 0).await.unwrap()
}

fn decided(pages: &[EventPage]) -> Vec<cherenkov_run_domain::MutantDecided> {
    pages
        .iter()
        .filter_map(|p| match mutant_event(p) {
            Some(MutantEvent::MutantDecided(d)) => Some(d),
            _ => None,
        })
        .collect()
}

fn ran(pages: &[EventPage]) -> Vec<cherenkov_run_domain::ProgramRan> {
    pages
        .iter()
        .filter_map(|p| match mutant_event(p) {
            Some(MutantEvent::ProgramRan(ran)) => Some(ran),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn test_a_planned_mutant_is_run_and_what_the_program_made_of_it_recorded_for_its_run() {
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, run, projector) = projector(&runs, 1).await;
    let a = plans()[0].id.0.clone();
    handed_planned(&projector, &core, &a).await;
    let pages = pages(&core, &a).await;
    let told = ran(&pages);
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].run.as_ref(), Some(&RunId::from(run.clone())));
    // The mutant decided from it: the runner decided nothing.
    let decided = decided(&pages);
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].run.as_ref(), Some(&RunId::from(run.clone())));
    assert_eq!(
        decided[0].verdict.as_ref().unwrap().verdict,
        Verdict::Killed
    );
    assert_eq!(decided[0].programs, ["calc-test"]);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_a_mutant_a_program_kills_is_not_run_in_the_programs_after() {
    // The runner reads from what the mutant recorded that it is decided,
    // and runs no further program: two programs, one run.
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, _, projector) = projector(&runs, 2).await;
    let a = plans()[0].id.0.clone();
    handed_planned(&projector, &core, &a).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(ran(&pages(&core, &a).await).len(), 1);
}

#[tokio::test]
async fn test_a_mutant_no_program_kills_is_run_in_each_and_survives_when_the_last_has() {
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, _, projector) = projector(&runs, 2).await;
    let b = plans()[1].id.0.clone();
    handed_planned(&projector, &core, &b).await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    let pages = pages(&core, &b).await;
    assert_eq!(ran(&pages).len(), 2);
    let decided = decided(&pages);
    assert_eq!(decided.len(), 1);
    assert_eq!(
        decided[0].verdict.as_ref().unwrap().verdict,
        Verdict::Survived
    );
}

#[tokio::test]
async fn test_a_mutant_handed_again_after_it_is_decided_runs_nothing() {
    // Delivery is at least once: a mutant handed again after a crash that
    // fell between its fact and its position is decided, and is not run.
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, _, projector) = projector(&runs, 1).await;
    let a = plans()[0].id.0.clone();
    for _ in 0..2 {
        handed_planned(&projector, &core, &a).await;
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(decided(&pages(&core, &a).await).len(), 1);
}

#[tokio::test]
async fn test_a_mutant_handed_again_half_run_is_run_in_the_programs_it_has_no_fact_of() {
    // The crash fell after the first program's fact: only the second runs.
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, run, projector) = projector(&runs, 2).await;
    let b = plans()[1].id.0.clone();
    let first = ProgramRunReported {
        run: Some(RunId::from(run)),
        ran: Some(cherenkov_run_domain::ProgramRun {
            program: 0,
            program_name: "calc-test".to_string(),
            took_ms: 1,
            tests: Vec::new(),
            result: cherenkov_run_domain::ProgramRunResult::Ran(cherenkov_run_domain::Ran {
                observation: Some(cherenkov_run_domain::observation::encode(&exited(0))),
                killed_by: Vec::new(),
            }),
        }),
    };
    let planned = pages(&core, &b).await;
    let id = format!(
        "{RUNNER} {} {} 0",
        mutant_root_text(&b),
        planned[0].sequence
    );
    core.report(mutant_cover(&b).unwrap(), &first, &id)
        .await
        .unwrap();
    handed_planned(&projector, &core, &b).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the second program only");
    let pages = pages(&core, &b).await;
    assert_eq!(ran(&pages).len(), 2);
    assert_eq!(decided(&pages).len(), 1);
}

/// A mutant's root, as the runner puts it in the ids of its facts.
fn mutant_root_text(id: &str) -> String {
    cherenkov_run_domain::mutant_root(id).unwrap().to_string()
}

#[tokio::test]
async fn test_a_decision_is_passed_over() {
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, _, projector) = projector(&runs, 1).await;
    let a = plans()[0].id.0.clone();
    let planned = pages(&core, &a).await;
    projector
        .dispatch(&request(&mutant_cover(&a).unwrap(), planned[0].clone()))
        .await
        .unwrap();
    let decision = pages(&core, &a).await[1].clone();
    projector
        .dispatch(&request(&mutant_cover(&a).unwrap(), decision))
        .await
        .unwrap();
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "only the planned mutant ran"
    );
}

#[tokio::test]
async fn test_a_mutant_planned_for_a_run_that_prepared_no_program_is_a_bug() {
    // A mutant appears on its stream only once its run is ready; one that
    // does not has nothing to run in.
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, _, projector) = projector(&runs, 1).await;
    let stray = new_run();
    let plan = PlanRun {
        mutants: vec![crate::test_plans::executable("stray")],
        ..PlanRun::default()
    };
    core.command(run_cover(&stray), &plan).await.unwrap();
    // The stray run never became ready: ask for its mutant by hand.
    let planned = PlanMutant {
        run: Some(RunId::from(stray)),
        mutant: Some(crate::test_plans::executables(&plans()).remove(0)),
        programs: 1,
    };
    core.command(mutant_cover("stray").unwrap(), &planned)
        .await
        .unwrap();
    let page = pages(&core, "stray").await.remove(0);
    let error = projector
        .dispatch(&request(&mutant_cover("stray").unwrap(), page))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("prepared no test program"),
        "{error}"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_a_ready_run_hands_its_mutants_on_with_their_plans_and_not_before() {
    // No `MutantPlanned` is on any mutant's stream before its run's
    // `RunReady`: a mutant exists only once its programs do.
    let core = Arc::new(RunCore::new(MemoryStore::new()).unwrap());
    let run = new_run();
    let plan = PlanRun {
        programs: 1,
        mutants: crate::test_plans::executables(&plans()),
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    for p in plans() {
        assert!(pages(&core, &p.id.0).await.is_empty(), "before it is ready");
    }
    let runs = Arc::new(AtomicUsize::new(0));
    let mut set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Fake { runs })];
    let mut prepared = Vec::new();
    workers::prepare(&plans(), &mut set, 1, explain, &mut |p| {
        prepared.push(p.unwrap())
    });
    let fact = crate::programs::report(prepared.remove(0), &plans());
    core.report(run_cover(&run), &fact, "program 0")
        .await
        .unwrap();
    crate::test_plans::serve_policies(&core).await;
    for p in plans() {
        let planned = pages(&core, &p.id.0).await;
        let Some(MutantEvent::MutantPlanned(planned)) = mutant_event(&planned[0]) else {
            panic!("{planned:?}");
        };
        let decoded = cherenkov_run_domain::plan::decode(planned.mutant.unwrap().plan.unwrap());
        assert_eq!(decoded, Ok(p));
    }
}
