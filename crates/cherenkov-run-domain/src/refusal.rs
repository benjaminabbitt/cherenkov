//! A rejection as a typed refusal, and back.
//!
//! The rules raise a rejection: a stable code, args and English, which the
//! edge prints. On the wire a command refuses with a typed refusal message
//! of the protos, one per rejection, listed in the `refuses` of the command
//! that raises it. `refuse!` turns the one into the other for a handler, and
//! `TryFrom<&TypedRefusal>` turns it back, keyed by the refusal's type and
//! by nothing else: the business status a refusal travels under is its
//! message's `(refusal_status)` option, read from the generated constant.

use angzarr_embedded::markup::TypedRefusal;

use crate::mutant::{NoMutant, NoRun, PlannedForRun};
use crate::run::AlreadyPlanned;
use crate::{MutantRejection, RunRejection};

/// What a command handler fails with for `rejection`: the typed refusal
/// message of the variant, one of the command's `refuses(..)` choice
/// `$choice` (its `From` each listed message), under the status the message
/// declares (`$package`'s `Refusal`). A rejection the command lists no
/// message for is a bug of the handler, not a refusal.
macro_rules! refuse {
    ($rejection:ident, $value:expr, $choice:ty, $package:ident, [$($variant:ident => $message:ident),* $(,)?]) => {
        match $value {
            $(
                $rejection::$variant => ::angzarr_embedded::markup::Failed::Rejected {
                    rejection: <$choice>::from($crate::$package::$message {}),
                    status: <$crate::$package::$message as $crate::$package::Refusal>::REFUSAL_STATUS,
                },
            )*
            #[allow(unreachable_patterns)]
            other => ::angzarr_embedded::markup::Failed::bug(&other),
        }
    };
}

pub(crate) use refuse;

/// A typed refusal that is none of this domain's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotOurs;

/// The rejection a run command's typed refusal is, by its type.
impl TryFrom<&TypedRefusal> for RunRejection {
    type Error = NotOurs;

    fn try_from(refusal: &TypedRefusal) -> Result<Self, NotOurs> {
        if refusal.object::<AlreadyPlanned>().is_some() {
            return Ok(RunRejection::AlreadyPlanned);
        }
        Err(NotOurs)
    }
}

/// The rejection a mutant command's typed refusal is, by its type.
impl TryFrom<&TypedRefusal> for MutantRejection {
    type Error = NotOurs;

    fn try_from(refusal: &TypedRefusal) -> Result<Self, NotOurs> {
        if refusal.object::<NoRun>().is_some() {
            return Ok(MutantRejection::NoRun);
        }
        if refusal.object::<NoMutant>().is_some() {
            return Ok(MutantRejection::NoMutant);
        }
        if refusal.object::<PlannedForRun>().is_some() {
            return Ok(MutantRejection::PlannedForRun);
        }
        Err(NotOurs)
    }
}

#[cfg(test)]
#[path = "refusal.test.rs"]
mod tests;
