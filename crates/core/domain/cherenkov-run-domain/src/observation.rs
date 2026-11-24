//! What an executor observed, between the executor port's types
//! (`cherenkov_mutation::execution::RawOutcome`) and the run layer's
//! (`cherenkov.mutant.v1.Observation`). As the plan's ([`crate::plan`]),
//! one place for both directions, and a value that goes through both comes
//! back equal; a duration travels as `google.protobuf.Duration`, exact to
//! the nanosecond.

use std::time::Duration;

use cherenkov_mutation::execution::{RawOutcome, TestReport, TestResult, Tier as CoreTier};
use prost_types::Duration as WireDuration;

use crate::mutant::{
    Crashed, Exited, KilledBy, NotReached, Observation, ObservationKind, Tier, TimedOut,
    Unsupported, Unviable,
};
use crate::rejection::Unreadable;

/// `raw`, as the run layer records it.
pub fn encode(raw: &RawOutcome) -> Observation {
    Observation {
        kind: match raw {
            RawOutcome::Exited { code, tier, wall } => ObservationKind::Exited(Exited {
                code: *code,
                tier: tier_of(*tier),
                wall: Some(duration(*wall)),
            }),
            RawOutcome::Crashed {
                kind,
                message,
                tier,
                wall,
            } => ObservationKind::Crashed(Crashed {
                kind: kind.clone(),
                message: message.clone(),
                tier: tier_of(*tier),
                wall: Some(duration(*wall)),
            }),
            RawOutcome::TimedOut { tier, wall } => ObservationKind::TimedOut(TimedOut {
                tier: tier_of(*tier),
                wall: Some(duration(*wall)),
            }),
            RawOutcome::NotReached { reason } => ObservationKind::NotReached(NotReached {
                reason: reason.clone(),
            }),
            RawOutcome::Unsupported { reason } => ObservationKind::Unsupported(Unsupported {
                reason: reason.clone(),
            }),
            RawOutcome::Unviable { reason } => ObservationKind::Unviable(Unviable {
                reason: reason.clone(),
            }),
        },
    }
}

/// The failed tests among `tests`, as the run layer records what killed a
/// mutant; a test that passed or was ignored killed nothing.
pub fn killed_by(tests: &[TestReport]) -> Vec<KilledBy> {
    tests
        .iter()
        .filter_map(|report| match &report.result {
            TestResult::Failed(failure) => Some(KilledBy {
                test: report.test.clone(),
                location: failure.location.clone(),
                message: failure.message.clone(),
                got: failure.got.clone(),
                expected: failure.expected.clone(),
            }),
            TestResult::Passed | TestResult::Ignored => None,
        })
        .collect()
}

/// The executor's outcome for `observation`.
pub fn decode(observation: Observation) -> Result<RawOutcome, Unreadable> {
    Ok(match observation.kind {
        ObservationKind::Exited(e) => RawOutcome::Exited {
            code: e.code,
            tier: core_tier(e.tier),
            wall: wall(e.wall, "wall")?,
        },
        ObservationKind::Crashed(c) => RawOutcome::Crashed {
            kind: c.kind,
            message: c.message,
            tier: core_tier(c.tier),
            wall: wall(c.wall, "wall")?,
        },
        ObservationKind::TimedOut(t) => RawOutcome::TimedOut {
            tier: core_tier(t.tier),
            wall: wall(t.wall, "wall")?,
        },
        ObservationKind::NotReached(n) => RawOutcome::NotReached { reason: n.reason },
        ObservationKind::Unsupported(u) => RawOutcome::Unsupported { reason: u.reason },
        ObservationKind::Unviable(u) => RawOutcome::Unviable { reason: u.reason },
    })
}

/// The tier an executor reports, as the run layer records it.
pub fn tier_of(tier: CoreTier) -> Tier {
    match tier {
        CoreTier::A => Tier::A,
        CoreTier::B => Tier::B,
        CoreTier::P => Tier::P,
    }
}

/// The executor's tier for `tier`.
pub fn core_tier(tier: Tier) -> CoreTier {
    match tier {
        Tier::A => CoreTier::A,
        Tier::B => CoreTier::B,
        Tier::P => CoreTier::P,
    }
}

fn duration(wall: Duration) -> WireDuration {
    WireDuration {
        seconds: i64::try_from(wall.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(wall.subsec_nanos()).unwrap_or(0),
    }
}

/// A wall time recorded: set, and not before the start.
fn wall(wall: Option<WireDuration>, field: &'static str) -> Result<Duration, Unreadable> {
    let wall = wall.ok_or(Unreadable::Missing { field })?;
    Ok(Duration::new(
        u64::try_from(wall.seconds).unwrap_or(0),
        u32::try_from(wall.nanos).unwrap_or(0),
    ))
}

#[cfg(test)]
#[path = "observation.test.rs"]
mod tests;
