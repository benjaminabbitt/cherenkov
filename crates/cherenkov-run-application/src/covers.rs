//! The streams of the run layer: where a run and a mutant are kept.

use angzarr_embedded::v1::{Cover, Uuid};

use crate::error::RunLayerError;
use cherenkov_run_domain::{MUTANT_DOMAIN, RUN_DOMAIN, mutant_root};

/// The stream of run `run`.
pub fn run_cover(run: &Uuid) -> Cover {
    Cover {
        domain: RUN_DOMAIN.to_string(),
        root: Some(run.clone()),
        edition: None,
    }
}

/// The stream of mutant `id`.
pub fn mutant_cover(id: &str) -> Result<Cover, RunLayerError> {
    Ok(Cover {
        domain: MUTANT_DOMAIN.to_string(),
        root: Some(mutant_root(id).map_err(|e| RunLayerError::kernel(&e))?),
        edition: None,
    })
}
