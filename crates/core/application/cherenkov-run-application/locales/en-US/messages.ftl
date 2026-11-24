# Messages for the run application's errors. Each id is an error's code, which
# begins with the crate's name; the variables are its args.

cherenkov-run-application-kernel = the run's kernel failed: { $cause }
cherenkov-run-application-unavailable = the run's core could not take a call for now: { $cause }
cherenkov-run-application-store = the run's store could not be opened: { $cause }
cherenkov-run-application-misconfigured = the run's kernel or bus did not take what the run registers: { $cause }
cherenkov-run-application-runtime = the run's runtime did not start: { $detail }
cherenkov-run-application-workers = the mutants could not run: { $cause }
cherenkov-run-application-workers-program = the mutants could not run ({ $program }): { $cause }
cherenkov-run-application-stalled = nothing is left to serve, and the run has not finished: { $owed } mutants are planned and not decided
cherenkov-run-application-file-unreadable = unreadable: { $detail }
cherenkov-run-application-file-analysis = analysis failed: { $cause }
cherenkov-run-application-file-syntax-errors = syntax errors; not planned

# The reasons the run layer gives for what it did not or could not do.

cherenkov-run-application-no-workers = no workers
cherenkov-run-application-no-test-programs = no test programs
cherenkov-run-application-no-executors = no executors
cherenkov-run-application-preparation-panicked = a test program's preparation panicked
cherenkov-run-application-worker-panicked = a worker panicked
cherenkov-run-application-no-program = the worker has no test program { $program }
cherenkov-run-application-none-prepared = run { $run } prepared no test program
cherenkov-run-application-no-plan = a mutant was planned with no plan
cherenkov-run-application-no-outcome = mutant { $mutant } was decided with no outcome
cherenkov-run-application-exit-disagrees = the run's exit status says it passed (exit code 0) but its output reports failed test(s): { $tests }
cherenkov-run-application-baseline-failed = not recorded: the baseline did not pass
cherenkov-run-application-resumed = a later run of the same plan was resumed
cherenkov-run-application-plan-changed = the plan changed
