# Messages for the debugger adapter's errors. Each id is an error's code,
# which begins with the crate's name; the variables are its args.

cherenkov-exec-dap-isa-toml = isa table: { $detail }
cherenkov-exec-dap-isa-unknown-token = isa table: unknown token "{ $name }"
cherenkov-exec-dap-isa-unknown-condition = isa table: unknown condition code "{ $name }"
cherenkov-exec-dap-isa-bad-displacement = isa table: jcc displacement must be 1 or 4 bytes, not { $rel }
cherenkov-exec-dap-isa-no-stub = isa table: no return stub for ABI "{ $abi }"
cherenkov-exec-dap-isa-bad-stub-line = isa table: stub line "{ $line }" is not `shape = hex bytes`
