# Messages for the run domain's refusals. Each id is a refusal's code, which
# begins with the crate's name.

cherenkov-run-domain-already-planned = this run is already planned
cherenkov-run-domain-no-run = this plan names no run
cherenkov-run-domain-no-mutant = this plan names no mutant
cherenkov-run-domain-planned-for-run = this mutant is already planned for this run

# Messages for snapshots refused on loading.

cherenkov-run-domain-undecided-unordered = this run's snapshot does not list its undecided mutants in strictly ascending order
cherenkov-run-domain-tallies-unordered = this run's snapshot does not list its tallies in strictly ascending order of verdict
cherenkov-run-domain-prepared-unordered = this run's snapshot does not list its prepared programs in strictly ascending order
cherenkov-run-domain-unstarted = this run's snapshot holds mutants, programs, tallies or a finish but no start
cherenkov-run-domain-runs-unordered = this mutant's snapshot does not list its runs in strictly ascending order
cherenkov-run-domain-ran-repeated = this mutant's snapshot holds a run with a program's run twice
cherenkov-run-domain-unplanned = this mutant's snapshot holds a run without the mutant as it was planned

# Messages for a stream root that cannot be derived.

cherenkov-run-domain-underivable-root = the stream of { $key } in the { $domain } domain has no derivable root

# Messages for a recorded plan or observation the planner's types cannot hold.

cherenkov-run-domain-unreadable-missing = this record has no { $field }
cherenkov-run-domain-unreadable-unknown = this record names a { $kind } the planner does not have: { $name }
