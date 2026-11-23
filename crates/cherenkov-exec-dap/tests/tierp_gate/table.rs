//! The shapes table the gate writes: which shapes passed, and the rest of
//! the ones the rules can make.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::shapes::{Shape, Table};

use crate::Verdict;

/// Where the committed table is.
pub fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../isa/tierp-shapes.toml")
}

/// What the gate found for the cases of each shape.
pub type Evidence = BTreeMap<Shape, Vec<Verdict>>;

/// Whether a shape passed: some case showed the patch right, and none
/// showed it wrong.
fn passed(verdicts: &[Verdict]) -> bool {
    verdicts.contains(&Verdict::Passed)
        && verdicts
            .iter()
            .all(|v| matches!(v, Verdict::Passed | Verdict::Vacuous))
}

/// The table's text for `evidence`: the shapes that passed, and every
/// other shape the rules can make.
pub fn render(rustc: &str, evidence: &Evidence) -> String {
    let pass: BTreeSet<String> = evidence
        .iter()
        .filter(|(_, v)| passed(v))
        .map(|(s, _)| s.to_string())
        .collect();
    let universe: BTreeSet<String> = Isa::amd64_unfiltered()
        .shape_universe()
        .iter()
        .map(Shape::to_string)
        .collect();
    let declined: BTreeSet<String> = universe.difference(&pass).cloned().collect();
    Table::render(rustc, &pass, &declined)
}

/// The shapes the gate saw that the rules' universe does not list.
pub fn unlisted(evidence: &Evidence) -> Vec<String> {
    let universe: BTreeSet<String> = Isa::amd64_unfiltered()
        .shape_universe()
        .iter()
        .map(Shape::to_string)
        .collect();
    evidence
        .keys()
        .map(Shape::to_string)
        .filter(|s| !universe.contains(s))
        .collect()
}
