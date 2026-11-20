//! The run's and the mutants' messages as protobuf, prost types generated
//! from `proto/cherenkov`: under `cherenkov.run.v1` the command that plans a
//! run, the facts reported to it, the events its stream holds, its refusal
//! and its snapshot; under `cherenkov.mutant.v1` a mutant as planned, its
//! outcome, what its own stream holds, its refusals and its snapshot; under
//! `cherenkov.output.v1` the documents a run prints. What
//! an aggregate decides with is their domain types, generated into
//! `cherenkov-run-domain`; these are what travels.

/// The generated packages, nested as their proto packages are.
pub mod cherenkov {
    pub mod run {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/cherenkov.run.v1.rs"));
        }
    }
    pub mod mutant {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/cherenkov.mutant.v1.rs"));
        }
    }
    pub mod output {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/cherenkov.output.v1.rs"));
        }
    }
}

/// The messages as proto3 JSON, generated from the protos' JSON options:
/// the runtime, and every message's `serde` impls.
pub mod json {
    include!(concat!(env!("OUT_DIR"), "/angzarr-json.rs"));
}

/// `cherenkov.mutant.v1`.
pub use cherenkov::mutant::v1 as mutant;
/// `cherenkov.output.v1`.
pub use cherenkov::output::v1 as output;
/// `cherenkov.run.v1`.
pub use cherenkov::run::v1 as run;

pub use mutant::*;
pub use run::*;

/// The names the documents print for the vocabulary's values: the enums'
/// generated JSON names (`not-called`, `recorded-with-failures`).
pub mod names {
    use crate::json::{EnumNames, Json};

    /// `number`'s name in `names`; the number's text for one it has no
    /// value of.
    fn text(names: &EnumNames, number: i32) -> String {
        match names.write(number) {
            Json::String(name) => name,
            _ => number.to_string(),
        }
    }

    /// A `cherenkov.mutant.v1.Verdict`'s name.
    pub fn verdict(number: i32) -> String {
        text(&crate::json::ENUM_CHERENKOV_MUTANT_V1_VERDICT, number)
    }

    /// A `cherenkov.mutant.v1.Tier`'s name.
    pub fn tier(number: i32) -> String {
        text(&crate::json::ENUM_CHERENKOV_MUTANT_V1_TIER, number)
    }

    /// A `cherenkov.run.v1.Selection`'s name.
    pub fn selection(number: i32) -> String {
        text(&crate::json::ENUM_CHERENKOV_RUN_V1_SELECTION, number)
    }

    /// A `cherenkov.run.v1.RunStatus`'s name.
    pub fn run_status(number: i32) -> String {
        text(&crate::json::ENUM_CHERENKOV_RUN_V1_RUNSTATUS, number)
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
