# Messages for the CLI's errors. Each id is an error's code, which begins
# with the crate's name; the variables are its args.

cherenkov-cli-unknown-operator-set = no operator set "{ $name }"; known sets: { $known }
cherenkov-cli-bad-operator-set = { $path }: not an operator set: { $reason }
cherenkov-cli-no-project = { $dir }: no go.mod or Cargo.toml; only Go and Rust are executable so far
cherenkov-cli-tool-missing = { $tool } not found on PATH or in $HOME/go/bin; pass --{ $tool }
cherenkov-cli-unreadable = { $path }: { $reason }
cherenkov-cli-syntax = the parsers did not load: { $reason }
cherenkov-cli-planning = the sources could not be planned: { $reason }
cherenkov-cli-toolchain = { $reason }
cherenkov-cli-run = the run could not run: { $cause }
