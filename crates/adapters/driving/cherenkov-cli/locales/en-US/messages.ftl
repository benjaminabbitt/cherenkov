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
