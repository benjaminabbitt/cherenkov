//! A failure's reason between [`Report`] and the wire's `Diagnostic`.
//!
//! angzarr types a diagnostic's args on the wire (decision 14.112): text,
//! signed and unsigned numbers, and a wrapped cause as a nested diagnostic,
//! an argless one included. angzarr-embedded's converters
//! ([`DiagnosticToWire`], [`WireDiagnosticExt`]) do the typing; this module
//! only crosses between the wire message and the generated domain type the
//! run's events hold.

use angzarr_embedded::v1;
use angzarr_embedded::{DiagnosticToWire, WireDiagnosticExt};
use coded_diagnostic::Report;

use crate::angzarr;
use crate::failure::Failure;

/// `report` as the domain's `Diagnostic`: its code and its typed args.
pub fn encode(report: &Report) -> angzarr::Diagnostic {
    angzarr::Diagnostic::try_from(report.to_wire())
        .expect("a report's wire form sets every arg's value")
}

/// The reason a record holds: `why` read back as a report, or, when the
/// record has none (it was written before reasons were coded), `text`, its
/// old string, as a [`Failure::Recorded`].
pub fn decode(why: Option<&angzarr::Diagnostic>, text: &str) -> Report {
    match why {
        Some(why) => v1::Diagnostic::from(why.clone()).to_report(),
        None => Report::of(&Failure::Recorded {
            reason: text.to_string(),
        }),
    }
}

#[cfg(test)]
#[path = "why.test.rs"]
mod tests;
