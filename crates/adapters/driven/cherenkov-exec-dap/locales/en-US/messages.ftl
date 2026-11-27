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
cherenkov-exec-dap-toolchain-blame-outside-subset = viability check blamed mutant { $mutant }, which the subset it checked does not hold: the check's blame is broken

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
cherenkov-exec-dap-session-cancelled = the debugger session was cancelled: the run failed elsewhere

# What a library said, which has no code of its own.

cherenkov-exec-dap-external = { $detail }

# Why every mutant runs every test.

cherenkov-exec-dap-not-recorded-no-test-list = the test program does not list its tests
cherenkov-exec-dap-not-recorded-nothing = the recording run recorded nothing
cherenkov-exec-dap-not-recorded-tests-failed = the tests did not pass while recording what they call ({ $end })
cherenkov-exec-dap-not-recorded-named-tests-failed = the tests did not pass while recording what they call ({ $end }: { $failing })

# Why a mutant was not run or not reached. What the debugger said is
# `detail`, as it said it; a reason that wraps another says its cause.

cherenkov-exec-dap-inlined = { $detail }
cherenkov-exec-dap-function-not-found = { $detail }
cherenkov-exec-dap-symbol-not-in-scope = { $detail }
cherenkov-exec-dap-not-scalar = { $detail }
cherenkov-exec-dap-evaluator-said = { $detail }
cherenkov-exec-dap-evaluation-hung = the evaluation did not finish within { $secs } s with the program's other threads stopped
cherenkov-exec-dap-evaluator-refused = the evaluator cannot apply the mutant: { $cause }
cherenkov-exec-dap-unsteered = { $cause } (not steered: { $unsteered })
cherenkov-exec-dap-unappliable = { $cause } (tier P: { $tier_p })
cherenkov-exec-dap-no-constructor = { $built }; no constructor: { $constructor }
cherenkov-exec-dap-generic-copies = a generic function's copies cannot all be patched: { $cause }
cherenkov-exec-dap-negated-not-from-call = a negated boolean not computed by a call
cherenkov-exec-dap-float-result = a { $bytes }-byte result holding a float
cherenkov-exec-dap-no-code-for-line = no code for line { $line }
cherenkov-exec-dap-no-tier-b-evaluator = no tier B evaluator here
cherenkov-exec-dap-no-code = no code
cherenkov-exec-dap-not-in-program = { $file }:{ $line } is not in this program
cherenkov-exec-dap-never-executed = the selected tests never execute the mutated code
cherenkov-exec-dap-unresolved-in-source = { $value } does not compile in the source: cannot find { $names } in this scope
cherenkov-exec-dap-rustc-rejects = rustc rejects the mutant: { $detail }

# Why a value is not built from its type's layout.

cherenkov-exec-dap-no-default = { $name } has no Default in the source
cherenkov-exec-dap-default-not-in-binary = { $name }'s Default, if it has one, is not in the test binary
cherenkov-exec-dap-default-not-linked = { $name }'s Default is not in the test binary (unused, so not linked)
cherenkov-exec-dap-not-built-yet = { $value } for { $layout } is not built yet
cherenkov-exec-dap-not-an-integer = { $value } is not an integer
cherenkov-exec-dap-no-variant = { $name } has no variant { $variant }
cherenkov-exec-dap-no-payload = { $name }::{ $variant } has no payload
cherenkov-exec-dap-tag-without-tag = { $name }: a tag value without a tag
cherenkov-exec-dap-does-not-fit = { $value } does not fit { $name }
cherenkov-exec-dap-not-a-string = { $value } is not a string
cherenkov-exec-dap-needs-program-code = { $value } for { $name } needs the program's code
cherenkov-exec-dap-needs-heap = { $value } needs heap memory

# Why the support library does not make a constructor.

cherenkov-exec-dap-support-poisoned = the support library's state is poisoned
cherenkov-exec-dap-no-support-library = no support library
cherenkov-exec-dap-not-in-support-library = not in the support library
cherenkov-exec-dap-no-layout-in-support = { $ty }: the support library has no layout for it
cherenkov-exec-dap-laid-out-differently = { $ty } is laid out differently in the support library: { $detail }
cherenkov-exec-dap-mirror-conflict = { $ty }: its mirror { $module } is laid out differently in another program
cherenkov-exec-dap-support-build = the support library does not build: { $detail }
cherenkov-exec-dap-support-io = { $detail }
cherenkov-exec-dap-support-never-builds = the support library does not build without its failing constructors
cherenkov-exec-dap-mirror-does-not-compile = { $ty }: the mirror of { $of } does not compile: { $detail }
cherenkov-exec-dap-name-not-imported = { $value } for { $ty }: a name the support library does not import: { $detail }
cherenkov-exec-dap-needs-mirror = { $value } for { $ty }: needs more of a mirrored type than its layout: { $detail }
cherenkov-exec-dap-does-not-compile = { $value } does not compile as { $ty }: { $detail }

# Why no tier P patch exists for a mutant.

cherenkov-exec-dap-unpatchable-unsupported-edit = no tier P rule for { $what }
cherenkov-exec-dap-unpatchable-no-code-at = no code at { $line }:{ $col }
cherenkov-exec-dap-unpatchable-conditional-jumps = expected one conditional jump on line { $line }, found { $found }
cherenkov-exec-dap-unpatchable-unknown-destination = jump destination { $pc } is outside the function
cherenkov-exec-dap-unpatchable-ambiguous-polarity = conditional jump on line { $line } lands on its own line
cherenkov-exec-dap-unpatchable-condition-mismatch = condition code { $cc } does not implement `{ $token }`
cherenkov-exec-dap-unpatchable-no-instructions = no instructions for line { $line }
cherenkov-exec-dap-unpatchable-no-call = statement on line { $line } contains no call instruction
cherenkov-exec-dap-unpatchable-arithmetic-instructions = expected one add/sub instruction of the source operator on line { $line }, found { $found }
cherenkov-exec-dap-unpatchable-no-constant = no store of the returned boolean on line { $line }
cherenkov-exec-dap-unpatchable-no-default = the result type has no known zero value
cherenkov-exec-dap-unpatchable-non-zero-value = a return stub cannot produce the value { $value }
cherenkov-exec-dap-unpatchable-no-return-stub = no return stub for this calling convention
cherenkov-exec-dap-unpatchable-jump-into-stub = a jump lands at { $pc }, inside the return stub
cherenkov-exec-dap-unpatchable-function-too-short = function is { $bytes } bytes, shorter than the { $stub }-byte return stub
cherenkov-exec-dap-unpatchable-no-resume-point = nothing follows the statement on line { $line }
cherenkov-exec-dap-unpatchable-too-short = statement on line { $line } is { $bytes } bytes, too short for a jump
cherenkov-exec-dap-support-type = { $detail }
cherenkov-exec-dap-not-its-poll = { $value } is no { $ty }: the poll of a future not known as the fn's body
