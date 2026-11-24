use angzarr_diagnostic::Report;
use cherenkov_cli::error::CliError;

use super::catalog;

#[test]
fn test_the_program_s_catalog_renders_the_cli_s_and_the_run_layer_s_messages() {
    let catalog = catalog().unwrap();
    let run = CliError::Run {
        cause: Report::new("cherenkov-run-application-workers").arg("reason", "no gdb"),
    };
    assert_eq!(
        catalog.render(&run).unwrap(),
        "the run could not run: the mutants could not run: no gdb"
    );
}
