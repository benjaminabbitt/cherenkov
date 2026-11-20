# Rust under GDB: findings

Measured against GDB 16.3 (Debian 16.3-1, built-in DAP server `gdb -i=dap`)
and Rust 1.100 (debug profile, linux/amd64) on the Rust fixture. The probe
that produced them is `spikes/gdb/gdb_probe.py`.

## GDB's DAP server

| Question | Answer |
|---|---|
| Transport | stdio: `gdb -q -i=dap` reads and writes DAP on stdin/stdout. There is no TCP listener, so the client needs a stdio transport. |
| Launch | `launch{program, args, stopAtBeginningOfMainSubprogram: true}`. **GDB answers `launch` only after `configurationDone`.** A client that waits for the launch response first deadlocks: send `launch`, then `configurationDone`, then wait for `stopped` (reason `breakpoint`, at `main`). |
| Code memory | **Pure DAP.** `readMemory`/`writeMemory` parse `memoryReference` as any integer address (`int(ref, 0)`, `gdb/dap/memory.py`). No escape hatch needed, unlike Delve. |
| Disassembly with lines | `disassemble{memoryReference, instructionCount}` returns per-instruction `address`, `instructionBytes` (hex), `line` and `location`, from GDB's line table (`find_pc_line`). It does not stop at the function end; bound the function separately. |
| Locating code | The binaries are PIE, so addresses exist only once the process is running. Resolve them after stopping at `main`. `evaluate{context: "repl"}` runs CLI commands, including `python`, so a source line can be turned into its function's runtime bounds through GDB's Python API (`gdb.decode_line`, `gdb.block_for_pc`). |
| Expressions | The repl's language follows the current frame (Rust at `main`); a C cast like `(long)&f` fails. Language-neutral commands (`info address`, `python ...`) avoid that. |
| goto | Not declared. Tier B on Rust stays on the `jump` CLI command (design §8), unchanged by this spike. |

## Debug-profile Rust code generation

- **Conditions** are `cmp; jl then` on the condition's line, taken when the
  condition holds. The Go patch rules apply as they are.
- **Arithmetic carries overflow checks** on the same line:
  `add rdi, rsi; mov; seto al; jo panic`, and `inc rax; …; jo panic` for
  `i = i + 1`. Swapping `add` for `sub` keeps the check meaningful, because
  OF is defined for both. A rule that requires exactly one conditional jump
  on the *line* of an arithmetic statement would be wrong here.
- **Returns go through a stack slot.** `return true` is `mov byte ptr
  [rsp-9], 1`, and a shared epilogue loads `al` from the slot. Return-flip is
  an immediate patch on that store; Go's result goes straight into `AX`.
- **The line table attributes following code to a call statement.** In
  `total`, line 5 (`log(s);`) is `call log` *and* `mov rax, [rsp+8]`, which
  loads `s` for `return s`. Jumping from the statement's first byte to the next
  line's first instruction skips that load and returns garbage, so a mutant
  that should survive would be killed. **Statement deletion therefore ends at
  the statement's last call instruction.** Instructions after it on the line
  still run. Go's `log(s)` was a lone `CALL`, so the rule gives the same patch
  there.
- **Library code is linked into every test binary.** An integration test
  binary contains the library functions it calls. A mutant is applied in each
  test binary that contains its function, and is killed if any of them fails.

## Tier P, end to end

The whole test binary is run once per mutant (`--test-threads=1`).

| Mutant | Patch | Exit | Verdict |
|---|---|---|---|
| negate `n < 10` (`is_small`) | `jl`→`jge` | 101 | killed |
| `<`→`<=` (`is_small`) | `jl`→`jle` | 0 | survived: no test at 10 |
| negate `i < n` (`count_up`) | `jl`→`jge` | 101 | killed |
| `<`→`<=` (`count_up`) | `jl`→`jle` | 101 | killed |
| delete `log(s)` (`total`), call only | `call log`→`jmp +3` | 0 | survived (correct) |
| `+`→`-` (`total`) | `add`→`sub` | 101 | killed |

A failing libtest run exits with 101.
