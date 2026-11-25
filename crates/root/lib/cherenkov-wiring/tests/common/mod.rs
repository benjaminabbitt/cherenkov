//! What the tests of the durable store share: plans, settings, a verdict
//! file that keeps what it is written, the sampling of the clock and the ids.

#![allow(dead_code)]

pub mod crashing;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use angzarr_embedded::ProjectorHandler;
use angzarr_embedded::angzarr_router::error::CodedError;
use angzarr_embedded::angzarr_router::hpb::{ProjectRequest, ProjectResponse};
use cherenkov_mutation::execution::{RawOutcome, Tier};
use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_run_application::host::Readers;
use cherenkov_run_application::layer::Settings;
use cherenkov_run_application::verdicts::VerdictFile;
use cherenkov_run_ports::Report;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_run_wire::output::VerdictLine;
use cherenkov_syntax::TreeSitterSyntax;
use std::path::Path;

/// `n` mutants, each a copy of one of two plans under an id of its
/// own: even ones in `a`, odd ones in `b`.
pub fn plans_of(n: usize) -> Vec<MutantPlan> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = "fn a(x: i32) -> i32 { x + 1 }\nfn b(x: i32) -> i32 { x - 1 }\n";
    let file = syntax
        .analyze(&rust, Path::new("src/lib.rs"), Arc::from(src))
        .unwrap();
    let mut two: Vec<MutantPlan> = plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .filter(|p| p.operator == "aor")
        .collect();
    two.sort_by_key(|p| p.stop_at.line);
    two.dedup_by_key(|p| p.stop_at.line);
    (0..n)
        .map(|i| MutantPlan {
            id: MutantId(format!("{i:016x}")),
            ..two[i % 2].clone()
        })
        .collect()
}

pub fn settings() -> Settings {
    Settings {
        root: "/project".to_string(),
        operators: vec![cherenkov_run_domain::OperatorSet {
            language: "rust".to_string(),
            set: "breadth".to_string(),
        }],
        jobs: 1,
    }
}

pub fn exited(code: i32) -> RawOutcome {
    RawOutcome::Exited {
        code,
        tier: Tier::P,
        wall: Duration::from_millis(1),
    }
}

/// An output that prints nothing.
pub struct Silent;

impl ProjectorHandler for Silent {
    fn name(&self) -> &str {
        "output"
    }

    async fn dispatch(&self, _: &ProjectRequest) -> Result<ProjectResponse, CodedError> {
        Ok(ProjectResponse::default())
    }
}

/// A verdict file that keeps what it was written.
#[derive(Clone, Default)]
pub struct Kept(pub Arc<Mutex<Vec<Vec<VerdictLine>>>>);

impl VerdictFile for Kept {
    fn write(&self, lines: &[VerdictLine]) -> Result<(), Report> {
        self.0.lock().unwrap().push(lines.to_vec());
        Ok(())
    }
}

pub fn readers(kept: &Kept) -> Readers {
    Readers::new(Box::new(Silent), Some(Arc::new(kept.clone())))
}
