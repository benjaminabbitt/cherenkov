//! The bus, as the run layer uses it: what is served and by how many. Read
//! it top to bottom for how an application is put on the bus. The root
//! hosts the bus ([`Serving`]: the store, the kernel its handlers' failures
//! and its policies' steps go through, the clock they are stamped by, the
//! timer, and the push and poll that wake it); this says what it serves.
//!
//! - **Subscriptions**, each a name that keys its positions:
//!   - the runners (`runner`), a projector with an instance for each
//!     worker, over the mutants' streams. A stream is served one event at
//!     a time, so a mutant is on one instance at once, and the bus's
//!     instances are the worker pool;
//!   - fan out (`run-mutant`) and fan in (`mutant-run`), policies with one
//!     instance each. A policy's outputs commit with its position through
//!     the store's outbox, so a store must be an outbox, and a policy
//!     cannot be ephemeral;
//!   - live runs (`live-runs`), an ephemeral projector over the runs'
//!     streams: rebuilt from the first event each serve;
//!   - the output (the caller's), an ephemeral projector over the runs'
//!     streams that prints each event it is handed: it reads nothing, for the
//!     run records each decision whole;
//!   - the verdicts (`verdicts`), when the caller has a file for them, an
//!     ephemeral projector that writes it when a run finishes. A dry run
//!     serves these alone, beside the live runs.
//! - **Stopping**: the bus is asked whether to go on each time it is idle,
//!   with what it served since it was last asked ([`Served`]). A held
//!   failure is retried by the bus after the delay the root says.
//!
//! The bus borrows the core it is hosted on, so it lives inside the call
//! that owns them: it cannot be spawned, and dropping the future is a
//! crash the next serve recovers from, which is what the restart test
//! does.

use std::num::NonZeroUsize;
use std::sync::Arc;

use angzarr_embedded::angzarr_bus::{Credit, Scope, Served, Subscription};
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use angzarr_embedded::angzarr_router::router::DynProjectorHandler;
use angzarr_embedded::v1::Uuid;
use angzarr_embedded::{BusPolicy, ProjectorHandler};
use cherenkov_run_domain::events::{RunEvent, run_event};
use cherenkov_run_domain::{MUTANT_DOMAIN, RUN_DOMAIN};
use cherenkov_run_ports::{Serving, Subscriptions};

use crate::error::RunLayerError;
use crate::live::{Handled, LiveRuns};
use crate::policies::{FanIn, FanOut};
use crate::verdicts::{VerdictFile, VerdictProjection};

/// The subscription of a projector over `domain`'s streams, `instances` at
/// once.
fn projector(
    domain: &str,
    instances: NonZeroUsize,
    handler: Box<dyn DynProjectorHandler>,
) -> Subscription<Box<dyn DynProjectorHandler>, BusPolicy> {
    Subscription::projector(Scope::domain(domain), Credit::new(instances), handler)
}

/// The output of a run: a projector over the run domain's streams that
/// prints each event it is handed. The caller's (the CLI's), since what a
/// run prints and where is the program's to say.
pub type Output = Box<dyn DynProjectorHandler>;

/// What reads a run's stream for the caller: its output, and the file its
/// verdicts are written to when it finishes, if there is one.
pub struct Readers {
    pub output: Output,
    pub verdicts: Option<Arc<dyn VerdictFile>>,
    /// The one run they read, when a store holds others: the readers are
    /// ephemeral, handed every run's stream from its first event, and print
    /// and write only this run's.
    pub only: Option<Uuid>,
}

impl Readers {
    /// Readers that print with `output` and write no verdict file.
    pub fn printing(output: Output) -> Readers {
        Readers {
            output,
            verdicts: None,
            only: None,
        }
    }

    /// Readers that print with `output` and, if there is one, write `file`.
    pub fn new(output: Output, verdicts: Option<Arc<dyn VerdictFile>>) -> Readers {
        Readers {
            output,
            verdicts,
            only: None,
        }
    }

    /// The same readers, reading `run` alone.
    pub fn of(self, run: Uuid) -> Readers {
        Readers {
            only: Some(run),
            ..self
        }
    }
}

/// `reader`, handed the stream of `only` alone when there is one.
fn scoped(
    only: &Option<Uuid>,
    reader: Box<dyn DynProjectorHandler>,
) -> Box<dyn DynProjectorHandler> {
    match only {
        Some(run) => Box::new(OfRun {
            run: run.clone(),
            reader,
        }),
        None => reader,
    }
}

/// A reader handed one run's stream and no other.
struct OfRun {
    run: Uuid,
    reader: Box<dyn DynProjectorHandler>,
}

impl ProjectorHandler for OfRun {
    fn name(&self) -> &str {
        DynProjectorHandler::name(&*self.reader)
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let root = request
            .events
            .as_ref()
            .and_then(|book| book.cover.as_ref())
            .and_then(|cover| cover.root.as_ref());
        match root == Some(&self.run) {
            true => DynProjectorHandler::dispatch(&*self.reader, request).await,
            false => Ok(ProjectResponse::default()),
        }
    }
}

/// `reader`, watched: `live` does not call a run ended until the reader
/// has handled the run's `RunFinished` (the handling completed, without
/// failing). The bus asks whether to go on when every subscription has been
/// idle since its last delivery, and a reader idle before the run's last
/// commit counts, so the run's own model having seen the finish is not
/// enough for a stop.
pub fn watching(
    live: &LiveRuns,
    reader: Box<dyn DynProjectorHandler>,
) -> Box<dyn DynProjectorHandler> {
    Box::new(Watched {
        handled: live.watch(),
        reader,
    })
}

/// A reader that records the finishes it has handled.
struct Watched {
    handled: Handled,
    reader: Box<dyn DynProjectorHandler>,
}

impl ProjectorHandler for Watched {
    fn name(&self) -> &str {
        DynProjectorHandler::name(&*self.reader)
    }

    async fn dispatch(&self, request: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        let response = DynProjectorHandler::dispatch(&*self.reader, request).await?;
        if let Some(book) = request.events.as_ref()
            && let Some(root) = book.cover.as_ref().and_then(|cover| cover.root.as_ref())
            && book
                .pages
                .iter()
                .filter_map(run_event)
                .any(|event| matches!(event, RunEvent::RunFinished(_)))
        {
            self.handled.record(root);
        }
        Ok(response)
    }
}

/// The runner and the instances it is subscribed with.
pub type Runners = (Box<dyn DynProjectorHandler>, NonZeroUsize);

/// What the run layer serves: `live` to fold the runs and the `readers`
/// (the output, the verdict file), all ephemeral over the runs' streams;
/// and, for a run that runs its mutants, the runners with `instances` at
/// once and both policies.
pub fn subscriptions(live: &LiveRuns, readers: Readers, runners: Option<Runners>) -> Subscriptions {
    let Readers {
        output,
        verdicts,
        only,
    } = readers;
    let output = watching(live, scoped(&only, output));
    let mut subscriptions = vec![
        projector(RUN_DOMAIN, NonZeroUsize::MIN, Box::new(live.clone())).ephemeral(),
        projector(RUN_DOMAIN, NonZeroUsize::MIN, output).ephemeral(),
    ];
    if let Some(file) = verdicts {
        let projection = watching(live, scoped(&only, Box::new(VerdictProjection::new(file))));
        subscriptions.push(projector(RUN_DOMAIN, NonZeroUsize::MIN, projection).ephemeral());
    }
    if let Some((runner, instances)) = runners {
        subscriptions.extend([
            projector(MUTANT_DOMAIN, instances, runner),
            Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanOut)),
            Subscription::policy(NonZeroUsize::MIN, BusPolicy::new(FanIn)),
        ]);
    }
    subscriptions
}

/// Serve `subscriptions` on the bus `core` hosts until `more` says no.
///
/// `more` is asked each time the bus is idle, with what it served since it
/// was last asked. Dropping the future stops serving where it is, as a
/// crash does: what the stored positions do not cover is handed again to
/// whatever serves next.
pub async fn serve<C: Serving>(
    core: &C,
    subscriptions: Subscriptions,
    mut more: impl FnMut(&Served) -> bool + Send,
) -> Result<(), RunLayerError> {
    core.serve(subscriptions, &mut more)
        .await
        .map_err(RunLayerError::from)
}
