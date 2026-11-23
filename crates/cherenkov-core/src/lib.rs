//! Cherenkov domain core.
//!
//! Pure Rust with no I/O dependencies. Everything that touches files,
//! processes, parsers or debuggers lives in an adapter crate behind one of
//! the port traits in [`ports`]. No code in this crate branches on language
//! or backend: languages contribute data ([`profile::Profile`]), adapters
//! contribute [`syntax::FileSyntax`].

pub mod concept;
pub mod deficiency;
pub mod diff;
pub mod edit;
pub mod execution;
pub mod id;
pub mod inventory;
pub mod operator;
pub mod operator_set;
pub mod plan;
pub mod plan_sets;
pub mod ports;
pub mod profile;
pub mod session;
pub mod set_spec;
pub mod syntax;
pub mod token;
pub mod types;
pub mod verdict;
