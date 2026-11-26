use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use angzarr_embedded::v1::{Cover, EventBook, EventPage};
use cherenkov_mutation::execution::{Budget, Observed, RawOutcome, Tier};
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};

use super::*;
use crate::covers::{mutant_cover, run_cover};
use crate::pool::Pool;
use crate::test_plans::plans;
use crate::testing::{Core, new_core, new_run};
use crate::workers;
use crate::workers::Worker;
use cherenkov_run_domain::{PlanMutant, PlanRun, ProgramRunReported, Verdict};
use cherenkov_run_ports::Edge;

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
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101).into(),
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }

    fn describe(&self) -> Option<String> {
        Some("calc-test".to_string())
    }
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
async fn projector(runs: &Arc<AtomicUsize>, programs: usize) -> (Arc<Core>, Uuid, Runner<Core>) {
    let plans = plans();
    let core = Arc::new(new_core().await);
    let run = new_run();
    let plan = PlanRun {
        programs: u32::try_from(programs).unwrap(),
        mutants: crate::test_plans::executables(&plans),
        ..PlanRun::default()
    };
    core.command(run_cover(&run), &plan).await.unwrap();
    let set: Vec<Box<dyn MutantExecutor + Send>> = (0..programs)
        .map(|_| Box::new(Fake { runs: runs.clone() }) as Box<dyn MutantExecutor + Send>)
        .collect();
    let mut prepared = Vec::new();
    let set = workers::prepare(&plans, set, 1, &mut |p| prepared.push(p.unwrap()));
    for preparation in prepared {
        let external_id = format!("program {}", preparation.program);
        let fact = crate::programs::report(preparation, &plans);
        core.report(run_cover(&run), &fact, &external_id)
            .await
            .unwrap();
    }
    crate::test_plans::serve_policies(&core).await;
    let projector = Runner::new(core.clone(), Pool::new(vec![Worker::new(set)]));
    (core, run, projector)
}

/// Hand `projector` the event that planned mutant `id`, as the bus does.
async fn handed_planned(projector: &Runner<Core>, core: &Core, id: &str) {
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
        error
            .to_string()
            .contains("cherenkov-run-application-none-prepared"),
        "{error}"
    );
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_a_ready_run_hands_its_mutants_on_with_their_plans_and_not_before() {
    // No `MutantPlanned` is on any mutant's stream before its run's
    // `RunReady`: a mutant exists only once its programs do.
    let core = Arc::new(new_core().await);
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
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Fake { runs })];
    let mut prepared = Vec::new();
    let _set = workers::prepare(&plans(), set, 1, &mut |p| prepared.push(p.unwrap()));
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

/// The core, counting the reads of runs' streams.
struct Counting {
    inner: Arc<Core>,
    run_reads: AtomicUsize,
}

impl Edge for Counting {
    async fn handle(
        &self,
        book: angzarr_embedded::v1::CommandBook,
    ) -> Result<cherenkov_run_ports::Reported, cherenkov_run_ports::EdgeError> {
        self.inner.handle(book).await
    }

    async fn events(
        &self,
        stream: &Cover,
        from: u64,
    ) -> Result<Vec<EventPage>, cherenkov_run_ports::EdgeError> {
        if stream.domain == cherenkov_run_domain::RUN_DOMAIN {
            self.run_reads.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.events(stream, from).await
    }

    fn next_commit(&self) -> angzarr_embedded::angzarr_ports::CommitWait {
        self.inner.next_commit()
    }
}

/// Run mutant `a` of a ready run on a runner over a new core (counted),
/// which knows `live` as it is given `run`'s events; how many times the
/// run's stream was read.
async fn run_a(known: Known) -> usize {
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, run, _) = projector(&runs, 1).await;
    let counting = Arc::new(Counting {
        inner: core.clone(),
        run_reads: AtomicUsize::new(0),
    });
    let set: Vec<Box<dyn MutantExecutor + Send>> = vec![Box::new(Fake { runs })];
    let mut runner = Runner::new(counting.clone(), Pool::new(vec![Worker::new(set)]));
    if let Known::Nothing | Known::Run = known {
        let live = LiveRuns::default();
        if let Known::Run = known {
            let seen = core.events(&run_cover(&run), 0).await.unwrap();
            let book = EventBook {
                cover: Some(run_cover(&run)),
                next_sequence: seen.len() as u64,
                pages: seen,
                snapshot: None,
            };
            live.dispatch(&ProjectRequest { events: Some(book) })
                .await
                .unwrap();
        }
        runner = runner.knowing(live);
    }
    let a = plans()[0].id.0.clone();
    let planned = pages(&core, &a).await;
    runner
        .dispatch(&request(&mutant_cover(&a).unwrap(), planned[0].clone()))
        .await
        .unwrap();
    assert_eq!(decided(&pages(&core, &a).await).len(), 1, "it ran");
    counting.run_reads.load(Ordering::SeqCst)
}

/// What the live runs model a runner is given has seen.
#[derive(Clone, Copy)]
enum Known {
    /// No model at all.
    Unasked,
    /// A model that has seen nothing.
    Nothing,
    /// A model that has seen the run ready.
    Run,
}

#[tokio::test]
async fn test_a_runner_reads_what_a_run_prepared_from_the_live_runs_model_when_it_knows_it() {
    assert_eq!(run_a(Known::Unasked).await, 1, "from the run's stream");
    assert_eq!(run_a(Known::Nothing).await, 1, "a model that knows nothing");
    assert_eq!(run_a(Known::Run).await, 0, "from the model");
}

#[tokio::test]
async fn test_a_mutant_planned_for_a_run_that_has_ended_is_not_run() {
    // The runner has no live runs model here, so it learns that the run
    // ended from the run's own stream: the case of a mutant a crashed
    // process left planned for a run the next one abandoned.
    let runs = Arc::new(AtomicUsize::new(0));
    let (core, run, projector) = projector(&runs, 1).await;
    let abandoned = cherenkov_run_domain::Abandoned {
        reason: "the plan changed".to_string(),
        why: None,
    };
    core.report(run_cover(&run), &abandoned, "abandoned")
        .await
        .unwrap();
    let a = plans()[0].id.0.clone();
    handed_planned(&projector, &core, &a).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0, "nothing ran for it");
    assert!(ran(&pages(&core, &a).await).is_empty());
}
