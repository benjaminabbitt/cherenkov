# Operator sets for Rust

Cherenkov's operators come in named sets. A run uses exactly one, and its
results say which, because the sets serve different goals
(docs/operator-research.md) and mixing them silently would make results
incomparable with any of them.

**What a set is.** An operator is a family (`ror`, `lcr`, `compound-assign`;
15 are implemented, docs/mutation-operators.md). A set lists the operators
it uses and gives each its own table of replacements, so the same operator
has different tables in different sets: `lcr` is `&&` <-> `||` in
`cargo-mutants` and `&&` -> `==`, `||` -> `!=` in `sufficient`. A
replacement is named `family/detail` (`lcr/and->or`); a custom set file may
name replacements or whole operators. The tables of each set are generated
below by `cherenkov operators --set NAME` (`just docs` regenerates them).

**Defaults.** Without `--operators`, a run uses `cargo-mutants` for Rust
(parity with cargo-mutants, for the mutations Cherenkov can handle; some stay
unsupported) and `breadth` for the other languages, chosen per language when
a project has several. `sufficient` is not a default: a research-backed,
non-redundant set composed across the languages Cherenkov supports is future
work, and `sufficient` is the Rust-only start of it.

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

<!-- generated: cherenkov operators --set cargo-mutants -->
Set `cargo-mutants`: what cargo-mutants generates, for a mutant-by-mutant comparison

11 operators.

| Operator | Definition | Derived from | Also called |
|---|---|---|---|
| `ror` | Replace a relational operator with another relational operator. | cargo-mutants; Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; Cherenkov design | Mothra ROR |
| `lcr` | Replace a logical connector with the other connector, or with an equality test of its operands. | cargo-mutants |  |
| `aor` | Replace an arithmetic operator with another arithmetic operator. | cargo-mutants; Dextool AORS; Cherenkov design | Mothra AOR |
| `shift` | Replace a shift operator with the opposite shift. | cargo-mutants |  |
| `bitwise` | Replace a bitwise operator with another bitwise operator. | cargo-mutants |  |
| `compound-assign` | Replace the operator of a compound assignment with another operator of its kind. | cargo-mutants; Dextool AORS |  |
| `unary` | Delete a unary operator, keeping its operand. | cargo-mutants UnaryOperator | cargo-mutants UnaryOperator |
| `match-arm` | Delete a match arm, when the match has a catch-all arm. | cargo-mutants MatchArm | cargo-mutants MatchArm |
| `match-guard` | Replace a match arm's guard with a constant boolean. | cargo-mutants MatchArmGuard | cargo-mutants MatchArmGuard |
| `struct-field` | Delete a field of a struct literal whose omitted fields take a default. | cargo-mutants StructField | cargo-mutants StructField |
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | cargo-mutants FnValue | cargo-mutants FnValue |

### `ror`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `ror/eq->ne` | Replace equal with not-equal. | derived cargo-mutants; same PIT NEGATE_CONDITIONALS; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT NEGATE_CONDITIONALS | cargo-mutants | `==` -> `!=` (go, python, rust) |
| `ror/ne->eq` | Replace not-equal with equal. | derived cargo-mutants; same PIT NEGATE_CONDITIONALS; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT NEGATE_CONDITIONALS | cargo-mutants | `!=` -> `==` (go, python, rust) |
| `ror/lt->eq` | Replace less-than with equal. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<` -> `==` (go, python, rust) |
| `ror/lt->gt` | Replace less-than with greater-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<` -> `>` (go, python, rust) |
| `ror/lt->le` | Replace less-than with less-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `<` -> `<=` (go, python, rust) |
| `ror/gt->eq` | Replace greater-than with equal. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>` -> `==` (go, python, rust) |
| `ror/gt->lt` | Replace greater-than with less-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>` -> `<` (go, python, rust) |
| `ror/gt->ge` | Replace greater-than with greater-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `>` -> `>=` (go, python, rust) |
| `ror/le->gt` | Replace less-or-equal with greater-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<=` -> `>` (go, python, rust) |
| `ror/ge->lt` | Replace greater-or-equal with less-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>=` -> `<` (go, python, rust) |

### `lcr`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `lcr/and->or` | Replace logical and with logical or. | derived cargo-mutants; same Mothra LCR | Mothra LCR | cargo-mutants | `&&` -> `\|\|` (go, rust); `and` -> `or` (python) |
| `lcr/or->and` | Replace logical or with logical and. | derived cargo-mutants; same Mothra LCR | Mothra LCR | cargo-mutants | `\|\|` -> `&&` (go, rust); `or` -> `and` (python) |

### `aor`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `aor/add->sub` | Replace addition with subtraction. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `+` -> `-` (go, python, rust) |
| `aor/add->mul` | Replace addition with multiplication. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `+` -> `*` (go, python, rust) |
| `aor/sub->add` | Replace subtraction with addition. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `-` -> `+` (go, python, rust) |
| `aor/sub->div` | Replace subtraction with division. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `-` -> `/` (go, python, rust) |
| `aor/mul->add` | Replace multiplication with addition. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `*` -> `+` (go, python, rust) |
| `aor/mul->div` | Replace multiplication with division. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `*` -> `/` (go, python, rust) |
| `aor/div->rem` | Replace division with remainder. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `/` -> `%` (go, python, rust) |
| `aor/div->mul` | Replace division with multiplication. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `/` -> `*` (go, python, rust) |
| `aor/rem->div` | Replace remainder with division. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS | cargo-mutants, sufficient | `%` -> `/` (go, python, rust) |
| `aor/rem->add` | Replace remainder with addition. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `%` -> `+` (go, python, rust) |

### `shift`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `shift/shl->shr` | Replace left shift with right shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `<<` -> `>>` (go, python, rust) |
| `shift/shr->shl` | Replace right shift with left shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `>>` -> `<<` (go, python, rust) |

### `bitwise`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `bitwise/and->or` | Replace bitwise and with bitwise or. | derived cargo-mutants |  | cargo-mutants, sufficient | `&` -> `\|` (go, python, rust) |
| `bitwise/and->xor` | Replace bitwise and with bitwise xor. | derived cargo-mutants |  | cargo-mutants | `&` -> `^` (go, python, rust) |
| `bitwise/or->and` | Replace bitwise or with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `\|` -> `&` (go, python, rust) |
| `bitwise/or->xor` | Replace bitwise or with bitwise xor. | derived cargo-mutants |  | cargo-mutants | `\|` -> `^` (go, python, rust) |
| `bitwise/xor->or` | Replace bitwise xor with bitwise or. | derived cargo-mutants |  | cargo-mutants | `^` -> `\|` (go, python, rust) |
| `bitwise/xor->and` | Replace bitwise xor with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `^` -> `&` (go, python, rust) |

### `compound-assign`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `compound-assign/add->sub` | Replace addition assignment with subtraction assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `+=` -> `-=` (go, python, rust) |
| `compound-assign/add->mul` | Replace addition assignment with multiplication assignment. | derived cargo-mutants; same Mothra AOR | Mothra AOR | cargo-mutants | `+=` -> `*=` (go, python, rust) |
| `compound-assign/sub->add` | Replace subtraction assignment with addition assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `-=` -> `+=` (go, python, rust) |
| `compound-assign/sub->div` | Replace subtraction assignment with division assignment. | derived cargo-mutants; same Mothra AOR | Mothra AOR | cargo-mutants | `-=` -> `/=` (go, python, rust) |
| `compound-assign/mul->add` | Replace multiplication assignment with addition assignment. | derived cargo-mutants; same Mothra AOR | Mothra AOR | cargo-mutants | `*=` -> `+=` (go, python, rust) |
| `compound-assign/mul->div` | Replace multiplication assignment with division assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `*=` -> `/=` (go, python, rust) |
| `compound-assign/div->rem` | Replace division assignment with remainder assignment. | derived cargo-mutants; same Mothra AOR | Mothra AOR | cargo-mutants | `/=` -> `%=` (go, python, rust) |
| `compound-assign/div->mul` | Replace division assignment with multiplication assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `/=` -> `*=` (go, python, rust) |
| `compound-assign/rem->div` | Replace remainder assignment with division assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `%=` -> `/=` (go, python, rust) |
| `compound-assign/rem->add` | Replace remainder assignment with addition assignment. | derived cargo-mutants; same Mothra AOR | Mothra AOR | cargo-mutants | `%=` -> `+=` (go, python, rust) |
| `compound-assign/shl->shr` | Replace left-shift assignment with right-shift assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `<<=` -> `>>=` (go, python, rust) |
| `compound-assign/shr->shl` | Replace right-shift assignment with left-shift assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `>>=` -> `<<=` (go, python, rust) |
| `compound-assign/and->or` | Replace bitwise-and assignment with bitwise-or assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `&=` -> `\|=` (go, python, rust) |
| `compound-assign/or->and` | Replace bitwise-or assignment with bitwise-and assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `\|=` -> `&=` (go, python, rust) |
| `compound-assign/xor->or` | Replace bitwise-xor assignment with bitwise-or assignment. | derived cargo-mutants |  | cargo-mutants | `^=` -> `\|=` (go, python, rust) |
| `compound-assign/xor->and` | Replace bitwise-xor assignment with bitwise-and assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `^=` -> `&=` (go, python, rust) |

### `unary`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `unary/delete-not` | Delete the logical not operator, keeping its operand. | derived cargo-mutants UnaryOperator; same cosmic-ray; same Dextool UOI | Dextool UOI | cargo-mutants | delete `!` (go, rust); delete `not` (python) |
| `unary/delete-neg` | Delete the negation operator, keeping its operand. | derived cargo-mutants UnaryOperator |  | cargo-mutants | delete `-` (go, python, rust) |

### `match-arm`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `match-arm/delete` | Delete a match arm, when the match has a catch-all arm. | derived cargo-mutants MatchArm |  | cargo-mutants |

### `match-guard`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `match-guard/true` | Replace a match arm's guard with the constant true. | derived cargo-mutants MatchArmGuard |  | cargo-mutants |
| `match-guard/false` | Replace a match arm's guard with the constant false. | derived cargo-mutants MatchArmGuard |  | cargo-mutants |

### `struct-field`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `struct-field/delete` | Delete a field of a struct literal whose omitted fields take a default. | derived cargo-mutants StructField |  | cargo-mutants |

### `fn-value`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | derived cargo-mutants FnValue |  | cargo-mutants, sufficient |
<!-- /generated -->

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

## Set `breadth`

The default for Go and Python: one replacement kind per action, the prototype's six operators. (Rust's default is `cargo-mutants`.)

<!-- generated: cherenkov operators --set breadth -->
Set `breadth`: the six prototype operators: one per action kind (design section 4.9)

6 operators.

| Operator | Definition | Derived from | Also called |
|---|---|---|---|
| `delete-statement` | Delete a call statement. | Cherenkov design; Just et al. 2014 |  |
| `aor` | Replace an arithmetic operator with another arithmetic operator. | cargo-mutants; Dextool AORS; Cherenkov design | Mothra AOR |
| `ror` | Replace a relational operator with another relational operator. | cargo-mutants; Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; Cherenkov design | Mothra ROR |
| `negate-condition` | Negate the condition of an if or loop. | Cherenkov design |  |
| `return-bool-flip` | Replace a returned boolean literal with its opposite. | Cherenkov design |  |
| `body-default` | Replace a function body with the zero value of its result type. | Cherenkov design |  |

### `delete-statement`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `delete-statement/call` | Delete a call statement. | derived Cherenkov design; derived Just et al. 2014; similar PIT VOID_METHOD_CALLS; similar Mothra SDL |  | breadth, sufficient |

### `aor`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `aor/add->sub` | Replace addition with subtraction. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `+` -> `-` (go, python, rust) |
| `aor/sub->add` | Replace subtraction with addition. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `-` -> `+` (go, python, rust) |
| `aor/mul->div` | Replace multiplication with division. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `*` -> `/` (go, python, rust) |
| `aor/div->mul` | Replace division with multiplication. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `/` -> `*` (go, python, rust) |

### `ror`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `ror/lt->le` | Replace less-than with less-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `<` -> `<=` (go, python, rust) |
| `ror/le->lt` | Replace less-or-equal with less-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `<=` -> `<` (go, python, rust) |
| `ror/gt->ge` | Replace greater-than with greater-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `>` -> `>=` (go, python, rust) |
| `ror/ge->gt` | Replace greater-or-equal with greater-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `>=` -> `>` (go, python, rust) |

### `negate-condition`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `negate-condition` | Negate the condition of an if or loop. | derived Cherenkov design; similar PIT NEGATE_CONDITIONALS |  | breadth |

### `return-bool-flip`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `return-bool-flip` | Replace a returned boolean literal with its opposite. | derived Cherenkov design |  | breadth |

### `body-default`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `body-default` | Replace a function body with the zero value of its result type. | derived Cherenkov design; similar cargo-mutants FnValue |  | breadth |
<!-- /generated -->

## Set `sufficient`

Chosen for a non-redundant score that still covers what real faults need.
<!-- generated: cherenkov operators --set sufficient -->
Set `sufficient`: non-redundant sufficient sets plus operators real faults need

8 operators.

| Operator | Definition | Derived from | Also called |
|---|---|---|---|
| `ror` | Replace a relational operator with another relational operator. | cargo-mutants; Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; Cherenkov design | Mothra ROR |
| `lcr` | Replace a logical connector with the other connector, or with an equality test of its operands. | Just & Schweiggert 2014 |  |
| `aor` | Replace an arithmetic operator with another arithmetic operator. | cargo-mutants; Dextool AORS; Cherenkov design | Mothra AOR |
| `shift` | Replace a shift operator with the opposite shift. | cargo-mutants |  |
| `bitwise` | Replace a bitwise operator with another bitwise operator. | cargo-mutants |  |
| `compound-assign` | Replace the operator of a compound assignment with another operator of its kind. | cargo-mutants; Dextool AORS |  |
| `delete-statement` | Delete a call statement. | Cherenkov design; Just et al. 2014 |  |
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | cargo-mutants FnValue | cargo-mutants FnValue |

### `ror`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `ror/lt->le` | Replace less-than with less-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `<` -> `<=` (go, python, rust) |
| `ror/lt->ne` | Replace less-than with not-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `<` -> `!=` (go, python, rust) |
| `ror/gt->ge` | Replace greater-than with greater-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `>` -> `>=` (go, python, rust) |
| `ror/gt->ne` | Replace greater-than with not-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `>` -> `!=` (go, python, rust) |
| `ror/le->lt` | Replace less-or-equal with less-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `<=` -> `<` (go, python, rust) |
| `ror/le->eq` | Replace less-or-equal with equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `<=` -> `==` (go, python, rust) |
| `ror/ge->gt` | Replace greater-or-equal with greater-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `>=` -> `>` (go, python, rust) |
| `ror/ge->eq` | Replace greater-or-equal with equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `>=` -> `==` (go, python, rust) |
| `ror/eq->le` | Replace equal with less-or-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `==` -> `<=` (go, python, rust) |
| `ror/eq->ge` | Replace equal with greater-or-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `==` -> `>=` (go, python, rust) |
| `ror/ne->lt` | Replace not-equal with less-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `!=` -> `<` (go, python, rust) |
| `ror/ne->gt` | Replace not-equal with greater-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `!=` -> `>` (go, python, rust) |

### `lcr`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `lcr/and->eq` | Replace logical and with equal. | derived Just & Schweiggert 2014 |  | sufficient | `&&` -> `==` (go, rust); `and` -> `==` (python) |
| `lcr/or->ne` | Replace logical or with not-equal. | derived Just & Schweiggert 2014 |  | sufficient | `\|\|` -> `!=` (go, rust); `or` -> `!=` (python) |

### `aor`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `aor/add->sub` | Replace addition with subtraction. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `+` -> `-` (go, python, rust) |
| `aor/sub->add` | Replace subtraction with addition. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `-` -> `+` (go, python, rust) |
| `aor/mul->div` | Replace multiplication with division. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `*` -> `/` (go, python, rust) |
| `aor/div->mul` | Replace division with multiplication. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `/` -> `*` (go, python, rust) |
| `aor/rem->div` | Replace remainder with division. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS | cargo-mutants, sufficient | `%` -> `/` (go, python, rust) |

### `shift`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `shift/shl->shr` | Replace left shift with right shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `<<` -> `>>` (go, python, rust) |
| `shift/shr->shl` | Replace right shift with left shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `>>` -> `<<` (go, python, rust) |

### `bitwise`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `bitwise/and->or` | Replace bitwise and with bitwise or. | derived cargo-mutants |  | cargo-mutants, sufficient | `&` -> `\|` (go, python, rust) |
| `bitwise/or->and` | Replace bitwise or with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `\|` -> `&` (go, python, rust) |
| `bitwise/xor->and` | Replace bitwise xor with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `^` -> `&` (go, python, rust) |

### `compound-assign`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `compound-assign/add->sub` | Replace addition assignment with subtraction assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `+=` -> `-=` (go, python, rust) |
| `compound-assign/sub->add` | Replace subtraction assignment with addition assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `-=` -> `+=` (go, python, rust) |
| `compound-assign/mul->div` | Replace multiplication assignment with division assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `*=` -> `/=` (go, python, rust) |
| `compound-assign/div->mul` | Replace division assignment with multiplication assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `/=` -> `*=` (go, python, rust) |
| `compound-assign/rem->div` | Replace remainder assignment with division assignment. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS; Mothra AOR | cargo-mutants, sufficient | `%=` -> `/=` (go, python, rust) |
| `compound-assign/shl->shr` | Replace left-shift assignment with right-shift assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `<<=` -> `>>=` (go, python, rust) |
| `compound-assign/shr->shl` | Replace right-shift assignment with left-shift assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `>>=` -> `<<=` (go, python, rust) |
| `compound-assign/and->or` | Replace bitwise-and assignment with bitwise-or assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `&=` -> `\|=` (go, python, rust) |
| `compound-assign/or->and` | Replace bitwise-or assignment with bitwise-and assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `\|=` -> `&=` (go, python, rust) |
| `compound-assign/xor->and` | Replace bitwise-xor assignment with bitwise-and assignment. | derived cargo-mutants |  | cargo-mutants, sufficient | `^=` -> `&=` (go, python, rust) |

### `delete-statement`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `delete-statement/call` | Delete a call statement. | derived Cherenkov design; derived Just et al. 2014; similar PIT VOID_METHOD_CALLS; similar Mothra SDL |  | breadth, sufficient |

### `fn-value`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | derived cargo-mutants FnValue |  | cargo-mutants, sufficient |
<!-- /generated -->

Each entry cites its evidence. **[choice]** marks a decision this project
made where the sources do not settle it.

| Operator | Rule | Evidence |
|---|---|---|
| ROR | `<` → `<=`, `!=`, `false`; `>` → `>=`, `!=`, `false`; `<=` → `<`, `==`, `true`; `>=` → `>`, `==`, `true`; `==` → `<=`, `>=`, `false`; `!=` → `<`, `>`, `true` | sufficient sets: Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014 (10 Java programs, 27% fewer mutants, no loss) |
| ROR on `bool` and non-ordered types | only `==` → `false`, `!=` → `true` (no ordering mutants); floats drop the `<`→`<=` boundary | Dextool's type-specific ROR (fewer junk mutants) **[choice: adopt]** |
| COR (operator `lcr`) | `a && b` → `false`, `a`, `b`, `a == b`; `a \|\| b` → `true`, `a`, `b`, `a != b` | Just & Schweiggert 2014 (4 of 10 sufficient) |
| AOR (counterparts) | `+` ↔ `-`; `*` ↔ `/`; `%` → `/` | Dextool AORS ("full AOR highly redundant"). `%` → `/` follows mutmut and Mull **[choice]** |
| Bitwise and shift | `&` ↔ `\|`; `^` → `&`; `<<` ↔ `>>` | PIT, mutmut and Gremlins agree on these pairs |
| Compound assignment (operator `compound-assign`) | the AOR, bitwise and shift rules applied to `op=` | same rules |
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

Every named mutation in `operator_set.rs` carries `origins`, repeated in the
comment above its entry; `cherenkov mutations` and `cherenkov operators`
list them. Each origin is one of three kinds:

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
