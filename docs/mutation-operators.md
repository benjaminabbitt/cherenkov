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

## Cherenkov's operators

An **operator** is a family: one kind of change, defined without reference
to a language (replace a relational operator with another, delete a match
arm). A **replacement** is one entry in an operator's table, and each set
uses its own table of each operator it includes (docs/operator-sets.md).
Operators are what is counted: **15 are implemented.** Their replacements
are 69 distinct entries across the three sets (breadth 12 in 6 operators,
`cargo-mutants` 53 in 11, `sufficient` 36 in 8). Earlier versions of these
docs counted 21 "rows" and the code counted 69 "mutations"; neither is the
number of operators.

Naming: an operator is its family id (`ror`), which is the operator id in
mutant IDs. A replacement is `family/detail` (`ror/lt->le`), also in mutant
IDs and in set files; a set file that names a family takes that family's
whole table. The names other tools use are listed as "Also called" and are
documentation only: a tool's name can cover several replacements (PIT's
`CONDITIONALS_BOUNDARY` is four), so none is accepted as input.

Each replacement's source and relation are the `origins` of
`crates/cherenkov-core/src/operator_set.rs`, repeated in the comment above
its entry (a test checks that the comment and the data agree). Relations:
**derived from** (the definition was taken from there), **same as** (a peer
has the identical mutation), **similar to** (a close one; the note says how
it differs). The tables below are generated by `cherenkov operators`
(`just docs` regenerates them; a test fails when they differ from the dump,
and `cherenkov operators --format json` is the same data for tools).

<!-- generated: cherenkov operators -->
Names: an operator is its family id (`ror`); a replacement is `family/detail` (`ror/lt->le`), the name mutant IDs and set files use, and a set file naming a family takes its whole table. "Also called" lists the names other tools use: documentation only, never accepted as input, since one tool name can cover several replacements.

15 operators.

| Operator | Definition | Derived from | Also called |
|---|---|---|---|
| `ror` | Replace a relational operator with another relational operator. | cargo-mutants; Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; Cherenkov design | Mothra ROR |
| `aor` | Replace an arithmetic operator with another arithmetic operator. | cargo-mutants; Dextool AORS; Cherenkov design | Mothra AOR |
| `bitwise` | Replace a bitwise operator with another bitwise operator. | cargo-mutants |  |
| `shift` | Replace a shift operator with the opposite shift. | cargo-mutants |  |
| `lcr` | Replace a logical connector with the other connector, or with an equality test of its operands. | cargo-mutants; Just & Schweiggert 2014 |  |
| `compound-assign` | Replace the operator of a compound assignment with another operator of its kind. | cargo-mutants; Dextool AORS |  |
| `unary` | Delete a unary operator, keeping its operand. | cargo-mutants UnaryOperator | cargo-mutants UnaryOperator |
| `match-guard` | Replace a match arm's guard with a constant boolean. | cargo-mutants MatchArmGuard | cargo-mutants MatchArmGuard |
| `match-arm` | Delete a match arm, when the match has a catch-all arm. | cargo-mutants MatchArm | cargo-mutants MatchArm |
| `struct-field` | Delete a field of a struct literal whose omitted fields take a default. | cargo-mutants StructField | cargo-mutants StructField |
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | cargo-mutants FnValue | cargo-mutants FnValue |
| `delete-statement` | Delete a call statement. | Cherenkov design; Just et al. 2014 |  |
| `negate-condition` | Negate the condition of an if or loop. | Cherenkov design |  |
| `return-bool-flip` | Replace a returned boolean literal with its opposite. | Cherenkov design |  |
| `body-default` | Replace a function body with the zero value of its result type. | Cherenkov design |  |

### `ror`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `ror/lt->le` | Replace less-than with less-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `<` -> `<=` (go, python, rust) |
| `ror/lt->eq` | Replace less-than with equal. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<` -> `==` (go, python, rust) |
| `ror/lt->gt` | Replace less-than with greater-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<` -> `>` (go, python, rust) |
| `ror/lt->ne` | Replace less-than with not-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `<` -> `!=` (go, python, rust) |
| `ror/le->lt` | Replace less-or-equal with less-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `<=` -> `<` (go, python, rust) |
| `ror/le->gt` | Replace less-or-equal with greater-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `<=` -> `>` (go, python, rust) |
| `ror/le->eq` | Replace less-or-equal with equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `<=` -> `==` (go, python, rust) |
| `ror/gt->ge` | Replace greater-than with greater-or-equal. | derived cargo-mutants; derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, cargo-mutants, sufficient | `>` -> `>=` (go, python, rust) |
| `ror/gt->eq` | Replace greater-than with equal. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>` -> `==` (go, python, rust) |
| `ror/gt->lt` | Replace greater-than with less-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>` -> `<` (go, python, rust) |
| `ror/gt->ne` | Replace greater-than with not-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `>` -> `!=` (go, python, rust) |
| `ror/ge->gt` | Replace greater-or-equal with greater-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; derived Cherenkov design; same PIT CONDITIONALS_BOUNDARY; same mutmut; same go-mutesting; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT CONDITIONALS_BOUNDARY | breadth, sufficient | `>=` -> `>` (go, python, rust) |
| `ror/ge->lt` | Replace greater-or-equal with less-than. | derived cargo-mutants; same Mothra ROR |  | cargo-mutants | `>=` -> `<` (go, python, rust) |
| `ror/ge->eq` | Replace greater-or-equal with equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `>=` -> `==` (go, python, rust) |
| `ror/eq->ne` | Replace equal with not-equal. | derived cargo-mutants; same PIT NEGATE_CONDITIONALS; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT NEGATE_CONDITIONALS | cargo-mutants | `==` -> `!=` (go, python, rust) |
| `ror/eq->le` | Replace equal with less-or-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `==` -> `<=` (go, python, rust) |
| `ror/eq->ge` | Replace equal with greater-or-equal. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `==` -> `>=` (go, python, rust) |
| `ror/ne->eq` | Replace not-equal with equal. | derived cargo-mutants; same PIT NEGATE_CONDITIONALS; same Stryker; same Gremlins; same Mull; same Mothra ROR | PIT NEGATE_CONDITIONALS | cargo-mutants | `!=` -> `==` (go, python, rust) |
| `ror/ne->lt` | Replace not-equal with less-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `!=` -> `<` (go, python, rust) |
| `ror/ne->gt` | Replace not-equal with greater-than. | derived Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014; same Mothra ROR |  | sufficient | `!=` -> `>` (go, python, rust) |

### `aor`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `aor/add->sub` | Replace addition with subtraction. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `+` -> `-` (go, python, rust) |
| `aor/add->mul` | Replace addition with multiplication. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `+` -> `*` (go, python, rust) |
| `aor/sub->add` | Replace subtraction with addition. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `-` -> `+` (go, python, rust) |
| `aor/sub->div` | Replace subtraction with division. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `-` -> `/` (go, python, rust) |
| `aor/mul->add` | Replace multiplication with addition. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `*` -> `+` (go, python, rust) |
| `aor/mul->div` | Replace multiplication with division. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `*` -> `/` (go, python, rust) |
| `aor/div->mul` | Replace division with multiplication. | derived cargo-mutants; derived Dextool AORS; derived Cherenkov design; same Mothra AOR | Dextool AORS | breadth, cargo-mutants, sufficient | `/` -> `*` (go, python, rust) |
| `aor/div->rem` | Replace division with remainder. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `/` -> `%` (go, python, rust) |
| `aor/rem->div` | Replace remainder with division. | derived cargo-mutants; derived Dextool AORS; same Mothra AOR | Dextool AORS | cargo-mutants, sufficient | `%` -> `/` (go, python, rust) |
| `aor/rem->add` | Replace remainder with addition. | derived cargo-mutants; same Mothra AOR |  | cargo-mutants | `%` -> `+` (go, python, rust) |

### `bitwise`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `bitwise/and->or` | Replace bitwise and with bitwise or. | derived cargo-mutants |  | cargo-mutants, sufficient | `&` -> `\|` (go, python, rust) |
| `bitwise/and->xor` | Replace bitwise and with bitwise xor. | derived cargo-mutants |  | cargo-mutants | `&` -> `^` (go, python, rust) |
| `bitwise/or->and` | Replace bitwise or with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `\|` -> `&` (go, python, rust) |
| `bitwise/or->xor` | Replace bitwise or with bitwise xor. | derived cargo-mutants |  | cargo-mutants | `\|` -> `^` (go, python, rust) |
| `bitwise/xor->or` | Replace bitwise xor with bitwise or. | derived cargo-mutants |  | cargo-mutants | `^` -> `\|` (go, python, rust) |
| `bitwise/xor->and` | Replace bitwise xor with bitwise and. | derived cargo-mutants |  | cargo-mutants, sufficient | `^` -> `&` (go, python, rust) |

### `shift`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `shift/shl->shr` | Replace left shift with right shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `<<` -> `>>` (go, python, rust) |
| `shift/shr->shl` | Replace right shift with left shift. | derived cargo-mutants |  | cargo-mutants, sufficient | `>>` -> `<<` (go, python, rust) |

### `lcr`

| Replacement | Definition | Sources | Also called | Sets | Spelling |
|---|---|---|---|---|---|
| `lcr/and->or` | Replace logical and with logical or. | derived cargo-mutants; same Mothra LCR | Mothra LCR | cargo-mutants | `&&` -> `\|\|` (go, rust); `and` -> `or` (python) |
| `lcr/or->and` | Replace logical or with logical and. | derived cargo-mutants; same Mothra LCR | Mothra LCR | cargo-mutants | `\|\|` -> `&&` (go, rust); `or` -> `and` (python) |
| `lcr/and->eq` | Replace logical and with equal. | derived Just & Schweiggert 2014 |  | sufficient | `&&` -> `==` (go, rust); `and` -> `==` (python) |
| `lcr/or->ne` | Replace logical or with not-equal. | derived Just & Schweiggert 2014 |  | sufficient | `\|\|` -> `!=` (go, rust); `or` -> `!=` (python) |

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

### `match-guard`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `match-guard/true` | Replace a match arm's guard with the constant true. | derived cargo-mutants MatchArmGuard |  | cargo-mutants |
| `match-guard/false` | Replace a match arm's guard with the constant false. | derived cargo-mutants MatchArmGuard |  | cargo-mutants |

### `match-arm`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `match-arm/delete` | Delete a match arm, when the match has a catch-all arm. | derived cargo-mutants MatchArm |  | cargo-mutants |

### `struct-field`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `struct-field/delete` | Delete a field of a struct literal whose omitted fields take a default. | derived cargo-mutants StructField |  | cargo-mutants |

### `fn-value`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `fn-value` | Replace a function body with each value the language profile generates for its result type. | derived cargo-mutants FnValue |  | cargo-mutants, sufficient |

### `delete-statement`

| Replacement | Definition | Sources | Also called | Sets |
|---|---|---|---|---|
| `delete-statement/call` | Delete a call statement. | derived Cherenkov design; derived Just et al. 2014; similar PIT VOID_METHOD_CALLS; similar Mothra SDL |  | breadth, sufficient |

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

Mothra's relations are recorded only where the one-line description in
Jia and Harman's Table III states the replacement (AOR: each arithmetic
operator by each other; ROR: each relational operator by each other; LCR:
each connector by the other); the finer detail is [M?].

### Not operators here, and not implemented

Rows of the earlier 21-row evaluation that are not operators in Cherenkov,
with what each referred to. None is counted.

| Was | Source and its name for it | Relation | Why it is not an operator here |
|---|---|---|---|
| 11 UOI / ABS | Mothra UOI, ABS. `unary` deletes a unary operator, which is cargo-mutants' UnaryOperator and Dextool's reduced UOI | similar: Mothra inserts, `unary` deletes | **not implemented, never tested.** The tier B spike's `m = -a` replaced the whole expression, which is not UOI. Excluded from `sufficient` as redundant (docs/operator-sets.md) |
| 12 constant replacement | Mothra CRP, SRC; PIT (experimental) | none implemented | **not implemented.** The replacement set is still unverified [M?] |
| 14 non-void call -> default | PIT non-void call removal; mutest-rs 2026 | none implemented | **not implemented;** listed as pending in `sufficient` |
| 19 SVR | Mothra SVR | none implemented | **not implemented.** The tier B spike's `m = b` replaced the whole expression `a * b`; real SVR is `a * b` -> `b * b` or `a * a` |
| 20 "index replacement" | Mothra AAR, ACR, ASR, CAR, CNR, SAR; `v[i]` -> `v[0]` is Mothra CSR inside an index | none implemented | **not one operator:** six Mothra operators, none implemented |
| 21 loop condition | instances of `ror` and `negate-condition` | n/a | **not a separate operator** |
| 3 "ASR" `+=` | cargo-mutants compound assignment | derived | the label was wrong (Mothra's ASR is array-for-scalar replacement). It is the operator `compound-assign` |
| 15 "RSR / FnValue" | cargo-mutants FnValue | derived | the label was wrong (Mothra RSR replaces a statement with `return`). It is `fn-value`, and `body-default` is the zero-value special case |

Parts of implemented operators that are not implemented, also not counted:
`ror` to `true`/`false` (Mothra ROR; Kaminski 2013); `aor` operand forms
`leftop`/`rightop` (Mothra AOR); `lcr` operand and constant forms (Mothra
LCR; Just & Schweiggert 2014 COR); statement deletion beyond call statements
(Mothra SDL); `break` <-> `continue`, range bound `..` <-> `..=` and call
result -> `Default::default()` (mutest-rs 2026).

Note on `ror`: `<` -> `<=` is PIT's conditionals boundary [P] and is also
in cargo-mutants' source (`visit.rs`), though its documented summary omits it.

## How mutants are judged

A runtime mutant counts as correct only if the program behaves as the
**compiled source mutant** does: same outputs on the same inputs,
including panics. Checking only that a test fails or passes under the
mutant is not enough, because the wrong mutant can pass the same test.
Each operator is checked with a differential harness: build the source
mutant for real, run both on the same inputs, compare the outputs.
