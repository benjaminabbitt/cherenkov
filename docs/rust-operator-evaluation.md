# Mutation operators on Rust: what Cherenkov can do

Evaluated 2026-11-17 by reading debug-profile codegen (Rust 1.100,
x86-64) for one probe function per operator, `spikes/rust-ops/src/lib.rs`.
`dump.py` there prints each function's instructions by source line.

The set merges the classic operators (Mothra's 22, design §4.2) with
cargo-mutants' Rust operators.

**Status:**
- **done**: implemented and tested.
- **easy**: same shape as something done, small table or rule addition.
- **moderate**: needs a new derivation (control-flow or layout reasoning) but no runtime help.
- **hard**: needs tier B (a stop, evaluate, jump) or more.

**Routes:**
- **P**: a one-time instruction patch.
- **B**: evaluate and jump under GDB at each hit.
- **X**: no route without recompiling.

## Operators

| # | Operator | Rust debug codegen (observed) | Route | Status |
|---|---|---|---|---|
| 1 | AOR `+` ↔ `-` | `add`/`sub` (or `inc`/`dec`), then `seto; jo panic` | P: opcode or ModRM swap; the overflow check stays valid | done |
| 2 | AOR `*`, `/`, `%` | `imul` + `jo`. Division is a block: zero check, `MIN/-1` check, `idiv`, result in `rax` (`/`) or `rdx` (`%`) | `/`↔`%`: P, take `rdx` instead of `rax` (one ModRM byte). `*`→`+`: P (`add` + NOP pad). `*`→`/`, `/`→`*`: no same-shape patch; B for pure operands | partial: `/`↔`%`, `*`→`+` moderate; others hard |
| 3 | ASR `+=` `-=` `*=` … | `add r, [mem]`, `imul r, [mem]` | as rows 1 and 2, on memory forms | `+=`/`-=` done (shapes listed); rest as row 2 |
| 4 | ROR, all swaps | branches: `cmp; jcc`. Values: `cmp; setcc` | P: condition-code rewrite, with polarity, signedness and operand order | boundary swaps done; full swap set and `setcc` easy |
| 5 | Condition negation; condition → true/false (`if`, `while`, guards) | one `jcc` on the condition's line | P: invert; `jcc`→`jmp` or NOP | negation done; constants easy |
| 6 | LCR `&&` ↔ `\|\|` | short-circuit: a `jcc` per operand (branch form); as a value, the last operand stored to a slot and a shared `movb $c, slot; jmp join` block for the short circuit | B, nothing evaluated: at each jump out of the left operand, read the flags; edges into the right operand go where the right operand's other exit goes (as a value: store the other constant, then the join); short-circuit edges go into the right operand. The operands are those the mutated text groups to (`a \|\| b && c` is `a \|\| (b && c)`) | done (conditions, chains, values) |
| 7 | Bitwise `&` `\|` `^` | `and`/`or`/`xor r, r` | P: opcode swap | easy |
| 8 | Shift `<<` ↔ `>>` | `shl r, cl` after a shift-amount check that is the same for both | P: ModRM reg swap (`shr` or `sar` by signedness) | easy |
| 9 | Delete `!` | folded into branch polarity (`test al,1; jne`); as a value, `xor al, 0xff` (or `1`) then `and al, 1` | P: same as negation; as a value, the `xor` becomes `and al, 1` | done |
| 10 | Delete unary `-` | `xor eax,eax; sub rax,rcx`, after a `== MIN` overflow check | P: `mov rax,rcx` + NOP. The `MIN` check stays, so the mutant still panics for `MIN` | easy (one edge case differs) |
| 11 | UOI / ABS insertion | needs new instructions | B (evaluate, set variable) | hard; dropped from the value ranking (§4.5.1) |
| 12 | Constant replacement (0, 1, −1, ±1) | literals are immediates (`add rdi, 0x2a`, `mov …, imm`) | P: patch the immediate | easy where the literal is an immediate |
| 13 | Statement deletion | the call, drops of its result, next-statement loads on the same line | P: path walk, resume after the last call | done |
| 14 | Non-void call → default result | `call`, result in `rax` (or sret). Trivial calls may be inlined even at debug (`v.len()` is a register move) | P: `call` → `xor eax,eax` + NOPs for register results; sret as row 15 | moderate |
| 15 | Function body → value (cargo-mutants' main operator) | see the table below | P: entry stub per result type | partial |
| 16 | Match arm deletion | compare chain (`test`/`sub`; `je arm`) or jump table | P: NOP the arm's `je` so it falls to the wildcard; or rewrite the table entry | moderate |
| 17 | Match guard → true/false | the guard's `jcc` | P: as row 5 | easy |
| 18 | Struct field deletion (`..Default::default()`) | `call <S as Default>::default`, then field stores | P: skip that field's store (DWARF offsets) | moderate; codegen not yet confirmed |
| 19 | Variable replacement (SVR) | `let x = a` often elided; values in registers or stack slots | B: set the variable at the stop; candidate choice at the stop is an open design question (§12) | hard |
| 20 | Index replacement (AAR, ACR) | bounds check `cmp; jae panic`, then `mov rax, [base + i*8]` | B (set the index; the bounds check must see the mutant's index too) | hard |
| 21 | Loop condition | `cmp; jb body` | P | done |

## Body replacement by result type (row 15)

| Result type | ABI shape (observed) | Stub | Status |
|---|---|---|---|
| `()` | nothing returned | `ret` | **missing**: the profile lists `()` as a unit type, so the planner never generates the mutant. cargo-mutants' most common mutant. Easy |
| `bool`, integers, `char` | `al`/`eax`/`rax` | `mov eax, imm; ret` (0, 1, −1) | zero done; other values easy |
| `f32`/`f64` | `xmm0` | `xorps; ret`, or `mov rax, imm; movq xmm0, rax; ret` | zero done; others easy |
| `&str`, `&[T]` | `rax` = pointer, `rdx` = length | `""`: any non-null aligned `rax`, `rdx` = 0. `"xyzzy"`: the bytes inside the stub | easy |
| `String`, `Vec<T>` | sret: caller's slot in `rdi` | write `cap`=0, `ptr`=align (dangling), `len`=0 at DWARF field offsets; `mov rax, rdi; ret` | moderate (needs DWARF layout). `"xyzzy".into()` needs allocation: hard |
| `Option<T>` | register pair (`rax` = discriminant, `rdx` = payload), or a niche | discriminant and payload from DWARF variant info | moderate |
| `Result<T, E>` | sret with niche (`Ok(4)` stored `-1` in word 0, the payload in word 1) | `Ok(default)`: niche value plus payload from DWARF | moderate for primitive `T`, harder when nested |
| `Box<T>` | heap pointer | needs allocation: a stub calling the allocator | hard |
| structs (`Default`) | registers (`S {i64, i64}` in `rax:rdx`) or sret | all-zero when every field's zero is valid; otherwise tail-jump to `<S as Default>::default` when the binary contains it (same return convention) | moderate |
| `async fn -> T` | the fn only makes its future; the body is the future's poll (`{async_fn#0}`), returning `Poll<T>` | the poll, not the constructor: tier B returns `Poll::Ready(value)` (built as for `T`) at its entry, so the future is ready on its first poll. Never the zeroing stub: a zero `Poll<i64>` is `Ready` with an unset payload | done for fn-value; a tail expression's evaluated value (`eval-return`) is refused |

## Summary

- **Done:** 6 of the 21 rows (1, 3 partly, 4 partly, 5, 13, 21).
- **Easy:** 9 more, mostly table additions to `isa/amd64.toml`: 4 (full), 5 (constants), 7, 8, 9, 10, 12, 17, and the unit, `&str` and constant-value body stubs.
- **Moderate:** about 6 (2 partly, 6, 14, 16, 18, the typed-layout body stubs). These need control-flow or DWARF-layout reasoning, but no runtime help.
- **Hard or tier B:** 11, 19, 20, the `*`↔`/` swaps, and `Box`. These are also the lowest-value operators in §4.5.1.
- **Nothing needs recompiling**, except constructing heap values that the binary has no code for.
- **Fidelity caveats are all overflow and bounds checks the compiler emitted for the original expression:** row 10's `MIN` check, and row 2's division checks. Each changes the outcome for one edge input only.

## Binary patching on disk instead of under a debugger

Tier P already patches machine code. It could patch a copy of the test
binary on disk and run it natively instead of in a debugged process.

- **Overhead, measured on the fixtures** (launch to exit, 10-run average, the debugger figures including about 20 ms of Python probe startup):
  - Rust: 1 ms native vs 155 ms under GDB.
  - Go: 2 ms native vs 123 ms under Delve.
  - The debugger costs about 120–150 ms per test-binary run, which dominates small suites. For a suite that takes seconds it is minor.
- **What it needs:**
  - Static reading of ELF and DWARF: line table, function bounds, layouts. The `object`, `gimli` and `addr2line` crates are standard for this.
  - An x86 instruction-length decoder (for example `iced-x86`). The debuggers currently supply instruction boundaries.
  - PIE is no obstacle: patches go at file offsets through the program headers.
  - One copy of the binary per worker, patched and restored per mutant, which costs nothing per mutant.
- **What it loses:**
  - Crash-stop capture: crashes become exit codes and signals, still kills.
  - Tier B operators, which need a live stop: rows 11, 19, 20 and the `*`↔`/` swaps.
- **The design invariant changes.** "No adapter writes files" becomes "nothing writes source; only build output and scratch copies of binaries."
- **Likely best:** a hybrid. Static patching for every tier P operator, the debugger only for tier B. Parallel runs then need nothing more than separate copies.
