# Messages for the run application's errors. Each id is an error's code, which
# begins with the crate's name; the variables are its args.

cherenkov-run-application-kernel = the run's kernel failed: { $cause }
cherenkov-run-application-misconfigured = the run's kernel or bus did not take what the run registers: { $cause }
cherenkov-run-application-runtime = the run's runtime did not start: { $reason }
cherenkov-run-application-workers = the mutants could not run: { $reason }
cherenkov-run-application-undelivered = { $letters } steps of the run were dead-lettered, never recorded
cherenkov-run-application-stalled = nothing is left to serve, and the run has not finished: { $owed } mutants are planned and not decided
