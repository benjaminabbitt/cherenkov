//! The run layer's domain types, generated into `OUT_DIR` from the protos.
//! Generated code is never committed.

fn main() -> Result<(), angzarr_build::BuildError> {
    angzarr_build::markup_domain_with(
        &[
            "cherenkov/run/v1/run.proto",
            "cherenkov/mutant/v1/mutant.proto",
            "cherenkov/mutant/v1/plan.proto",
        ],
        &["../../proto"],
        &[
            ("cherenkov.run.v1", "::cherenkov_run_wire::run"),
            ("cherenkov.mutant.v1", "::cherenkov_run_wire::mutant"),
        ],
        &[
            ("cherenkov.run.v1", "crate::run"),
            ("cherenkov.mutant.v1", "crate::mutant"),
        ],
    )
}
