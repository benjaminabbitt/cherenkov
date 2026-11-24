//! angzarr's `Diagnostic` as the run layers hold it: a code and its args.
//!
//! The domain generator refuses angzarr-proto's own message, whose map is a
//! `HashMap`, where it requires a `BTreeMap`; it is written here, exactly as
//! the generator writes a message with the same fields, until angzarr-proto
//! builds its maps as `BTreeMap`.

use std::collections::BTreeMap;

use angzarr_embedded::WireError;

/// A coded reason: its key and its named data, never English.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub args: BTreeMap<String, String>,
}

impl From<Diagnostic> for angzarr_embedded::v1::Diagnostic {
    fn from(value: Diagnostic) -> Self {
        angzarr_embedded::v1::Diagnostic {
            code: value.code,
            args: value.args.into_iter().collect(),
        }
    }
}

impl TryFrom<angzarr_embedded::v1::Diagnostic> for Diagnostic {
    type Error = WireError;

    fn try_from(value: angzarr_embedded::v1::Diagnostic) -> Result<Self, Self::Error> {
        Ok(Diagnostic {
            code: value.code,
            args: value.args.into_iter().collect(),
        })
    }
}
