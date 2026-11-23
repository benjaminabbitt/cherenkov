# Mutation operators on Rust: what Cherenkov can do

Evaluated 2026-11-17 by reading debug-profile codegen (Rust 1.100,
x86-64) for one probe function per operator, `spikes/rust-ops/src/lib.rs`.
`dump.py` there prints each function's instructions by source line.

The operators are Cherenkov's 15 (docs/mutation-operators.md): the classic
operators (Mothra's 22, design §4.2) that apply to Rust and cargo-mutants'
Rust operators, as one family each. The evaluation was first written as 21
rows; rows that are not operators, or are not implemented, are listed apart
below and not counted.

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

One row for each operator (family; definitions, tables per set and sources
in docs/mutation-operators.md). "Derived from" is where Cherenkov's
definition comes from; "Mothra" names are the classic operators the family
corresponds to.

| Operator | Derived from | Rust debug codegen (observed) | Route | Status |
|---|---|---|---|---|
| `aor` | cargo-mutants (visit.rs); Dextool AORS; Cherenkov design. Same as Mothra AOR | `+`/`-`: `add`/`sub` (or `inc`/`dec`), then `seto; jo panic`. `*`, `/`, `%`: `imul` + `jo`; division is a block: zero check, `MIN/-1` check, `idiv`, result in `rax` (`/`) or `rdx` (`%`) | P: `+`<->`-` opcode or ModRM swap, the overflow check stays valid. `/`<->`%`: take `rdx` instead of `rax`. `*`->`+`: `add` + NOP pad. `*`->`/`, `/`->`*`: no same-shape patch; B for pure operands | `+`<->`-` done; `/`<->`%`, `*`->`+` moderate; others hard |
| `compound-assign` | cargo-mutants (visit.rs assignment forms); Dextool AORS. (Not Mothra's ASR, which is array-for-scalar) | `add r, [mem]`, `imul r, [mem]` | as `aor` and the bitwise and shift rows, on memory forms | `+=`/`-=` done (shapes listed); rest as `aor` |
| `ror` | cargo-mutants (visit.rs); Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; Cherenkov design. Same as Mothra ROR; same as PIT CONDITIONALS_BOUNDARY | branches: `cmp; jcc`. Values: `cmp; setcc` | P: condition-code rewrite, with polarity, signedness and operand order | done by column for every ordering swap, gated; `==`/`!=` to an ordering and float swaps that reorder operands are declined (docs/language-profiles.md, "Tier P comparisons") |
| `negate-condition` | Cherenkov design. Similar to PIT NEGATE_CONDITIONALS (that negates the relational operator) | one `jcc` on the condition's line | P: invert | done. Loop conditions (`cmp; jb body`) are the same operator and `ror` on a loop's condition, not a row of their own |
| `lcr` | cargo-mutants (visit.rs); Just & Schweiggert 2014 (COR: `&&` -> `==`, `\|\|` -> `!=`). Same as Mothra LCR for `&&` <-> `\|\|` | short-circuit: a `jcc` per operand (branch form); as a value, the last operand stored to a slot and a shared `movb $c, slot; jmp join` block | B, nothing evaluated: at each jump out of the left operand, read the flags; edges into the right operand go where the right operand's other exit goes (as a value: store the other constant, then the join); short-circuit edges go into the right operand. The operands are those the mutated text groups to (`a \|\| b && c` is `a \|\| (b && c)`) | `&&` <-> `\|\|` done (conditions, chains, values); `==`/`!=` forms not checked |
| `bitwise` | cargo-mutants (visit.rs) | `and`/`or`/`xor r, r` | P: opcode swap | easy |
| `shift` | cargo-mutants (visit.rs) | `shl r, cl` after a shift-amount check that is the same for both | P: ModRM reg swap (`shr` or `sar` by signedness) | easy |
| `unary` | cargo-mutants UnaryOperator (visit.rs). Same as cosmic-ray, Dextool UOI (reduced to deletion) | `!`: folded into branch polarity (`test al,1; jne`), as a value `xor al, 0xff` (or `1`) then `and al, 1`. `-`: `xor eax,eax; sub rax,rcx`, after a `== MIN` overflow check | P: `!` as negation, as a value the `xor` becomes `and al, 1`. `-`: `mov rax,rcx` + NOP; the `MIN` check stays, so the mutant still panics for `MIN` | `!` done; `-` easy (one edge case differs) |
| `delete-statement` | Cherenkov design; Just et al. 2014. Similar to PIT VOID_METHOD_CALLS; similar to Mothra SDL (call statements only) | the call, drops of its result, next-statement loads on the same line | P: path walk, resume after the last call | done |
| `fn-value` | cargo-mutants FnValue (fnvalue.rs) | see the table below | P: entry stub per result type; non-zero `true`, `1`, `-1`, `None`, `Ok(())` from the shapes table (docs/language-profiles.md "Tier P function-value stubs") | zero and trivial values done |
| `body-default` | Cherenkov design. Similar to cargo-mutants FnValue (only the zero value) | the zero-value row of the table below | as `fn-value` | zero done |
| `match-arm` | cargo-mutants MatchArm (visit.rs) | compare chain (`test`/`sub`; `je arm`) or jump table | P: NOP the arm's `je` so it falls to the wildcard; or rewrite the table entry | moderate |
| `match-guard` | cargo-mutants MatchArmGuard (visit.rs) | the guard's `jcc` | P: as `negate-condition` | easy |
| `struct-field` | cargo-mutants StructField (visit.rs) | `call <S as Default>::default`, then field stores | P: skip that field's store (DWARF offsets) | moderate; codegen not yet confirmed |
| `return-bool-flip` | Cherenkov design | a constant `al` store before `ret` | P: patch the immediate | not evaluated for codegen |

### Not operators, not implemented

Rows of the earlier 21-row table that are not counted as operators (sources
and reasons in docs/mutation-operators.md). Their codegen findings stay for
when they are built.

| Was | Source | Rust debug codegen (observed) | Route | Why not an operator here |
|---|---|---|---|---|
| 11 UOI / ABS insertion | Mothra UOI, ABS | needs new instructions | B (evaluate, set variable) | not implemented; hard; dropped from the value ranking (§4.5.1) |
| 12 constant replacement (0, 1, −1, ±1) | Mothra CRP, SRC; PIT (experimental) | literals are immediates (`add rdi, 0x2a`, `mov …, imm`) | P: patch the immediate | not implemented; easy where the literal is an immediate |
| 14 non-void call -> default result | PIT non-void call removal; mutest-rs 2026 | `call`, result in `rax` (or sret). Trivial calls may be inlined even at debug | P: `call` -> `xor eax,eax` + NOPs for register results | not implemented; moderate |
| 19 variable replacement | Mothra SVR | `let x = a` often elided; values in registers or stack slots | B: set the variable at the stop; candidate choice is an open design question (§12) | not implemented; hard |
| 20 index replacement | Mothra AAR, ACR, ASR, CAR, CNR, SAR (and CSR) | bounds check `cmp; jae panic`, then `mov rax, [base + i*8]` | B (the bounds check must see the mutant's index too) | six operators, none implemented; hard |
| 21 loop condition | `ror` and `negate-condition` on a loop's condition | `cmp; jb body` | P | not a separate operator |

## Body replacement by result type (`fn-value`, `body-default`)

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

- **15 operators are implemented.** By status of their Rust route:
  - **Done:** `delete-statement`, `negate-condition`; `lcr` for `&&` <-> `\|\|`; `aor` for `+` <-> `-`; `compound-assign` for `+=` and `-=`; `ror` for the boundary swaps; `unary` for `!`.
  - **Easy:** `bitwise`, `shift`, `match-guard`, the rest of `ror` and `unary`, and the unit, `&str` and constant-value body stubs.
  - **Moderate:** the rest of `aor`, `match-arm`, `struct-field`, and the typed-layout body stubs. These need control-flow or DWARF-layout reasoning, but no runtime help.
  - **Not evaluated:** `return-bool-flip`.
- **Not implemented, not counted:** UOI/ABS, constant replacement, call -> default, SVR, the array-reference family. The tier B ones (UOI/ABS, SVR, the array family) and the `*`↔`/` swaps are also the lowest-value operators in §4.5.1.
- **Nothing needs recompiling**, except constructing heap values that the binary has no code for.
- **Fidelity caveats are all overflow and bounds checks the compiler emitted for the original expression:** `unary`'s `MIN` check for `-`, and `aor`'s division checks. Each changes the outcome for one edge input only.

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
  - Tier B operators, which need a live stop: UOI/ABS, SVR, the array family and the `*`↔`/` swaps.
- **The design invariant changes.** "No adapter writes files" becomes "nothing writes source; only build output and scratch copies of binaries."
- **Likely best:** a hybrid. Static patching for every tier P operator, the debugger only for tier B. Parallel runs then need nothing more than separate copies.
