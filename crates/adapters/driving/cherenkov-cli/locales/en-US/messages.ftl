# Messages for the CLI's errors. Each id is an error's code, which begins
# with the crate's name; the variables are its args.

cherenkov-cli-unknown-operator-set = no operator set "{ $name }"; known sets: { $known }
cherenkov-cli-bad-operator-set = { $path }: not an operator set: { $cause }
cherenkov-cli-no-project = { $dir }: no go.mod or Cargo.toml; only Go and Rust are executable so far
cherenkov-cli-tool-missing = { $tool } not found on PATH or in $HOME/go/bin; pass --{ $tool }
cherenkov-cli-unreadable = { $path }: { $reason }
cherenkov-cli-syntax = the parsers did not load: { $cause }
cherenkov-cli-planning = the sources could not be planned: { $reason }
cherenkov-cli-toolchain = { $reason }
cherenkov-cli-run = the run could not run: { $cause }
cherenkov-cli-store-locked = { $path } is in use by { $holder }: wait for it to finish, name another store with --store FILE, or keep nothing with --ephemeral
cherenkov-cli-store-held = { $path } is held by { $holder }, which is still running: stop it first (a lock is released when its process ends, so only a running process holds one)
cherenkov-cli-no-store = { $path }: no store there

# What a command says on its way without failing (`notice.rs`). A count
# beside a list is the message's selector for saying the list or not.

cherenkov-cli-notice-operators = operators: { $label }
cherenkov-cli-notice-planned = { $lang }: { $mutants } mutants; concepts mapped { $mapped }/{ $total }{ $unmapped_count ->
        [0] {""}
       *[other] {"; unmapped: "}{ $unmapped }
    }{ $unrepresentable_count ->
        [0] {""}
       *[other] {"; unrepresentable: "}{ $unrepresentable }
    }{ $no_action ->
        [0] {""}
       *[other] {"; skipped (no action for context): "}{ $no_action }
    }{ $regions_count ->
        [0] {""}
       *[other] {"; skipped regions: "}{ $regions }
    }{ $disabled ->
        [0] {""}
       *[other] {"; disabled: "}{ $disabled }
    }{ $deduplicated ->
        [0] {""}
       *[other] {"; deduplicated: "}{ $deduplicated }
    }
cherenkov-cli-notice-file-problem = problem: { $path }: { $cause }
cherenkov-cli-notice-provenance = provenance: { $each }; in more than one set: { $shared }
cherenkov-cli-notice-phase = phase { $phase }: { $whole }.{ $tenth } s
cherenkov-cli-notice-phases = phase preparation and recording: { $preparation_whole }.{ $preparation_tenth } s; phase mutants: { $mutants_whole }.{ $mutants_tenth } s
cherenkov-cli-notice-viability-off = viability check off (--no-viability-check): mutants rustc would reject stay unsupported
cherenkov-cli-notice-viability = viability check: { $rejected } of { $checked } function-body replacements rejected by rustc (unviable when the debugger cannot run them), { $compiles } cargo check runs, { $whole }.{ $tenth } s
cherenkov-cli-notice-viability-skipped = viability check skipped ({ $detail }): mutants rustc would reject stay unsupported; --no-viability-check turns it off
cherenkov-cli-notice-carried-on = carried on run { $run }
cherenkov-cli-notice-workers = { $cpus } workers (cores; available memory unknown)
cherenkov-cli-notice-workers-sized = { $jobs } workers ({ $cpus } cores; { $available_mib } MiB available, about { $per_worker_mib } MiB per worker)
cherenkov-cli-notice-in-diff = --in-diff: { $skipped } not-called mutants not listed (counted above); changed-test mutants are judged by the changed tests alone, so a survivor means those tests do not catch it
