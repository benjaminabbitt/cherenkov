//! The core the application's tests run on: the edge over a store, the
//! domains registered, every call sampled from angzarr's `Samples` (a
//! second apart, each under its own command id), and a run id from a
//! counter. No test here reads a clock or mints an id of its own.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_ports::memory::MemoryStore;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::testing::Samples;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{OnRetryable, Registry};
use cherenkov_embedded_edge::{EmbeddedEdge, RunStore, TokioWait};
use cherenkov_run_domain::events::{RunEvent, run_event};

use crate::host::Output;

/// The core over the in-memory store.
pub type Core = EmbeddedEdge<MemoryStore>;

/// The core over `store`, its kernel retrying as `retry` says.
pub async fn new_core_on<S: RunStore>(store: S, retry: OnRetryable) -> EmbeddedEdge<S> {
    let mut registry = Registry::new();
    cherenkov_run_domain::register(&mut registry).expect("the run layer registers");
    let samples = Mutex::new(Samples::starting_at(1_000_000));
    EmbeddedEdge::start(
        store,
        registry,
        retry,
        move || {
            samples
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .next_sample()
        },
        Arc::new(TokioWait),
    )
    .await
}

/// The core over an empty in-memory store.
pub async fn new_core() -> Core {
    new_core_on(MemoryStore::new(), OnRetryable::default()).await
}

/// A run id no other call of this process has been given.
pub fn new_run() -> Uuid {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0x5a; 16];
    bytes[12..].copy_from_slice(&n.to_be_bytes());
    Uuid::from_bytes(bytes)
}

/// An output that prints nothing: for a test of the bus that reads no
/// document.
pub fn silent() -> Output {
    struct Silent;

    impl ProjectorHandler for Silent {
        fn name(&self) -> &str {
            "output"
        }

        async fn dispatch(&self, _: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
            Ok(ProjectResponse::default())
        }
    }

    Box::new(Silent)
}

/// The name a run's event is printed under (`docs/output-events.md`).
pub fn event_name(event: &RunEvent) -> &'static str {
    match event {
        RunEvent::RunStarted(_) => "run.started",
        RunEvent::MutantsPlanned(_) => "mutant.planned",
        RunEvent::ProgramPrepared(_) => "program.prepared",
        RunEvent::ProgramFailed(_) => "program.failed",
        RunEvent::RunAbandoned(_) => "run.abandoned",
        RunEvent::RunReady(_) => "run.ready",
        RunEvent::DecisionRecorded(_) => "mutant.decided",
        RunEvent::RunFinished(_) => "run.finished",
    }
}

/// An output that names each event of a run it is handed, to `name`.
struct Naming(Box<dyn Fn(&'static str) + Send + Sync>);

impl ProjectorHandler for Naming {
    fn name(&self) -> &str {
        "output"
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let pages = request.events.iter().flat_map(|book| &book.pages);
        for event in pages.filter_map(run_event) {
            (self.0)(event_name(&event));
        }
        Ok(ProjectResponse::default())
    }
}

/// An output that sends the name of each event it is handed to `to`.
pub fn recording_to(to: Sender<&'static str>) -> Output {
    let to = Mutex::new(to);
    Box::new(Naming(Box::new(move |name| {
        let _ = to.lock().unwrap_or_else(PoisonError::into_inner).send(name);
    })))
}

/// An output that keeps the names of the events it is handed, in order.
pub fn recording() -> (Output, Arc<Mutex<Vec<&'static str>>>) {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let keeping = Arc::clone(&kept);
    let output = Naming(Box::new(move |name| {
        keeping
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name);
    }));
    (Box::new(output), kept)
}
