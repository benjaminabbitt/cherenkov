# Cherenkov: Debugger-Driven Mutation Testing

Design notes, draft 4, 2026-11-17. "Cherenkov" is a working name (section 13). Status: written as a concept before the code, and partly superseded.

> **Note.** Most of this design is now built and measured, and some of it differs from what is written here (the concept and action vocabularies, the executor structure, Delve's primitives, the value policy, the application layer). [`docs/language-profiles.md`](language-profiles.md) describes the implemented model and lists the deviations from this document, section by section. Where the two disagree, that document describes the code.

Changes since draft 3: the Go overlay backend (tier E) is removed and Go now runs under an unmodified Delve, driven over DAP like the other backends and augmented with Delve's JSON-RPC API only where DAP lacks a primitive; a new instruction-patch tier (tier P) is added for native code; the source-control invariant is now absolute (no adapter writes files); the prototype operator set is chosen for breadth (section 4.9); the concept model gains `Expr`, `Name` and `Index`, the planner gains structural targets, declared requirements and collision-free IDs, with a value policy and value-ranked operators (section 4.5); the run is event-sourced on angzarr-embedded (section 4.10); milestones are added (section 11).

Implementation language: Rust. Architecture: hexagonal (ports and adapters). Initial target languages: Python, Rust, Go.

**Provenance tags used throughout**

- **[S]** Read from the tool's source code, official documentation or registry API during research. Reference number in brackets, e.g. [S3].
- **[K]** Known tool behavior taken from standard documentation that was not re-fetched for this draft. Verify before relying on it.
- **[I]** Inference from sources. Not stated directly by any of them.
- **[P]** Proposal. Untested design. Code blocks are sketches, not compiled.

---

## 1. Summary

Compile the code under test once, with debug info. Run each test under a debugger driven through the Debug Adapter Protocol (DAP). Apply each mutant at runtime: stop *before* the mutated statement, execute the mutated form of that statement in the stopped frame, then move execution past the original statement. The original never runs, so only the mutant's side effects occur. At statement granularity this reproduces source-level mutant semantics without rebuilding [P].

Mutation operators are written once against a small language-neutral concept model inside a pure domain core. Each language contributes data (tree-sitter queries and a token table); each execution backend is an adapter behind one port. No operator or planner code branches on language (section 4).

Nothing in Cherenkov writes source files: the core's only source port is read-only by construction, and no adapter writes files either. Every mutant exists only in the memory of a process under a debugger. Mutants therefore cannot reach the working tree or source control [P]. The unmutated original, run under the same backend, is the reference every verdict is measured against.

Initial language plan, driven by verified debugger capabilities:

| Language | Backend | Why |
|---|---|---|
| Python | debugpy over DAP, tier A | declares `gotoTargets` [S1]; repl executes statements in the frame [S11][S12] |
| Rust | GDB's built-in DAP, tier B | GDB has its own Rust expression parser and generally works better on Linux [S24]; LLDB's Rust support is partial [S25]; GDB's repl runs CLI commands, giving `jump` and `return` [S13][S15] |
| Go | unmodified Delve over DAP, augmented by its JSON-RPC API; tiers P and B (section 8) | Delve is the debugger Go recommends; Go's docs call GDB unreliable for Go [S27]. Delve's DAP server declares no goto and no setExpression but does declare writeMemory [S3], so the missing primitives are worked around with what Delve ships: instruction patching via DAP `writeMemory` (tier P), Delve's `jump` command over JSON-RPC [S40] if it exists (unverified, [S38]), or an emulated jump built from memory writes and breakpoints |

Test frameworks share no control protocol, but each initial ecosystem has a standard hook for wrapping the test process (cargo's target runner, `go test -exec`, pytest plugins), and results can be published in standard formats with each mutant as a test case, so CI shows survivors as failures without runner changes (section 4.8).

The trade is deliberate: per-mutant compile cost is replaced by a per-hit stop at the mutated site (tiers A and B), or by nothing at all where the mutant can be applied as a one-time instruction patch (tier P, native code). Some runtime degradation is accepted.

---

## 2. Motivation and prior art

Rebuilding per mutant dominates cost for compiled languages. Existing compile-once approaches:

- **Mutant schemata** encode every mutant into one program, compiled once, with a runtime switch selecting the active mutant [R1]. Rust implementations: mutagen selects mutants via an environment variable [R2] but its last push was May 2023 [R3]; mutest-rs integrates with rustc and uses meta-mutants [R4]. Schemata require compiler- or AST-level tooling per language.
- **Binary mutation** rewrites the compiled binary [R5][R6]. Mutants are instruction-level, so survivors do not always map to a source change.

This design needs an adapter per execution backend and a data profile per language instead of compiler integration, and its mutants are expressed in source terms. Tier P (section 8) does change machine instructions, but each patch is derived from a source mutant through the parse tree and the line table, so a survivor always maps back to a source change; patches that cannot be derived this way are not applied.

**Standard operator sets** [K, not re-fetched]. Mothra's 22 classic operators, PIT (Java), cargo-mutants (Rust), mutmut (Python) and go-mutesting (Go) all centre on operator swaps, literal replacement, condition negation, statement deletion and return-value replacement. Call-related operators are limited to removing a void call and replacing a call's result with a default (PIT), and replacing a whole function body with a default return (cargo-mutants' main operator). Replacing one call with a different call is not standard; Stryker's curated method-pair swaps (`min`/`max`, `startsWith`/`endsWith`) are the only common exception. Cherenkov does not need call injection.

---

## 3. Core mechanism: stop-before, substitute, skip

### 3.1 Per-mutant flow [P]

1. **Plan.** The planner (section 4) produces a language- and backend-independent `MutantPlan`: where to stop, what text to evaluate, where to resume.
2. **Resolve locations.** Map the plan's source positions to breakpoint locations. Use the `breakpointLocations` request where available (declared by js-debug, java-debug and lldb-dap [S2][S4][S5]) and the `column` field on source breakpoints.
3. **Arm** the breakpoint for the scope described in section 6.
4. **On every stop at the site:**
   1. `evaluate` the mutated statement in the stopped frame (use `context: "repl"` where statements, not just expressions, must be accepted).
   2. Move the PC to the resume target (DAP `goto`, or the native escape in section 8).
   3. Continue.
5. **Verdict** from the test outcome (section 7).

### 3.2 Why stop-before beats run-then-overwrite

The earlier idea was to let the original line run, evaluate the alternative expression afterward and `setVariable` the result. That executes the original's side effects and then the alternative's. Stop-before executes only the mutated statement. It also covers mutants with no assignment target (mutated call arguments, statement deletion, branch negation), which overwrite cannot express. Run-then-overwrite survives only as the restricted fallback in section 8, tier D.

### 3.3 Statement kinds [P]

| Statement kind | Action at the stop | Requires |
|---|---|---|
| Assignment `x = a + b` | evaluate `x = a - b`, jump to next statement | evaluator accepts assignment; jump |
| Expression statement with mutated args `f(a + 1)` | evaluate `f(a)`, jump to next statement | evaluator can call `f`; jump |
| Statement deletion | jump to next statement, no evaluate | jump |
| Branch `if (c)` | evaluate mutated `c'`; jump to first statement of then-block if true, else-block or following statement if false | jump; static branch targets from the parse tree |
| Loop condition `while (c)` | same as branch, at each condition check | jump; the line table must give a stop at the condition every iteration |
| Increment `i += 1` to `i -= 1` | evaluate the mutated statement, jump to next statement | jump |
| `return e` | evaluate `e'`, then native return-with-value (GDB `return` [K], LLDB `thread return` [S16]); under Delve, `setVariable` the result variable (`~r0`) and jump to the epilogue (section 8) | native escape, or setVariable plus jump; DAP has no return request |
| Function body replaced by default return | stop at function entry, return the result type's default immediately | as for `return e`; the default comes from the variable's DWARF type |
| Declaration with initializer `let x = a + b` | evaluate assignment to `x`, jump | in compiled code the variable's storage may not be live at the statement's first instruction [I]; in Python, exec in the frame does update frame locals [S12] |

Branch negation via evaluate plus jump is the notable case: the harness decides the branch itself, so any condition mutation (relational swap, negation, constant true/false) uses the same code path.

### 3.4 Evaluator fidelity (main technical risk)

A debugger's expression evaluator is not the compiler [I]. Known failure classes to design for:

- **Rust.** GDB's Rust expression parser is a hand-written subset of Rust, unrelated to rustc's parser [S24]. LLDB's Rust support is partial and goes through its C/C++ type system [S25]. Expect generics, trait method calls, closures and macros to fail in either; this is the main reason the Rust backend is GDB.
- **Native code generally.** Optimized-out locals; inlined functions with no out-of-line copy cannot be called; overload resolution and implicit conversions may differ from the compiler [I]. Build with the debug profile. Results then reflect debug-build behavior.
- **Exceptions and panics inside evaluate.** The debugger catches the error and reports a failed evaluate. The program's own handlers never run, so "the mutant threw" cannot be scored as killed. debugpy's repl path tries `eval` first and falls back to `exec`, and an older implementation swallowed errors on the exec path [S11]; the adapter must distinguish evaluator errors from success explicitly.
- **Evaluator limits.** Timeouts, threads used for expression evaluation, and side-effect restrictions vary by debugger [I].

Section 10 calibrates all of this against real source mutants.

---

## 4. Architecture: hexagonal core, operators defined once [P]

### 4.1 Hexagonal layout

The domain core is pure Rust with no I/O dependencies. Everything that touches files, processes, parsers or debuggers is an adapter behind a port trait defined in the core.

| Port (trait in core) | Direction | Adapters |
|---|---|---|
| `SourceReader` | driven | filesystem, read-only (no write method exists on the trait) |
| `SyntaxPort` | driven | tree-sitter plus language profiles |
| `TestCatalog` | driven | pytest plugin, cargo/libtest, `go test` (section 4.8) |
| `MutantExecutor` | driven | DAP executor, one implementation over a `Driver` trait: debugpy, GDB and Delve drivers (the Delve driver augmented by JSON-RPC); tier P patching for native drivers |
| `ReportSink` | driven | JSON verdict stream; JUnit XML and TAP 14 with one test case per mutant (section 4.8); realized as angzarr-embedded projectors (section 4.10) |
| `MutationSession` use case | driving | CLI first; CI integration later |

```rust
// cherenkov-mutation::ports (sketch)

/// Read-only by construction: the core has no port that writes source.
pub trait SourceReader {
    fn list(&self, root: &Path, lang: LanguageId) -> io::Result<Vec<PathBuf>>;
    fn read(&self, path: &Path) -> io::Result<Arc<str>>;
}

pub trait SyntaxPort {
    fn analyze(&self, lang: LanguageId, path: &Path, src: &str) -> Result<FileSyntax>; // concepts, statements, functions
}

pub trait TestCatalog {
    fn tests(&self, project: &Project) -> Result<Vec<TestId>>;
}

pub trait MutantExecutor {
    fn capabilities(&self) -> ExecCaps;
    fn run(&mut self, plan: &MutantPlan, tests: &[TestId], budget: Budget) -> Result<RawOutcome>;
}

pub trait ReportSink {
    fn record(&mut self, verdict: &Verdict) -> Result<()>;
}
```

**Source-control invariant.** The core cannot write (no port exists), and no adapter writes files. Mutants are applied only to process memory under a debugger: evaluated text, variable writes, PC moves and instruction patches. Build artifacts are produced by the language's own toolchain in its usual output directory (cargo's `target/`, Go's build cache), as for any test run. An architecture test asserts that no adapter crate depends on file-writing APIs outside the toolchain invocation.

**Stable mutant IDs.** Derive IDs from source structure, not line numbers (section 4.5), so IDs survive unrelated edits and support baselines and incremental runs.

**Crate layout.**

```
cherenkov/
  crates/
    cherenkov-mutation/          domain: concepts, operators, planner, verdicts, port traits. No I/O deps.
    cherenkov-syntax/        adapter: tree-sitter parsing, profile loading (SyntaxPort)
    cherenkov-dap-types/     generated DAP protocol types (checked in, with a regeneration script)
    cherenkov-exec-dap/      adapter: DAP client, Driver trait, tier P patcher; debugpy, GDB and Delve drivers,
                             with a small Delve JSON-RPC client used only by the Delve driver (MutantExecutor)
    cherenkov-tests/         adapters: pytest, cargo test, go test (TestCatalog)
    cherenkov-shim/          cargo target-runner shim: starts the test binary under GDB's DAP
    cherenkov-report/        adapter: JSON verdicts, JUnit XML, TAP (ReportSink), as projectors
    cherenkov-app/           application layer on angzarr-embedded: the Cherenkov aggregate, the run projector, report projectors (section 4.10)
    cherenkov-cli/           composition root
  integrations/
    pytest/                  pytest plugin (Python package; starts debugpy in the test process)
  profiles/
    python/  concepts.scm  tokens.toml
    rust/    concepts.scm  tokens.toml
    go/      concepts.scm  tokens.toml
  isa/
    amd64.toml               instruction shapes and patch rules for tier P
    arm64.toml
  fixtures/
    python/  rust/  go/      small projects with hand-labelled expected verdicts per mutant
```

**Dependencies.** `tree-sitter` is current (0.27.2, updated 2026-11-07) [S28]. The existing Rust DAP crates are stale: `dap` 0.4.1-alpha1 (2023-09), `dap-types` 0.0.1 (2024-05), `debug_types` 1.0.0 (2022-05) [S28]. Generating client types from the DAP JSON schema [R7] is likely the better path [I]. Tier P needs an instruction decoder for each instruction set (for example iced-x86 for amd64 [K]).

**Concurrency.** DAP is event-driven: stop events and output arrive while requests are in flight. Delve's JSON-RPC API, where used, is request/response [K]. Async (tokio) is confined to `cherenkov-exec-dap`; the core's ports stay synchronous and the executor blocks internally, so the core remains pure.

**Verdicts are computed in the core.** Executors return `RawOutcome` facts only (failing tests, crash, wall time, evaluator error versus evaluator-raised, tier applied). Classifying them into verdicts (section 7), including the timeout limit relative to the baseline, is a pure core function shared by every backend.

### 4.2 Concept model

A deliberately small vocabulary, each concept with named roles:

| Concept | Roles |
|---|---|
| `Binary` (subclassed by token into arith / compare / logical) | `op`, `lhs`, `rhs` |
| `Unary` (not, negate) | `op`, `operand` |
| `CompoundAssign` (`+=`, `-=`, ...) | `op`, `target`, `value` |
| `Literal` (bool, int, string) | `value` |
| `Condition` (of if / while / ternary) | `cond`, `then`, `else` |
| `Return` | `value` |
| `Call` | `callee`, `args` |
| `Assign` | `target`, `value` |
| `Expr` (arithmetic expression as an insertion site) | span, `type_hint` (from the profile, when the grammar gives one) |
| `Name` (variable reference) | `ident` |
| `Index` (array or slice subscript) | `base`, `index` |
| `Statement`, `Block`, `Function` | span only |

**Concepts are not operators.** Operators match concepts many-to-one: ROR, AOR and LCR all match `Binary` and differ only in the tokens they swap. The classic sets have around 20 operators (Mothra's 22 for Fortran [K]); the model above has 14 concepts. `Expr`, `Name` and `Index` were added in draft 4 to cover the classic families:

| Classic operators [K] | Concept |
|---|---|
| AOR, ROR, LCR | `Binary` |
| UOI (unary insertion), ABS (absolute-value insertion) | `Expr` |
| CRP, SRC (constant replacement) | `Literal` |
| SDL (statement deletion) | `Statement` |
| RSR (return replacement) | `Return` |
| SVR, CSR, SCR (variable and constant swaps) | `Name`, `Literal` |
| AAR, ACR, CAR, CNR, ASR, SAR (array-reference swaps) | `Index`, `Name` |
| GLR, DER, DSA, SAN | Fortran-specific; no equivalent |

**Post-prototype operator target.** Offutt's selective-mutation result found that five operators (ABS, AOR, LCR, ROR, UOI) give mutation scores close to the full set [K]. That result is about approximating the full set's score, not about value to a developer, so the post-prototype target is instead the value ranking in section 4.5.1, which drops UOI and ABS. `Expr` stays in the model as capacity.

**Variable replacement chooses candidates at the stop** [P]. SVR-style operators need another in-scope variable of a compatible type, which tree-sitter cannot supply. The debugger can: DAP `scopes` and `variables` return the in-scope variables and their types at the stop. This needs an action that picks the substitute at the first hit, which departs from plans being complete before execution; the mutant ID must then include the chosen variable so it stays stable across runs (section 12).

### 4.3 Language profile

**Queries.** A per-language `.scm` file captures grammar nodes into concepts using shared capture names. Grammar node names differ between languages; capture names do not. Illustrative only, not checked against the grammars:

```scheme
; profiles/rust/concepts.scm
(binary_expression left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(if_expression condition: (_) @cond consequence: (_) @then alternative: (_)? @else) @concept.condition
(return_expression (_) @value) @concept.return

; profiles/python/concepts.scm
(binary_operator left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(boolean_operator left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(if_statement condition: (_) @cond consequence: (_) @then alternative: (_)? @else) @concept.condition
(return_statement (_) @value) @concept.return

; profiles/go/concepts.scm
(binary_expression left: (_) @lhs operator: _ @op right: (_) @rhs) @concept.binary
(if_statement condition: (_) @cond consequence: (_) @then alternative: (_)? @else) @concept.condition
(return_statement (_) @value) @concept.return
```

**Token table.** Maps concrete operator spellings to abstract tokens, and back for rendering:

```toml
# profiles/rust/tokens.toml  (Go is the same except where noted)
add = "+"   sub = "-"   mul = "*"   div = "/"
lt = "<"    le = "<="   gt = ">"    ge = ">="   eq = "=="   ne = "!="
and = "&&"  or = "||"
not_template = "!({})"
true = "true"   false = "false"

# profiles/python/tokens.toml
and = "and"   or = "or"
not_template = "not ({})"
true = "True"   false = "False"
```

**Exclusions.** A profile may disable an operator/concept pair as data when the language's semantics make it meaningless (section 4.7).

### 4.4 Operators

An operator matches concepts and emits abstract edits. Rendering is a text splice over captured byte ranges using the profile's token table, so there is no code generation: the mutated statement is the original text with captured spans replaced.

```rust
pub enum Edit {
    ReplaceToken { capture: Capture, with: Token },    // rendered via the profile's token table
    WrapNot { capture: Capture },                      // rendered via not_template
    ReplaceWithLiteral { capture: Capture, lit: Lit }, // rendered via the profile's literal spellings
    DeleteStatement,
}

pub trait Operator {
    fn id(&self) -> &'static str;
    fn concepts(&self) -> &'static [Concept];
    fn mutate(&self, m: &ConceptMatch) -> Vec<Edit>;
}

/// Relational operator replacement. Written once; applies to every
/// language whose profile maps Binary with comparison tokens.
pub struct Ror;

impl Operator for Ror {
    fn id(&self) -> &'static str { "ror" }
    fn concepts(&self) -> &'static [Concept] { &[Concept::Binary] }
    fn mutate(&self, m: &ConceptMatch) -> Vec<Edit> {
        use Token::*;
        let alternatives: &[Token] = match m.token(Capture::Op) {
            Some(Lt) => &[Le, Ge],
            Some(Le) => &[Lt, Gt],
            Some(Gt) => &[Ge, Le],
            Some(Ge) => &[Gt, Lt],
            Some(Eq) => &[Ne],
            Some(Ne) => &[Eq],
            _ => &[],
        };
        alternatives
            .iter()
            .map(|&t| Edit::ReplaceToken { capture: Capture::Op, with: t })
            .collect()
    }
}
```

Simple token swaps could be pure data tables; structural operators (negate condition, delete statement, replace return value with a default literal) need code. Both live in the one catalog.

### 4.5 Mutant plan (language- and backend-independent)

The planner combines an operator edit, its concept match and the enclosing statement into a plan. The statement kind (section 3.3) decides the action. The plan stays in source terms; how it is realized (per-hit evaluate and jump, or a one-time instruction patch in tier P) is chosen by the core from the plan's requirements and the driver's capabilities, and the tier used is recorded.

```
SourceReader ─► SyntaxPort.analyze ─► FileSyntax (concept matches, statements, blocks, functions)
             ─► operator catalog: mutate() per ConceptMatch ─► Vec<Edit>
             ─► profile exclusions ─► statement kind ─► Action, Target, Requires
             ─► render (token-table splice) ─► MutantPlan + diagnostics (concept coverage, unmapped concepts, exclusions)
             ─► value policy (section 4.5.1) ─► scheduled plans
```

```rust
pub struct MutantPlan {
    pub id: MutantId,
    pub operator: &'static str,
    pub file: PathBuf,
    pub stop_at: SourcePos,     // start of the enclosing statement or condition; function entry for body replacement
    pub action: Action,
    pub edit: Edit,             // the abstract edit, so tier P can map it to an instruction change
    pub requires: Requires,     // what the evaluated text needs from a backend (see below)
    pub function_text: String,  // enclosing function with the edit spliced in (tier C)
}

/// Resume and branch targets are structural; the driver resolves them, because only it knows the line table.
pub enum Target {
    AfterStatement(StmtId),     // native: end of the statement's instruction range; Python: next code line in the same code object
    BlockStart(BlockId),        // then-block or else-block
    LoopHead(LoopId),           // loop condition / increment
    Epilogue(FnId),             // return paths
}

pub enum Action {
    EvalThenJump { text: String, resume: Target },
    JumpOnly { resume: Target },
    EvalBranch { cond_text: String, if_true: Target, if_false: Target },
    EvalReturn { value_text: String },
    ReturnDefault,                                      // function body replaced by the result type's default
    EvalThenOverwrite { text: String, target: String }, // tier D only
}

bitflags! { pub struct Requires: u32 {
    const CALL_IN_EVAL    = 1 << 0;  // evaluated text contains a call
    const ASSIGN_IN_EVAL  = 1 << 1;  // evaluated text contains an assignment or increment
    const FORCE_RETURN    = 1 << 2;
    const CHOOSE_AT_STOP  = 1 << 3;  // substitute chosen at the stop (variable replacement)
}}
```

**Why targets are structural.** "The next statement in the source" is not where execution goes after the last statement of a loop body (the loop head), of a then-block (after the whole `if`), or before a closing brace and a `return`. The planner names the structural successor; the driver turns it into an address or a line. For native code the end of the statement's instruction range is a better resume point than any source position, since execution falls through naturally.

**Requirements and tier choice.** `requires` is computed syntactically by the planner. Matching plans to backends is then a pure set comparison in the core: `unsupported` is decided before anything runs, and tier choice is core policy (configurable, e.g. "tier P only"), not adapter-internal. **Rule: a plan with `CALL_IN_EVAL` or `ASSIGN_IN_EVAL` is never sent to a native evaluator** (GDB, Delve), because call injection there is limited (safe points only, resumes all goroutines, restricted arguments [S41]). On native backends such plans go to tier P, or are not scheduled. Python's evaluator runs `exec` in the frame and has no such limit.

**Mutant IDs.** Path, enclosing function name, operator id, normalized statement text, the edit, and an occurrence index among identical statements within that function. Without the function name and index, two identical `i += 1` or `return nil` statements collide. The ID still survives edits elsewhere in the file.

**Duplicates.** Mutants that render identically at the same position with the same action (condition negation and constant false can coincide) are deduplicated. The planner emits everything, deterministically and in sorted order; sampling and selection belong to the value policy, never to the planner.

Plans whose action no available executor supports are reported as `unsupported`, never silently dropped.

#### 4.5.1 Value policy [P]

Value is the probability that a surviving mutant reveals a real test gap someone would act on, per unit cost. The policy is a separate pure step after the planner. Its levers follow Google's published practice (Petrović and Ivanković, ICSE-SEIP 2018, and the 2021 follow-up) [K]:

1. **Arid-code suppression, as profile data.** Per-language callee patterns and node kinds whose mutants survive and nobody cares: logging, metrics, tracing, `defer` cleanup, error-message strings, `String()` / `Display` implementations. Lives in the language profile beside the token table.
2. **At most one mutant per statement** by default, the highest-ranked operator that applies.
3. **Code the selected tests drive only**: a mutant they never reach is left out (section 4.11).

**Operator ranking by value:**

| Rank | Operator | Value | Realization | Injection |
|---|---|---|---|---|
| 1 | Function body to zero / default return | very high: does any test check this function's result? One mutant per function | tier P stub at function entry: zero the result registers, `RET`. A Go zero value is all-zero bits, so this is exact for every register-returned Go type; Rust primitive and unit returns only (sret-returning functions excluded); Python: swap the function's `__code__` once at arm time (tier C) | none |
| 2 | Void-call removal | high: is this side effect verified? | tier P `JMP`-over, or jump; arid calls excluded | none |
| 3 | Boundary ROR (`<`/`<=`, `>`/`>=`) | high: off-by-one, missing boundary tests | tier P condition-code patch; evaluate-and-branch only when pure | none |
| 4 | Condition negation, guards especially (`err != nil`, `x is None`) | high | tier P branch inversion | none |
| 5 | Arithmetic swap | medium | tier P `ADD`/`SUB`; evaluate only when pure | none |
| — | Constant replacement | low to medium, many equivalent mutants | later | — |
| — | UOI, ABS insertion | low developer value, many equivalents | dropped | — |
| — | Variable replacement | costly, needs choosing at the stop | deferred | — |

Most of the ranked set needs no per-hit stop: ranks 1, 2, 4 and 5 are tier P patches on native code.

### 4.6 Debugger drivers (inside the DAP adapter)

`Driver` is internal to `cherenkov-exec-dap`, not a core port. The DAP executor implements `MutantExecutor` once in terms of these primitives; drivers differ only in how they provide them. DAP is the protocol for every driver; a driver may augment it with a debugger's native interface for a primitive DAP lacks (GDB's repl commands, Delve's JSON-RPC API).

```rust
pub(crate) trait Driver {
    fn capabilities(&self) -> DriverCaps; // DAP initialize response plus known native escapes
    fn set_breakpoint(&mut self, at: SourcePos, condition: Option<&str>) -> Result<BpId>;
    fn evaluate(&mut self, frame: FrameId, text: &str) -> Result<EvalOutcome>;
    fn jump(&mut self, thread: ThreadId, to: SourcePos) -> Result<()>;     // DAP goto or native escape
    fn force_return(&mut self, thread: ThreadId, value_text: &str) -> Result<()>;
    fn set_variable(&mut self, frame: FrameId, name: &str, value_text: &str) -> Result<()>;
    fn read_memory(&mut self, addr: u64, len: usize) -> Result<Vec<u8>>;   // native drivers only
    fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<()>;     // native drivers only (tier P)
    fn resume(&mut self, thread: ThreadId) -> Result<()>;
}
```

- **debugpy driver:** all primitives via standard DAP except `force_return` (unsupported; Python `return` mutants fall back to mutating the caller-side assignment or are reported unsupported) and memory access (not applicable).
- **GDB driver:** generic DAP for breakpoints, evaluate and variables; `jump` and `force_return` via repl CLI commands. Because GDB's `jump` resumes execution, `resume` is skipped after it [K]. Memory access for tier P via DAP or the repl.
- **Delve driver:** Delve is used unmodified. DAP for breakpoints, evaluate, setVariable and memory reads and writes [S3]. Augmented with Delve's JSON-RPC API [S40] only for what DAP lacks: Delve's own terminal is a client of that API, so every terminal command, including `jump` if it exists (unverified [S38]), is reachable over it [I]. Without `jump`, an emulated jump over DAP (section 8). `force_return` is setVariable on the result variable followed by a jump to the epilogue. Whether one Delve process can serve a DAP client and a JSON-RPC client at once is open (section 12). The augmentation is contained in the Delve adapter: it still presents only the `Driver` primitives, and nothing outside it (executor, planner, core) knows JSON-RPC exists, so dropping or replacing the augmentation changes no other code. Test binaries are built with `-gcflags=all=-N -l`.

### 4.7 Where DRY stops

- **Same token, different semantics.** Python `/` is true division; Python `and` returns an operand rather than a bool; Python chains comparisons (`a < b < c`); Go has no ternary and no `++` expressions (they are statements). The operator stays the same. The profile disables pairs that are meaningless, and anything else that fails is classified `unevaluable` or `evaluator-raised`.
- **Language-specific operators** (Rust `?` removal, Go `err != nil` negation, Python `is` to `is not`) still live in the shared catalog and are still written once. They apply only where a profile maps their concept.
- **Mapping coverage bounds everything.** An unmapped concept yields no mutants without any error. The engine reports per-language concept coverage so gaps are visible.
- **Positions.** Sites come from parser spans; the debugger's line table decides whether a stop exists there. Unresolvable sites are reported, not dropped.

Reuse: reprise's multi-language tree-sitter front end (which already covers Rust, Python and Go) could supply `cherenkov-syntax`.

### 4.8 Test framework integration

**There is no cross-language control protocol for test frameworks.** Standards exist for parts of the problem:

| Standard | Covers | Status |
|---|---|---|
| TAP (Test Anything Protocol) | results stream; `# TODO` marks a test not expected to pass [S33] | v14 exists; the Linux kernel's KTAP docs call it a stagnant draft and add an `XFAIL` directive [S33] |
| JUnit XML | results file | de facto, no formal spec [K]; nextest's main machine-readable run output [S35] |
| `go test -json` (test2json) | Go results stream | stable part of the Go command [S32] |
| libtest JSON | Rust results stream | unstable; stabilization is a Rust project goal [S35]; nextest has an experimental compatible mode [S35] |
| Bazel test encyclopedia | runner-to-test-binary contract (environment variables, sharding, XML output path) [S34] | Bazel only |
| Build Server Protocol `debugSession/start` | starts a DAP server for a build target's run or tests and returns its URI [S30] | the only standard pairing test execution with DAP; used mainly in the Scala/JVM ecosystem (Bloop) [S30] |

**Two integration directions.**

1. **Cherenkov drives the runner (default).** The `TestCatalog` adapter discovers tests and launches them one at a time with the runner's own selection flags; the executor reads the outcome from the runner's machine-readable output or exit code.
2. **The runner launches Cherenkov.** Each initial ecosystem has a standard hook for wrapping the test process, so the runner keeps doing build and discovery and Cherenkov is inserted alongside the system under test:
   - **Rust:** cargo's `target.<triple>.runner` (or `CARGO_TARGET_<TRIPLE>_RUNNER`) wraps every executed binary for `cargo run`, `cargo test` and `cargo bench`, with the binary passed as the first argument [S31]; nextest honors the same setting [S31]. Cherenkov sets the variable when it invokes cargo, and its runner shim starts the test binary under GDB's DAP.
   - **Go:** `go test -exec xprog` runs the test binary via `xprog` [S32], so Cherenkov can start each test binary under Delve the same way the cargo shim does under GDB.
   - **Python:** a pytest plugin running in the test process can start debugpy's server with `debugpy.listen()` [S36], and Cherenkov attaches as the DAP client.

**Expected failures.** Under a mutant, covering tests are expected to fail: a failure is a kill, a pass is a survivor. Runner-native expected-failure markers exist but are not uniform: pytest's `xfail(strict=True)` turns an unexpected pass into a failure and can be applied dynamically from a plugin [K]; TAP has `# TODO` [S33]; libtest's `#[should_panic]` is compile-time only [K]; Go has no expected-failure mechanism [K]. Rather than invert each runner's semantics, Cherenkov keeps the inversion in the core and publishes its own results, with each **mutant as a test case**: killed reports as `ok`/pass, survived as `not ok`/failure, and non-scoring outcomes (section 7) as `skip`. Any CI system that reads JUnit XML or TAP then shows survivors as failures with no runner changes [P].

**Initial adapters** [P]:

| Language | Discovery | Single-test run | Outcome |
|---|---|---|---|
| Python | pytest plugin collection hook | node id selection, in-process debugpy | plugin reports per-test result to Cherenkov |
| Rust | `cargo test --no-run --message-format=json` for binary paths, then `<binary> --list` [K] | `<binary> --exact <name>` under GDB via the cargo runner shim [K] | exit code per single-test process (avoids unstable libtest JSON) |
| Go | `go test -list` [K] | test binary built once with `go test -c -gcflags=all=-N -l`, launched under Delve as `<binary> -test.run '^Name$'` (headless, with a DAP client attached, and a JSON-RPC client if augmentation is needed) [K] | exit code per single-test process; test2json stream where available [S32] |

### 4.9 Breadth-first operator set [P]

The prototype operators are chosen so that the smallest set exercises every `Edit` variant, every `Action` variant (except tier D) and every verdict class in all three languages, rather than going deep on one operator family.

| # | Operator | Example | Edit | Action | What it exercises that the others do not |
|---|---|---|---|---|---|
| 1 | Statement deletion | `log(x)` removed | `DeleteStatement` | `JumpOnly` | the jump alone, with no evaluation: the control case; a disagreement here is a jump or line-table fault, not an evaluator fault. Tier P `JMP`-over on native code |
| 2 | Arithmetic swap on an assignment's right-hand side | `x = a + b` to `x = a - b` | `ReplaceToken` | `EvalThenJump` | evaluator fidelity directly (GDB's Rust parser); Python `/` semantics (section 4.7); division by zero gives `evaluator-raised`. Tier P `ADD`/`SUB` patch on native code |
| 3 | Relational swap in a condition | `if a < b` to `if a <= b` | `ReplaceToken` | `EvalBranch` | static branch targets from the parse tree; Python chained comparisons test profile exclusions. Tier P condition-code patch |
| 4 | Condition negation, including loops | `while i < n` to `while not (i < n)` | `WrapNot` | `EvalBranch` | the per-language `not_template`; a stop at the condition on every iteration; the `timeout` verdict |
| 5 | Return boolean flip (literal `true`/`false` only) | `return true` to `return false` | `ReplaceWithLiteral` | `EvalReturn` | per-language literal spelling; GDB `return`; Delve result write plus jump; Python gives `unsupported`. Restricted to boolean literals so it stays type-safe |
| 6 | Function body to default return (cargo-mutants' main operator) | `fn f(..) -> bool { .. }` returns `false` immediately | `ReplaceWithLiteral` | `ReturnDefault` | mutates a whole function at once with no per-hit stop: native code takes a tier P entry stub (zero the result registers, `RET`), Python a `__code__` swap at arm time (tier C); the default comes from the result's DWARF type |

Verdict coverage: killed and survived (all), timeout (4), not reached (any site with no covering test), unevaluable (2 or 3 on a generic or trait-based Rust comparison), evaluator-raised (2), unsupported (5 and 6 under Python). Logical connector swap (`and`/`or`) is deferred: Python's `and` returns an operand, so it mostly tests profile exclusions rather than a new code path.

### 4.10 Application layer: angzarr-embedded [P]

The run itself (planning, scheduling, execution, verdicts, reports) is event-sourced on angzarr-embedded, angzarr's in-app CQRS/ES kernel: single process, in memory or on SQLite. The domain logic stays in `cherenkov-mutation`; the application layer (`cherenkov-app`) is thin aggregates and projectors that call it.

```
 PlanRun / PlanFile ─► Cherenkov aggregate ── decide = core::plan + value policy ──► MutantPlanned (one event per mutation)
                              ▲                                                    MutantScheduled | MutantNotScheduled(reason)
                              │                                                          │
                              │                                         run projector dequeues ◄┘
                              │                                                          │ MutantExecutor: arm, run covering tests, disarm
                              └── fact: OutcomeObserved(raw outcome, tier, timings) ◄────┘
                                  ingest = core::classify ──► VerdictReached
                                                                   │
                                  report projectors: JSON / JUnit / TAP, concept coverage, baseline diff
```

- **Planning is a Cherenkov aggregate.** A planning command carries the file's path, digest and content; decide runs the core planner and the value policy, both pure, and emits one `MutantPlanned` event per mutation, then `MutantScheduled` or `MutantNotScheduled` with its reason (excluded, arid, not covered, unsupported by executor). The kernel stamps each event with the build's rules version, so a planner change is visible in the log, and replay never re-plans.
- **A projector runs the mutants.** The run projector dequeues `MutantScheduled` events, drives the `MutantExecutor` port, and reports the result back to the aggregate as a **fact**, not a command: an outcome is something that already happened, which the aggregate ingests and cannot refuse. Ingest calls `core::classify` and emits `VerdictReached`.
- **Delivery is at least once.** A crash between finishing a run and recording it re-runs that mutant on resume. Ingest keeps the first outcome per (mutant, run) and ignores repeats, so duplicates are harmless.
- **Reports are projectors** over `VerdictReached`: JSON, JUnit and TAP, per-language concept coverage, and a baseline diff (survivors new since a previous run or commit), which stable mutant IDs and the event log make a plain read model.
- **Boundary.** `cherenkov-mutation` does not depend on angzarr. Aggregates call the core's pure functions; the composition root (`cherenkov-cli`) wires the kernel, the store and the adapters.

**Transport gap: events must queue.** angzarr-embedded's catch-up hands each projector its sources one stream at a time, from one position per stream. That is pub/sub with a cursor, not a work queue: there is no parallel dequeue, no per-item completion, no redelivery of an abandoned item. Running mutants in parallel needs queue semantics. Options:

| Option | Change | Trade-off |
|---|---|---|
| (a) Partitioned projectors | none: N named run projectors, each taking mutants with `hash(MutantId) mod N == k`, each with its own stored position | works today; static balance, so one slow partition holds back its share |
| (b) Per-mutant streams plus a concurrent catch-up | angzarr-embedded: a catch-up that hands books of different streams concurrently, bounded, committing each stream's position as its handler completes; mutants live on their own streams | smallest semantic change: the contract already says no order crosses streams, and positions are already per stream, so a stream's position is the mutant's acknowledgement |
| (c) Queue delivery mode | angzarr-embedded and angzarr-ports: competing consumers over one stream, bounded in flight, completion recorded as a watermark plus the completed set above it, redelivery on resume, dead letters after repeated failure | the general mechanism; the store contract is shared with angzarr core, so it crosses repositories |
| (d) angzarr core's bus | none: AMQP, Kafka and SNS/SQS consumer groups provide competing consumers, with ack, nack and a dead-letter queue | the distributed deployment, on the same wire contract; not single-process |

Recommendation: (a) for the prototype, so the application layer does not wait on a kernel change; (b) as the angzarr-embedded change, decided there. Under any option, a mutant that repeatedly kills the worker itself (the debugger or the harness, not the debuggee) is dead-lettered and reported as an infrastructure failure, which does not count in the score (section 12).

**When.** M0 (planning) and the M1 Delve spike do not need the application layer. It arrives at M2 with the executor, verdicts and reports, where it replaces code that would otherwise be written by hand.

**Validation corpus next door.** The angzarr repositories already run cargo-mutants at 100% (`angzarr-embedded/mutants.out`, the router's `just mutation-test`), which gives the Rust backend a mutant-by-mutant comparison on a real codebase (section 10).

---

## 5. Capability matrix

Legend: **Y** declared true, **N** declared false, **–** not declared in the source checked, **?** not checked.

| Adapter | setVariable | gotoTargets | Cond. / hit-count bp | read / writeMemory | Native escape via repl | Sources |
|---|---|---|---|---|---|---|
| debugpy (Python) | Y | **Y** | Y / Y | – | Python `exec` in frame | S1, S11, S12 |
| GDB native DAP (Rust, C, C++) | Y | – (no goto module) | ? | ? | repl evaluates GDB CLI commands | S9, S13, S15 |
| Delve (Go), DAP server (primary) | Y | – | Y / Y | Y / Y | none over DAP | S3 |
| Delve (Go), JSON-RPC API (augmentation) | Y [K] | – | Y / Y [K] | ? | terminal commands, including `jump` if it exists (unverified) | S38, S40 |
| lldb-dap (C/C++/Rust/Swift) | source: handler exists; docs table: N | **N** (PR open Mar 2025) | Y / Y | Y / Y | LLDB commands via `commandEscapePrefix` (default backtick) | S5, S6, S7, S14 |
| js-debug (JS/TS) | Y | **N** | Y / Y | Y / Y | JS evaluate | S2 |
| java-debug (JVM) | Y | – | Y / Y | – | hot code replace (custom, non-DAP) | S4, S17, S18 |
| cpptools / MIEngine | ? | **Y** (gotoTargets merged Jan 2021; goto request was a separate PR) | ? | ? | GDB/LLDB via MI | S8 |
| Ruby debug | ? | **N** ("Never") | ? | ? | ? | S10 |
| CodeLLDB, netcoredbg | ? | ? | ? | ? | ? | not read |

Other declared capabilities of note: setExpression is absent in Delve and java-debug [S3][S4]; js-debug declares restartFrame and breakpointLocations [S2].

**Conclusion.** Breakpoint conditions, hit counts and setVariable are effectively universal across the adapters checked. gotoTargets is not. Control-flow mutants therefore depend on the adapters that declare goto, a native escape, or, for native code, instruction patching through memory writes (tier P), which Delve's DAP server, lldb-dap and js-debug declare. Rows below the first three are outside the initial language set and kept for later expansion.

---

## 6. Arming and scoping

**Mechanism.** VS Code's triggered breakpoints (enabled once another breakpoint is hit) work for all languages [S19], which implies the client arms them rather than the adapter [I]. The DAP executor can do the same with plain `setBreakpoints` calls.

**What arming buys, and what it does not** [P]. A mutant must apply on every execution of the site within the test, otherwise it is a transient fault, not a mutant. Tests that never execute the site never hit the breakpoint anyway. So arming does not reduce stops at the mutated site. Its value is elsewhere:

- **Process reuse.** One debuggee runs many (mutant, test) pairs in sequence: arm mutant A for test T1, disarm, arm mutant B for T2. This amortizes runtime startup (Python imports, dynamic linking, test binary load). Risk: a mutant that corrupts global state contaminates later runs; a rerun-on-unexpected-result policy limits damage.
- **Excluding bootstrap.** Test discovery and framework startup may execute the site. Arming at the earliest per-test hook keeps those runs unmutated.

**Semantic deviation to minimize.** A source mutant is active for the whole run, including fixtures. Arm at the earliest per-test hook available, not at the test body, so fixtures see the mutant too. Where in doubt, fall back to one test per process with the breakpoint set before `configurationDone`.

**Native trap cost.** On native debuggers a conditional breakpoint traps the process even when the condition is false. GDB can push conditions to gdbserver as bytecode, which avoids notifying GDB on false conditions but not the trap itself [S20]. Put trigger breakpoints on test entry points, never on hot paths [P].

---

## 7. Verdicts [P]

| Outcome | Definition | Counts in score |
|---|---|---|
| killed | any covering test fails or the process crashes | yes, detected |
| survived | all covering tests pass | yes, undetected |
| timeout | exceeds a limit relative to the unmutated run under the *same* backend (the debugger itself slows the baseline), judged on wall time less the time the run was runnable but waiting for a CPU (kernel `schedstat` of the waiting thread, the debugger and the debuggee), so machine load does not change a verdict; a run is timed out at the latest at six times the limit | yes, detected; jump-based branch mutants can create infinite loops |
| not reached | no selected test reaches the site | no; left out of the result (coverage is the project's concern) |
| unevaluable | evaluator rejected the mutated statement (unsupported construct, optimized-out variable, failed resolution); or the mutant would not compile as source (for example, deleting a declaration that later code uses) | no; report per operator and language |
| evaluator-raised | the mutated statement threw inside the evaluator | no; report separately, optionally re-verify with a real source mutant |
| unsupported | no available executor supports the plan's action | no; report per backend |

Every verdict records which tier (section 8) applied the mutant, so a language with fewer operators cannot look better-tested.

---

## 8. Execution tiers

**Tier A: pure DAP.** `evaluate` plus `gotoTargets`/`goto`. Initial use: debugpy (Python). Also cpptools if C/C++ is added.

**Tier B: DAP plus native escape.** DAP for everything except moving the PC and returning. Initial use: GDB (Rust).

- GDB DAP: `evaluate` with `context: "repl"` runs the input as a GDB CLI command [S13][S15]. `jump` resumes execution at a location and `return` pops the frame [K]. Because `jump` resumes, the driver must not also send `continue` [K].
- lldb-dap (not initially used): repl input prefixed with the escape prefix runs as an LLDB command [S14]. `thread jump --file F --line N` sets the PC [S16]; `thread return <expr>` pops the frame with a value [S16]. The SB API also exposes `SBThread.JumpToLine` [S6].
- Open: whether the adapter's view of thread state stays consistent after a native PC change (section 12).

**Tier C: managed-runtime redefinition, no per-hit stops.** Not in the initial set; kept for later languages. JVM hot code replace sends new class files over the debug channel [S17], limited to method-body changes [S18]. In Python, replacing a function's `__code__` would keep existing references pointing at the mutant [I]; this is a possible later optimization for Python hot sites. JS redefinition is limited by module bindings and closures [I].

**Tier D: restricted fallback (run-then-overwrite).** For backends with no goto and no escape. Only assignments whose right-hand side is pure (no calls, no increments, no nested assignment, determined by the profile's purity predicates): let the statement run, evaluate the mutated RHS, `setVariable` the target. Double evaluation is harmless when pure. Branch, deletion and return mutants are unavailable. Not used for the initial languages.

**Tier P: instruction patch, armed once (native code).** For native backends (Delve for Go; GDB for Rust), apply the mutant by changing the machine instructions that implement the mutated construct, once when the mutant is armed, through DAP `writeMemory` (declared by Delve [S3]; for GDB via DAP or its repl); restore the original bytes when it is disarmed. The program then runs natively with no per-hit stop, and code with side effects still runs exactly once, so the patch is exact for every shape it supports [P]. Instruction shapes and patch rules are data per instruction set (`isa/amd64.toml`, `isa/arm64.toml`), the same pattern as the language token tables, and the patcher is shared by every native driver.

| Source mutant | Patch (amd64; arm64 analogous) |
|---|---|
| Condition negation | invert the condition code of the conditional branch (`Jcc`); on amd64 the inverse condition differs in the opcode's low bit [K]; arm64 `B.cond` likewise [K] |
| Condition constant true / false | replace the branch with `JMP` or with `NOP`s, according to which target is the then-block |
| Relational swap (`<` to `<=`) | change the condition code (`JL` to `JLE` signed, `JB` to `JBE` unsigned) when the comparison feeds the branch directly |
| Conditions materialized as values | `SETcc` and `CMOVcc` encode the condition code the same way as `Jcc` [K] |
| Arithmetic `+` / `-` | `ADD` to `SUB`, same length; immediate forms differ only in the ModRM byte, which selects the operation [K] |
| Mutated constant or argument (`f(a + 1)` to `f(a)`) | patch the immediate, or `NOP` the instruction |
| Statement deletion, void-call removal | `JMP` from the statement's first instruction to the next statement's first instruction |

Locating the instructions: the line table gives each statement's PC range; the parse tree gives the then- and else-targets, so a branch emitted with inverted polarity (branch to else when false) is patched correctly. Test binaries are built without optimization or inlining (`-gcflags=all=-N -l` for Go, the debug profile for Rust) so code generation is regular. Hazards: compound conditions (`a && b`) compile to several branches that must be mapped to their sub-expressions or excluded; float comparisons use different flags and need NaN-correct rules, or fall back to evaluate-and-branch; skipping a statement must not skip compiler-inserted initialization that later instructions rely on, which Delve's documentation warns about for jumps (checked conservatively against the skipped range; deleting a declaration that later code uses is not valid source and is reported `unevaluable`). Shapes the ISA tables do not recognize fall back to the per-hit path.

**Go under unmodified Delve.** Delve's DAP server declares no goto and no setExpression [S3]. Delve is used as shipped: over DAP, augmented by its JSON-RPC API [S40] where DAP lacks a primitive. In order of preference:

1. **Tier P** over DAP `writeMemory`, for every operator the ISA tables can express.
2. **Delve's `jump` command**, reached over the JSON-RPC augmentation, for evaluate-then-skip, compound and float conditions, and returns. Whether this command exists is unverified [S38]; an old feature request asks for "set next statement" [S39].
3. **Emulated jump** if Delve has no `jump`: at a stop at address A, remove the breakpoint and write `JMP target` at A, set a one-shot breakpoint at the target, continue, and at the target restore A's bytes and re-arm. Two stops per hit. Another goroutine that reaches A during the swap also takes the jump, so run with `GOMAXPROCS=1`.

Returns under Delve: `setVariable` the result variable (`~r0`) to the mutated value, then jump to the epilogue that loads results into registers. This assumes that without optimization the results stay in stack slots until the epilogue [I]. Why not GDB for Go: Go's documentation says Delve understands Go better and that GDB is not reliable for Go programs, especially concurrent ones [S27].

**Tier E (overlay rebuild) is removed.** Draft 3 rebuilt each Go mutant with `go build -overlay`. It was the only adapter that wrote files, and it reintroduced the per-mutant compile the design exists to avoid.

---

## 9. Performance model

**Where the time goes** [I]. Tiers A and B, per hit at an armed site: OS trap and context switch (native), adapter processing, DAP round trip, evaluate, PC move, continue; plus startup per debuggee process. The emulated jump costs two stops per hit. Tier P: one `writeMemory` to arm and one to disarm per mutant, then native speed with no per-hit cost. No measured numbers were found; section 10 measures them.

**Hot-site mitigation (only if needed)** [P]. Run evaluate-and-jump inside the debugger so the executor never sees the stop:

- GDB: a `gdb.Breakpoint` subclass whose `stop()` returns `False` lets the inferior continue [S22]. An older GDB manual advises against altering inferior data inside `stop()` [S23], so this needs testing before use.
- LLDB: a Python breakpoint command that returns `False` does not stop the process [S21].
- Install either through the repl escape, so the control plane is still DAP. This is driver-internal; plans and operators do not change.

**Test-time reduction** [P]. Separate from compile-time savings.

1. Reach pass: one-shot breakpoints (removed on first hit) at every site, per selected test, under the same debugger. Produces a site-to-tests map; it is a scheduling aid, not a coverage report.
2. Per mutant, run only the tests that reach it, fastest first, stop at the first kill.
3. Parallelize across independent processes.

---

## 10. Validation plan [P]

- **Reference.** The unmutated original, run under the same backend, is the reference for every verdict.
- **Fidelity against source mutants.** For a sample of mutants, build the equivalent real source mutant with an established tool (e.g. cargo-mutants for Rust, mutmut for Python) and compare verdicts. Classify disagreements: evaluator fidelity, line-table granularity, timing, arming deviation, patch mapping.
- **Patch integrity (tier P).** After disarm, the patched range is byte-identical to the original; a run with the mutant armed and immediately disarmed matches the reference.
- **Fixture projects.** `fixtures/{python,rust,go}/` carry hand-labelled expected verdicts per mutant, including at least one expected survivor per operator; they are the integration tests from the first executor onward.
- **Evaluate success rate** per language and operator.
- **Concept coverage** per language profile.
- **Per-hit overhead** for debugpy, GDB and Delve (real and emulated jump), from a microbenchmark loop over an armed site; tier P arm and disarm cost.
- **End-to-end wall clock** against rebuild-per-mutant on the project that motivated this.
- **Kill conditions.** A tier is not viable for a language if agreement or evaluate-success falls below thresholds set before the prototype (values to be decided).

---

## 11. Prototype scope [P]

- **Operators (catalog, written once):** the six breadth operators of section 4.9.
- **Language profiles (data):** Python, Rust, Go. **ISA tables (data):** amd64 first, arm64 after.
- **Executors:** one DAP executor with the debugpy driver (Python, tier A), the GDB driver (Rust, tier B, plus tier P) and the Delve driver (Go, tier P, plus `jump` over JSON-RPC or the emulated jump).
- **Test catalogs:** pytest plugin (with in-process debugpy), cargo test via the target-runner shim, go test with a prebuilt test binary under Delve (section 4.8).
- **Report sinks:** JSON verdicts, plus JUnit XML with one test case per mutant.
- **Acceptance criteria:**
  - No operator, planner or verdict code branches on language or backend; all differences come from profiles, ISA tables and adapters.
  - `cherenkov-mutation` has no I/O dependencies and no port that writes files; no adapter writes files (architecture test).
  - Tier P disarm leaves the patched range byte-identical to the original (tested).
- **Deliverables:** CLI; per-mutant verdict JSON with tier and timings; per-language concept coverage; fidelity report.

### 11.1 Milestones

The first milestone after planning runs one thin slice through every backend, because each backend's riskiest assumption is different and none is tested until that backend runs end to end.

| Milestone | Scope | Exit check |
|---|---|---|
| **M0: core and planning** | concepts, `Edit`, `MutantPlan`, `Verdict`, port traits; stable mutant IDs; tree-sitter profiles for all three languages; concept coverage | `cherenkov plan <dir>` lists mutants for Python, Rust and Go; golden-file tests; core builds with no I/O dependencies |
| **M1: Delve spike** | generated DAP types and a minimal DAP client; Delve over DAP: confirm `writeMemory` on the code section, setVariable on result variables; whether `jump` exists and whether a JSON-RPC client can join the same session; tier P branch patch on amd64 with `-N -l` | operators 1, 3 and 4 kill and survive as labelled on the Go fixture; disarm is byte-identical |
| **M2: executor, application layer, reports** | shared debugger executor; verdict classification in core; the Cherenkov aggregate, coverage and run projectors on angzarr-embedded with partitioned projectors (section 4.10, option a); JSON and JUnit report projectors; Go test catalog | first full verdict run on the Go fixture, all six operators, with tier recorded; a run killed midway resumes and finishes with no duplicate verdicts |
| **M3: debugpy (tier A)** | debugpy driver; pytest plugin | the Python fixture; evaluator errors always reported, never scored as success |
| **M4: GDB (tiers B and P)** | GDB driver; cargo runner shim; tier P reused for Rust | the Rust fixture; first evaluate-success rate for GDB's Rust parser |
| **M5: speed and validation** | coverage probe; covering tests only; process reuse; comparison against cargo-mutants and mutmut | fidelity report (section 10), with kill thresholds set before M5 starts |

Delve comes first among the backends because the whole Go path rests on unverified primitives, which makes it the riskiest assumption in the design.

---

## 12. Open questions

- GDB DAP: conditional and hit-count breakpoint support not yet read from source; consistency of DAP thread state after a CLI `jump`.
- GDB Rust parser coverage in practice: what fraction of generated Rust mutants evaluate successfully.
- Python `return` mutants under debugpy: no DAP return request and no native escape identified; caller-side fallback or `__code__` swap.
- Delve DAP: whether `writeMemory` is accepted on the code section (declared [S3], not yet exercised); tier P for Go depends on it.
- Delve: whether a `jump` command exists (`dlv help jump`); if not, the emulated jump is the only per-hit path, at two stops per hit, and no JSON-RPC augmentation is needed.
- Delve: whether one headless Delve process can serve a DAP client and a JSON-RPC client at the same time; if not, the augmentation needs another arrangement.
- Delve: how memory writes interact with Delve's own breakpoints, which keep the original bytes under each breakpoint; a patch at a breakpointed address could be lost or overwritten when Delve restores them.
- Go without optimization: whether result values stay in stack slots (`~r0`) until the epilogue, which forced returns depend on.
- Emulated jump: the goroutine race during the patch-and-restore window; whether `GOMAXPROCS=1` is sufficient.
- Tier P: what fraction of mutant sites the ISA tables recognize on real code; compound and float conditions; arm64 parity with amd64.
- Multi-threaded and async code: adapters differ on stopping one thread versus all; stopping perturbs timing; resume targets across `await`.
- Line-table granularity: statements spanning lines, several statements per line, macro expansion (Rust macros in particular).
- Whether arming at the earliest per-test hook is close enough to whole-run mutant semantics in practice.
- Whether the concept model stays small as operators are added, or grows language-shaped. Draft 4 takes it to 14 concepts for the classic families; anything beyond that should be weighed against this question.
- angzarr-embedded queue semantics: option (b) or (c) in section 4.10, decided in angzarr-embedded; until then, how far static partitioning (option a) is from balanced on real runs.
- Event volume: one `MutantPlanned` per mutation is tens of thousands of events per run on a large project; store size and catch-up time on SQLite.
- Infrastructure failures: a mutant that crashes the debugger or the harness rather than the debuggee. Proposed as a non-scoring outcome after dead-lettering; to be added to section 7 once the executor shows how often it happens.
- Variable replacement at the stop: how a plan whose substitute is chosen at runtime fits the planner, mutant IDs and baselines; whether candidates differ between hits (different frames, different scopes) and what that means for mutant semantics.
- lldb-dap setVariable docs/source conflict and gotoTargets PR status (relevant only if LLDB is added later).

---

## 13. Name

Working name **cherenkov**: Cherenkov radiation is emitted when a charged particle moves faster than light can in the medium it is passing through, which covers both "radiation" and "fast".

Registry check run 2026-11-17 (HTTP status from each registry's API; 404 means the name is unclaimed) [S29]:

| Name | crates.io | PyPI | npm | Notes |
|---|---|---|---|---|
| cherenkov | free | free | free | GitHub hits are physics projects, none in testing tools |
| cerenkov | free | free | free | alternate spelling |
| spallation | free | free | free | runner-up: a fast particle knocking fragments off a nucleus |
| elephantsfoot | free | free | free | long |
| demoncore | free | free | free | criticality accident that killed two people; tone risk |
| gammaray | free | free | free | conflicts with KDAB GammaRay, an established Qt introspection/debugging tool |
| corium | free | taken | free | |
| sievert | free | free | taken | |
| geiger, ionize, scram | taken | taken | taken | |

RubyGems could not be reached from the research environment, so it is unchecked. Registry state is a snapshot.

---

## References

Sources read during research (S) and background references (R).

- S1. debugpy adapter initialize capabilities: https://raw.githubusercontent.com/microsoft/debugpy/main/src/debugpy/adapter/clients.py
- S2. js-debug capabilities: https://raw.githubusercontent.com/microsoft/vscode-js-debug/main/src/adapter/debugAdapter.ts
- S3. Delve DAP server: https://raw.githubusercontent.com/go-delve/delve/master/service/dap/server.go
- S4. java-debug InitializeRequestHandler: https://raw.githubusercontent.com/microsoft/java-debug/main/com.microsoft.java.debug.core/src/main/java/com/microsoft/java/debug/core/adapter/handler/InitializeRequestHandler.java
- S5. lldb-dap documentation (capability table): https://lldb.llvm.org/use/lldbdap.html
- S6. lldb-dap jump-to-cursor PR #130503: https://lists.llvm.org/pipermail/lldb-commits/Week-of-Mon-20250324/135323.html and https://lists.llvm.org/pipermail/lldb-commits/Week-of-Mon-20250310/134006.html
- S7. lldb-dap SetVariableRequestHandler: https://llvm.googlesource.com/llvm-project/+/e612f37f2c110987ec43f8aa4fe8e86d6f64186f/lldb/tools/lldb-dap/Handler/SetVariableRequestHandler.cpp
- S8. MIEngine GoToTargets PR #1102: https://gh.ali360.cloudns.ch/microsoft/MIEngine/pull/1102
- S9. GDB DAP source directory: https://gnu.googlesource.com/binutils-gdb/+/refs/heads/master/gdb/python/lib/gdb/dap
- S10. Ruby debug server_dap.rb: https://gem.sh/gems/debug/v1.9.1/files/lib%2Fdebug%2Fserver_dap.rb
- S11. ptvsd issue 1234 (repl eval/exec fallback): https://github.com/Microsoft/ptvsd/issues/1234
- S12. pydevd evaluate tests (exec updates frame locals): https://sources.debian.org/src/pydevd/3.3.0%2Bds-4/tests_python/test_evaluate_expression.py
- S13. GDB manual, Debugger Adapter Protocol: https://www.gnu.org/software/emacs/manual/html_node/gdb/Debugger-Adapter-Protocol.html
- S14. lldb-dap commandEscapePrefix: https://src.rcs.uwaterloo.ca/xref/llvm-project/lldb/tools/lldb-vscode/README.md
- S15. GDB DAP evaluate.py (repl context dispatch): https://gnu.googlesource.com/binutils-gdb/+/b33c4f8f828e168d96b60d4ac828fdc19c8cdcb4/gdb/python/lib/gdb/dap/evaluate.py
- S16. LLDB thread jump test: https://src.rcs.uwaterloo.ca/xref/llvm-project/lldb/test/API/functionalities/thread/jump/TestThreadJump.py ; thread return: https://www.kodeco.com/2426-video-tutorial-using-lldb-in-ios-part-3-backtraces-threads-and-frames
- S17. Hot Code Replacement for Java in VS Code: https://devblogs.microsoft.com/java/?p=16275
- S18. vscode-java-debug Hot Code Replace wiki: https://github.com/Microsoft/vscode-java-debug/wiki/Hot-Code-Replace
- S19. VS Code 1.86 triggered breakpoints: https://www.infoworld.com/article/3712704/visual-studio-code-adds-hey-code-voice-command.html
- S20. GDB stub-side breakpoint conditions RFC: https://inbox.sourceware.org/gdb-patches/831urdp8vb.fsf@gnu.org/T
- S21. LLDB breakpoint-triggered scripts: https://lldb.llvm.org/use/tutorials/breakpoint-triggered-scripts.html
- S22. GDB Python breakpoints (ROCgdb manual): https://rocm.docs.amd.com/projects/ROCgdb/en/docs-6.0.2/ROCgdb/gdb/doc/gdb/Breakpoints-In-Python.html
- S23. GDB Python breakpoints (older manual): https://www-zeuthen.desy.de/unix/unixguide/infohtml/gdb/Breakpoints-In-Python.html
- S24. rustc dev guide, debugging support (GDB Rust parser; GDB better on Linux): https://rust.googlesource.com/rustc-guide/+/refs/heads/master/src/debugging-support-in-rustc.md and https://rustc-dev-guide.rust-lang.org/debugging-support-in-rustc.html
- S25. rustc dev guide, LLDB internals (partial Rust support, TypeSystemClang): https://rust.googlesource.com/rust-lang/rustc-dev-guide/+/e57cc5df56c50ba7bfa7e5171d331c067ed4fc9d/src/debuginfo/lldb-internals.md
- S26. go build -overlay: https://manpages.ubuntu.com/manpages/questing/man1/go-build.1.html ; fsys overlay semantics: https://godocs.io/cmd/go/internal/fsys@v1.16.3 ; overlay cache staleness test: https://golang.org/src/cmd/go/testdata/script/build_overlay.txt ; limitations: https://dyn.manpages.debian.org/trixie/gccgo-go/go-build.1
- S27. Debugging Go Code with GDB: https://golang.google.cn/doc/gdb and https://tip.golang.org/doc/gdb
- S28. crates.io API, queried 2026-11-17: https://crates.io/api/v1/crates/tree-sitter , https://crates.io/api/v1/crates/dap , https://crates.io/api/v1/crates/dap-types , https://crates.io/api/v1/crates/debug_types
- S29. Name availability, queried 2026-11-17: crates.io, PyPI and npm registry APIs; GitHub repository search
- S30. BSP `debugSession/start`: https://docs.rs/bsp-types/0.1.0/bsp_types/struct.DebugSessionStart.html ; Bloop debugging reference: https://scalacenter.github.io/bloop/docs/debugging-reference
- S31. Cargo target runner: https://docs.rs/vkcargo/latest/cargo/util/config/struct.TargetConfig.html , https://github.hscsec.cn/rust-lang/cargo/issues/1411 ; nextest target runners: https://nexte.st/book/target-runners
- S32. `go test` flags (-exec, -json): https://go.googlesource.com/go/+/master/src/cmd/go/internal/test/test.go
- S33. TAP 14 specification: https://testanything.org/tap-version-14-specification.html ; KTAP: https://docs.kernel.org/5.17/dev-tools/ktap.html
- S34. Bazel test encyclopedia: https://docs.bazel.build/versions/main/test-encyclopedia.html
- S35. nextest machine-readable output: https://nexte.st/docs/machine-readable ; libtest JSON: https://nexte.st/docs/machine-readable/libtest-json/ ; Rust project goal: https://goals.rust-lang.org/2026/libtest-json.html
- S36. debugpy API (listen, wait_for_client): https://docsearch.algolia.com/mcp/docs/repo/microsoft/debugpy
- S37. Delve `pkg/proc/threads.go` (`Thread.SetReg`; a minimal implementation need only support setting the PC), v1.26.0: https://sources.debian.org/src/delve/1.26.0-2/pkg/proc/threads.go
- S38. Delve CLI command reference, v1.26.0 (whether a `jump` command exists is unverified: search summaries conflicted and the page could not be fetched): https://sources.debian.org/src/delve/1.26.0-2/Documentation/cli/README.md
- S39. Delve issue 1045, support for instruction reordering / jumping: https://gh.ali360.cloudns.ch/go-delve/delve/issues/1045
- S40. Delve JSON-RPC API documentation (not fetched for this draft): https://gh.ali360.cloudns.ch/go-delve/delve/tree/master/Documentation/api/json-rpc
- S41. Delve function call injection (safe points only; resumes all goroutines; restricted arguments): Delve CLI README v1.8.2, https://gh.ali360.cloudns.ch/go-delve/delve/blob/v1.8.2/Documentation/cli/README.md ; implementation, https://sources.debian.org/src/delve/1.26.0-2/pkg/proc/fncall.go ; Red Hat overview, https://developers.redhat.com/index%2Ephp/articles/2023/07/20/new-delve-features-rhel-92
- R1. Untch, Offutt, Harrold, Mutation Analysis Using Mutant Schemata (1993): https://typeset.io/pdf/mutation-analysis-using-mutant-schemata-1k0msw5h9z.pdf
- R2. mutagen: https://github.com/llogiq/mutagen
- R3. mutagen index entry (last push 2023-05-29): https://awesome.ecosyste.ms/projects/19364
- R4. mutest-rs (2026): https://eprints.whiterose.ac.uk/id/eprint/244320
- R5. Emamdoost et al., binary-level mutation analysis (NDSS BAR 2019): https://www.ndss-symposium.org/wp-content/uploads/bar2019_58_Emamdoost_paper.pdf
- R6. SN4KE: Practical Mutation Testing at Binary Level: https://arxiv.org/abs/2102.05709v2
- R7. DAP specification: https://microsoft.github.io/debug-adapter-protocol/specification

## 4.11 Selection: from the tests

Cherenkov measures the tests: whether they detect faults in the code they
drive. Coverage is the project's concern, not Cherenkov's; it runs no
coverage tool and reports no coverage.

1. **Tests.** The `TestCatalog` lists tests by the runner's own names
   (libtest `--list`, `go test -list`, pytest collection). The user or agent
   names any subset; `cherenkov run --test NAME` (repeatable) passes the
   runner's selection flags (libtest exact filters, an anchored `-test.run`).
2. **The code they drive.** The code under test is what a non-test build
   compiles: for Rust, the sources rustc's dep-info lists for `cargo check`
   (`toolchain::rust_code_files`). Test modules, inline or in their own
   `#[path]` files, are never mutated. Before the mutants run, one recording
   run per test program breaks at each planned function's entry
   (`MutantExecutor::driven`), and a mutant runs only in the programs whose
   tests call its function; with `--test NAME`, only what that test calls.
   A mutant no selected test calls is counted as `not-called` and
   not run. A mutant whose function is called but whose site is never
   reached comes back `NotReached` and is left out of the result. Neither
   says anything about those tests.
3. **Changed code.** A `VersionControl` port (git adapter first) lists the
   lines changed between two revisions; the changed mode mutates those
   lines and runs the selected tests (all by default) against them.
