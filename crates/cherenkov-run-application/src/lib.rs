//! A mutation-test run, event-sourced on angzarr-embedded: the
//! application over the run domain (`cherenkov-run-domain`), its messages
//! (`cherenkov-run-wire`) and any store with angzarr's ports, served by
//! angzarr's bus ([`host`]).
//!
//! A run is one stream of the run aggregate and each of its mutants one of
//! the mutant aggregate. Planning the run records its start and its
//! mutants; each test program's preparation is told to the run, which
//! judges it and, with the last, is ready. Then, on the bus: the fan-out
//! policy ([`policies`]) plans each mutant on its own stream; the runner
//! ([`runner`]), a projector with an instance for each worker, runs each
//! planned mutant program by program and tells its stream what each made of
//! it, and the mutant decides its verdict; the fan-in policy tells each
//! mutant's run, whose last decision finishes it. The live runs read model
//! ([`live`]) says when a run has ended. A reader follows the run's stream
//! and prints each event as one JSON document per line. A dry run plans the
//! run and runs nothing. [`layer::run`] runs one end to end.

pub mod core;
pub mod error;
pub mod follow;
pub mod host;
pub mod layer;
pub mod live;
pub mod policies;
pub mod programs;
pub mod render;
pub mod runner;
pub mod workers;

#[cfg(test)]
#[path = "plans.test.rs"]
mod test_plans;

#[cfg(test)]
#[path = "restart.test.rs"]
mod restart;

#[cfg(test)]
#[path = "yielding.test.rs"]
mod yielding;
