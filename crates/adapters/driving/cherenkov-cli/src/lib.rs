//! The command line's driving side: what a run's events print as (JSON Lines,
//! or text for a person), the errors a command fails with and the exit each
//! ends the process with. The program that composes these with the
//! toolchains and the run layer is `cherenkov` (root/bin).

pub mod error;
pub mod notice;
pub mod output;
pub mod render;
