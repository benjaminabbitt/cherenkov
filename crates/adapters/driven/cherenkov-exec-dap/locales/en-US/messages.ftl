# Messages for the debugger adapter's errors. Each id is an error's code,
# which begins with the crate's name; the variables are its args.

cherenkov-exec-dap-isa-toml = isa table: { $detail }
cherenkov-exec-dap-isa-unknown-token = isa table: unknown token "{ $name }"
cherenkov-exec-dap-isa-unknown-condition = isa table: unknown condition code "{ $name }"
cherenkov-exec-dap-isa-bad-displacement = isa table: jcc displacement must be 1 or 4 bytes, not { $rel }
cherenkov-exec-dap-isa-no-stub = isa table: no return stub for ABI "{ $abi }"
cherenkov-exec-dap-isa-bad-stub-line = isa table: stub line "{ $line }" is not `shape = hex bytes`

# The debugger's DAP and delve's JSON-RPC.

cherenkov-exec-dap-dap-io = dap i/o: { $detail }
cherenkov-exec-dap-dap-protocol = dap protocol: { $detail }
cherenkov-exec-dap-dap-failed = dap { $command } failed: { $detail }
cherenkov-exec-dap-dap-timeout = dap timed out waiting for { $what }
cherenkov-exec-dap-dap-closed = dap connection closed
cherenkov-exec-dap-rpc-io = delve rpc i/o: { $detail }
cherenkov-exec-dap-rpc-protocol = delve rpc protocol: { $detail }
cherenkov-exec-dap-rpc-call = delve rpc { $method }: { $detail }

# The tools the executor runs and the files it reads.

cherenkov-exec-dap-toolchain-spawn = running { $tool }: { $detail }
cherenkov-exec-dap-toolchain-failed = { $tool } failed in { $dir }: { $detail }
cherenkov-exec-dap-toolchain-output = { $what } output: { $detail }
cherenkov-exec-dap-toolchain-path = { $path }: { $detail }
cherenkov-exec-dap-toolchain-binary = { $binary }: { $step }: { $detail }
cherenkov-exec-dap-toolchain-viability = viability check: { $detail }
cherenkov-exec-dap-toolchain-baseline-unchecked = the unmutated project does not pass cargo check: { $detail }
cherenkov-exec-dap-toolchain-no-line-table = { $binary } has no debug line information: build its tests with debug info and without strip (for a project whose profile turns them off: CARGO_PROFILE_TEST_DEBUG=2 CARGO_PROFILE_TEST_STRIP=false)

# A debugger session.

cherenkov-exec-dap-session-spawn = starting { $tool }: { $detail }
cherenkov-exec-dap-session-no-pipe = no { $pipe } pipe
cherenkov-exec-dap-session-not-listening = delve did not start listening: { $detail }
cherenkov-exec-dap-session-connect = connecting { $what } to { $addr }: { $detail }
cherenkov-exec-dap-session-step = { $step }: { $cause }
cherenkov-exec-dap-session-wait = waiting for exit: { $detail }
cherenkov-exec-dap-session-no-code = no code at { $at }
cherenkov-exec-dap-session-no-function = no function at { $at }
cherenkov-exec-dap-session-output-incomplete = the program exited but its output has no { $what } after { $waited }: the run's output is incomplete
cherenkov-exec-dap-session-output-unended = the program exited but its output did not end within { $waited } ({ $cause }): the run's output is incomplete
cherenkov-exec-dap-session-unresolved = { $value } for { $ty }: a constructor not resolved to a library
cherenkov-exec-dap-session-patch-mismatch = expected { $expected } at { $addr }, found { $found }
cherenkov-exec-dap-session-write-mismatch = wrote { $wrote } at { $addr }, read back { $back }
cherenkov-exec-dap-session-no-restart = the debugger cannot restart with selected tests

# What a library said, which has no code of its own.

cherenkov-exec-dap-external = { $detail }

# Why every mutant runs every test.

cherenkov-exec-dap-not-recorded-no-test-list = the test program does not list its tests
cherenkov-exec-dap-not-recorded-nothing = the recording run recorded nothing
cherenkov-exec-dap-not-recorded-tests-failed = the tests did not pass while recording what they call ({ $end })
cherenkov-exec-dap-not-recorded-named-tests-failed = the tests did not pass while recording what they call ({ $end }: { $failing })
