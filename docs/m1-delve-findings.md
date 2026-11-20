# M1 Delve spike: findings

Measured against Delve 1.27.2 and Go 1.27.4 on linux/amd64, with the Go
fixture built with `go test -c -gcflags=all='-N -l'`. These findings answer
the Delve items in design section 12. They are measured, not inferred. The
probes that produced them are in `spikes/delve/`.

## Answers to the open questions

| Question (design §12) | Answer |
|---|---|
| Does Delve have a `jump` command? | **Yes**, marked EXPERIMENTAL. Over JSON-RPC it is `RPCServer.SetExecutionPoint{Addr}`. It does not resume, unlike GDB's `jump`. |
| Can one headless Delve serve DAP and JSON-RPC at once? | **Yes.** `dlv exec --headless --accept-multiclient --api-version=2` checks the first byte of each connection (`C` for DAP's `Content-Length`, otherwise JSON-RPC). Both clients drive the same debugger. After a JSON-RPC jump, DAP's `stackTrace` shows the new line. |
| Is DAP `writeMemory` accepted on the code section? | **Not as intended.** Delve's DAP memory references are handles for the backing arrays of slices and strings only, and they are cleared on every resume. `readMemory` is bounded by the handle's size. `writeMemory` adds an unchecked offset to the handle's address, but using that to reach code is an abuse of the protocol, so Cherenkov does not. |
| How else can code be read and written? | Read with JSON-RPC `ExamineMemory{Address, Length}`, which takes any address. Write with JSON-RPC `Set{Symbol: "*(*uint8)(0xADDR)", Value: "N"}`, which goes through Delve's evaluator and its process-memory write. Both work on the text section of a stopped process. |
| Do results stay in stack slots (`~r0`) until the epilogue? | **No.** `~r0` exists (`ListFunctionArgs` flags it as a return argument), but a constant `return true` compiles to `MOVL $1, AX` straight into the result register; the slot is written separately. Setting `~r0` and jumping to the epilogue does not change the returned value. The expression `~r0` is also rejected by Delve's parser ("could not find symbol value for r0"). |
| Does a crashing mutant crash under Delve? | **No, it stops, and Cherenkov relies on that.** Delve keeps default breakpoints on unrecovered panics (id -1) and fatal throws (id -2). A crashing mutant stops under the debugger with DAP `stopped{reason: "exception", description: "panic"}` instead of taking the process down, and the driver reports it at once as `Crashed` (a kill). The run is never lost to a crash, and the crash can be attributed: `exceptionInfo.details.stackTrace` names the crash site and the test that hit it (`calc.Sum calc.go:29` from `TestSum`). For Go runtime errors such as an index out of range, Delve provides no message text (`description`, `text` empty), so the stack is the reported detail. Delve also sends `breakMode: ""`, which is not a valid DAP `ExceptionBreakMode`, so that response is read leniently rather than through the generated type. |
| Delve's own breakpoints vs memory writes | Not exercised: tier P patches are written while the process is stopped at entry, with no breakpoints set. A breakpoint and a patch at the same address must still be prevented, because Delve restores the original bytes when it clears a breakpoint. |

## Tier P, end to end

Each mutant: start Delve stopped at entry, patch over JSON-RPC, continue, and
read the test result.

| Source mutant | Site (amd64) | Patch | Test | Verdict |
|---|---|---|---|---|
| negate `n < 10` (`IsSmall`) | `JL +2` | `7c`→`7d` (`JGE`) | TestIsSmall fails | killed |
| `<`→`<=` (`IsSmall`) | `JL +2` | `7c`→`7e` (`JLE`) | passes: no test at 10 | survived (expected) |
| delete `log(s)` (`Total`) | `CALL log` (5 bytes) | `eb 03` (short `JMP` to line 8) | passes: `log` is a no-op | survived (expected) |
| `+`→`-` (`Total`) | `ADDQ BX, AX` | `48 01 d8`→`48 29 d8` | TestTotal fails | killed |
| body→default (`IsSmall`, `Total`) | function entry | `31 c0 c3` (`XORL AX,AX; RET`) | both fail | killed |
| arm then disarm before running | `JL` | restore | passes; 32-byte window identical | integrity holds |

`SetExecutionPoint` at a breakpoint on `return true`, jumping to the
else-block, makes `IsSmall(3)` return false, and the test fails. The
evaluate-and-jump path (tier B) works on Delve with no emulated jump.

## Code generation facts the patcher must handle

From the `-N -l` fixture binary:

- **Conditions are one compare and one conditional jump on the condition's
  line**, followed by an unconditional `JMP`. In all three fixture
  conditions the `Jcc` is taken when the condition is true and lands in the
  then-block or loop body. Polarity must still be derived per site from where
  the jump lands, not assumed.
- **Operands can be swapped.** `for i < n` compiles to `CMPQ n, i; JG body`.
  Relational swaps therefore have to recognize the swapped form: `<` taken
  when true is either `L` (direct) or `G` (swapped). Once the polarity is
  known, the interpretation is unambiguous. A `Jcc` matching neither form is
  unsupported.
- **Signedness comes from the condition code**: `L/LE/G/GE` signed,
  `B/BE/A/AE` unsigned.
- **`i = i + 1` compiles to `INCQ`, not `ADDQ`.** Arithmetic patches need
  `INC`/`DEC` (`FF /0`, `FF /1`) as well as `ADD`/`SUB`.
- **The prologue is not where line locations point.** `FindLocation
  "file:line"` resolves past the prologue; the function's true entry is
  `Location.function.value`. An entry stub (`XOR; RET`) must go at the true
  entry, before the frame is set up.
- **A `for` clause puts init, condition and post on one line**, but only
  one conditional jump. The bounds check of the body is on the body's line.

## Consequences for the design

1. **Tier P on Go needs JSON-RPC, not DAP.** Memory read and write, function
   entry, disassembly with line info (`Disassemble` returns bytes, line and
   destination per instruction) and the run itself (`Command{continue}` returns
   `Exited`/`ExitStatus`) are all JSON-RPC. DAP adds nothing to the tier P
   path on Delve.
2. **Forced returns on Go are tier P only.** "setVariable `~r0` plus jump to
   the epilogue" (design §8) is invalid at `-N -l`. Return mutants patch the
   instruction that loads the result register (`MOVL $1, AX`→`MOVL $0, AX`).
   Body replacement uses the entry stub, which is exact for every
   register-returned Go result, because a Go zero value is all-zero bits.
3. **Tier B on Go is viable**: breakpoint (DAP or JSON-RPC), evaluate, then
   `SetExecutionPoint`. Evaluating mutated text with calls or assignments is
   still excluded by the native-evaluator rule (design §4.5).
4. **Decoding can use Delve's disassembly** (per-instruction bytes, line and
   destination). Cherenkov decides patches from the bytes against the ISA
   table, not from Delve's text formatting.
