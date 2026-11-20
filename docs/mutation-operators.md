# Mutation operators: definitions this project evaluates against

Cherenkov's operators have to be the operators their names claim, or an
evaluation measures the wrong thing. This file pins each one to a primary
source.

**Sources:**
- **[M]** The 22 Mothra (Fortran 77) operators. Names and one-line
  descriptions from Jia and Harman, *An Analysis and Survey of the
  Development of Mutation Testing*, IEEE TSE 37(5), 2011, Table III ("adapted
  from" King and Offutt, Software: Practice and Experience 21(7), 1991).
- **[M?]** Finer detail of a Mothra operator (which replacements it makes),
  from memory of King and Offutt 1991, **not re-checked** against the
  paper. Verify before relying on it.
- **[C]** cargo-mutants' documented mutations, https://mutants.rs/mutants.html,
  read 2026-11-17 through a summarizing fetch (not verbatim).
- **[P]** PIT (Java) operator names, from memory: conditionals boundary, negate conditionals, remove conditionals, empty and null returns, non-void call removal. Not re-checked.

## The 22 Mothra operators [M]

| Op | Description (Table III) | Detail [M?] | Rust meaning |
|---|---|---|---|
| AAR | array reference for array reference replacement | each array reference replaced by each other array reference of the same type | `a[i]` → `b[j]` (same element type) |
| ABS | absolute value insertion | each arithmetic (sub)expression `e` → `abs(e)`, `-abs(e)`, `zpush(e)` (kill when `e == 0`) | `e` → `e.abs()`, `-e.abs()`, "fail if `e == 0`" |
| ACR | array reference for constant replacement | each constant replaced by each array reference of the same type | `3` → `a[i]` |
| AOR | arithmetic operator replacement | each arithmetic operator replaced by each other one, plus `leftop`/`rightop` (the expression replaced by one operand) | `a + b` → `a - b`, `a * b`, `a / b`, `a % b`, `a`, `b` |
| ASR | array reference for scalar variable replacement | each scalar variable reference replaced by each array reference of the same type | `x` → `a[i]` |
| CAR | constant for array reference replacement | each array reference replaced by each constant | `a[i]` → `3` |
| CNR | comparable array name replacement | each array name replaced by another array of the same shape | `a[i]` → `b[i]` |
| CRP | constant replacement | each constant replaced by a different constant (the exact set needs checking) | `3` → … |
| CSR | constant for scalar variable replacement | each scalar variable reference replaced by each constant of the same type | `x` → `3`; also `v[i]` → `v[0]` |
| DER | DO statement alterations | loop end/bounds altered | `for` range bounds (Fortran-specific; closest Rust analogue only) |
| DSA | DATA statement alterations | initialization data altered | none (Fortran DATA) |
| GLR | GOTO label replacement | each GOTO target replaced | none (no `goto`; labelled `break`/`continue` closest) |
| LCR | logical connector replacement | each logical connector replaced by each other | `&&` ↔ `\|\|` |
| ROR | relational operator replacement | each relational operator replaced by each other, and the predicate by `true`/`false` | `<` → `<=`, `>`, `>=`, `==`, `!=`, `true`, `false` |
| RSR | RETURN statement replacement | each statement replaced by a `return` | statement → `return` (with a value for non-`()` functions) |
| SAN | statement analysis | a trap on reaching a statement (coverage, not a fault) | coverage probe, not a mutant |
| SAR | scalar variable for array reference replacement | each array reference replaced by each scalar variable of the same type | `a[i]` → `x` |
| SCR | scalar for constant replacement | each constant replaced by each scalar variable of the same type | `3` → `x` |
| SDL | statement deletion | each statement deleted | delete a statement |
| SRC | source constant replacement | each constant replaced by each other constant in the program | `3` → `7` (another literal in the source) |
| SVR | scalar variable replacement | each scalar variable reference replaced by each other scalar variable of the same type in scope | `a * b` → `b * b`, `a * a` |
| UOI | unary operator insertion | a unary operator (minus; logical negation) inserted before each (sub)expression | `a * b` → `-a * b`, `a * -b`, `-(a * b)`; `c` → `!c` |

## cargo-mutants' mutations [C]

| Genre | Definition |
|---|---|
| Function body → value (`FnValue`) | body replaced by a value of the return type. `()`: `()`. Signed ints: 0, 1, −1. Unsigned: 0, 1. Floats: 0.0, 1.0, −1.0. `bool`: true, false. `String`: `String::new()`, `"xyzzy".into()`. `&str`: `""`, `"xyzzy"`. `&T`: a leaked `Box` of a replacement. `&[T]`: a leaked `Vec`, empty or one element. `Result<T>`: `Ok(r)` for each replacement `r` of `T`. `Option<T>`: `None`, `Some(r)`. `Box`/`Arc`/`Rc<T>`: the wrapped replacement. `Vec<T>` and other collections: empty, one element. Maps: empty, plus key/value combinations. `Cow`: borrowed or owned. `[T; L]`: filled with each replacement. Tuples: element combinations. `impl Iterator`: empty, one element. Anything else: `Default::default()`. Nested recursively |
| Binary operators | `==`↔`!=`. `&&`↔`\|\|`. `<` → `==`, `>`. `>` → `==`, `<`. `<=` → `>`. `>=` → `<`. `+` → `-`, `*`. `-` → `+`, `/`. `*` → `+`, `/`. `/` → `%`, `*`. `%` → `/`, `+`. `<<`↔`>>`. `&` → `\|`, `^`. `\|` → `&`, `^`. `^` → `&`, `\|`. `&=`↔`\|=`. `^=` → `\|=`, `&=`. Other compound assignments → the assignment form of the operator rows above. Equality is never swapped for ordering, and `&=`/`\|=` never become `^=` |
| Unary operator deletion | `-a` → `a`, `!a` → `a` (never replaced by another unary operator) |
| Match arm deletion | an arm deleted, only in matches with a wildcard pattern |
| Match guard replacement | a guard replaced by `true` or `false` |
| Struct literal field deletion | a field deleted from a struct literal that has a base (`..Default::default()`) |

Excluded from mutation: `#[cfg(test)]` code, test functions, `#[mutants::skip]`, `unsafe` functions.

## The 21 rows of docs/rust-operator-evaluation.md, corrected

That evaluation's rows were a merge of the two sets above, and some labels
were wrong. Each row is restated here with the definition it must be
evaluated against.

| Row | Was labelled | Correct definition | Correction |
|---|---|---|---|
| 1 | AOR `+`↔`-` | [C] `+` → `-`, `*`; `-` → `+`, `/`. [M] AOR adds `leftop`/`rightop` | implemented: `+`↔`-` only. Missing: `+` → `*`, `-` → `/`, and the operand forms |
| 2 | AOR `*` `/` `%` | [C] `*` → `+`, `/`; `/` → `%`, `*`; `%` → `/`, `+` | as stated |
| 3 | "ASR" `+=` … | [C] compound assignment replacement | **label wrong**: Mothra's ASR is array-for-scalar replacement |
| 4 | ROR, all swaps | [C] `<` → `==`, `>`; `<=` → `>`; … `==`↔`!=`. [M] ROR: each to each other, plus `true`/`false` | implemented `ror` is the **boundary** swap (`<` → `<=`), which is PIT's conditionals boundary [P], in neither [C] nor the stated [M] detail |
| 5 | condition negation, true/false | [P] negate and remove conditionals; [M] ROR's `true`/`false` | implemented: negation |
| 6 | LCR | [M] LCR, [C] `&&`↔`\|\|` | as stated |
| 7 | bitwise | [C] `&` → `\|`, `^`; … | as stated |
| 8 | shifts | [C] `<<`↔`>>` | as stated |
| 9 | delete `!` | [C] unary deletion | as stated |
| 10 | delete unary `-` | [C] unary deletion | as stated |
| 11 | UOI / ABS | [M] UOI and ABS, as above | **never tested**. The tier B spike's `m = -a` replaced the whole expression, which is not UOI |
| 12 | constant replacement | [M] CRP and SRC | the replacement set is still unverified [M?] |
| 13 | statement deletion | [M] SDL | as stated |
| 14 | non-void call → default | [P] non-void call removal | not in [M] or [C] |
| 15 | body → value ("RSR / FnValue") | [C] `FnValue`, the full type table above | **label wrong**: Mothra RSR replaces statements with `return`, a different operator. Each [C] value is required, not only the zero |
| 16 | match arm deletion | [C] | as stated |
| 17 | match guard → true/false | [C] | as stated |
| 18 | struct field deletion | [C] | as stated |
| 19 | SVR | [M] SVR: one *reference* replaced | **the tier B spike tested the wrong thing**: `m = b` replaced the whole expression `a * b`. Real SVR is `a * b` → `b * b` or `a * a` |
| 20 | "index replacement" | not one operator. `v[i]` → `v[0]` is [M] CSR inside an index. The array family is AAR, ACR, CAR, CNR, SAR, ASR | **misdefined**: to be split into those operators |
| 21 | loop condition | instances of rows 4 and 5 | not a separate operator |

## How mutants are judged

A runtime mutant counts as correct only if the program behaves as the
**compiled source mutant** does: same outputs on the same inputs,
including panics. Checking only that a test fails or passes under the
mutant is not enough, because the wrong mutant can pass the same test.
Each operator is checked with a differential harness: build the source
mutant for real, run both on the same inputs, compare the outputs.
