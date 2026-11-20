# Tier B on Rust under GDB: findings

Measured 2026-11-17 against GDB 16.3 (DAP) and Rust 1.100 (debug profile)
on `spikes/rust-ops`. Every mutant was checked by a test that passes only
when the mutant is applied (`spikes/rust-ops/tests/tierb.rs`). Probes:
`spikes/gdb/tierb_probe.py` (stop, evaluate, resume) and the GDB-side
helpers in `spikes/gdb/cherenkov_gdb.py`.

These are the operators the Rust evaluation (docs/rust-operator-evaluation.md)
marked hard, because no same-shape instruction patch exists.

## Results

| Mutant | Source | Mechanism | Result |
|---|---|---|---|
| `*` → `/` | `let m = a * b;` → `m = a / b` | stop at the line, evaluate `a / b`, write `m`, resume past the store | mutant applied (test passes) |
| `*` → `/`, every iteration | `t = t * 2;` in a `while` | same, at each hit | applied on all 3 hits |
| division by zero | `m = a / (b - 4)` | GDB reports `Division by zero` | detectable: the plan's `evaluator-raised` verdict |
| index replacement | `let x = v[i];` → `x = v[0]` | GDB's Rust evaluator indexes slices | applied |
| variable replacement (SVR) | `m = a * b` → `m = b` | another local in scope | applied |
| negation insertion (UOI) | `m = -a` | GDB's Rust evaluator | applied |
| `Box<T>` body → `Box::new(0)` | `fn r_box() -> Box<i64>` | instruction breakpoint at the entry; `malloc(8)` called by GDB; write 0; `return`; set `rax` | applied |

## What makes it work

1. **A `let` target is not in scope at its own statement.** DWARF starts the
   variable's lexical block after the declaration, so `m = a / b` fails
   with "No symbol 'm'" at the stop. The variable's stack slot is
   frame-relative and valid for the whole call, so it is found by searching
   the function's nested blocks (`gdb.block_for_pc` over its instructions),
   and the evaluated value is written into the slot directly.
2. **Resume after the store, not after the line.** Debug Rust attributes
   the next statement's operand loads to the current line, so "the end of
   the statement's lines" skips code that must run (the same problem as
   statement deletion). The resume point is the instruction after the store
   to the target's slot.
3. **The next statement may read the register the store came from, not
   the slot.** After `mov %rax, 0x20(%rsp)` (the store to `m`), line 160
   is `inc %rax`. So the store's source register gets the mutated value too.
4. **Moving the PC:** `set var $pc = ADDR` through the repl does not resume
   execution (unlike the `jump` command). DAP's `stackTrace` then shows the
   new line, and `continue` runs on. A breakpoint inside a loop stops at
   every iteration.
5. **Forced return needs the true entry.** A function breakpoint resolves
   past the prologue into the first inlined callee (`Box::new`), and GDB
   refuses with "Can not force return from an inlined function". An
   instruction breakpoint at the entry address (`info address`) avoids it.
6. **Calling into the program:** libc's `malloc` has no debug info, so it
   is called with `set language c` and a cast, then `set language auto`.
   This assumes the default global allocator (the system one, `__rdl_alloc`
   present), so that the program's `Box` drop frees it with `free`. A custom
   `#[global_allocator]` would need its own allocation function.

## Limits found

- Only scalar targets are written so far (`int.to_bytes` by type size). A
  struct- or 16-byte-valued assignment has several stores.
- GDB's arithmetic is not Rust's: overflow (`i64::MIN / -1`, `MAX * 2`)
  needs checking against Rust semantics before this is relied on.
- The store is found by parsing GDB's AT&T disassembly text for
  `mov %reg, disp(%rsp)`. Other store forms (rbp-relative, `movq`,
  immediates) are not recognized yet.
- Variable replacement needs a rule for choosing candidates at the stop
  (in-scope locals of the same type, via DAP `scopes`/`variables`). That is
  the open question in design §12, now with a working mechanism under it.

## Cost

Each hit is a stop plus a few requests. Native runs of the fixtures take
1–2 ms; under a debugger, launch to exit takes 120–155 ms. The project's
judgement is that a tenth of a second per run is fine compared to
compiling a mutant.
