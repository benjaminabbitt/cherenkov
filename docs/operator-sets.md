# Operator sets for Rust

Cherenkov's operators come in named sets. A run uses exactly one, and its
results say which, because the sets serve different goals
(docs/operator-research.md) and mixing them silently would make results
incomparable with any of them.

## Set `cargo-mutants`

What cargo-mutants 26.2.0 generates, read from its source (crate tarball
from crates.io, `src/visit.rs` and `src/fnvalue.rs`). The point of this set
is a mutant-by-mutant comparison, so where cargo-mutants' docs and source
differ, the source wins. (The docs summary omits `<` → `<=` and `>` → `>=`.)

**Skipped** (the item and everything inside it). The set honours every
skip kind; the Rust profile maps each rule to one:

| cargo-mutants rule | Skip kind |
|---|---|
| `#[cfg(test)]` (with `test` directly inside `cfg(...)`), any attribute whose path ends in `test`, `#![cfg(test)]` on a file | `test` |
| `#[mutants::skip]`, also through `cfg_attr` | `annotated` |
| `unsafe fn` | `unsafe` |
| `fn new` in an `impl` or a trait (a free `fn new` is mutated) | `constructor` |
| `impl Default for T` | `default_impl` |
| calls to `with_capacity` | `capacity_call` |

Empty bodies, and a body replacement identical to the existing body, give
no mutant.

| Genre | Rule (source) |
|---|---|
| Binary operators (`visit.rs:576-601`) | `==` → `!=`; `!=` → `==`; `&&` → `\|\|`; `\|\|` → `&&`; `<` → `==`, `>`, `<=`; `>` → `==`, `<`, `>=`; `<=` → `>`; `>=` → `<`; `+` → `-`, `*`; `-` and `*` → `+`, `/`; `/` → `%`, `*`; `%` → `/`, `+`; `<<` → `>>`; `>>` → `<<`; `&` → `\|`, `^`; `\|` → `&`, `^`; `^` → `\|`, `&`. Assignment forms: `+=` → `-=`, `*=`; `-=` and `*=` → `+=`, `/=`; `/=` → `%=`, `*=`; `%=` → `/=`, `+=`; `<<=` → `>>=`; `>>=` → `<<=`; `&=` → `\|=`; `\|=` → `&=`; `^=` → `\|=`, `&=` |
| Unary operators (`visit.rs:616-626`) | `!x` → `x`; `-x` → `x` (operator deleted) |
| Match arm deletion (`visit.rs:640-668`) | only in a `match` that has a `_` arm; each arm deleted except the `_` arm and arms with guards |
| Match guard (`visit.rs:670-690`) | each guard → `true` and → `false` |
| Struct field deletion (`visit.rs:700-735`) | each field of a struct literal with a base (`..base`) deleted |
| Function body → value (`fnvalue.rs:20-241`) | `()` → `()`. `bool` → `true`, `false`. `String` → `String::new()`, `"xyzzy".into()`. `&str` → `""`, `"xyzzy"`. Unsigned ints → `0`, `1`. Signed → `0`, `1`, `-1`. `NonZero*` → `1.try_into().unwrap()` (and `-1` for signed). Floats → `0.0`, `1.0`, `-1.0`. `Result<T, E>` → `Ok(r)` for each replacement `r` of `T` (`Ok(Default::default())` if none), plus `Err(e)` for configured error expressions. `Option<T>` → `None`, `Some(r)`. `Vec<T>` → `vec![]`, `vec![r]`. `Cow` → `Cow::Borrowed(r)`, `Cow::Owned(r.to_owned())`. `Box`/`Rc`/`Arc` → `T::new(r)`. Known collections → `new()` and `from_iter([r])`; maps → also `from_iter([(k, v)])`. `[T; N]` → `[r; N]`. `&[T]` and `&mut [T]` → `Vec::leak(Vec::new())`, `Vec::leak(vec![r])`. `&T` and `&mut T` → `Box::leak(Box::new(r))` (`&str` → `""`, `"xyzzy"`). Tuples → each combination. `impl Iterator` → `::std::iter::empty()`, `::std::iter::once(r)`. Anything else → `Default::default()`. Nested recursively |

### Parity

**A swap means its text.** cargo-mutants edits the source, so a swapped
operator takes the operands the mutated text parses to: the first `&&` of
`a && b && c` as `||` is `a || (b && c)`, and the top `||` of `a || b || c`
as `&&` is `a || (b && c)` too (`Token::binds_tighter`). The fidelity
fixture has tests that fail under exactly one grouping.

Execution on ctxloom's `deps` package (305 mutants): cargo-mutants catches
264 in 605 s (41 unviable, none missed). Cherenkov kills 233, with 1
timeout and 71 it cannot run yet, in 176 s, and disagrees on none it runs.

Generation compared mutant by mutant against `cargo mutants --list --json`
(matched on file, line, column and replacement text;
`spikes/compare/compare_lists.py`):

| Code | cargo-mutants | matched | missing | extra, in files cargo-mutants visits |
|---|---|---|---|---|
| `spikes/rust-ops` (probe crate, one site per genre) | 158 | 158 | 0 | 0 |
| angzarr-embedded (export) | 2393 | 2393 | 0 | 0 |
| Cherenkov's own workspace | 1993 | 1989 | 4 | 0 |

Known divergences, all listed in the set's `pending`:
- **Files, in `cherenkov plan` only.** cargo-mutants visits only files in a
  package's module tree and does not follow a `mod` behind `#[cfg(test)]`
  (angzarr's `#[cfg(test)] #[path = "x.test.rs"] mod tests;`).
  `cherenkov plan` plans every file under the directory, so it lists those
  files (3112 extra mutants on angzarr-embedded) and non-member crates
  (`fixtures/`, `spikes/` here). `cherenkov run` mutates only what a
  non-test build compiles (rustc's dep-info, design section 4.11), so on
  ctxloom's `deps` it runs exactly cargo-mutants' 305 mutants.
- **Let chains.** cargo-mutants swaps `&&` to `||` inside `if a && let
  P = b`; that mutant never compiles. tree-sitter parses a let chain as its
  own node, which Cherenkov does not map (the 4 missing above).

## Set `sufficient` (Cherenkov's default)

Chosen for a non-redundant score that still covers what real faults need.
Each entry cites its evidence. **[choice]** marks a decision this project
made where the sources do not settle it.

| Operator | Rule | Evidence |
|---|---|---|
| ROR | `<` → `<=`, `!=`, `false`; `>` → `>=`, `!=`, `false`; `<=` → `<`, `==`, `true`; `>=` → `>`, `==`, `true`; `==` → `<=`, `>=`, `false`; `!=` → `<`, `>`, `true` | sufficient sets: Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014 (10 Java programs, 27% fewer mutants, no loss) |
| ROR on `bool` and non-ordered types | only `==` → `false`, `!=` → `true` (no ordering mutants); floats drop the `<`→`<=` boundary | Dextool's type-specific ROR (fewer junk mutants) **[choice: adopt]** |
| COR | `a && b` → `false`, `a`, `b`, `a == b`; `a \|\| b` → `true`, `a`, `b`, `a != b` | Just & Schweiggert 2014 (4 of 10 sufficient) |
| AOR (counterparts) | `+` ↔ `-`; `*` ↔ `/`; `%` → `/` | Dextool AORS ("full AOR highly redundant"). `%` → `/` follows mutmut and Mull **[choice]** |
| Bitwise and shift | `&` ↔ `\|`; `^` → `&`; `<<` ↔ `>>` | PIT, mutmut and Gremlins agree on these pairs |
| Compound assignment | the AOR, bitwise and shift rules applied to `op=` | same rules |
| Unary deletion | `!x` → `x`; `-x` → `x` | cargo-mutants, Mull, Dextool (UOI reduced to deletion: full UOI "produces a large number of unproductive mutants") |
| Statement deletion | each statement deleted, including `return`, `break`, `continue` (Rust: a call statement, an assignment, or `return`/`break`/`continue` without a value) | couples to real faults, "should always be employed": Just et al. FSE 2014; most productive at Google |
| `break` ↔ `continue` | swap | mutest-rs 2026; real faults (FSE 2014) |
| Range bound | `a..b` ↔ `a..=b` | mutest-rs 2026 |
| Call result → default | a non-`()` call's value → `Default::default()` | mutest-rs 2026; argument and call faults (FSE 2014) |
| Call deletion | a `()` call statement deleted | mutest-rs; PIT void-call removal |
| Function body → value | cargo-mutants' value table (above) | type-aware replacement, needed for Rust (mutest-rs; cargo-mutants' main operator) |
| Match arm deletion, guard → true/false, struct field deletion | as in `cargo-mutants` | Rust-specific structure |

**Excluded from `sufficient`, with reasons:**

| Operator | Reason |
|---|---|
| ABS | "predominantly not useful" (Google 2018); "produces many equivalent mutants" (Kushigian et al., ISSTA 2024) |
| `++`/`--` insertion (AOIS) | 94.8% of TCE-detected equivalent mutants in Kintis et al.'s data (Kushigian 2024); Rust has no `++` anyway |
| UOI insertion on boolean expressions | subsumed by the COR set (Just & Schweiggert 2014) |
| UOI negation insertion on arithmetic | **[choice: exclude by default]**. Google keeps UOI (74.5% productive), so this is opt-in, not dropped |
| Full ROR (7 per operator), full AOR | redundant: subsumed by the sufficient sets above |
| SVR, the array-reference family, CSR/SCR | **[choice: opt-in]**. They produce the most mutants (Mathur's suggestion to omit SVR and ASR, cited in Jia & Harman 2011, not re-checked) and need candidates chosen at the stop. Kept available, not in the default |
| Constant replacement (CRP/SRC) | **[choice: opt-in]**. Neither Google nor the sufficient-set work uses it; PIT has it as experimental |

## Non-default operators (opt-in, either set)

These are evaluated and implemented, but not on by default:
- SVR (rules in docs/mutation-operators.md, plus the candidate rule)
- CSR inside indices (`v[i]` → `v[0]`)
- UOI arithmetic insertion
- ABS
- constant replacement

## Provenance of named mutations

Every named mutation in `operator_set.rs` carries `origins`; `cherenkov
mutations` lists them. Each origin is one of three kinds:

- **derived from**: where the definition was taken from. Every mutation has
  at least one, naming the tool or paper. When Cherenkov's output
  diverges from cargo-mutants, a `cargo-mutants` origin shows that the
  mutation was meant to match it.
- **same as**: a peer tool has the identical mutation, perhaps under another
  name (PIT `CONDITIONALS_BOUNDARY`).
- **similar to**: a peer has a close mutation; the note says how it differs.

A peer is recorded only when docs/operator-research.md supports it. A guess
from memory does not count.

## Language independence

Named mutations are written against concepts and tokens, never against a
language. A language takes part through its profile alone: concept queries,
token spellings, and for `fn-value` a return-value generator.
`crates/cherenkov-syntax/tests/cross_language.rs` plans one program written
in Go, Python and Rust under every set and requires the same named mutations
from each. The one exception is `fn-value`: only the Rust profile has a
generator so far (cargo-mutants' type table), so Go and Python plan none.

## What every implemented operator must pass

1. **Its rule generates exactly the mutants above:** planner tests on source.
2. **Every generated mutant renders as source that compiles,** or the planner reports it unviable.
3. **Fidelity:** the run-time mutant's per-test outcomes equal the compiled source mutant's (the `cherenkov-fidelity` harness).
