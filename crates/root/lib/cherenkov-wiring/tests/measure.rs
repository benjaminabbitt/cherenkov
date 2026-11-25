//! What a whole run of instant mutants costs on each store. Run only when
//! asked, in release:
//! `CHERENKOV_MEASURE=500,1000,2000 cargo test --release -p cherenkov-wiring --test measure -- --ignored --nocapture`

mod common;

use std::time::Instant;

use cherenkov_mutation::execution::{Budget, Observed};
use cherenkov_mutation::plan::MutantPlan;
use cherenkov_run_application::layer::Workers;
use cherenkov_run_ports::executor::{ExecError, MutantExecutor};
use common::{Kept, exited, plans_of, readers, settings};
use test_dir::TestDir;

/// Kills the mutants in `a`, at once.
struct Instant8;

impl MutantExecutor for Instant8 {
    fn run(&mut self, plan: &MutantPlan, _: Budget) -> Result<Observed, ExecError> {
        Ok(match plan.function.as_deref() {
            Some("a") => exited(101).into(),
            _ => exited(0).into(),
        })
    }

    fn baseline(&mut self, _: Budget) -> Result<Observed, ExecError> {
        Ok(exited(0).into())
    }
}

fn workers() -> Workers {
    let set = || -> Vec<Box<dyn MutantExecutor + Send>> { vec![Box::new(Instant8)] };
    Workers {
        sets: (0..8).map(|_| set()).collect(),
        jobs: 2,
    }
}

fn sizes() -> Option<Vec<usize>> {
    let asked = std::env::var("CHERENKOV_MEASURE").ok()?;
    Some(
        asked
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect(),
    )
}

#[test]
#[ignore = "measurement: prints what a whole run costs in memory and on the SQLite store"]
fn measure_a_whole_run_in_memory_and_on_sqlite() {
    let Some(sizes) = sizes() else { return };
    let dir = TestDir::new("cherenkov-measure");
    for n in sizes {
        let plans = plans_of(n);
        let memory = Instant::now();
        cherenkov_wiring::start(
            cherenkov_run_store::memory(),
            &plans,
            &settings(),
            Some(workers()),
            readers(&Kept::default()),
            cherenkov_wiring::testing::sampling(),
        )
        .unwrap();
        let memory = memory.elapsed();

        let file = dir.join(format!("runs-{n}.sqlite"));
        let sampling = cherenkov_wiring::testing::sampling();
        let sqlite = Instant::now();
        cherenkov_wiring::start_in_file(
            &file,
            &plans,
            &settings(),
            Some(workers()),
            readers(&Kept::default()),
            sampling.clone(),
        )
        .unwrap();
        let sqlite = sqlite.elapsed();

        // The same project run again on that store: three earlier runs of
        // it are in the file, and the readers are handed them each time.
        let mut again = std::time::Duration::ZERO;
        for _ in 0..3 {
            let started = Instant::now();
            cherenkov_wiring::start_in_file(
                &file,
                &plans,
                &settings(),
                Some(workers()),
                readers(&Kept::default()),
                sampling.clone(),
            )
            .unwrap();
            again = started.elapsed();
        }
        let size = std::fs::metadata(&file).map_or(0, |m| m.len());
        eprintln!(
            "{n} mutants: memory {memory:?}, sqlite {sqlite:?}; the fifth run on that file {again:?}; file {} KiB",
            size / 1024
        );
    }
}
