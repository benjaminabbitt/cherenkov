//! The run's, the mutants' and the output's prost types, generated into
//! `OUT_DIR` with angzarr's settings, and their JSON. Generated code is
//! never committed.

const PROTOS: &[&str] = &[
    "cherenkov/run/v1/run.proto",
    "cherenkov/mutant/v1/mutant.proto",
    "cherenkov/mutant/v1/plan.proto",
    "cherenkov/output/v1/output.proto",
];
const OUTPUT: &[&str] = &["cherenkov/output/v1/output.proto"];
const INCLUDES: &[&str] = &["../../proto"];

fn main() -> Result<(), angzarr_build::BuildError> {
    angzarr_build::wire(PROTOS, INCLUDES)?;
    // The JSON is the output documents' (`docs/output-events.md`), with the
    // vocabulary they print. The plan's messages (`plan.proto`) are left
    // out on purpose: nothing prints them, and the generated reader of
    // `Edit`, `Action` and `MutantPlan` is past the complexity gate's line
    // limit, which generated code is held to as well. The store's codec
    // (phase 5.7) will want them.
    angzarr_build::json(
        OUTPUT,
        INCLUDES,
        &[
            ("cherenkov.run.v1", "crate::cherenkov::run::v1"),
            ("cherenkov.mutant.v1", "crate::cherenkov::mutant::v1"),
            ("cherenkov.output.v1", "crate::cherenkov::output::v1"),
        ],
    )
}
