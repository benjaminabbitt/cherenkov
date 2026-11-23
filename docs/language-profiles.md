# Language profiles: how a mutation is defined once and realised per language

This document describes the model as the code implements it, and where the
code differs from the design. It is for people who need to add a language or
an operator, and for anyone judging how far the implementation has moved
from `docs/design.md` (draft 4, written before the code).

Every claim is checked against the code or against output from the
`cherenkov` binary. Code is cited as `path:line`. Where something could not
be checked, it says "unverified".

How the outputs below were produced: `cherenkov plan|sets|mutations` from a
release binary built shortly before the last run-layer commits. The planner,
the profiles and the executors have not changed since; only
`deficiency.rs`, `execution.rs` and `verdict.rs` lost some helper methods.
The Rust fixture is `crates/cherenkov-fidelity/fixtures/rust-ops`. The Python
and Go programs are small files written for this document. The catalogue of
cargo-mutants' exact mutations that the comparison with cargo-mutants will
eventually cite is still pending; section 8.2 uses the existing comparison
reports instead.

Contents:

1. The pipeline in one page
2. The concept model
3. A language profile
4. Operators and operator sets
5. The plan
6. Execution
7. A worked example across languages
8. Deviations
9. Open questions and what is next

---

## 1. The pipeline in one page

```
 project directory
   |
   |  SourceReader        cherenkov-fs: walk the tree, keep files whose extension is the
   |                      language's and that no exclude glob matches. Read-only.
   v
 source text
   |
   |  SyntaxPort          cherenkov-syntax: tree-sitter parse + the language's concepts.scm
   v                      -> FileSyntax { statements, concept matches, skip regions }
 FileSyntax
   |
   |  operator_inputs     the matches, plus one synthetic Statement match per statement
   v
 for each match, each Operator whose concepts() contains the match's concept:
   |
   |  Operator::mutate(MatchCx) -> Vec<Edit>      abstract edits, written in tokens
   |      profile.disabled_operators   -> counted, not planned
   |      the set's skip regions       -> counted, not planned
   v
 plan::realize(edit, context) -> Draft { Action, Requires, stop_at, site, anchor }
   |      context = innermost condition / assignment / compound assignment /
   |      return value / match guard around the edit, else "in place"
   v
 sort, deduplicate on (stop_at, action), derive ids
   v
 MutantPlan { id, operator, mutation, action, edit, site, requires, operands,
              original, mutated }
   |                                          `cherenkov plan` stops here
   |  --in-diff filter; for Rust, only files a non-test build compiles
   v
 MutantExecutor (port, in cherenkov-core)
   = TierPExecutor<L: Launcher> in cherenkov-exec-dap, one per test program
   |      locate the function's instructions with the line table
   |      derive a tier P patch from (action, edit)       -> arm, run, disarm
   |      else, GDB only: a tier B effect                 -> stop, evaluate, set pc / return
   |      else Unsupported(reason)
   v
 RawOutcome -> classify -> Verdict + tier -> the run layer (events, workers, store)
```

What is language-neutral, what is data, and what is an adapter:

| Part | Where | Kind |
|---|---|---|
| Concepts, captures, tokens, edits, operators, sets, planner, ids, verdict classification | `crates/cherenkov-core/src/` | Neutral code. Nothing branches on language: `LanguageId` is an opaque string (`syntax.rs:14`) and no source file in core, syntax or exec-dap compares it to `"rust"`, `"go"` or `"python"`; the only literals are the registrations in `profiles.rs:207-233`, the Delve adapter id and the Delve calling-convention name. |
| Which grammar nodes are which concept | `profiles/<lang>/concepts.scm` | Data (tree-sitter queries). |
| How a token is spelled, how to negate | `profiles/<lang>/tokens.toml` | Data. |
| File extensions, exclude globs, default values, body syntax, comment syntax, disabled operators | `profiles/<lang>/profile.toml` | Data. |
| Function-body replacement values for a result type | `ReturnValues` function pointer (`profile.rs:22`), set only for Rust (`profiles.rs:231`) | Code, one per language that has it. |
| Instruction shapes and patch rules | `isa/amd64.toml`, `crates/cherenkov-exec-dap/src/{isa,x86,tierp}.rs` | Data plus hand-written amd64 code. |
| Debugger sessions | `crates/cherenkov-exec-dap/src/{gdb,delve}.rs` | Adapters. |

---

## 2. The concept model

The capture contract is Cherenkov's own. tree-sitter supplies the query
language (S-expression patterns, field names, anchors, text predicates) and
the `.scm` file extension. It does not define what a capture name means:
the conventions in the `highlights.scm` and `tags.scm` files shipped with
grammars (`@keyword`, `@definition.function`) are tree-sitter's, and none
of them is used here. A Cherenkov capture name is read by
`capture_role` (`crates/cherenkov-syntax/src/profiles.rs:115`):

| Capture name | Meaning |
|---|---|
| `@concept.<name>` | The captured node is a match of that [`Concept`](#concepts). |
| `@stmt.<kind>` | The captured node is a statement of kind `call`, `assign`, `return`, `if` or `loop` (`syntax.rs:111`). |
| `@skip.<kind>` | The captured node is a region a set may decline to mutate: `test`, `annotated`, `unsafe`, `constructor`, `default_impl`, `capacity_call` (`syntax.rs:204`). |
| `@<role>` | A named part of the concept match in the same query pattern: one of the 21 [`Capture`](#roles) roles. |
| `@_<anything>` | A helper capture, there only to feed a predicate such as `#eq?`. Ignored. |

Any other name is a load error (`ProfileError::UnknownCapture`). Predicates
used by the three profiles are the built-in `#eq?`, `#match?` and
`#not-match?` (Rust profile, `concepts.scm:27,33,81`); there are no custom
predicates.

One concept match is one grammar node. A query that matches the same node
several times (a repeated field such as a chained comparison's operators)
is folded into one `ConceptMatch` with every distinct role capture
(`crates/cherenkov-syntax/src/lib.rs:74-111`). The core then decides what a
repeated role means: `Binary` must have exactly one `@op`, `Condition` one
`@cond`, `Assign` one `@target` and one `@value`; otherwise the match is
counted as *unrepresentable* and skipped (`plan.rs:473`).

<a id="concepts"></a>
### Concepts

`crates/cherenkov-core/src/concept.rs:11`. Sixteen. Query lines are the
`@concept.<name>` lines in each profile's `concepts.scm`.

| Concept | Meaning | Rust | Python | Go |
|---|---|---|---|---|
| `Binary` | Binary operator expression; roles `op`, `lhs`, `rhs`. | 3 | 7, 8, 9 | 3 |
| `Unary` | Prefix operator; roles `op`, `operand`. | 4 | 11, 12 | 4 |
| `CompoundAssign` | `t op= v`; roles `target`, `op`, `value`. | 5 | 14 | 8 |
| `Literal` | A literal. No role. | 9 | 17 | 12 |
| `Condition` | `if` or loop condition; roles `cond`, `then`, `else`. | 17, 21 | 22, 23 | 14, 19, 20 |
| `Return` | A return value: `return x`, and for Rust a closure body or a function's tail expression. Role `value`. | 23, 26, 32 | 25 | 23 |
| `Call` | A call; roles `callee`, `args`. | 34 | 26 | 24 |
| `Assign` | Assignment or declaration with initializer; roles `target`, `value`. | 6, 7 | 15 | 9, 10 |
| `Expr` | Arithmetic expression as an insertion site. | none | none | none |
| `Name` | Variable reference; role `ident`. | none | none | none |
| `Index` | Subscript; roles `base`, `index`. | 35 | 27 | 25 |
| `Statement` | A statement. Not captured with `@concept.statement`; produced from every `@stmt.*` capture, and a synthetic match is added per statement (`operator.rs:46`). | via `@stmt.*` | via `@stmt.*` | via `@stmt.*` |
| `Block` | A block. | none | none | none |
| `Function` | A function or method; roles `name`, `returns`, `body`, and for Rust `async`. | 38 | 29 | 27, 28 |
| `Arm` | One arm of a `match` or `switch`; roles `guard`, `scope`, `catch_all`, `value` (the arm's body). | 56, 57 | 42, 43 | 40-46 |
| `FieldInit` | One field of a struct literal whose omitted fields take a default; role `scope`. | 63 | none | 52 |

<a id="roles"></a>
### Roles

`concept.rs:82`. `op`, `lhs`, `rhs`, `operand`, `target`, `value`, `cond`,
`then`, `else`, `callee`, `args`, `ident`, `base`, `index`, `name`, `body`,
`returns`, `guard`, `scope`, `catch_all`, `async`. `ident` is never captured
by any profile (its concept, `Name`, is unmapped). `callee`, `args`, `base`
and `index` are captured but no core code reads them; `Call` matters only
because the planner asks whether a condition or value contains one
(`plan.rs`, `calls_in`).

### Which concepts operators consume

Concepts are not operators; several operators match one concept
(`concept.rs:5`). Nine concepts have an operator: `Binary` and
`CompoundAssign` (token swaps), `Unary` (deletion), `Condition` (negation),
`Return` (boolean flip), `Function` (body replacement), `Statement` (call
deletion), `Arm` (guard replacement, arm deletion), `FieldInit` (field
deletion). `Call` and `Assign` are read by the planner for context.
`Literal` and `Index` are captured and used by nothing. `Expr`, `Name` and
`Block` are in the vocabulary and no profile maps them.

---

## 3. A language profile

A profile is three data files in `profiles/<lang>/` plus one line in
`profiles::builtin()` (`profiles.rs:207`) that embeds them with
`include_str!` and names the grammar crate.

### `concepts.scm`

Tree-sitter queries with the capture contract above. Each pattern has the
shape `(grammar_node field: (_) @role ...) @concept.name`. Excerpts, from
the profiles:

```scheme
; profiles/rust/concepts.scm:3
(binary_expression left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
; profiles/python/concepts.scm:8-9
(boolean_operator left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(comparison_operator operators: _ @op) @concept.binary
; profiles/go/concepts.scm:14
(if_statement condition: (_) @cond consequence: (block) @then alternative: (_)? @else) @concept.condition
```

### `tokens.toml`

`not_template` (must contain exactly one `{}`) and a `[tokens]` table from
the 34 abstract token names (`token.rs:9`: arithmetic, bitwise, shift,
the ten compound-assignment forms, six comparisons, `and`, `or`, `assign`,
`true`, `false`, unary `not` and `neg`, and the list separator `sep`) to the
language's spelling. The table is read both ways: a captured `@op` text is
turned into a `Token` (`TokenTable::token`), and an edit's `Token` is turned
back into text (`TokenTable::spell`). Unary spellings have their own
namespace, because `-` is both `Sub` and `Neg`.

| Token | Rust | Go | Python |
|---|---|---|---|
| `and`, `or` | `&&`, `\|\|` | `&&`, `\|\|` | `and`, `or` |
| `not`, `neg` | `!`, `-` | `!`, `-` | `not`, `-` |
| `true`, `false` | `true`, `false` | `true`, `false` | `True`, `False` |
| `not_template` | `!({})` | `!({})` | `not ({})` |

The three files otherwise spell every token the same way.

### `profile.toml`

Parsed with `deny_unknown_fields` (`profiles.rs:77,85`):

| Key | Meaning | Rust | Go | Python |
|---|---|---|---|---|
| `extensions` | Which files are this language's. | `rs` | `go` | `py` |
| `exclude` | File globs to skip, relative to the project root. | `target`, `tests`, `benches`, `examples`, `build.rs` | `*_test.go`, `vendor`, `testdata` | `test_*.py`, `*_test.py`, `tests`, `conftest.py`, `.venv`, `venv` |
| `unit_types` | Result types that mean "returns nothing"; no body replacement. | `()`, `!` | none | `None` |
| `disabled_operators` | Operator ids this language declines. | none | none | none |
| `defaults` | Result type text to the literal a body replacement returns. | bool, 12 integers, `f32`, `f64` | bool, 11 integers, floats, `string`, `error` | `bool`, `int`, `float`, `str` |
| `body_template` | Source of a body that returns a value; `{}` is the value. | `{}` | `return {}` | `return {}` |
| `body_braces` | Bodies are `{...}`; contents lie between. | true | true | false |
| `line_comment`, `block_comment` | So a body's contents start at its first code. | `//`, `/* */` | `//`, `/* */` | `#` |

### How a profile is loaded and checked

`profiles::load` (`profiles.rs:130`) does the following, in order, and
returns the first error:

1. Parse `tokens.toml`. A token name that is not one of the 34 is
   `UnknownToken`. Two tokens with one spelling, or a `not_template` without
   exactly one `{}`, are `Tokens` errors (`token.rs:197`).
2. Parse `profile.toml`.
3. Compile `concepts.scm` against the grammar. A query error is `Query`.
4. Check every capture name in the query with `capture_role`. An unknown
   name is `UnknownCapture`.
5. Compute `mapped`: the concepts named by an `@concept.*` capture in the
   query, plus `Statement` if any `@stmt.*` capture exists
   (`profiles.rs:163-172`).

`mapped` is a fact about the query text, not about matches. A concept that
the query names but that never matches in a given file is still mapped.
Nothing checks that every token a set can ask for has a spelling, or that
`body_template` contains `{}`: a missing spelling shows up later as a
mutant with no source form (`mutated: None`) or as a skipped edit.

### How unmapped concepts are reported

`plan_file` starts every file's `Diagnostics` with the concepts missing from
`profile.mapped` (`plan.rs:283-289`) and `cherenkov plan` prints them on
stderr (`crates/cherenkov-cli/src/main.rs:352`). Real output:

```
$ cherenkov plan x/py --format text            # Python
python: 9 mutants; concepts mapped 12/16; unmapped: expr, name, block, field_init
$ cherenkov plan x/go --format text            # Go
go: 5 mutants; concepts mapped 13/16; unmapped: expr, name, block
$ cherenkov plan x/rust --format text          # Rust
rust: 5 mutants; concepts mapped 13/16; unmapped: expr, name, block
```

The same line also reports `unrepresentable`, `skipped (no action for
context)`, `skipped regions`, `disabled` and `deduplicated` counts when they
are non-zero. An unmapped concept produces no error and no mutants.

### How to add a language

1. Add the grammar crate to the workspace `Cargo.toml` and to
   `crates/cherenkov-syntax/Cargo.toml`.
2. Write `profiles/<lang>/concepts.scm`. Map each concept the grammar has,
   with its roles; add `@stmt.<kind>` for the statement kinds; add
   `@skip.<kind>` only if the language has the idea. Read
   `profiles/rust/concepts.scm` for roles that are subtle (`@then` is the
   block, `@else` the first alternative, `@value` of an arm is its body,
   `@scope` ties arms and fields to their enclosing match or literal).
3. Write `tokens.toml` (all 34 tokens the language has; omit the ones it
   lacks) and `profile.toml`.
4. Register the three files in `profiles::builtin()` (`profiles.rs:207`).
   If the language has a body-replacement value rule, supply a
   `ReturnValues` function; without it, `fn-value` plans nothing for the
   language.
5. Add a fixture under `fixtures/<lang>/` with an `expected-plan.txt`, add
   the language's sample program to
   `crates/cherenkov-syntax/tests/cross_language.rs` (it requires every set
   to plan the same named mutations in every language), and read the
   `concepts mapped N/16` line.
6. To run mutants, add an executor: a `Launcher` and a `NativeSession` for
   the language's debugger, an ISA table for the architecture and calling
   convention (a `return_stub` key), a toolchain function that builds the
   test program, and a branch in `crates/cherenkov-cli/src/run.rs:107` that
   recognises the project (today `go.mod` or `Cargo.toml`).

Step 6 is where a language costs the most; steps 1 to 5 are what makes
`plan` work, which is all Python has today.

---

## 4. Operators and operator sets

### The `Operator` trait

`crates/cherenkov-core/src/operator.rs:71`:

```rust
pub trait Operator: Send + Sync {
    fn skips(&self) -> &[SkipKind] { &[] }          // regions this operator plans nothing in
    fn id(&self) -> &'static str;                   // the family: "ror", "aor", "fn-value", ...
    fn concepts(&self) -> &'static [Concept];
    fn mutate(&self, cx: &MatchCx<'_>) -> Vec<Edit>;
}
```

`MatchCx` gives an operator the file, the profile, the concept match and the
innermost statement. An operator reads roles through the profile's token
table (`cx.token(Capture::Op)`) and never sees a language. It returns edits;
it never builds text.

The operators in the code:

| Operator (id) | Concepts | Edits |
|---|---|---|
| `TokenSwap` (`aor`, `ror`, `lcr`, `cor`, `shift`, `bitwise`) | `Binary`, `CompoundAssign` | `ReplaceToken` per table row |
| `DeleteUnary` (`unary`) | `Unary` | `DeleteToken` |
| `DeleteStatement` (`delete-statement`) | `Statement` | `DeleteStatement`, call statements only (`operator.rs:101`) |
| `NegateCondition` (`negate-condition`) | `Condition` | `WrapNot` |
| `ReturnBoolFlip` (`return-bool-flip`) | `Return` | `ReplaceWithLiteral(Bool)` for `true`/`false` literals only |
| `BodyDefault` (`body-default`) | `Function` | `ReplaceWithLiteral(Default)` |
| `FnValue` (`fn-value`) | `Function` | `ReplaceWithLiteral(Text)` per generated value |
| `MatchGuard` (`match-guard`) | `Arm` | `ReplaceWithLiteral(Bool)` on the guard |
| `DeleteArm` (`match-arm`) | `Arm` | `DeleteMatch`, only when the match has a catch-all and the arm is neither it nor guarded |
| `DeleteField` (`struct-field`) | `FieldInit` | `DeleteMatch` |

### Edits

`crates/cherenkov-core/src/edit.rs:20`:

| `Edit` | Rendered as (`plan.rs`, `replacement`) |
|---|---|
| `ReplaceToken { capture, from, with }` | The profile's spelling of `with`. `from` is kept so a backend can map the edit to an instruction without re-reading the source. |
| `DeleteToken { capture, token }` | Nothing: `!x` becomes `x`. |
| `WrapNot { capture }` | The captured text through `not_template`. |
| `ReplaceWithLiteral { capture, lit }` | `Lit::Bool` is the profile's `true`/`false`. `Lit::Default` is `body_template` with `defaults[result type]`. `Lit::Text(v)` is `body_template` with `v`. |
| `DeleteStatement` | Nothing. |
| `DeleteMatch` | Nothing; the planner extends the span over the separator that follows (`with_separator`, so `a: 1,` goes whole). |

`Edit::describe` (`edit.rs:212`) is language-neutral text such as
`replace-token op lt -> le`. It is part of the mutant id.

### Named mutations

A mutation is defined once, in a superset, under a stable `family/detail`
name, and carried out by a reusable `Rule` (`operator_set.rs:19`). There are
69 (`cherenkov mutations`):

| Family | Count | Family | Count |
|---|---|---|---|
| `aor` | 20 | `lcr` | 2 |
| `ror` | 20 | `cor` | 2 |
| `bitwise` | 10 | `unary` | 2 |
| `shift` | 4 | `match-guard` | 2 |
| `delete-statement`, `body-default`, `fn-value`, `match-arm`, `negate-condition`, `return-bool-flip`, `struct-field` | 1 each | | |

Each mutation lists its origins (`operator_set.rs:56`). An origin is
`Derived` (the definition was taken from there; at least one is required,
`tests/provenance.rs:11`), `Same` (a peer tool has the identical mutation)
or `Similar(note)`. Real output:

```
$ cherenkov mutations
ror/lt->le
  derived from cargo-mutants
  derived from Kaminski, Ammann & Offutt 2013; Just & Schweiggert 2014
  derived from Cherenkov design
  same as PIT CONDITIONALS_BOUNDARY
  same as mutmut
  same as go-mutesting
  same as Stryker
  same as Gremlins
  same as Mull
ror/lt->eq
  derived from cargo-mutants
...
```

A plan carries the name of the mutation that made it (`MutantPlan.mutation`,
found by `mutation_for`, `operator_set.rs:1196`: operator family plus the
`from`/`with` tokens for token rules).

### Sets

A set is a list of mutation names, plus a name, a source, the regions it
skips and a `pending` list of rules in its specification not implemented yet
(`operator_set.rs:1224`). The first in the registry is the default.

`--operators` takes several, comma-separated: `--operators
cargo-mutants,breadth,mine.toml`. An item ending in `.toml` defines a set of
one's own (`name`, optional `summary` and `source`, `mutations` from
`cherenkov mutations`, optional `skips`); it is read by the CLI and checked
against the mutation table (`set_spec.rs`). Each set plans with its own
operators and skips (`plan_file_sets`, `plan_sets.rs`); the lists are merged
in the order asked for. Two mutants with the same span and replacement are one
mutant (counted in `Diagnostics.deduplicated`), and `ProjectPlan.provenance`
records, for each mutant id, the sets that produced it. Text output ends
with `provenance: breadth 12, cargo-mutants 53; in more than one set: 9`, and
`run.started` names the sets joined with commas. Provenance is not yet in
`cherenkov.mutant.v1.MutantPlan` (a repeated `sets` field would be the
change): the listing of mutants does not show it.

```
$ cherenkov sets
breadth (default): the six prototype operators: one per action kind (design section 4.9)
  source: docs/design.md section 4.9
  operators: delete-statement, aor, ror, negate-condition, return-bool-flip, body-default
cargo-mutants: what cargo-mutants generates, for a mutant-by-mutant comparison
  source: cargo-mutants 27.1.0
  operators: ror, lcr, aor, shift, bitwise, unary, match-arm, match-guard, struct-field, fn-value
  pending: files: only those in a package's module tree, not behind #[cfg(test)] mod
  pending: `&&` inside let chains (cargo-mutants swaps it; the mutant never compiles)
sufficient: non-redundant sufficient sets plus operators real faults need
  source: docs/operator-sets.md (Kaminski 2013, Just & Schweiggert 2014, Just et al. 2014, mutest-rs 2026)
  operators: ror, cor, aor, shift, bitwise, delete-statement, fn-value
  pending: ror: predicate -> true/false
  pending: ror: type-specific limits (bool, floats)
  pending: cor: lhs, rhs, true/false
  pending: unary operator deletion
  pending: statement deletion beyond call statements (assignments, return, break, continue)
  pending: break <-> continue
  pending: range bound .. <-> ..=
  pending: call result -> Default::default()
  pending: match arm deletion, match guards, struct field deletion
```

| Set | Mutations | Skips | Notes |
|---|---|---|---|
| `breadth` | 12 | none | The six design operators as 12 named mutations. The default. |
| `cargo-mutants` | 53 | all six `SkipKind`s | What cargo-mutants 27.1.0 generates, for comparison. |
| `sufficient` | 36 | `test`, `annotated` | From the sufficient-mutation literature plus `fn-value` and call deletion. |

`OperatorSet::operators()` turns the names into operators: all
`ReplaceToken` rules of one family become one `TokenSwap` table; the
parameterised families (`unary`, `match-guard`) become one operator each; the
rest are single rules. A set that skips regions wraps each operator in
`Honouring` (`operator_set.rs:1239`), which only reports the skip kinds; the
planner does the declining (`plan.rs:316-327`).

---

## 5. The plan

### `MutantPlan`

`plan.rs:126`:

| Field | Meaning |
|---|---|
| `id` | 16 hex digits, see below. |
| `operator` | The family. |
| `mutation` | The named mutation, if any. |
| `file`, `function` | Path relative to the root; innermost enclosing function name, `None` at module level. |
| `stop_at` | Where a debugger stops: the statement, condition or guard start; function entry for a body replacement. |
| `action` | What to do there (below). |
| `edit` | The abstract edit. |
| `site` | Span of the capture the edit changes. |
| `requires` | Flags computed syntactically (below). |
| `operands`, `operand_texts` | The operands of a mutated operator *as the mutated source would parse them* (`regrouped`, `plan.rs:489`): swapping the first `&&` of `a && b && c` to `\|\|` gives `a \|\| (b && c)`, so the operands are `a` and `b && c`. |
| `original`, `mutated` | Text at `site` before and after; `mutated` is `None` when the mutant has no source form (a body replacement whose value the profile does not know). |

`render_source` (`plan.rs:406`) splices `mutated` into the file; the
fidelity harness builds that file and compares test outcomes with the
run-time mutant's.

### `Action`

`plan.rs:31`. The statement kind and context decide the action
(`realize`, `plan.rs:531`):

| Action | When | Resume or target |
|---|---|---|
| `EvalThenJump { text, target, value, resume }` | A token swap inside an assignment's value or a compound assignment. `target` and `value` are apart so a backend need not parse `text`. | `AfterStatement` |
| `JumpOnly { resume }` | `DeleteStatement`. | `AfterStatement` |
| `EvalBranch { cond_text, if_true, if_false }` | A condition, a negated condition, or a match guard. | `BlockStart(then)`; `BlockStart(else)` or `AfterStatement`. For a guard: the arm body, or past the arm. |
| `EvalReturn { value_text }` | A token swap in a return value, or `return-bool-flip`. | none (the frame is popped) |
| `InPlace { expr_text }` | A token swap with no statement-level context, or a struct field deletion. | none: applied at the operator itself |
| `Redirect { from, to }` | A deleted match arm: its body continues as the catch-all's body. | the two bodies |
| `ReturnDefault { value_text, zero_bits, async_fn }` | A body replacement. `zero_bits` says the value is the result type's all-zero bits, which a register-zeroing stub can return exactly. | none |

`Target` has two variants, both structural spans: `AfterStatement(Span)` and
`BlockStart(Span)` (`plan.rs:22`). The driver resolves them to addresses
because only it has the line table.

On the Rust fixture, the three sets plan different numbers of mutants
and different action mixes:

```
$ cherenkov plan crates/cherenkov-fidelity/fixtures/rust-ops --operators breadth        -> 177 mutants
$ cherenkov plan crates/cherenkov-fidelity/fixtures/rust-ops --operators sufficient     -> 285 mutants
$ cherenkov plan crates/cherenkov-fidelity/fixtures/rust-ops --operators cargo-mutants  -> 357 mutants
    157 return-default   92 eval-return   53 eval-then-jump   51 eval-branch   3 redirect   1 in-place
    by operator: fn-value 157, aor 118, ror 52, bitwise 10, lcr 6, unary 4, shift 4,
                 match-arm 3, match-guard 2, struct-field 1
rust: 357 mutants; concepts mapped 13/16; unmapped: expr, name, block; skipped regions: annotated 9, default_impl 2
```

(The single `in-place` plan is the struct field deletion; `jump-only` does
not occur because `cargo-mutants` has no statement deletion.)

### `Requires`

Three flags (`plan.rs:100`): `CALL_IN_EVAL` (the evaluated text contains a
call), `ASSIGN_IN_EVAL`, `FORCE_RETURN`. They are computed from the concept
matches (`calls_in`), shown in listings (`[assign]`, `[force-return]`,
`[call]`) and **not read by any executor**: a grep for the flag names finds
only the planner and tests. See D10.

### Ids

`MutantId::derive` (`id.rs:26`): SHA-256 over length-prefixed fields: path,
function (absent is distinct from empty), operator family, the *anchor*,
the edit's `describe()`, and an occurrence index; first 8 bytes as 16 hex
digits. The anchor is the text the mutant hangs on: the statement for
assignment, deletion and in-place mutants, the condition text for branches,
the guard text for guards, the arm text for deleted arms, empty for body
replacements. Whitespace runs in the anchor are collapsed, so reformatting
does not change an id. The occurrence index counts identical
(function, operator, anchor, edit) in source order, so two `i += 1`
statements do not collide.

### Deduplication

After sorting by (`stop_at`, operator, edit, site), a plan is dropped if an
earlier one has the same `(stop_at, action)` (`plan.rs:350`). The count is
`Diagnostics.deduplicated`. The key ignores operator, edit and site.
No test exercises a collision between two different mutants on it;
whether one exists is unverified.

### `unsupported` and reason codes

The planner never decides "unsupported": it emits everything it can express.
Unsupported is an *executor* outcome (`RawOutcome::Unsupported { reason }`,
`execution.rs`), and the free-text reason is reduced to a code by
substring matching (`deficiency.rs:36`):

| Code | Meaning |
|---|---|
| `inlined` | The function has no out-of-line copy to return from. |
| `generic-copies` | A generic function's copies cannot all be patched. |
| `value-not-buildable` | The replacement value cannot be built in the debugger. |
| `value-name-unresolved` | The value names something the support library does not import. |
| `evaluation-hung` | The debugger's evaluation never returned. |
| `function-not-found` | The debugger cannot find a function to call. |
| `symbol-not-in-scope` | A name the expression needs is not visible. |
| `evaluator-cannot-apply` | Another limit of the debugger's evaluator. |
| `debugger-failed` | The debugger session failed on this mutant. |
| `no-breakpoint-site` | The compiled code has no site matching the source. |
| `no-patch-rule` | No instruction patch exists for this shape. |
| `other` | Anything else. |

`unviable_code` (`deficiency.rs:10`) has two codes for the other outcome,
`Unviable` (the mutated source would not compile): `no-default` and
`does-not-compile`. The plan's own `Diagnostics` has a different, planner-side
vocabulary: `unmapped`, `unrepresentable`, `disabled`, `skipped` (per
operator, by `no action for context` or a skip region) and `deduplicated`.

---

## 6. Execution

### The port

`MutantExecutor` (`execution.rs:197`): `run(plan, budget)`, `baseline(budget)`,
and variants that also return the program's output; `recording` (one
unmutated run that records which tests call each planned function, so a
mutant runs only those tests, and a mutant no test calls is `not-called`),
`select_tests`, `describe`, `close_session`. Executors return `RawOutcome`
facts; `classify` (`execution.rs:103`) is the only place a verdict is made:
exit code 0 is `Survived`, any other exit or a crash is `Killed`, a timeout
is `Timeout`, and `NotReached`, `Unsupported` and `Unviable` pass through.
`combine` merges one mutant's outcomes across several test programs: the
first detection decides.

### Tier selection

There is one executor for every native debugger,
`TierPExecutor<L: Launcher>` (`native.rs:281`). A `Launcher` starts a
`NativeSession` (`native.rs:114`), the primitives a debugger provides:
`function_code`, `read`, `write`, `run_reaching`, `run_recording`,
`select_tests`, `restart`, and optionally `run_evaluating` for tier B. The
two implementations are `GdbSession` (`gdb.rs:530`) and `DelveSession`
(`delve.rs:155`). The choice of tier is made inside the executor, per
mutant (`run_in`, `native.rs:993`, `derive_all`, `native.rs:944`):

1. Locate every copy of the plan's function with the line table
   (`locate_all`). No code: `NotReached`.
2. Derive a tier P patch over all copies (`tierp::derive`). If a patch
   exists, arm it, run with a one-shot breakpoint on the patched addresses
   (so "never executed" is `NotReached`), disarm.
3. If no patch can be derived and `effect_of(plan)` exists (an assignment,
   a returned value, a branch, a `&&`/`||` swap, a negated computed
   boolean), run it as tier B: stop at the mutant's stops and apply the
   effect.
4. Otherwise `Unsupported` with the patch's reason, e.g. `no tier P rule
   for aor InPlace` (code `no-patch-rule`).

The tier that applied is recorded in the verdict (`Tier::{A, B, P}`,
`execution.rs:15`). `Tier::A` has no producer; see D15.

### Tier P: what an `Edit` becomes

The patcher is pure: the driver supplies the function's instructions (bytes,
line, column), the ISA table supplies the rules (`tierp.rs:178`,
`derive_in_context`, `tierp.rs:326`). A patch is one or more regions,
`original` and `replacement`, applied and removed together; `arm` refuses to
write unless memory still holds every `original` (`native.rs:191`), and
`disarm` verifies the read-back.

| Action and edit | Patch (amd64) |
|---|---|
| `EvalBranch` with `WrapNot` | Invert the condition code of the single conditional jump on the condition. |
| `EvalBranch` with `ReplaceToken` on a comparison | Rewrite the jump's condition code. Polarity (does the jump go to the then-block?) and operand order are derived from where the jump lands; signedness from the condition code. |
| `EvalBranch` with `ReplaceWithLiteral(Bool)` | Replace the jump with an unconditional jump to its target, or to the next instruction, padded to length. |
| Any `ReplaceToken` on a comparison whose result is a value | The `setcc` that makes the bool, its condition code rewritten (`compared_value`). |
| `JumpOnly` with `DeleteStatement` | `JMP` over the statement, ending at its last call instruction; refused if the statement branches before it. |
| `EvalThenJump` with `+` to `-` or `-` to `+` | The add or sub opcode (or `inc`/`dec`) swapped in place. Tried by line first, then by column. |
| An arithmetic `ReplaceToken` (plain or compound) in any other action | The operation's instruction, found by the line-table column of its expression (an argument, a field, a returned value, a condition, a compound assignment): add/sub with its overflow check, scalar float `+ - * /`, and the bitwise compound assignments. See "Tier P arithmetic: the shapes gate". |
| `EvalReturn` with a bool flip | The immediate of the store of the returned constant, or `xor eax,eax` to `mov al,1`. |
| `ReturnDefault` with `zero_bits` | An entry stub written over the function's first bytes: zero the result registers, `RET`. The stub is per calling convention (`sysv`, `go`). |
| `Redirect` | A `JMP` over the first instructions of the deleted arm's body to the catch-all body, found by exact line and column. |
| `==` to `!=` or back, `!` deletion (`negatable`) | Negate the bool where the program consumes it: invert the consuming jump, `and`/`xor` on `al`, or `setcc`. |

Everything else in the tier P column is `UnsupportedEdit`. In particular:
integer `*`, `/`, `%` swaps, shifts, plain (non-compound) bitwise operators,
`+`/`-` to `*`/`/` on integers, and non-zero body values have no patch.

### Tier P arithmetic: the shapes gate

An in-place opcode swap is right only where the compiler's instruction
selection makes it so, and a compiler upgrade can change that. So arithmetic
of Rust code is patched only for a *shape* the gate has shown right with the
pinned rustc. A shape is the kind of operation (integer with its overflow
check `of` or `carry`, float, bitwise), the operand width, the swap, the
source instruction's encoding and whether its second operand is in memory
(`int w32 of add->sub 83/0 reg`).

- **The gate** (`crates/cherenkov-exec-dap/tests/tierp_gate`, in `just test`,
  alone as `just tierp-gate`): for every arithmetic or bitwise operator
  replacement the planner makes (all operator sets) in a one-function snippet
  of each integer and float type and each operand form (register pair,
  literal on either side, field, call argument, chain, `&mut`, hash-map entry,
  index, compound assignment), it compiles the original and the mutated source
  with rustc (debug, overflow checks on), derives the real patch from the
  original's line table (`tierp::derive_shaped`, over the instructions and
  columns `lines::rows` reads), patches the original's machine code and runs
  both natively: edge values (0, 1, -1, MIN, MAX and their neighbours, NaN,
  infinities, signed zeros), seeded random inputs, and every value of an
  8-bit type. The results and the inputs that panic must be the same.
  Cases whose original and mutant agree on every input are no evidence and
  do not pass a shape.
- **The table** (`isa/tierp-shapes.toml`): the shapes that passed, and every
  other shape the rules can make, for one rustc (`rustc -vV` release and
  commit). `just tierp-harvest` regenerates it with the toolchain in use and
  fails on any difference, so a toolchain bump shows as a diff to review
  (`just check` fails on a stale table too). Tier P declines an arithmetic
  shape of Rust code that is not in `pass` (`no harvested rustc shape ...`);
  Go code is not covered by it. A test fails when the rules (`isa/amd64.toml`)
  can make a shape the table has not decided.
- **Hand rules the table does not replace:** a commutative source (`+`, `*`)
  swapped to `-` or `/` is declined when the instruction's second operand is
  in memory (rustc folds the left operand of `a += b`, `s.x + k`, `*m += k`,
  `v[i] += k` from memory and puts the right one in the register, so the
  swap would compute `b - a`), and when the left operand is a literal; an
  unsigned check loses its compare when swapped, so `inc`/`dec` (which leave
  the carry flag alone) are declined under it; integer `*` has no rule.
- **The panic call** of a patched overflow check is kept (`panic_call =
  "kept"` in the table): after an add to sub patch the panic still says
  "attempt to add with overflow". The verdict of a test is the compiled
  mutant's; a `#[should_panic(expected = ...)]` on the message could differ.
- **Not covered:** arm64 has no arithmetic rules; optimised builds were not
  harvested; the operands the gate runs are scalars, so a shape is shown
  right on straight-line snippets, not in every enclosing function.

### Tier B: stop, evaluate, set the PC or return

Only the GDB adapter implements `run_evaluating` (`gdb.rs:565`). It loads
`gdb_tierb.py` into GDB through the repl, sets instruction breakpoints at the
mutant's stops, and at each hit runs an `Effect` (`native.rs:41`):

| Effect | From | What it does |
|---|---|---|
| `Assign { target, value }` | `EvalThenJump` | Evaluate `value` with other threads held, store the integer into the local's stack slot, then set the PC to after the original's store. |
| `Return { value }` | `EvalReturn`, `ReturnDefault` with a value | Evaluate and `return` it from the frame (scalar results only). |
| `ReturnBuilt` / `ReturnConstructed` | `ReturnDefault` with a compound result | Return a value built from the DWARF layout, or made by a compiled constructor from the support library (`docs/support-library.md`). |
| `Branch { condition, if_true, if_false }` | `EvalBranch` | Evaluate the mutated condition and set the PC to one of the two exits found in the function's code. |
| `FlipResult` | negated computed boolean | Flip `al` after a call, at every hit. |
| `Steer { steers }` | `&&`/`||` swap | At each jump out of the left operand, read the flags and continue where the mutant would go; nothing is evaluated. |
| `Guarded { guard, inner }` | `+`, `-`, `*` swaps | If the mutated operation overflows its integer type, go to the compiled operation's overflow panic instead, as the mutated source's checked arithmetic would. |

### Per-language status

| Language | Debugger | `plan` | `run` | Tiers |
|---|---|---|---|---|
| Rust | GDB 14+ over its built-in DAP, plus Python helpers through the repl | yes | yes: the mature path, fidelity-checked | P, then B |
| Go | Delve, one headless process serving DAP and JSON-RPC | yes | yes: earlier prototype | P only |
| Python | none | yes | no: `cherenkov run` reports `py: no go.mod or Cargo.toml; only Go and Rust are executable so far` and exits 48 | none |

Go details: memory read and write, disassembly and function entry go over
Delve's JSON-RPC; DAP carries the session lifecycle and crash events
(`delve.rs:155-250`). Delve has no `restart` here, so each run starts a new
Delve; its disassembly carries no columns, so a Go mutant is located by line
(`column: 0`, `delve.rs:179`). `DelveSession` does not implement
`run_evaluating`, so a Go mutant with no patch is
`Unsupported("no tier B evaluator here ...")`.

### Timeouts and budgets

`execution.rs:30-60`: `Budget::from_baseline` is ten times the baseline plus
five seconds (the first run, the recording run); `Budget::for_mutant` is five
times the baseline and at least two seconds; `evaluating()` triples a budget
for tier B, which pays a debugger round trip per hit. A run past its budget
is `Timeout`, even if its end was already queued (`native.rs`,
`outcome_in`), and counts as detected.

### `--in-diff`

`plan` and `run` accept a unified diff and keep a mutant when its `site`
touches an added line (`crates/cherenkov-cli/src/run.rs:48`,
`crates/cherenkov-core/src/diff.rs`). The diff's paths are from the
repository root; the run's root is placed below it with `git rev-parse
--show-prefix`. A `fn-value` mutant's site is the whole body, so any added
line in the body keeps all of that function's value mutants.

---

## 7. A worked example across languages

The same function in three languages; the mutation is `a < b` to `a <= b`
(`ror/lt->le`).

```rust
// x/w/rs/src/lib.rs
pub fn f(a: i32, b: i32, c: bool) -> bool {
    if a < b && c {
        return true;
    }
    false
}
```
```python
# x/w/py/w.py
def f(a, b, c) -> bool:
    if a < b and c:
        return True
    return False
```
```go
// x/w/go/w.go
func f(a, b int, c bool) bool {
	if a < b && c {
		return true
	}
	return false
}
```

### What the profiles capture

The concept matches for the condition, from each profile's `concepts.scm`
(the matches the planner sees; `a < b` is the `Binary` inside the
`Condition`):

```
rust    @concept.binary="a < b"       @lhs="a" @op="<"  @rhs="b"
        @concept.binary="a < b && c"  @lhs="a < b" @op="&&" @rhs="c"
        @concept.condition="if a < b && c {...}" @cond="a < b && c" @then="{ return true; }"
python  @concept.binary="a < b"       @op="<"                          <- no lhs or rhs
        @concept.binary="a < b and c" @lhs="a < b" @op="and" @rhs="c"
        @concept.condition="if a < b and c: ..." @cond="a < b and c" @then="return True"
go      @concept.binary="a < b"       @lhs="a" @op="<"  @rhs="b"
        @concept.binary="a < b && c"  @lhs="a < b" @op="&&" @rhs="c"
        @concept.condition="if a < b && c {...}" @cond="a < b && c" @then="{ return true }"
```

Python's comparison pattern, `(comparison_operator operators: _ @op)`
(`profiles/python/concepts.scm:9`), captures the operator only, because a
chained comparison has several operators and a fixed `lhs`/`rhs` pair
cannot describe it. The plan for a Python comparison therefore has empty
`operands`. See D31.

### What the operators do

`ror/lt->le` (a `TokenSwap`) reads `@op` through the token table (`<` is
`Token::Lt`) and emits `ReplaceToken { capture: Op, from: Lt, with: Le }`.
The planner finds the smallest evaluable context around the `<`: the
`Condition`'s `cond`, so the action is `EvalBranch` with the mutated
condition (`splice` of the profile's spelling of `Le` at the site), `if_true`
the then-block, `if_false` past the `if`.

### Real `plan` output

```
$ cherenkov plan x/w/rs --format text
d142e62e529363be src/lib.rs:1:1 f body-default return-default "false" [force-return]
e023dcaaf5176811 src/lib.rs:2:8 f negate-condition eval-branch "!(a < b && c)"
52a0dd551bfadcb7 src/lib.rs:2:8 f ror eval-branch "a <= b && c"
441703277bae105b src/lib.rs:3:9 f return-bool-flip eval-return "false" [force-return]
4932c94d8d1f507e src/lib.rs:5:5 f return-bool-flip eval-return "true" [force-return]
rust: 5 mutants; concepts mapped 13/16; unmapped: expr, name, block

$ cherenkov plan x/w/py --format text
c22462ab1708100f w.py:1:1 f body-default return-default "False" [force-return]
33f3f68d98630fa9 w.py:2:8 f negate-condition eval-branch "not (a < b and c)"
fe5656ff5e4ebb91 w.py:2:8 f ror eval-branch "a <= b and c"
30afe79b2431e932 w.py:3:9 f return-bool-flip eval-return "False" [force-return]
20efbaee1148f412 w.py:4:5 f return-bool-flip eval-return "True" [force-return]
python: 5 mutants; concepts mapped 12/16; unmapped: expr, name, block, field_init

$ cherenkov plan x/w/go --format text
02c7aa6a6b9c84ba w.go:3:1 f body-default return-default "false" [force-return]
d88fda6d1b8aa8cf w.go:4:5 f negate-condition eval-branch "!(a < b && c)"
3c585ddfae5968c6 w.go:4:5 f ror eval-branch "a <= b && c"
df1356a9c14872b6 w.go:5:3 f return-bool-flip eval-return "false" [force-return]
ab0954793b6ff215 w.go:7:2 f return-bool-flip eval-return "true" [force-return]
go: 5 mutants; concepts mapped 13/16; unmapped: expr, name, block
```

The plan's action for `ror` is `eval-branch` in all three. The ids differ
because the path differs. With `--operators cargo-mutants`, the connector
swap on the same condition shows the token table:

```
rust    ff7b01eec412f045 src/lib.rs:2:8 f lcr eval-branch "a < b || c"
python  349a33fa752e53fa w.py:2:8 f lcr eval-branch "a < b or c"
go      4c679b2e9f27ed53 w.go:4:5 f lcr eval-branch "a < b || c"
```

### The places the profiles differ

| Place | Rust | Python | Go |
|---|---|---|---|
| `negate-condition` text | `!(a < b && c)` | `not (a < b and c)` | `!(a < b && c)` |
| `lcr` text | `\|\|` | `or` | `\|\|` |
| `return-bool-flip` text | `false` / `true` | `False` / `True` | `false` / `true` |
| `body-default` | `"false"` from `defaults.bool` | `"False"`, **only because the function has `-> bool`**; without an annotation no `@returns` is captured and no mutant is planned | `"false"` |
| operands of `a < b` | `a`, `b` | none | `a`, `b` |
| `body-default` position | `1:1` (the `fn`) | `1:1` (the `def`) | `3:1` (the `func`) |

Everything else in the output is identical: the same operators, the same
action kinds, the same `requires`. That is the claim the model makes, and it
is tested by `crates/cherenkov-syntax/tests/cross_language.rs`, which plans
one program in all three languages under every set and requires the same
named mutations from each (except `fn-value`, which only Rust has).

### Where the plan stops being equal

Running the same kind of file under `cargo-mutants` shows the limits
(Python, from `x/py2/m.py`):

```
def f(a, b, c, xs):
    if a < b < c:           # chained: unrepresentable, no mutant
        return 1
    elif a > c:             # not a Condition: planned in place
        return 2
...
$ cherenkov plan x/py2 --format text --operators cargo-mutants
cfbade4f8849b7fd m.py:4:10 f ror in-place "a < c"
fecc4915679e07ac m.py:4:10 f ror in-place "a >= c"
25f0a29e7eed0f02 m.py:4:10 f ror in-place "a == c"
...
b1af682f1cd35208 m.py:12:5 g unary eval-then-jump "z =  a" [assign]
...
python: 10 mutants; concepts mapped 12/16; unmapped: expr, name, block, field_init; unrepresentable: binary 1
```

Go and Rust plan `else if a > c` as `eval-branch`. Python plans it as
`in-place`, an action only a tier P patch could realise, and Python has no
tier P. The `"z =  a"` has two spaces: deleting `not` leaves the space
before the operand (a display text only; the Rust `!` leaves none). See D32
and D33.

---

## 8. Deviations

Legend: **Intentional** (a choice, with a reason), **Drift** (accidental or
stale; should be fixed), **Open** (the owner should decide). "Fix" says what
should change: the design document, the code, or both.

### 8.1 From `docs/design.md` draft 4

Design sections that are now wrong or missing are collected at the end of
this subsection.

| # | The design said | The code does | Why | Status | Fix |
|---|---|---|---|---|---|
| D1 | Header: "Status: concept. Nothing has been built or measured." | Most of it is built and measured. | Written first. | Drift | Design header (done in this change). |
| D2 | 14 concepts (4.2). | 16: adds `Arm` and `FieldInit` (`concept.rs:26-29`). | Needed for cargo-mutants parity: match arm deletion, guard replacement, struct field deletion. Commit "delete match arms, replace guards, delete struct fields": "Probe crate generation matches cargo-mutants 158/158". | Intentional | Design 4.2 and the 14-concept open question in section 12. |
| D3 | `Expr`, `Name`, `Index` added "to cover the classic families" (UOI, ABS, SVR). | `Expr`, `Name`, `Block` are in the enum and mapped by no profile; `Index`, `Literal`, `Call` are mapped and consumed by no operator. The classic insertion families were dropped (see D14). | The value ranking dropped UOI and ABS. | Open | Decide whether to remove the unused concepts (they inflate the `mapped N/16` figure: no profile can reach 16/16) or keep them as capacity. |
| D4 | A profile is `concepts.scm` and `tokens.toml`. | A third file, `profile.toml` (extensions, exclude globs, unit types, defaults, body template, comments, disabled operators). | The planner needs body syntax and default values; file selection needs globs. | Intentional | Design 4.3. |
| D5 | Roles as in the 4.2 table. | 21 roles; adds `guard`, `scope`, `catch_all`, `async`, `body`, `returns`, `name`. Two more capture namespaces: `@stmt.<kind>` and `@skip.<kind>`; and `@_helper`. | Statement kinds drive actions (3.3); skip regions implement cargo-mutants' exclusions per set. | Intentional | Design 4.2 and 4.3. |
| D6 | Profiles are data only: queries and a token table. | Plus a `ReturnValues` function pointer, supplied in code for Rust only (`profiles.rs:233`); and a Rust-specific `rust_values.rs` (cargo-mutants' type table) and `rust_types.rs` in the syntax crate. | Inferred: a type-to-value table is a grammar of its own and cannot be expressed as data without a mini-language. The code does not say. | Open | Decide whether value generation per language stays code behind a function pointer, or becomes data. |
| D7 | `Edit` has four variants; `ReplaceToken { capture, with }`. | Six: `ReplaceToken` gains `from`; adds `DeleteToken` and `DeleteMatch`; `Lit` gains `Default` and `Text` (`edit.rs`). | `from` lets tier P map an edit without re-reading source; deletion edits serve unary, arm and field deletion; `Text` serves `fn-value`. | Intentional | Design 4.4. |
| D8 | `Operator::mutate(&ConceptMatch)`; `Ror` has the table in code. | `mutate(&MatchCx)`; token swaps are one `TokenSwap` table built from named mutations in a set. `Aor`, `Ror` and `catalog()` remain as the hand-written `breadth` set, used by tests and asserted equal to the registry's `breadth` (`tests/operator_sets.rs:67`). | Operator sets (commit "Add named operator sets drawn from one superset of mutations"): "sets share mutations and their IDs instead of reimplementing them". | Intentional; the duplicate `breadth` is Drift | Design 4.4; consider deleting `Aor`, `Ror` and `catalog()` once the tests use the registry. |
| D9 | `Action` has `EvalThenJump{text, resume}`, `JumpOnly`, `EvalBranch`, `EvalReturn`, `ReturnDefault`, `EvalThenOverwrite`. | Adds `InPlace` and `Redirect`; `EvalThenJump` gains `target` and `value`; `ReturnDefault` gains `value_text`, `zero_bits`, `async_fn`; `EvalThenOverwrite` is gone (`plan.rs:31`). | `InPlace`: commit "Plan operator swaps in any context and compound assignments" ("planned in place ... rather than being skipped"). `Redirect`: "delete match arms by redirecting to the catch-all body". `target`/`value`: "so a backend assigns without parsing source text". `zero_bits`: a stub can only return all-zero. Tier D was never built. | Intentional | Design 4.5 and 3.3. |
| D10 | `requires` is computed by the planner; matching plans to backends is "a pure set comparison in the core"; tier choice is core policy. **Rule: a plan with `CALL_IN_EVAL` or `ASSIGN_IN_EVAL` is never sent to a native evaluator.** `CHOOSE_AT_STOP` flag. | `Requires` has three flags (`CHOOSE_AT_STOP` is gone). No code reads them. Tier choice is inside `TierPExecutor` (patch, else effect, else unsupported). GDB tier B evaluates calls, with other threads held (`gdb_tierb.py:9-17`), so the rule is not enforced. | Native evaluation of calls became workable under GDB with other threads held (commit "evaluate mutated expressions with other threads held"); the executor knows what it can patch. | Drift | Either use `requires` (a core policy before dispatch) or remove it. Design 4.5. |
| D11 | `Target` has `AfterStatement`, `BlockStart`, `LoopHead`, `Epilogue`, with `StmtId`/`BlockId` payloads. | Two variants, `AfterStatement(Span)` and `BlockStart(Span)`. | Loop conditions are branches to the body block; returns are forced by `return`. No id tables. | Intentional | Design 4.5. |
| D12 | `MutantPlan` has `id, operator, file, stop_at, action, edit, requires, function_text`. | Adds `mutation`, `site`, `operands`, `operand_texts`, `original`, `mutated`; `function` is an `Option<String>`; `function_text` (tier C) is gone. | `site`, `operands`: tier P needs the span and the operands as the mutated source parses them (commit "give a swapped && or || the operands its source parses to"). `original`/`mutated`: run events carry them. | Intentional | Design 4.5. |
| D13 | Ids from "path, enclosing function name, operator id, normalized statement text, the edit, and an occurrence index". | As designed (`id.rs`), with the anchor defined per context (condition text for branches, guard text for guards, empty for body replacements). | | Matches | Add the anchor rule to 4.5. |
| D14 | 4.5.1 value policy: arid-code suppression as profile data; at most one mutant per statement; operator ranking; "the planner emits everything... sampling and selection belong to the value policy". | **Not built.** There is no policy step, no ranking, no arid-code patterns. The planner emits everything (as designed) and the named sets are the selection. `breadth` plans both `negate-condition` and `ror` at one condition (section 7), so "one per statement" does not hold. Skipping is by region kinds (`@skip.*`, Rust only) per set, not arid callee patterns per language. | Sets made the selection explicit, named and comparable (`docs/operator-sets.md`); the cargo-mutants set must not drop mutants. | Open | Decide whether 4.5.1 is still the plan. If so, it is a step after the planner that does not exist; if not, rewrite 4.5.1 as "sets". |
| D15 | Tiers A (pure DAP), B, C (managed-runtime redefinition, Python `__code__`), D (run-then-overwrite), P; tier E removed. | `Tier` has `A`, `B`, `P` (`execution.rs:15`). Nothing produces `A`. C and D do not exist; `function_text` and `EvalThenOverwrite` are gone. | Python was deferred ("MVP: Rust first"); C and D were fallbacks for backends not yet built. | Intentional | Design section 8: mark A, C, D as unbuilt. |
| D16 | Python via debugpy, tier A: evaluate plus `goto`; a pytest plugin; `__code__` swap for `ReturnDefault`. | No debugpy driver, no pytest plugin, no `__code__` swap (`grep` for `__code__` finds nothing). Python is plan-only; `cherenkov run` rejects it. | MVP targets Rust first; Go second. | Intentional | Design 1, 4.8, 11.1 (M3 not started). |
| D17 | Rust via GDB tier B: `evaluate` the mutated statement, `jump`, `return`. | Tier B is a loop inside GDB, run from Python helpers: `Assign` writes the value into the local's stack slot and sets `$pc` after the store; `Branch` sets `$pc`; `Return` uses `return`. The mutated statement text is for display. | Inferred: a DAP round trip per hit was the cost the design wanted to avoid (its own "hot-site mitigation", section 9). The assign helper also sets the register the next statement may read instead of the slot (`gdb_tierb.py:53-55`). Evaluating calls needed other threads held (commit "evaluate mutated expressions with other threads held"). | Intentional | Design 3.1, 3.3, 4.6, 8. |
| D18 | `Driver` trait with nine primitives (`set_breakpoint`, `evaluate`, `jump`, `force_return`, `set_variable`, ...). Also `capabilities() -> DriverCaps`. | `NativeSession` plus `Launcher` (`native.rs:114,244`) with coarse primitives: `function_code`, `read`, `write`, `run_reaching`, `run_recording`, `run_evaluating(stops, effect)`. No capabilities object. `MutantExecutor::run(plan, budget)` has no `tests` parameter. | Inferred: whole-run primitives avoid DAP round trips and let each debugger run its own loop. | Intentional | Design 4.1, 4.6. |
| D19 | Go: tier P through DAP `writeMemory`; JSON-RPC only to augment; `jump` unverified; emulated jump; `~r0` plus jump to the epilogue for returns. | Tier P memory goes over JSON-RPC; DAP is only the session lifecycle and crash events. Delve does have `jump` (`SetExecutionPoint`) but nothing uses it: Go has no tier B and no emulated jump. `~r0` plus a jump does not work at `-N -l` and is not used (`docs/m1-delve-findings.md`). | Measured: DAP memory references are handles to slices and strings, not code. | Intentional | Design 8 (Go section), 5 (matrix), 12 (Delve questions are answered). Open: add Go tier B with `SetExecutionPoint`. |
| D20 | `isa/amd64.toml` and `isa/arm64.toml`; "the patcher has no amd64 knowledge of its own". | Only `amd64.toml`. The table holds condition codes, jump and call encodings, arithmetic pairs, bool stores, return stubs; but `x86.rs` (iced-x86) holds hand-written amd64 rewrites (inverting a jump, negating a bool normalisation), and `tierp.rs` has amd64 idioms. | arm64 is later (macOS and ARM64 are planned). | Drift | Design 4.1 layout and 8. When arm64 starts, the amd64-specific code needs a per-architecture trait (the data-only claim is not true). |
| D21 | Crates: `cherenkov-tests`, `cherenkov-shim` (a cargo target runner), `cherenkov-report`, `cherenkov-app` on angzarr-embedded. | `cherenkov-fs`, `cherenkov-fidelity`, and `cherenkov-run-wire`, `-domain`, `-application`, `-store`. No `TestCatalog` or `ReportSink` port. Tests are listed by the test program (`libtest.rs`); GDB starts the test binary directly. The run layer is on the angzarr bus, with a proto-defined wire (`CLAUDE.md`). | The run layer was redesigned (`docs/runlayer-plan.md`). | Intentional | Design 4.1, 4.8, 4.10, 11. |
| D22 | Verdicts: killed, survived, timeout, not reached, unevaluable, evaluator-raised, unsupported. | Eight in the core `Verdict`: adds `Unviable`; `UnsupportedByExecutor` is `unsupported` in events; `not-called` is a run-layer verdict. **Nothing produces `Unevaluable` or `EvaluatorRaised`**: `classify` has no path to them (`execution.rs:103`). | Cherenkov refuses rather than reporting an evaluator failure as a verdict. | Drift | Remove the two unused verdicts, or define when they are produced. Design 7. |
| D23 | "An architecture test asserts that no adapter crate depends on file-writing APIs." | The test covers `cherenkov-core` only (`tests/architecture.rs`). `cherenkov-exec-dap` writes a scratch crate for the support library (`support_lib.rs:1030-1036`) and the CLI builds a Go test binary into a temp directory. Nothing is written into the project. | Compiled constructors need a scratch crate. | Intentional; invariant reworded | Design 1 and 4.1: say "nothing writes into the project or its build"; fix the test claim. |
| D24 | A profile "may disable an operator/concept pair". | `disabled_operators` lists operator ids; no pairs; empty in all three profiles. Counted in `Diagnostics.disabled`. | Never needed. | Open | Remove the key or use it (for example Python's `/`). |
| D25 | Python chained comparisons are handled by profile exclusions. | A chained comparison is `unrepresentable` and counted. | The core, not the profile, decides what a repeated role means. | Intentional | Design 4.7. |
| D26 | Prototype operator 2: arithmetic swap on an assignment's right-hand side; operator 3 relational swap in a condition. | Token swaps apply in any context: assignment, compound assignment, condition, guard, return value, else in place. | Needed for cargo-mutants parity. | Intentional | Design 4.9. |
| D27 | Logical connector swap deferred (4.9). | `lcr` is in `cargo-mutants`; tier B `Steer` swaps `&&`/`||` at the program's own jumps. | | Intentional | Design 4.9. |
| D28 | Profile files are `profiles/<lang>/concepts.scm tokens.toml`; `tokens.toml` is flat (`add = "+"`). | `tokens.toml` has `not_template` at the top and a `[tokens]` table. | | Intentional | Design 4.3 sample. |
| D29 | 4.11 selection from the tests, no coverage tool; VersionControl port. | Built as described (recording run; `not-called`). The "VersionControl port" is a `diff.rs` module and `--in-diff`, not a port. | | Intentional | Design 4.11 (small). |
| D30 | Budget "relative to the unmutated run under the same backend". | `for_mutant`: 5x the baseline, at least 2 s; `evaluating()`: 3x for tier B (`execution.rs`). | Tuned against load (comparison report: five mutants flipped to timeout between runs under the 2 s floor, so tier B got 3x). | Intentional | Design 7. |

Concept-profile gaps that the design did not anticipate, and their status,
are in 8.3 (D31 onward).

**Sections of `docs/design.md` that are now wrong and should be fixed**

| Section | Problem |
|---|---|
| Header, 1 | "Nothing has been built." Python tier A and the `__code__` swap are described as the plan; they are not built. |
| 3.1, 3.3 | Stop-before with DAP `evaluate` and `goto`; GDB `jump`. Tier B is a script in GDB that sets the PC. |
| 4.1 | Crate layout, port list (`TestCatalog`, `ReportSink`), `Driver` trait, "no adapter writes files". |
| 4.2 | 14 concepts. |
| 4.3 | Two profile files; flat token table. |
| 4.4 | The `Edit` and `Operator` sketches. |
| 4.5 | `MutantPlan`, `Target`, `Action`, `Requires` sketches; "pure set comparison" tier choice and the native-evaluator rule; the value policy (4.5.1). |
| 4.6 | Driver descriptions; debugpy, GDB, Delve. |
| 4.9 | Operator table (actions and tiers). |
| 4.10 | Application layer on angzarr-embedded aggregates and a partitioned projector. |
| 5, 8 | Delve: DAP `writeMemory`, `~r0`, emulated jump; tier C and D; `isa/arm64.toml`. |
| 7 | The verdict table lacks `unviable` and `not-called`. |
| 11, 11.1, 12 | Milestones M2 to M5 are not what happened; several open questions are answered. |

### 8.2 From cargo-mutants

The generation rules for the `cargo-mutants` set are cargo-mutants 27.1.0's,
read from its source and cited in `docs/operator-sets.md`. The catalogue of
cargo-mutants' exact mutations is **pending**; the figures below are from
the existing reports (`docs/operator-sets.md` for generation, a comparison
on angzarr's `bus` and `embedded` packages for execution), and they were not
re-measured here.

**Generation**

| Difference | Detail | Status |
|---|---|---|
| Generation matches | Compared on file, line, column and replacement text: probe crate 158 of 158; angzarr-embedded 2393 of 2393; Cherenkov's own workspace 1989 of 1993. | Matches |
| Let chains | cargo-mutants swaps `&&` to `||` inside `if a && let P = b`; the mutant never compiles. tree-sitter parses a let chain as its own node; Cherenkov does not map it (the 4 missing). Listed in the set's `pending`. | Open |
| Files | `plan` lists every file under the directory; cargo-mutants visits a package's module tree and does not follow a `mod` behind `#[cfg(test)]`. On angzarr-embedded `plan` lists 3112 extra mutants. `run` mutates only files rustc's dep-info lists, and on ctxloom's `deps` runs exactly cargo-mutants' 305. | Drift between `plan` and `run`; listed in `pending` |
| Column | cargo-mutants reports the operator's column; Cherenkov's `stop_at` is the expression's start. Comparison matches same-line twins by mutated text. | Intentional |
| A swap means its text | The first `&&` of `a && b && c` to `\|\|` is `a \|\| (b && c)`; the operands are regrouped (`Token::binds_tighter`). The fidelity fixture has tests that fail under exactly one grouping. | Intentional (parity) |
| Values for non-Rust | `fn-value` needs a `ReturnValues` function; only Rust has one. Go and Python plan none. | Intentional |

**Application**

| Difference | Detail | Status |
|---|---|---|
| Not rebuilt | Mutants are applied in a debugger; no per-mutant compile, no scratch copy of `target/`. | Intentional (the point) |
| Tests chosen | A recording run notes which tests call each planned function; a mutant runs only those. `not-called` (no test calls it) has no counterpart: cargo-mutants runs the package's tests. | Intentional |
| Unviable | cargo-mutants finds "does not compile" by compiling. Cherenkov decides `unviable` without compiling: `no-default` (a type with no `Default` in the source or DWARF) or `does-not-compile`. It was wrong once (prost `Message` derives give a type a `Default`; commit "a prost Message derive gives a type a Default"). | Intentional; a source of false verdicts |
| Overflow | A swapped checked operation is guarded so it panics on overflow where rustc's compiled operation would. | Intentional |
| Cannot run | Mutants Cherenkov cannot apply are `unsupported`, with a reason code. angzarr `bus`: 194 of 476 not runnable (value-name-unresolved 61, symbol-not-in-scope 35, value-not-buildable 34, function-not-found 21, generic-copies 19, no-patch-rule 14, evaluator-cannot-apply 7, evaluation-hung 2, other 1); of those, 72 do not compile in source either, so about 122 are real gaps. `embedded`: 58 of 90 (generic-copies 36, no-patch-rule 9, value-not-buildable 9, value-name-unresolved 4). | Accepted, counted |
| Disagreements | On `bus`, of 476 matched mutants, 170 agree detected, 75 agree missed, 31 agree unviable. Known wrong verdicts: `hub.rs:634` `delete !` in a closure local (cargo-mutants caught, Cherenkov survived: the tier B assign into a closure local is unreliable); `hub.rs:743` match arm deletion (cargo-mutants missed, Cherenkov killed). | Drift: not fixed |
| Speed | `bus`: cargo-mutants about 70 minutes; Cherenkov 93 to 116 s (loaded machine). `embedded`: 49 minutes against 122 s. `deps`: 605 s against 38 s. | Context |

**What Cherenkov refuses**

`ACCEPTED_REFUSALS` in `crates/cherenkov-fidelity/tests/faithful.rs:117`
lists 26 `(function, mutation, reason)` entries the debugger may refuse on
the `rust-ops` fixture; any other refusal fails the test, and so does an
entry that is no longer refused. By reason:

| Entries | Reason |
|---|---|
| 11 | An async fn or block's poll returns `Poll<T>` and GDB returns only scalars. |
| 5 | A mutated expression returning a heap value; GDB returns only scalars. |
| 2 + 2 | `let y = x - 1`: no `y` to assign at the statement's start (`inc`); a 128-bit operation GDB cannot evaluate in Python (`wide`). |
| 2 | A compound assignment to a field has no tier P rule or local (`bump`). |
| 2 | `fn-value` mutants that do not compile in source (`pick`, `pool`). |
| 1 | `struct-field/delete`: no patch or effect. |
| 1 | `aor/rem->div` in a closure: GDB cannot evaluate `*x / 2` through `&&i64`. |

(11 + 5 + 4 + 2 + 2 + 1 + 1 = 26.) The README's description of this list
names two entries and is stale (D40).

### 8.3 Between our language profiles

Verified against the profiles and `plan` output.

| # | Difference | Detail | Status |
|---|---|---|---|
| D31 | Python comparison has no `lhs`/`rhs` | `profiles/python/concepts.scm:9` captures the operator only, so `operands` is empty for every Python comparison. No executor uses it for Python today (there is none), but the same plan would break `regrouped` and any tier P rule that reads operands. | Drift |
| D32 | Python `elif` conditions are not conditions | `concepts.scm:19-21` says so. An `elif` comparison is planned `in-place` (output above), which needs a patch Python cannot have. Go's `else if` is a nested `if_statement` and works. | Open: map `elif_clause` with a false target the model can name, or drop the in-place plan for Python |
| D33 | Deleting `not` leaves a space | `"z =  a"` (display text only) | Drift (cosmetic) |
| D34 | Chained comparisons | Python only: `unrepresentable`. | Intentional |
| D35 | Mapped concepts | Rust 13, Go 13, Python 12 (no `FieldInit`: no struct literal with defaulted fields). `Expr`, `Name`, `Block` unmapped everywhere. | Intentional (see D3) |
| D36 | Disabled operators | None in any profile. | Open (D24) |
| D37 | Skip regions | Only Rust has `@skip.*` captures (18). A set that skips `test`, `annotated`, ... skips nothing in Go or Python; tests there are excluded by file globs alone. A test function inside a non-test Python file is mutated (checked: a `test_add` in `lib.py` gets `ror` mutants). | Open |
| D38 | Return concept | Rust: `return`, closure bodies, a function's tail expression. Python and Go: `return` statements only; Go `return a, b` has no `Return` concept (single value only, `concepts.scm:23`). | Intentional |
| D39 | Body replacement | Rust, Go and Python `body-default` (breadth) use the profile `defaults`; a type not in `defaults` plans `<default>` (`String`, a Go `(int, error)`, a Python `list`), left to the driver to derive from debug info, which only the Rust GDB path can. Python needs `-> T` for any body mutant. `fn-value` is Rust only. | Intentional |
| | Executors | Rust: GDB, tiers P and B, session reuse, columns from DWARF. Go: Delve, tier P only, new Delve per run, line-granular. Python: none. | Intentional (D16, D19) |
| | Unit types | Rust `()`, `!`; Python `None`; Go none (a function without a result has no `@returns`). | |
| | Unary | Rust's query lists `-`, `!`, `*`; `*` has no unary spelling so a dereference is never deleted. Go captures every unary operator; only `!` and `-` have spellings. | Intentional |

### 8.4 From tree-sitter conventions and other tools

- The `.scm` extension and S-expression syntax are tree-sitter's. The
  capture vocabulary is not; names such as `@name` and `@scope` also appear
  in `tags.scm` and `locals.scm` conventions with different meanings, so a
  Cherenkov `concepts.scm` cannot be used as a tags or highlights query, nor
  the reverse. The grammar crates' bundled queries are not used.
- `@concept.x` as a dotted namespace follows the style of `@definition.x`
  but is separate. `@_name` as a helper capture is an editor convention that
  Cherenkov enforces itself (`capture_role`).
- Predicates are the built-in text predicates. The core's behaviour never
  depends on a predicate: they narrow what a query matches.
- A query's repeated matches for one node are folded into one concept match
  (`lib.rs:74-111`); this is Cherenkov's, not tree-sitter's.
- Operator names follow the literature, not the tools: `ror` in the
  `breadth` set is the *boundary* swap (`<` to `<=`), which is PIT's
  conditionals boundary, whereas Mothra's ROR replaces each operator by
  every other and by `true`/`false`; `docs/mutation-operators.md` records the
  mismatch. Origins on each named mutation (`cherenkov mutations`) say
  which tools have the same or a similar mutation; a peer is recorded only
  where `docs/operator-research.md` supports it.
- Other tools mutate source or bytecode and rebuild or reload; Cherenkov
  does neither, which is why a mutation also has to be expressible as an
  instruction patch or a debugger effect. Mutations that need neither (for
  example UOI insertion) are not in any set.

### 8.5 Stale statements elsewhere

| Where | Statement | Fact |
|---|---|---|
| D40 `README.md`, "Fidelity" | The refusal list names `struct-field/delete` and one `aor/rem->div`. | The list has 26 entries (section 8.2). |
| D41 `docs/operator-sets.md:74` | "Set `sufficient` (Cherenkov's default)". | `cherenkov sets` marks `breadth` as the default; `--operators` defaults to `breadth` (`main.rs:66,97`). |
| D42 `docs/rust-operator-evaluation.md` | Statuses ("done", "easy", "missing") as of the time written, by numbered row. | The document is now one row for each operator (family), with the rows that are not operators or not implemented listed apart (docs/mutation-operators.md). Its codegen statuses are as of 2026-11-17 and unverified against current code. |

---

## 9. Open questions and what is next

1. **Value policy (D14).** Build the step design 4.5.1 describes, or rewrite
   it as "the named sets are the selection".
2. **`Requires` (D10).** Use it for tier choice and for the native-evaluator
   rule, or delete it.
3. **Unused vocabulary (D3, D22, D24).** `Expr`, `Name`, `Block`,
   `Unevaluable`, `EvaluatorRaised`, `Tier::A`, `disabled_operators`: remove,
   or say what will produce them.
4. **Python.** Fix the comparison operands (D31) and `elif` (D32) before a
   debugger exists; decide how a Python mutant runs (design said debugpy and
   a `__code__` swap; neither is built).
5. **Go tier B (D19).** Delve's `SetExecutionPoint` would give Go the
   mutants it refuses today; nothing is built.
6. **arm64 and the ISA tables (D20).** Decide where amd64 knowledge lives
   (data versus `x86.rs`) before adding a second architecture.
7. **Return values as data (D6).** Whether `ReturnValues` stays code.
8. **Skip regions beyond Rust (D37).** Go and Python have no equivalent of
   `#[cfg(test)]` skipping in the profile.
9. **Design document.** Rewrite the sections listed in 8.1, or replace
   `docs/design.md` with a shorter statement of intent and let this document
   carry the model.
10. **cargo-mutants catalogue.** Cite it here when it exists, and recheck the
    figures in 8.2 against it.
