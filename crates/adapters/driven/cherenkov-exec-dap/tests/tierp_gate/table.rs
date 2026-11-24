//! The shapes table the gate writes: which shapes passed, and the rest of
//! the ones the rules can make.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::shapes::{Shape, Table};

use crate::Verdict;

/// Where the committed table is.
pub fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../isa/tierp-shapes.toml")
}

/// What the gate found for the cases of each shape.
pub type Evidence = BTreeMap<Shape, Vec<Verdict>>;

/// Whether a shape passed: some case showed the patch right, and none
/// showed it wrong.
pub fn passed(verdicts: &[Verdict]) -> bool {
    verdicts.contains(&Verdict::Passed)
        && verdicts
            .iter()
            .all(|v| matches!(v, Verdict::Passed | Verdict::Vacuous))
}

/// What the gate found for the cases of each function-value stub shape,
/// and the bytes rustc makes of the shapes it could harvest.
pub struct StubEvidence {
    pub verdicts: BTreeMap<String, Vec<Verdict>>,
    pub bytes: BTreeMap<String, Vec<u8>>,
}

/// The stub shapes that passed with their bytes, and the others seen.
fn stub_split(stubs: &StubEvidence) -> (BTreeMap<String, Vec<u8>>, BTreeSet<String>) {
    let mut pass = BTreeMap::new();
    let mut declined = BTreeSet::new();
    for (shape, verdicts) in &stubs.verdicts {
        match stubs.bytes.get(shape) {
            Some(bytes) if passed(verdicts) => {
                pass.insert(shape.clone(), bytes.clone());
            }
            _ => {
                declined.insert(shape.clone());
            }
        }
    }
    (pass, declined)
}

/// The table's text for `evidence`: the shapes that passed, and every
/// other shape the rules can make; and the stubs.
pub fn render(rustc: &str, evidence: &Evidence, stubs: &StubEvidence) -> String {
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
    let (stub_pass, stub_declined) = stub_split(stubs);
    Table::render(rustc, &pass, &declined, &stub_pass, &stub_declined)
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
