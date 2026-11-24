# The language contract evaluated for Java, Python, Rust, Go, C++, C#, JavaScript and TypeScript

Evaluated 2026-11-19 against master `43e2884`. Research and design only; no
code changed. Revised the same day to add JavaScript and TypeScript
(section 7) and to correct the execution model to the implemented one in
docs/language-profiles.md section 6 (see "Corrections" below).

The question: can the language-neutral contract (docs/design.md; the core in
`crates/core/domain/cherenkov-mutation`) carry eight languages (Java, Python, Rust, Go, C++,
C#, JavaScript, TypeScript), measured by the canonical
mutations Cherenkov already defines? That means the 21 rows of
docs/rust-operator-evaluation.md and what was added since, the 65 named
mutations of `cherenkov mutations` (families: `ror` 20, `aor` 20, `bitwise`
10, `shift` 4, `lcr` 2, `cor` 2, `unary` 2, `negate-condition`,
`return-bool-flip`, `delete-statement`, `body-default`, `fn-value`,
`match-arm`, `match-guard` 2, `struct-field`). Language features that no
canonical mutation touches are out of scope.

The owner's principle is the frame: the portable unit is the named mutation
and the named set. A language is implemented by a **profile** (syntax to the
existing concepts; `concepts.scm`, `tokens.toml`, `profile.toml`) and an
**adapter** (build, tests, execution; any technique that fits the language).

Marks: **[V]** verified in a source or on this machine this session, **[M]**
from memory, **[I]** inference. Machine: Linux x86-64; java/javac 21.0.12.1,
dotnet SDK 8.0.426, gdb 16.3 (DAP built in), python 3.13.5, g++ 14.2.0,
clang++ 19.1.7, go 1.24.4, dlv 1.27.2 (`~/go/bin`, not on PATH), cmake
3.31.6, gradle 4.4.1, googletest 1.16 headers [V]. Not installed: lldb,
lldb-dap, netcoredbg, debugpy, maven [V].

## 1. Summary

### 1.1 Findings

1. **The profile layer carries all eight languages with no new `Concept`,
   `Capture`, `Token` or `Edit` variant.** Every canonical mutation has a
   site in every grammar except `struct-field` (no analogue in Java or
   Python; section 2). Four profile-layer problems force small core
   changes, listed in 4.1. None breaks an existing profile.
2. **The contract's risk is in the adapter layer**, and one part of it is
   wrong for managed runtimes: a JVM has **no jump** (set next statement)
   in JDWP, so tier B as written (`EvalThenJump`, `EvalBranch`) cannot run
   on Java. The managed-runtime tier replaces it (3.3). It works: a
   `<` to `<=` mutant was applied to a running JVM by `RedefineClasses` with
   one patched opcode, no source written, in 20 ms [V, 3.3.1].
3. **The test side is not language-neutral.** `MutantExecutor` and the CLI
   are bound to libtest: `libtest::list_tests`, `rust_test_args`, and
   `explain()` calling `libtest::failures` for every language [V,
   `crates/root/bin/cherenkov/src/run.rs:349`]. A `TestRunner` port is needed (4.2).
4. **Overloaded operators defeat tier P's assumption** that `a < b` is a
   compare instruction. Observed with g++ -O0: `V::operator<` is a `call`,
   `std::string <` is a `call`, `int <` as a value is `cmp; setl` [V, 3.2].
   A mutation that is not a polarity flip needs a language rule (2.2).
5. **`Requires` cannot see overloads** (4.1, item 2). It is only
   informational today, so this costs nothing yet.
6. **vsdbg cannot be used.** Its licence limits it to Visual Studio Code,
   Visual Studio and Visual Studio for Mac [V]. netcoredbg is MIT [V].
7. **JavaScript and TypeScript need one profile change the other six did
   not: a second equality spelling** (`===`/`!==` beside `==`/`!=`), because
   the token table has one spelling per token (section 7.2). They are also
   the cheapest proof of the managed tier: V8's `Debugger.setScriptSource`
   replaced a function's source text in a running Node 22 and the mutant
   ran, for plain functions, ES modules, closures, arrows, async functions
   and getters; it refuses a suspended async activation and the active
   top-level frame [V, section 7.3].

**Corrections to the first version.** docs/language-profiles.md section 6 and
8.1 (D15-D19) describe the implemented executor, which differs from
docs/design.md: the port is `NativeSession` + `Launcher` (not a `Driver`
trait); GDB tier B is a loop in GDB run by Python helpers that set `$pc`, not
DAP `evaluate` plus `jump`; Go is tier P only (Delve has no
`run_evaluating`) and writes memory over JSON-RPC; `Tier::A` has no
producer and there is no debugpy driver (Python is plan-only); `Requires`
is computed and no executor reads it. This version corrects the Go row,
the Python row, the GDB jump cell, the `lcr` row (the `Steer` effect is GDB
only) and the note that the family matrix shows what a mechanism can
realise, not what exists today.

### 1.2 Matrix

Readiness: **now** (works), **small** (days, no new mechanism), **medium**
(a new adapter on a known mechanism), **large** (new mechanism).

| | Grammar (tree-sitter) | Profile | Build and test | Debugger, protocol, licence | Tiers | Adapter | Main risks |
|---|---|---|---|---|---|---|---|
| Rust | tree-sitter-rust 0.24 [V in repo] | now | cargo; libtest | GDB 16.3, DAP + repl; GPLv3 [M] | P, B | now | generics copies; async; not a gap in the contract |
| Go | tree-sitter-go 0.25 [V] | now (no fn-value generator, no field-init) | `go test -c -gcflags=all='-N -l'`; `-test.v` output | Delve 1.27, DAP + JSON-RPC; MIT [M] | P only: `DelveSession` has no `run_evaluating` [V language-profiles §6] | now (fix parser choice) | Go output parsed as libtest; `*`, `/`, `%`, shifts, bitwise have no tier P rule and no tier B |
| Python | tree-sitter-python 0.25 [V] | now (no fn-value generator) | pytest, unittest | debugpy: `setExpression`, `gotoTargets`, logpoints [V]; MIT [M]; no Python executor exists (D16) | none today; `Tier::A` (evaluate plus goto) has no producer (D15); interpreter tier I proposed | medium | no machine code; debugger slows the interpreter; `__code__` swap needs closures care |
| C++ | tree-sitter-cpp [V node types] | small | CMake/CTest; GoogleTest, Catch2, doctest | GDB (DAP) now; lldb-dap later (no goto) [V]; LLDB Apache-2.0 with LLVM exception [M] | P, B (as Rust) | medium | templates (3 locations for one line [V]); overloaded operators [V]; test identity for Catch2/doctest; non-trivial return types |
| Java | tree-sitter-java [V node types] | small | Maven/Gradle; JUnit Platform ConsoleLauncher | JDWP (direct) or java-debug (EPL-1.0 [V]); JDI capabilities [V 3.3.1] | managed M; tier B partial (no jump) | large | class-file writer; JDWP client; test classpath and JVM flags |
| C# | tree-sitter-c-sharp [V node types] | small | `dotnet build`; `dotnet test` (vstest, testhost child) | netcoredbg, MIT, DAP + MI [V]; vsdbg barred [V] | managed M (unverified how); B unverified | large | netcoredbg capabilities unknown; IL rewriting; testhost attach; Tiered JIT |
| JavaScript | tree-sitter-javascript [V node types] | small (second equality spelling, 7.2) | npm; Jest, Vitest, Mocha, `node:test`, AVA; transform caches | V8 via CDP (`node --inspect-brk`), direct client or vscode-js-debug (licence [M] MIT; README silent); Node is MIT [M] | managed-source tier S (`setScriptSource`) [V]; tier B partial (no jump) | medium | async test attribution; per-test-file module reloads; scope-changing edits [I] |
| TypeScript | tree-sitter-typescript (typescript and tsx) [V node types] | small (shares JS rules; adds `return_type`) | as JS plus a transform (tsc, ts-jest, esbuild, swc, Babel); source maps | as JavaScript; the runtime executes generated JS | as JavaScript, through source maps | medium-large | source-map mapping at operator granularity [I]; two grammars in play |

The 65 named mutations by family and language. P = tier P instruction patch,
B = tier B (stop, evaluate, set the PC or return), M = managed tier (bytecode
or IL redefinition), S = source-redefinition tier (JS: `setScriptSource`),
I = interpreter tier (Python in-memory code swap), `-` = no analogue,
`x` = not feasible. The cells say what the mechanism can realise [I unless
3.x or 7.x says otherwise], not what exists today. Implemented today
(docs/language-profiles.md section 6): tier P covers branch condition-code
rewrites, `+`/`-` swaps, bool flips, statement deletion, `==`/`!=` and `!`
deletion, entry stubs for zero values and `Redirect`; shifts, bitwise,
`*` `/` `%` swaps and `InPlace` have no patch rule; tier B is GDB only
(Rust); Go is tier P only; Python is plan-only.

| Family | Rust | Go | C++ | Python | Java | C# | JavaScript | TypeScript |
|---|---|---|---|---|---|---|---|---|
| `ror` boundary and swaps, ints | P | P | P (value form `setl`; branch form `jcc`) | I | M (`if_icmpXX`) | M (`blt`.. in branch form; value form needs a longer sequence) | S | S |
| `ror` on floats | P (flags, NaN rules) | P | P | I | M (also swap `fcmpl`/`fcmpg`) | M (`.un` variants) | S (source-exact, NaN handled by V8) | S |
| `ror` on overloaded types, strings | P: call result polarity only | - | call result: polarity (`==`/`!=`, negate) only; `x` for the rest | I (always dispatches) | `==` on objects is `if_acmp`; `String` `<` n/a | polarity only; `x` for the rest | S (exact: `valueOf`, Proxy, strings run as written) | S |
| `aor` `+ - * / %` | P (`+` `-`); B for `*` `/` `%` | P (`+` `-` only) | P (no overflow check) | I | M (`iadd`..; same size) | M (`add`/`add.ovf` variants) | S (`+` is also string concat; no overflow) | S |
| `bitwise`, `shift` | P (planned: no rule yet) | x (no rule, no tier B) | P (planned) | I | M (`>>>` needs a token, 4.1) | M | S (`>>>` needs a token, 7.2) | S |
| `lcr`, `cor` | B (steer, GDB only) | x (steer is a GDB effect) | B (steer, via GDB) | I | M (branch retarget, same size [I]) | M | S (logical and/or return operands; `??` extra) | S |
| `unary` delete `!` `-` | P | P | P | I | M (`ineg` -> `nop`) | M | S | S |
| `negate-condition`, `return-bool-flip` | P | P | P | I | M | M | S (truthiness: `!(x)`) | S |
| `delete-statement/call` | P | P | P | I | M (NOP the range; stack-neutral) | M | S (`;` or `void 0;`) | S |
| `body-default`, `fn-value` | P stub; B | P stub | P stub; hard for non-trivial classes | I | M stub, or `ForceEarlyReturn` [V cap] | M stub; `async` special | S (`undefined`; arrow expression bodies) | S (types give `0`, `""`, `[]`) |
| `match-arm`, `match-guard` | P | P | P (`case_statement`; jump table) | I (`case` removal) | M (retarget table entry; pattern switch via `invokedynamic`: `x`) | M (`switch` IL; `switch` expression arms) | S (`switch_case`; no guards; fallthrough) | S |
| `struct-field` | P | P (keyed literal) | P (member-init list) | - | - | M (object initializer) | S (delete an object-literal property) | S |

Incumbent sets: Cherenkov already records the peer tool of each named
mutation (`same as PIT CONDITIONALS_BOUNDARY / mutmut / go-mutesting /
Stryker / Gremlins / Mull`; docs/operator-sets.md). I did not rebuild those
mappings. Spot checks against the tools' own docs this session:
`ror/lt->le` (PIT, Stryker, Mull `cxx_lt_to_le`, mutmut, Gremlins boundary)
and `ror/eq->ne` (PIT, Stryker, Gremlins, Mull) are right [V]. Under-tagged,
not wrong, since the doc says a peer is recorded only with evidence: `aor`,
`bitwise`, `shift` and `lcr` carry only `cargo-mutants` although PIT `MATH`
(`+ - * / % & | ^ << >> >>>`), Stryker (Arithmetic, Logical, Bitwise) and
Mull (`cxx_and_to_or`, `cxx_lshift_to_rshift`, ...) have them [V];
`unary/delete-not` lacks Stryker `!x`->`x` and Mull `cxx_remove_negation`,
and `unary/delete-neg` lacks Mull `cxx_minus_to_noop` and PIT
`INVERT_NEGS` [V]. Differences in meaning: PIT and Stryker send `%` to `*`;
ours sends `%` to `/` (the Mull and Dextool choice). Absent from the
catalogue and in the default sets of the tools: `++`/`--` swaps (PIT
`INCREMENTS`, Stryker Update, Mull) and null/constant returns (PIT).

## 2. Profile layer

Grammars were checked by reading each repository's `src/node-types.json`
this session [V]. All three (`tree-sitter-java`, `-c-sharp`, `-cpp`) exist
and are maintained under the tree-sitter organisation [V: fetched from the
master branch]. The existing `tree-sitter` 0.27 accepts grammars generated
for compatible ABIs [I]; adding a grammar is a `Cargo.toml` line and a
profile directory.

### 2.1 Concept to node, per language

Rust, Go and Python are in `profiles/`. New columns are from the node types
and fields read above.

| Concept | Java | C# | C++ |
|---|---|---|---|
| binary | `binary_expression` (left, operator, right) | same | same (`left`, `operator`, `right`) |
| unary | `unary_expression` (operand; `! - + ~`) | `prefix_unary_expression`, no fields: `["!" "-"] @op (_) @operand` as the Rust profile does | `unary_expression` (argument; `! - + ~ compl`) |
| compound assign | `assignment_expression` with a compound `operator` | same | same |
| assign | `assignment_expression` with `operator: "="`; `variable_declarator` | same; `variable_declaration` | same; `declaration` init |
| literal | `decimal_integer_literal`, `true`, `false`, `string_literal` .. | `integer_literal`, `boolean_literal`, `string_literal` .. | `number_literal`, `true`, `false`, `string_literal` |
| condition | `if_statement`, `while_statement`, `do_statement` (condition is a `parenthesized_expression`); `for_statement` (condition); `ternary_expression` | `if_statement`, `while_statement`, `do_statement`, `for_statement`; `conditional_expression` (no parentheses in the field) | `if_statement`, `while_statement` (`condition_clause` -> `value`); `do_statement`; `for_statement`; `conditional_expression` |
| return | `return_statement`; lambda expression body | `return_statement`; `arrow_expression_clause` (expression-bodied) | `return_statement` |
| call | `method_invocation` (name, object, arguments) | `invocation_expression` (function, arguments) | `call_expression` (function, arguments) |
| index | `array_access` | `element_access_expression` | `subscript_expression` |
| function | `method_declaration` (type, name, body) | `method_declaration` (returns, name, body: block or arrow), `accessor_declaration` | `function_definition` (type, declarator, body) |
| arm | `switch_rule`, `switch_block_statement_group` | `switch_expression_arm`, `switch_section` | `case_statement` |
| guard | `when` on a pattern label [M] | `when` clause [M] | none (no guards) |
| field_init | none | object initializer entry (`initializer_expression`) | `field_initializer` (member-init list), `initializer_pair` (designated) |
| statement | `expression_statement`, `local_variable_declaration`, `return_statement` .. | `expression_statement`, `local_declaration_statement` .. | `expression_statement`, `declaration` .. |
| async | n/a | `async` modifier on a method (`Capture::Async` exists) | `co_return_statement` (not mapped) |

Points that need a decision in the `.scm`, none of which needs core:

- **Assignment and compound assignment share one node** in all three
  grammars. The Go profile already splits them with an operator
  alternation (`operator: ["+=" "-=" ..]`), so the same works.
- **C++ function names and return types are nested.** The name is in the
  declarator (`identifier`, `qualified_identifier`, `field_identifier`,
  `operator_name`, `destructor_name`), and a trailing return type
  (`-> T`) or `auto` has no `type` field. Two patterns are needed;
  `@returns` stays optional, as the Rust profile already allows.
- **C# properties and expression-bodied members.** A getter's return type
  is the property's `type`, one level up; a nested pattern captures both.
  Expression-bodied members have `arrow_expression_clause`, which the
  Return capture treats like the Rust tail expression. `profile.toml`'s
  `body_braces`/`body_template` are one shape per language and do not
  describe `=> expr;` (4.1, item 4).
- **C++ alternative tokens** (`and`, `or`, `not`, `bitand`, `compl`) are
  distinct token texts. The token table maps one spelling per token, so
  code using them is silently unmatched: no wrong mutant, a missing one.
- **Preprocessor.** tree-sitter-cpp does not preprocess; both arms of an
  `#if` are parsed, and code inside a macro argument is parsed as an
  expression. A site in an inactive arm has no machine code: the adapter
  reports `no-breakpoint-site` (an existing deficiency code). Test macros
  (`TEST(Suite, Name) { .. }`) parse as a function-definition-shaped node
  [M]; a skip region with a `#match?` on the name set (`TEST`, `TEST_F`,
  `TEST_P`, `TYPED_TEST`, `TEST_CASE`, `SCENARIO`) removes test code.
- **Skip regions** (`@skip.test`, `@skip.annotated`): Java `@Test` (a
  `marker_annotation` in `modifiers`) and `src/test/**` in `exclude`; C#
  `[Fact]`, `[Test]`, `[TestMethod]` attributes and `*Tests.cs`; C++
  `*_test.cc`, `test/**`. The `SkipKind` set is fixed (`test`,
  `annotated`, `unsafe`, `constructor`, `default_impl`, `capacity_call`);
  the unused Rust-named kinds simply capture nothing.
- **Lambdas and local functions** matter only because a canonical site can
  sit in one. A lambda body is a function of its own in all three
  languages (Java synthetic `lambda$f$0`, C# compiler-generated method,
  C++ `operator()`); the Rust profile treats closures the same way
  (`closure_expression` as a return site).

Grammar-level differences that change the meaning of a captured token:

| Language | Difference | Effect |
|---|---|---|
| Java, C# | `>>>` unsigned shift | not in `Token`; no shift mutation on it (4.1, item 3) |
| Python | `is`, `is not`, `in`, `not in`; chained `a < b < c` | unmapped tokens give no mutant; chains are reported unrepresentable already [V in the Python profile] |
| Python | `and`/`or`/`not` are words; `and`/`or` return an operand | `lcr` mutants are value-correct in the interpreter (I), and `tokens.toml` already spells them |
| Go | `&` has multiplicative precedence | `Token::binds_tighter` knows only `&&` over `||` [V]; a bitwise swap's regrouping is the same fidelity issue as in Rust |
| C++ | `a < b > c` and template angle brackets | tree-sitter resolves with its grammar; a wrong parse is a missing mutant [I] |

### 2.2 Operator semantics per language

The neutral meaning is "the source with the token swapped". It holds
except for these language rules, which belong to the adapter that realises
the mutant, not to the `.scm`:

| Topic | Java | C# | C++ | Python | Go | Rust |
|---|---|---|---|---|---|---|
| overloaded comparison | none, except `String` `+` | yes (`op_LessThan`, string `==` is `op_Equality`) | yes (`operator<` is a `call` [V]) | always (`__lt__`) | none | traits (existing) |
| integer overflow | wraps | wraps unless `checked`; separate `add.ovf` opcodes [M] | signed overflow undefined, wraps in practice; no check | none (bignum) | wraps | panics in debug (existing guard) |
| division by zero | `ArithmeticException` | `DivideByZeroException` | SIGFPE (a crash, which Delve and GDB stop on) | `ZeroDivisionError` | panic | panic |
| float NaN in ordering | `fcmpl` for `>`, `>=`; `fcmpg` for `<`, `<=` [M]: a `<` -> `>=` swap must change the compare opcode as well as the branch | `.un` branch variants [M] | `ucomisd` parity flag | rich comparison false | as C++ | existing |
| `==` | reference equality on objects; `equals` is a `call` | reference equality unless overloaded | value for scalars; `operator==` call for classes | `__eq__` | value | `PartialEq` |
| short circuit | `&&`, `||` | `&&`, `||` (also `?.`, `??` outside the canonical set) | `&&`, `||` | `and`, `or` | `&&`, `||` | `&&`, `||` |
| nullable / optional compare | boxed types unbox with a call (`intValue`) before the compare | lifted operators on `int?` emit several compares for one source operator | n/a | `None < 1` raises | n/a | `Option` is a type |

The rule for an operator that compiles to a call (C++, C#, Java `equals`,
Python): **a mutation that only reverses the result is exact** (`negate`,
`==` <-> `!=`, `!x` deletion at the branch). `<` -> `>=` is **not** a
reversal for a partial order or a user type, so for a call it is
unsupported unless the sibling function is called instead (swap the call
target to `operator>=`, `op_GreaterThanOrEqual`). The adapter recognises the
call at the site and answers `no-patch-rule`; the deficiency code exists.

### 2.3 Value policy for `ReturnDefault`

`Profile.defaults` maps a return-type text to a literal; `ReturnValues` is a
Rust `fn` pointer for per-type value lists [V, `profile.rs:22`]. The values
the owner's canonical `fn-value` needs, by language:

| Language | Defaults | Type learned from |
|---|---|---|
| Java | `boolean` false/true; `int long short byte char float double` 0 (`char` `'\0'`); `String` `""`; reference types `null`; `Optional<>` `Optional.empty()`; `List/Set/Map/Collection<>` empty (`List.of()`); boxed numerics 0 (PIT `EMPTY_RETURNS`, `NULL_RETURNS`, `PRIMITIVE_RETURNS` [V pitest.org]) | the method descriptor in the class file or JDWP `Method.SignatureWithGeneric`; not needed in the profile except for display |
| C# | `default(T)`: 0, false, `null`; `string` `""`; structs `default` (`initobj`); `Task<T>`/`ValueTask<T>` for `async` methods: a completed task holding the default | the method signature blob in metadata [M] |
| C++ | value-initialisation `T{}`; scalars all-zero bits (`xor eax,eax; ret`); `std::vector` zero bits is an empty vector in libstdc++ [M], but `std::string` zero bits is **not** a valid string (the SSO pointer points into the object) [M]; classes without a default constructor: unviable (`no-default`, as Rust) | DWARF type DIE (`DW_TAG_base_type`, class layout, constructors in `DW_TAG_subprogram`) |
| Python | `None`; `bool` False; `int` 0; `float` 0.0; `str` `""`; `list` `[]`; `dict` `{}`; `set` `set()`; `tuple` `()`; `Optional[..]` `None`; unannotated: `None` | the annotation text (unenforced: returning `None` for `-> int` is legal Python, so there is no "unviable by type") |
| Go | zero value; `error` `nil`; slices, maps, pointers `nil`; structs all-zero bits (exact, as in docs/m1-delve-findings.md) | DWARF |
| Rust | existing table | DWARF (existing) |

What this implies for the contract: `ReturnValues` as a function pointer
means **every language's fn-value needs Rust code in the core build**, not
a profile file. Go and Python have none yet [V, docs/operator-sets.md,
"Language independence"]. A data-driven table in `profile.toml` (a type
pattern, a value, and a `needs_runtime` mark for `Optional.empty()`-like
values) would let a profile alone carry it. See 4.1, item 1.

## 3. Adapter layer

### 3.1 Build and test, per language

| | Build with debug info, no optimisation | List tests | Select | Result |
|---|---|---|---|---|
| Rust | `cargo test --no-run` with the debug profile env (existing) | `--list` (libtest) | name args | libtest text |
| Go | `go test -c -gcflags=all='-N -l'` [V in repo] | `-test.list .` [M] | `-test.run '^name$'` | `-test.v` text (`--- FAIL`, `file.go:N: msg`) or `go tool test2json` [M]; **not libtest** |
| C++ | CMake `-DCMAKE_BUILD_TYPE=Debug` (`-g -O0`) [M]; `ctest --show-only=json-v1` lists test executables and arguments [M] | GoogleTest `--gtest_list_tests`; Catch2 `--list-tests`; doctest `--list-test-cases` [M] | `--gtest_filter=`; Catch2 name argument; doctest `-tc=` [M] | `--gtest_output=json`; Catch2 `-r junit`; doctest `--reporters=junit` [M]; CTest `--output-junit` |
| Python | none; run `python -m pytest` | `pytest --collect-only -q` [M] | node ids as arguments | `--junitxml` or `-rA` text [M]; `unittest -v` text |
| Java | Maven (`maven-compiler-plugin` `-g` by default [M]), Gradle (`options.debug` true by default [M]); line numbers are in javac's default `-g:source,lines` [M] | JUnit Platform ConsoleLauncher `--scan-classpath --details=tree` [M]; JUnit 4 via the Vintage engine; TestNG `-testclass` | `--select-method pkg.C#m` [M] | ConsoleLauncher `--reports-dir` XML [M] |
| C# | `dotnet build -c Debug` (Optimize off, portable PDB) [M] | `dotnet test --list-tests` [M] | `--filter FullyQualifiedName=` [M] | `--logger trx` or `--logger "junit"`; TRX is XML [M]. `dotnet test` starts a **testhost child process**; `VSTEST_HOST_DEBUG=1` makes it wait for a debugger [M] |

Gradle 4.4.1 on this machine is too old for the JUnit Platform and maven is
absent [V], so the Java spike needs the ConsoleLauncher jar and an explicit
classpath: build output plus `gradle` init script or `mvn
dependency:build-classpath` to print it [M].

**The `TestRunner` port** (4.2). One recorded fact decides its shape: the
recording run finds "which test is on the stack at a function's entry" by
matching stack frame names to the test list. That works where a test is a
function whose symbol equals its name:

| Framework | Test on the stack | Strategy |
|---|---|---|
| libtest, Go, pytest | function `module::test_x` / `TestX` / `test_x` | frame match (existing, per language rules) |
| JUnit, xUnit, NUnit, MSTest | method `pkg.Class.method` in the frame | frame match; parameterised cases collapse to the method |
| GoogleTest | `Suite_Name_Test::TestBody()` | frame match on the mangled class |
| Catch2, doctest | an anonymous function registered at start-up; the name is a runtime string | **hook**: break at the invoker (`Catch::TestInvokerAsFunction::invoke`, `doctest::Context::run`) and read the current name, or run one test per process (about 0.1 s each [V, docs]) |

### 3.2 Debugger capabilities for the primitives

Primitives the contract uses (`NativeSession`, `Effect`): conditional break,
logpoint, instruction breakpoint, evaluate (with calls), set variable, jump,
force return, read/write code memory, stack inspection, stepping.

| Primitive | GDB 16.3 (DAP + repl) | lldb-dap | Delve 1.27 | debugpy | JDWP | netcoredbg |
|---|---|---|---|---|---|---|
| conditional break | DAP [V] | DAP [M] | DAP [V repo] | DAP [V] | no condition in `EventRequest.Set`; filters only; client evaluates [M] | unverified |
| logpoint | DAP [V] | DAP [V: in the base feature set] | DAP [M] | DAP [V] | client-side | unverified |
| instruction breakpoint | DAP [V] | handler exists [M] | JSON-RPC [V repo] | n/a | `Location` with a bytecode index [V spec] | unverified |
| evaluate with calls | `evaluate` repl; calls into the program [V repo] | DAP [M] | DAP, restricted [V repo] | DAP, runs Python [V] | **not in JDWP**; `InvokeMethod` only; java-debug's evaluator uses Eclipse JDT [M] | func-eval [M] |
| set variable | DAP `setVariable`, `setExpression` [V local] | DAP [M] | DAP [V repo] | `setExpression`, `setVariable` [V] | `StackFrame.SetValues` [V spec] | unverified |
| jump / set next statement | tier B sets `$pc` from Python helpers run in the repl (`gdb_tierb.py`); no DAP goto [V local: no `gotoTargets`; V language-profiles §6] | **no `gotoTargets` handler** [V]; CLI `thread jump` through the repl [M] | JSON-RPC `SetExecutionPoint` [V repo] | `gotoTargets` [V]; line-level, same frame [M] | **none** [V: not among the JDWP commands read; JDI has no API] | ICorDebug `SetIP` exists [V Microsoft blog]; netcoredbg exposure unverified |
| force return | repl `return`; entry breakpoint needed (inlined callee) [V repo] | CLI `thread return` [M] | none usable at `-N -l` [V repo] | none | `ThreadReference.ForceEarlyReturn` [V spec, cap true on JVM 21 V local] | none known |
| read/write code memory | DAP `readMemory`, `writeMemory`, `disassemble` [V local] | DAP handlers exist [M] | JSON-RPC only [V repo] | n/a | `Method.Bytecodes` read only [V spec] | n/a |
| redefine code | n/a | n/a | n/a | `__code__` swap through `evaluate` [M] | `VirtualMachine.RedefineClasses` [V spec, V local] | ICorDebug `ApplyChanges` / Hot Reload [M] |
| stack and threads | DAP [V] | DAP [M] | DAP [V] | DAP [V] | JDWP [V] | DAP [M] |

Licences: GDB GPLv3 [M], Delve MIT [M], LLDB Apache-2.0 with LLVM
exception [M], debugpy MIT [M], java-debug EPL-1.0 [V], netcoredbg MIT [V],
vsdbg proprietary, "only ... with Visual Studio Code, Visual Studio or
Visual Studio for Mac" [V: quoted in dotnet/core issue 4788; no Microsoft
reply recorded]. Cherenkov runs the debugger as a separate process, so none
of the copyleft or EPL terms attach to Cherenkov's code [I]; vsdbg is
excluded regardless.

Platforms: GDB on Apple Silicon is not supported [M], so macOS means
lldb-dap, which has no goto [V]; tier B then rests on the repl's `thread
jump`. Tier P needs an ARM64 table: `isa.rs` has only `Isa::amd64()` [V],
and ARM64 affects C++, Rust and Go equally. netcoredbg lists ARM64, x64, x86
on Linux; macOS arm64 is "community supported" [V].

### 3.3 The managed-runtime tier

#### 3.3.1 Java: proven

Probe (`JDI`, JDK 21, this machine) [V]:

| Capability | Value |
|---|---|
| `canRedefineClasses` | true |
| `canForceEarlyReturn` | true |
| `canPopFrames` | true |
| `canGetBytecodes`, `canGetMethodReturnValues` | true |
| `canAddMethod`, `canUnrestrictedlyRedefineClasses` | false (deprecated since 15; never true on a JVM TI back end [V spec]) |

End to end: a class `Target` with `static boolean lt(int a, int b) { return
a < b; }` printed `lt(3,3)=true` after Cherenkov-style handling: launch the
JVM suspended, catch the `ClassPrepare` of `Target`, patch the one
`if_icmpge` (0xa2) after `iload_0 iload_1` to `if_icmpgt` (0xa3) in the
class bytes read from the build output, and call `RedefineClasses`. The
original prints `false`. Redefinition took 19.7 ms for this class. Nothing
was written to disk. This is the managed analogue of tier P.

How the mutants of each family map to bytecode [I unless noted]:

- **Same-size opcode swaps**: `ror` on ints (`if_icmpXX`, `ifXX` after
  `lcmp`), `aor`, `bitwise`, `shift` (`ishl`/`ishr`/`iushr`), `unary`
  (`ineg` -> `nop`), `negate-condition` (swap `ifeq`/`ifne`).
  javac emits the **negated** branch to the else-block (`if (a < b)` ->
  `if_icmpge else`), so the polarity derivation of tier P applies as is.
  Booleans as values are also emitted as a branch (`iconst_1`/`iconst_0`
  around a `goto`), unlike Rust's `setcc`.
- **`lcr`**: `&&` -> `||` retargets the first operand's branch to the
  then-block and flips its sense; same size.
- **`match-arm`**: retarget a `tableswitch`/`lookupswitch` entry to the
  default label; same size. Pattern switches (Java 21) compile to
  `invokedynamic` `typeSwitch`: not supported.
- **`delete-statement`**: `nop` over the statement's line range from the
  `LineNumberTable`, valid for stack-neutral statements.
- **`body-default`, `fn-value`**: either a new `Code` attribute
  (`iconst_0; ireturn`, `aconst_null; areturn`; no `StackMapTable` is needed
  since there is no branch target [I]), or no class rewrite at all: a
  breakpoint at the method's first bytecode and `ForceEarlyReturn` with the
  value (`VirtualMachine.CreateString` for strings, a static
  `InvokeMethod` for `List.of()` and `Optional.empty()`) [M]. The second
  costs a stop per call; `finally` blocks do not run in either, as the
  source mutant also skips the original body [V spec for the first].
- **Not same size** (UOI, SVR, constant insertion): need a class-file
  writer with stack map frames; out of scope for the canonical set.
- **Locating the site**: javac's line table is per line, with no column.
  Several comparisons on one line are told apart by order of the opcodes,
  as the native tier P does by instruction order [I].
- **Type distinctions in one opcode family**: float and double compares
  are `fcmpl`/`fcmpg`/`dcmpl`/`dcmpg` plus a branch; a swap that changes
  NaN behaviour changes both [M].

Limits [V spec]: RedefineClasses cannot add or delete methods or fields,
change modifiers or hierarchy (`SCHEMA_CHANGE_NOT_IMPLEMENTED`,
`ADD_METHOD_NOT_IMPLEMENTED`..); bodies only. Active frames of the old
method keep running the old code (obsolete methods), so the mutant must be
applied before the code runs, which `ClassPrepare` allows for test targets;
breakpoints in the class are cleared. HotSpot deoptimises compiled code
that depends on a redefined method [M], so JIT inlining of the old body is
not a correctness problem; cost grows with loaded classes [M].

Tier B on the JVM keeps `EvalReturn`/`FORCE_RETURN` (via `ForceEarlyReturn`)
but loses every jump: `EvalThenJump`, `JumpOnly`, `EvalBranch` have no
JDWP mechanism. **Recommendation: Java realises every canonical mutation
through the managed tier, from the mutant's `Edit` and `operands`**, as tier
P does today (`effect_of`, `derive`); the `Action` is a tier B recipe the
managed tier ignores.

A direct JDWP client in Rust (about 15 commands: `IDSizes`,
`ClassesBySignature`, `Method.Bytecodes`, `Method.LineTable`,
`EventRequest.Set` for `CLASS_PREPARE`/`BREAKPOINT`, `RedefineClasses`,
`ForceEarlyReturn`, `StackFrame.GetValues`, `ThreadReference.Frames`,
`Resume`) avoids java-debug, which is JDI-based with an Eclipse JDT
evaluator [V README] and whose README lists no hot-code-replace or goto [V].
The JVM must start with `-agentlib:jdwp=transport=dt_socket,server=y,
suspend=y` [M]: Cherenkov must launch the test JVM itself (ConsoleLauncher
and a printed classpath), not through Gradle's or Surefire's forked workers.

"Which test reached this method" (recording): method-entry breakpoints are
a stop per hit, with no condition at the protocol level, so a hot method is
slow; the existing trick of a breakpoint per distinct (entry, test) that
disarms after the first hit applies, with `Class.method` of the JUnit
frame as the test identity. JUnit 5 parallel execution must be off.

#### 3.3.2 C#: probable, unverified

- ICorDebug has `SetIP` on its frame interfaces and `CanSetIP`; jumps are
  unreliable in optimised code [V, Microsoft blog and docs found by
  search]. netcoredbg's README does not list goto, hot reload, or
  `setExpression` [V: README silent]. **Run `initialize` against
  netcoredbg before planning**: that single response decides whether
  tier B exists for C#.
- IL redefinition options [M], none verified here:
  1. `ICorDebugModule2::ApplyChanges` (Edit and Continue, as .NET Hot
     Reload), which takes metadata, IL and PDB deltas, normally made by
     Roslyn's `EmitDifference`; a hand-built IL-only delta is possible but
     laborious [I].
  2. A profiler (`CORECLR_ENABLE_PROFILING`, `CORECLR_PROFILER`,
     `CORECLR_PROFILER_PATH`) calling `ICorProfilerInfo::SetILFunctionBody`
     at `JITCompilationStarted`, with ReJIT to re-apply: a native library
     is not a debugger, but is "in memory" and writes no source [I].
  3. A scratch copy of the assembly with IL patched, loaded instead.
     Cheapest. Changes the invariant "no adapter writes files" to the
     already-discussed "nothing writes source; only build output and
     scratch copies" (docs/rust-operator-evaluation.md, last section).
- IL opcodes: branch-form compares have one opcode (`blt`, `ble`, `bge`,
  `bgt`, `beq`, `bne.un`, `.un` for floats and unsigned); value-form
  compares are `clt`/`cgt`/`ceq`, with no `cle`: `a <= b` is `cgt;
  ldc.i4.0; ceq`, **two instructions longer**. A `<` -> `<=` value-form
  mutant changes the body length, so IL rewriting needs branch-offset
  fix-ups, a larger job than Java's. `add` vs `add.ovf` (checked
  contexts) are separate opcodes [M].
- JIT: tiered compilation compiles a method at tier 0, counts calls, and
  re-JITs at tier 1 in the background; R2R images (the framework) ignore a
  patched IL until rejected. Set `TieredCompilation=0`, `ReadyToRun=0`
  (`DOTNET_TieredCompilation`, `DOTNET_ReadyToRun`) for the test run [M].
  Debug builds are `DisableOptimizations` JIT-compiled [M].
- Test host: `dotnet test` runs tests in a `testhost` child process;
  the debugger must attach to it (`VSTEST_HOST_DEBUG=1`, attach by pid)
  [M]. Alternatives: xUnit v3 tests are executables; `dotnet vstest`
  with a custom runsettings [I].
- `async` methods: the body is in a compiler-generated `MoveNext`, as
  Rust's `{async_fn#0}` poll. `ReturnDefault` for an async method must
  return a completed task from the kick-off method (needs a
  `Task.FromResult<T>` method spec: a metadata addition) [I]. The profile
  can already mark it (`Capture::Async`, `ReturnDefault.async_fn`).

#### 3.3.3 Python: the interpreter tier

There is no machine code and no JVM-style bytecode patching worth doing; the
natural mutant is **the mutated source compiled in memory and swapped into
the running function** (`func.__code__ = compiled.__code__`), derived from
the file's text and the plan's `site`/`mutated`, written nowhere [I]. It
realises all families uniformly, including `fn-value` and `match-arm`, and
needs no debugger, only the ability to run code in the process: an
in-process pytest plugin loaded with `-p` (no file written), or debugpy's
`evaluate` in the repl context, which can run statements [M]. Constraints:
free variables must match (compile inside a wrapper that defines them),
functions found by file and first line through `gc`/`inspect`, decorated
and bound methods by their `__wrapped__`/`__func__`.

debugpy as a tier B host: `gotoTargets` and `setExpression` are advertised
[V], and `Tier::A` ("pure DAP: evaluate plus goto" [V in `execution.rs`])
fits (nothing produces `A` today and there is no debugpy driver or pytest plugin: D15, D16). goto sets `f_lineno`, with CPython's limits on jumping into/out of
blocks [M]; a tracing debugger slows CPython much more than on 3.12+ where
pydevd uses `sys.monitoring` [M]. Cost per run is the unknown (section 5).

#### 3.3.4 Compare: tier B vs managed tier, on the JVM

| | Tier B (stop, evaluate, jump) | Managed tier (redefine) |
|---|---|---|
| Java feasible | no jump in JDWP [V] | yes [V] |
| Cost per mutant | a stop and several requests per hit; the process must run | one `RedefineClasses` (20 ms [V] for a tiny class) then a normal run, no stops |
| Fidelity | evaluator semantics (GDB's arithmetic is not Rust's [V repo]); the Java evaluator is JDT, with its own cost | the JVM executes the mutant's bytecode: exact opcode semantics |
| Restricted to | any evaluable expression | same-size body edits unless a class writer exists |
| Hot code | every hit stops | none |

### 3.4 C++ and the native path

Tier P and tier B are built on DWARF, an x86 table and GDB; the GDB driver
needs no language-specific piece, so C++ mostly reuses it. Observed with
g++ 14 at -O0 [V]:

- `bool f(int a, int b) { return a < b; }` is `cmp; setl %al`, the value
  form the Rust notes describe; patchable by condition code.
- `a < b` on a class with `operator<` is `call V::operator<(V const&)`;
  on `std::string` it is `call std::operator< <char,..>`. No compare
  instruction: see 2.2.
- A template function used with three types has **three locations** for one
  source line (`break t.cpp:3` -> "3 locations"), so the `generic-copies`
  gap (the biggest runnable gap found in the bus work) is the normal case in
  C++, not an edge. `locate_all` already collects every copy; header-only
  code is one COMDAT copy per instantiation.
- GoogleTest 1.16 headers are installed [V], so a test fixture exists on
  this machine.

C++ fidelity risks: `constexpr` and templates evaluated at compile time
leave no runtime code to patch (the mutant survives); `inline`/
`always_inline` functions at -O0 are still emitted but may be folded; the
build must be -O0 (CMake Debug) so no function is inlined; signed overflow
is undefined behaviour and does not trap; exceptions in a condition unwind
past the patch point; destructors run on a stub return (the `ReturnDefault`
stub must preserve what the epilogue expects, as the Rust stub does with
the sret pointer). Whether a stub for a non-trivial return type can
construct the value is the same question the support library answers for
Rust (docs/support-library.md): load a library of constructors built by
the project's own compiler.

## 4. Required contract changes

### 4.1 Profile layer (the `.scm`, `tokens.toml`, `profile.toml`, and the core they feed)

| # | Change | Needed by | Breaks existing profiles? |
|---|---|---|---|
| 1 | **`ReturnValues` is a Rust `fn` pointer.** Make it data in `profile.toml`: a list of `{ type pattern, value, runtime }`. Without it each language needs Rust code in the core build for `fn-value` (Go and Python have none today). | all except Rust | No: keep the pointer as an alternative |
| 2 | **`Requires::CALL_IN_EVAL` is syntactic** (`calls_in` counts `Concept::Call` matches inside the evaluated span [V, `plan.rs:880`]). An overloaded operator or a property getter is not a `Call` match, so the flag is a lower bound in C++, C#, Python and Rust traits. Nothing outside the planner reads `Requires` [V: grep]. Decision: document `Requires` as a lower bound and let the adapter detect the call from debug info (2.2); add a profile field only if tier B evaluation of an overloaded text proves unsafe. | C++, C#, Python | No |
| 3 | `Token` has no `>>>` (`Ushr`) or `>>>=`. Add only if a Java/C#/JavaScript set wants it (PIT `MATH` does [V]). JavaScript's `??` (StrykerJS `??` -> `&&` [V]) is the same kind of token. C++ alternative tokens (`and`, `bitand`) need an `aliases` list in `tokens.toml`. | Java, C#; C++ | No (additive) |
| 4 | `body_braces` and `body_template` are one shape per language. C# expression-bodied members, Java/C# lambdas and JavaScript/TypeScript arrow expression bodies have another. Only the displayed `mutated` text uses it; allow `mutated: None` for such bodies (the type already allows it). | C#, Java | No |
| 5 | `function` is the innermost name only. Overloads (Java, C#, C++) and same-named methods in two classes collide in `MutantPlan.function` and in the id's `function` part; the id's `occurrence` index separates them but the report is ambiguous. Capture a qualified name (`@name` plus an enclosing class capture) in the profile. | Java, C#, C++ | No (ids change only for those languages) |
| 6 | **A second equality spelling.** `TokenTable` maps one spelling per token and rejects duplicates, so `===`/`!==` and `==`/`!=` cannot both be `Eq`/`Ne`: a swap would render the wrong strictness. Add `StrictEq`/`StrictNe` tokens (with `ror` rows for them) or a per-token spelling family chosen by the site's original spelling. The only change JavaScript and TypeScript need that the other six did not. | JavaScript, TypeScript | No (additive; Rust, Go, Python keep one spelling) |

No new `Concept`, `Capture`, `Edit` or `Action` variant is needed: all
canonical sites map to the 16 concepts and 21 captures (2.1), and every
edit is a `ReplaceToken`, `DeleteToken`, `WrapNot`, `ReplaceWithLiteral`,
`DeleteStatement` or `DeleteMatch` the languages already share.

**Profile-layer problems that force a core change** (as asked): items 1, 2
and 6 above. Item 2 is the case the question described: `Requires` needs type
information the `.scm` cannot capture.

### 4.2 Adapter and port layer

| # | Change | Needed by | Breaks existing? |
|---|---|---|---|
| A | **A `TestRunner` port**: `list(program) -> Vec<TestId>`, `select(&[TestId]) -> launch arguments`, `parse(output or report file) -> map TestId -> Outcome + Failure{location, message, left, right}`, `identify(frames) -> Option<TestId>` (frame match, a hook, or evaluate-in-frame for JavaScript), and `kind()` naming the parser. libtest becomes one implementation. Fixes Go being parsed as libtest. | all | No; moves `libtest` and the CLI's `explain()` behind it |
| B | **A `BuildPort`** returning `TestProgram` (command, arguments, environment, working directory, debug artefacts). Rust and Go are two implementations; `build_rust_test_programs`/`build_go_test_binary` already have this shape. CMake/CTest, Gradle/Maven classpath, and `dotnet build` are three more. | C++, Java, C# | No |
| C | **A managed session**, beside `NativeSession`: `class_bytes(site)`, `redefine(patch)`, `run`, `run_recording`. And a capability descriptor (`code_memory`, `evaluate`, `jump`, `force_return`, `redefine`) that decides the tier from the plan's needs, replacing "the executor is a GDB executor" as the unit of choice. | Java, C#, Python | No |
| D | **`Tier::M`** (managed) next to `A`, `B`, `P`: `TIER_M` is an additive value of the `Tier` enum in `proto/cherenkov/mutant/v1/mutant.proto` (A=1, B=2, P=3 [V]). The `encodings.txt` tests will change for the wire form; the run application's tier mapping (`workers.rs`) gets one arm. Python's interpreter tier and JavaScript's `setScriptSource` can share `M`: what differs is the representation (bytecode, IL, code object, source text). | Java, C#, Python | Wire: additive; the encodings tests must be updated by the change |
| E | **Value realisation** per backend: `ReturnDefault.value_text` is a source literal; Java/C# need the method descriptor (class file or metadata), C++/Go the DWARF type. All adapter-side, as Rust's layout code is. | all | No |
| F | **Deficiency codes** for the new failure modes: `operator-call` (compiled site is a call), `lifted-compare` (C# nullable), `pattern-switch` (Java), `constexpr-folded` (C++), `class-schema` (JVM redefine refused). Additive. | C++, C#, Java | No |
| G | **Test identity hooks** for Catch2/doctest (3.1); or one-process-per-test. | C++ | No |
| H | **ARM64 ISA table** for tier P (and lldb-dap for macOS). | C++, Rust, Go | No |
| I | **Test targets under a harness**: Java and C# need Cherenkov to start the test process itself with the debug agent flags (JDWP agent; testhost attach). | Java, C# | No |
| J | **Per-script reapplication.** A plan site can be many script instances at run time (Jest and Vitest evaluate modules per test file; the Rust analogue is `generic-copies`). The managed session applies a mutant at `beforeScriptExecution`/class-prepare time for every instance, not once. | JavaScript, TypeScript; Java (class loaders) | No |
| K | **Source-map resolution** (TS site to generated token) in the TypeScript adapter; a `no-breakpoint-site` when ambiguous. | TypeScript | No |

## 5. Recommended order, spikes, and owner decisions

Order: **C++, JavaScript/TypeScript, Java, Python, C#.** Rust and Go are done.
JavaScript moves ahead of Java because it proves the redefinition tier
with text edits, with no class-file writer, and its probe already passes
(section 7.3).

| # | Language | Cheapest informative spike | What would show the contract is wrong |
|---|---|---|---|
| 1 | C++ | Build a GoogleTest project with CMake Debug; run the existing GDB tier P on `ror` and `negate` for one function through a `BuildPort` and `TestRunner` stub. Include one template used with two types and one class with `operator<`. | tier P patches the wrong copy or misses a copy (the `generic-copies` assumption); the value-form `setl` rule breaks; the test cannot be identified on the stack |
| 2 | JavaScript, then TypeScript | The 7.3 probe through a CDP client against a real Jest `--runInBand` and a `node:test` file, applying at `beforeScriptExecution`; then a ts-jest and an esbuild project through source maps. | LiveEdit refuses statement-deleting or body-replacing edits (captured scope); per-script reapplication dominates the cost; test attribution by `currentTestName` fails; source maps cannot place operators |
| 3 | Java | The probe in 3.3.1 turned into an executor: JDWP `RedefineClasses` for `ror` and `lcr`, ConsoleLauncher, `Class.method` identity. Measure per-mutant time with 100 loaded classes. | redefine cost per mutant dominates (then batch mutants of one class in one redefine); javac's line table cannot place two comparisons on one line; the actions (`EvalThenJump`) have no use at all |
| 4 | Python | A pytest plugin that swaps `__code__` for one `ror` mutant, and the same through debugpy `evaluate`. Measure the cost of debugpy tracing with and without 3.13 `sys.monitoring`. | the plan lacks something an in-memory recompile needs (the whole function's text, free variables); the debugger is not needed, and tier A is dead weight |
| 5 | C# | `netcoredbg --interpreter=vscode` `initialize` response, then one IL patch by approach 3 (3.3.2) of a `blt` -> `ble` branch-form compare, and one value-form compare. | IL rewriting with length changes is unavoidable for value-form `<=`; `testhost` cannot be attached to under netcoredbg; no jump |

Decisions that are the owner's:

1. Whether the managed tier is a **debugger-resident** redefinition (JDWP
   `RedefineClasses`, ICorDebug) or a **scratch copy of build output**
   loaded instead, which would relax the invariant "no adapter writes
   files" (as docs/rust-operator-evaluation.md already discusses for
   tier P). The JVM probe worked under the first; .NET is easier under the
   second.
2. A direct JDWP client versus java-debug (EPL-1.0).
3. netcoredbg only (vsdbg is barred); and whether C# waits for its
   `initialize` response.
4. Python: in-process agent versus debugpy; whether "no debugger" is a
   legal adapter (the owner's principle says yes).
5. `ReturnValues` data-driven (4.1, item 1), and `Requires` as a lower
   bound (4.1, item 2).
6. `TIER_M` on the wire (4.2, D).
7. Test frameworks supported first in C++ (GoogleTest: frame match;
   Catch2/doctest: hook or per process).
8. macOS/ARM64: lldb-dap and an ARM64 table before or after Java.
9. The peer-tag under-tagging in 1.2: add tags when the sources are
   cited in docs/operator-research.md.
10. JavaScript/TypeScript: a direct CDP client or vscode-js-debug (7.3);
    `setScriptSource` live edit versus a load-time rewrite hook (7.3, a
    non-debugger option that writes nothing); one process per mutant or
    reapplication in a long-lived runner process; mutate the executed
    JavaScript and plan on TypeScript (7.4, recommended); strict equality
    as new tokens or a spelling family (4.1, item 6); whether Bun and Deno
    are in scope (Bun is not CDP).

## 6. Open questions and unverified claims

- netcoredbg's DAP capabilities (set expression, goto, function
  evaluation, logpoints) are unverified; its README is silent [V].
- lldb-dap's capability list: only the absence of a `GotoTargets` handler
  and the logpoint feature were seen [V]; the rest is [M]. The licences of
  GDB, Delve, LLDB, debugpy are from memory.
- Whether ICorDebug `ApplyChanges`, a profiler `SetILFunctionBody`, or
  netcoredbg supports IL replacement in a running CoreCLR on Linux; and
  the exact behaviour of `testhost` attach: all [M].
- Java: whether a class-file `Code` attribute without `StackMapTable` is
  accepted on a version-61 class for a body with no branch; the cost of
  `RedefineClasses` with a realistic classpath; how often javac puts two
  comparisons on one line [I].
- Python: debugpy cost per hit; `f_lineno` assignment limits; `evaluate`
  running statements [M].
- C++: macro-heavy test code (`TEST` parse shape), constexpr folding, and
  whether libstdc++/libc++ zero-bit rules hold across versions [M].
- Incumbent tools: the mutmut and cosmic-ray operator lists came from a
  secondary comparison page and the mutmut README (`node_mutation.py` was
  not reachable); Gremlins' individual mutator pages were not read. Mull
  0.34.0 (May 2026) is from an aggregator [unverified]. No maintained C++
  alternative to Mull was checked beyond Dextool and universalmutator
  named in docs/operator-research.md.

## 7. JavaScript and TypeScript

Added 2026-11-19. Same frame as the other languages: the 65 named mutations
(the family matrix in 1.2 has the JavaScript and TypeScript columns), then
the profile layer, then the adapter layer.

Machine: Node v22.22.2, tsc present, bun present, deno absent [V]. Probes
ran under `nice -n 19` while the machine was heavily loaded, so the
timings are an upper bound.

### 7.1 Profile layer

Grammars: `tree-sitter-javascript` and `tree-sitter-typescript` (which holds
two grammars, `typescript` and `tsx`) exist and are maintained under the
tree-sitter organisation; node types read from `src/node-types.json` [V].
TypeScript's grammar extends JavaScript's, so the node names for the
canonical sites are the same; the differences that matter are listed below.

| Concept | JavaScript node | TypeScript difference |
|---|---|---|
| binary | `binary_expression` (left, operator, right; `===`, `!==`, `&&`, `\|\|`, `??`, `>>>`, `**`, `in`, `instanceof`) | same |
| unary | `unary_expression` (argument; `!`, `-`, `+`, `~`, `typeof`, `delete`) | same; `x!` is a separate `non_null_expression`, so it is never a negation |
| compound assign | `augmented_assignment_expression` (operator includes `&&=`, `\|\|=`, `??=`, `**=`) | same; left may be `non_null_expression` |
| assign | `assignment_expression`; `variable_declarator` (name, value) | declarator adds `type` |
| literal | `number`, `string`, `true`, `false`, `template_string` | same |
| condition | `if_statement`, `while_statement`, `do_statement` (a `parenthesized_expression`), `for_statement` (condition), `ternary_expression` | same |
| return | `return_statement`; an `arrow_function` whose `body` is an expression | same |
| call | `call_expression` (function, arguments; `optional_chain` field) | adds `type_arguments` |
| index | `subscript_expression` | same |
| function | `function_declaration`, `method_definition`, `function_expression`, `arrow_function`, `generator_function_declaration` (body, parameters; `async` is an anonymous child) | adds `return_type` (a `type_annotation`) and `type_parameters` |
| arm | `switch_case` (value, body), `switch_default` | same |
| guard | none | none |
| field_init | `pair` and `shorthand_property_identifier` inside `object` | same |
| statement | `expression_statement`, `lexical_declaration`, `return_statement` .. | same |

Points for the `.scm`, none needing core:

- **TypeScript can share the JavaScript queries.** The shared part is
  every row above. The TypeScript profile adds `return_type: (_)? @returns`
  to the function patterns (the JavaScript grammar has no such field, so
  the capture stays optional exactly as Rust's `@returns` does) and
  `type_parameters`. Type annotations in expressions are not part of any
  canonical site, so "skipped" is automatic; `as`, `satisfies` and `<T>`
  casts wrap an expression and are traversed. Two profile directories with
  one query body is the simplest layout; `.tsx` needs the `tsx` grammar
  (JSX makes `<` ambiguous with generics) [M].
- **A function's name is not always its own child.** `const f = (a, b) =>
  a < b`, `{ g() {..} }` and `class { m = () => .. }` take their name from
  the parent `variable_declarator`, `pair` or `public_field_definition`;
  a nested pattern captures it. Anonymous callbacks have `function:
  None`, as the Rust closure does.
- **Arrow expression bodies** are return sites by themselves (like Rust
  closures): `(arrow_function body: (_) @value @concept.return (#not-match?
  ...))` for a non-block body. `body_braces` is false for those and true
  for `{ .. }` bodies; the profile's single `body_braces` flag cannot say
  both, so `mutated` is `None` for the arrow-expression `fn-value`, the
  same case as C# expression-bodied members (4.1, item 4). A source-edit
  tier can still apply it, because the value is known (`undefined`, or
  the annotation's default in TypeScript).
- **`switch` has no guard and falls through.** `switch_case` and
  `switch_default` map to `Arm`, `default` as the catch-all. Deleting a
  `case` whose body ends without `break` changes which code falls into the
  next arm; as a source edit this is exactly what the textual mutant does,
  so the fidelity question disappears for a source-redefinition tier and is
  a `Redirect` question for a machine tier (it has none here).
- **`struct-field`** maps to deleting a `pair` or shorthand property of an
  object literal (StrykerJS `ObjectLiteral` does the same [V]). Unlike
  Rust it needs no `..base` marker.
- **Skip regions.** Test files by `exclude` globs (`**/*.test.*`,
  `**/*.spec.*`, `**/__tests__/**`, `**/node_modules/**`). Only Rust has
  `@skip.*` captures today (D37); a `// Stryker disable` comment is the
  nearest JavaScript convention and could use `@skip.annotated` on the
  preceding `comment` node.
- **Generated code.** A `.js` next to a `.ts` of the same name and
  `.d.ts` files must be excluded or the same site is planned twice.
- **JSX/TSX** adds `jsx_expression` positions; a condition inside `{..}`
  is an expression like any other. Not mapped separately.

Token spellings (`tokens.toml`): the shared set matches except
**equality**. JavaScript has two:

| Neutral | Spelling | Why a problem |
|---|---|---|
| `Eq` / `Ne` | `===`, `!==` (strict) and `==`, `!=` (loose) | `TokenTable` maps one spelling per token and rejects duplicates (`DuplicateSpelling`). Whichever one is chosen, a mutation at the other spelling is unmatched, or the swap renders the wrong strictness (`===` to `!=`) |
| `Ushr` | `>>>` (also Java, C#) | no token; PIT `MATH` has it [V] |
| nullish | `??`, `??=` | no token; StrykerJS has `??` -> `&&` [V] |

So JavaScript needs the one profile-layer change the other six did not (4.1,
item 6).

### 7.2 Operator semantics

Where the neutral meaning ("the source with the token swapped") needs a
language rule for JavaScript:

| Topic | Rule |
|---|---|
| `===` vs `==` | `===`/`!==` are the canonical `ror` equality mutation (StrykerJS: `===` <-> `!==` and `==` <-> `!=` [V]). Swapping strictness (`==` to `===`) is a different mutation, not in the 65. A swap keeps the spelling family of the site |
| ordering, mixed types | `<` coerces: strings compare lexicographically, `undefined` and objects go through `ToPrimitive`/`valueOf`, NaN comparisons are false. So `<` -> `>=` is **not** a reversal (`NaN < 1` and `NaN >= 1` are both false). The textual mutant is exact, so a source-redefinition tier needs no rule; a machine tier would |
| `+` | string concatenation or number addition by runtime type. `+` -> `-` on strings gives NaN: viable, no "unviable" class exists as in Python |
| numbers | doubles: no overflow, division by zero gives `Infinity` or NaN (no exception, so such mutants survive more often); `\|0`, `<<`, `>>` coerce to int32, `>>>` to uint32; BigInt mixes throw `TypeError` |
| `&&`, `\|\|`, `??` | return an operand, not a boolean. `lcr` as a value changes which operand is produced; `cor` (`a && b` -> `false`, `a`, `b`, `a == b`) is fine textually. `??` is an extra operator (null/undefined only) |
| truthiness | any value is a condition; `negate-condition` is `!(x)` (coerces); condition -> `true`/`false` literal is valid. `return-bool-flip` applies to `return true`/`false` only |
| `!` deletion | `!x` -> `x` in a value position yields the operand, not a boolean (textually exact) |
| optional chaining | `a?.b < c` is a binary whose left operand short-circuits to `undefined`; no canonical mutation touches `?.` itself |
| getters, Proxy, `valueOf` | an operator may call user code. A source-level mutant keeps whatever the source does; `Requires::CALL_IN_EVAL` is a lower bound as in 4.1 item 2 |
| async | an `async` function returns a promise whatever its body returns: a body replaced by `return 0;` stays correct, so the Rust `Poll::Ready` problem does not arise for a source edit |

Value policy for `ReturnDefault`: JavaScript has no return types, so the
default is `undefined` (equivalent to StrykerJS `BlockStatement`, which
empties the body [V] and to PIT `NULL_RETURNS` in spirit); an async
function resolves to `undefined`. TypeScript knows the annotation: `boolean`
`false`, `number` `0`, `string` `""`, `T[]` `[]`, `Promise<T>` the default of
`T` (the `async` wrapper makes it a promise), `void`/`undefined` unit,
object types `{}` where valid, union with `null`/`undefined` `undefined`.
Annotations are erased and not enforced, so there is no unviable-by-type
case at runtime, but `tsc` strictness does not matter: the test run does not
type-check unless ts-jest does (7.4).

### 7.3 Adapter layer: execution

V8 is driven through the Chrome DevTools Protocol (CDP); `node
--inspect-brk=0` prints a `ws://` URL, Node 22 has a global `WebSocket` [V
used in the probe]. Deno exposes the same V8 inspector (`deno test
--inspect-brk`) [M]. Bun speaks the WebKit inspector protocol of
JavaScriptCore, not CDP [M]; the approach here does not apply to Bun.

Primitives (CDP `Debugger` domain) [M unless noted]:

| Primitive | CDP | Note |
|---|---|---|
| breakpoint, conditional | `setBreakpointByUrl` / `setBreakpointByUrlRegex` with `condition`; `setBreakpoint` by script id | by generated URL and position |
| instrumentation breakpoint | `setInstrumentationBreakpoint('beforeScriptExecution')` | pauses before each newly parsed script: where a mutant can be applied before any of its code runs |
| evaluate with calls | `evaluateOnCallFrame` | side effects allowed unless `throwOnSideEffect` |
| set variable | `setVariableValue` | locals and closure scopes |
| return value | `setReturnValue` (at a return position) | no jump exists |
| restart frame | `restartFrame` | re-executes a frame; not a goto |
| exceptions | `setPauseOnExceptions` | a crashing mutant stops instead of exiting, like Delve |
| **live edit** | **`setScriptSource`** | replaces a script's source in the running isolate; no file is written |
| stack | `Debugger.paused.callFrames`; `setAsyncCallStackDepth` | async parents only along await chains |
| code memory, jump | none | no tier P, no goto: tier B cannot jump, as on the JVM |

**setScriptSource probe** [V, Node v22.22.2, `Debugger.setScriptSource` from
an external CDP client over WebSocket; the script paused at a `debugger;`
statement, the source was fetched with `getScriptSource`, one operator was
changed in the text, and execution resumed]:

| Case | Edit | Result |
|---|---|---|
| plain function, CommonJS | `return a < b;` -> `a <= b` | `{"status":"Ok"}`; `lt(3,3)` printed `true` (unmutated: `false`) |
| plain function, ES module (`.mjs`) | same | `Ok`; `true` |
| closure created before the edit | `() => n < 3` -> `n <= 3` | `Ok`; the existing closure returned `true` |
| arrow with expression body | `(a, b) => a < b` -> `<=` | `Ok`; `true` |
| async function, not yet called | `await null; return a < b` | `Ok`; the later call resolved `true` |
| getter | `get g() { return 1 < 1; }` | `Ok`; `true` |
| top-level code of the paused frame | edit a statement of the script that is running | `BlockedByActiveFunction` |
| async function **suspended at an await** when the edit is applied | `a < b` -> `a <= b` | `BlockedByActiveGenerator`; the suspended call kept the old code; a fresh call afterwards was not tested (the edit was refused) |
| 20 successive edits of one function, after 2000 warm-up calls | toggle `<` and `<=` | all `Ok`; min 20.7 ms, median 56.1 ms, max 165.5 ms (machine loaded) |

Reading the probe:

- The edit is **text to text**, the mutant is the plan's `site`/`mutated`
  applied to the script source. That is the original design's tier C
  (`function_text`); the plan does not carry it any more (D12) but
  `file` + `site` + `mutated` rebuild it [I]. It is exact for every
  canonical family because V8 compiles what the source says; no operand
  type rule is needed.
- **Limits** [V cases above, rest M]: a function with an active frame
  cannot be edited (the top frame can in some versions; Node 22 refused the
  top-level script frame); suspended generators and async functions are
  refused; changes that alter a function's captured-variable layout (a
  statement deletion removing the last use of a captured variable; a
  `body-default` dropping references) may be refused by LiveEdit as scope
  changes [I, not probed]; top-level `const` initialisers that already ran
  stay as they ran.
- **When to apply.** Apply the mutant **before** its code runs and before
  any activation exists: either at the first pause (as in the probe) or,
  for runners that load source files per test file, at
  `beforeScriptExecution` for every script whose URL matches. Jest, Vitest
  and `node:test` re-evaluate modules per test file in new contexts (Jest:
  a `vm` context per file with its own module registry; Vitest: per-file
  module graph) [M], so a mutant is applied to **each** script instance:
  a script-copies analogue of `generic-copies`. A per-process mutant (new
  process per mutant, applied at start) avoids reapplication at the cost of
  Node and runner start-up (hundreds of milliseconds to seconds for Jest
  [M]); reuse of one process across mutants needs reapplication and works
  with Vitest's in-process rerun or Mocha's `--watch`-style APIs [I].

**Driver: a direct CDP client or vscode-js-debug.**

| | Direct CDP client | vscode-js-debug (DAP) |
|---|---|---|
| Surface needed | about ten commands; WebSocket plus JSON (a Rust `tungstenite` client) [I] | a DAP session; `setScriptSource` is not a DAP request, so a mutation would use its custom or evaluate path [M] |
| Licence | the protocol is open; Node is MIT [M] | MIT [M, README silent]; used standalone by nvim-dap [V] |
| Features | everything above | return-value interception, instrumentation breakpoints, source-map handling, child processes and workers [V README]; hot reload, set variable, restart frame not listed there [V README silent] |
| Source maps | to write or add (`sourcemap` crate [M]) | built in, but its position mapping is for the user's breakpoints, not for token-level edits |
| Verdict | **simplest and sufficient**: the contract's primitives are session lifecycle, breakpoints, `setScriptSource`, evaluate, and a way to learn the exit | useful only if the owner wants Chrome and Node behind one DAP surface |

**Costs and fidelity, by family** (S = setScriptSource, B = CDP tier B,
H = an in-memory source-rewrite hook loaded with `--require`/`module.register`
before the program, which writes nothing but is not a debugger):

| Family | B (evaluate and set) | S (live edit) | H (load-time rewrite) |
|---|---|---|---|
| `ror`, `aor`, `bitwise`, `shift`, `unary` | evaluate the mutated expression at a breakpoint and `setVariableValue`: only where the value is assigned to a local; no jump to skip the original | exact; one edit per mutant (about 20 to 60 ms [V loaded]) | exact; cost at module load, no debugger |
| `lcr`, `cor` | short-circuit cannot be emulated by evaluating (side effects, operand values) | exact | exact |
| `negate-condition`, bool flip | `setReturnValue` for returns only; conditions need a jump | exact | exact |
| `delete-statement` | not possible without a jump | exact (`;` / `void 0;`), may be refused if it changes captured scope [I] | exact |
| `body-default`, `fn-value` | breakpoint at entry, `setReturnValue` needs a return position; practical form: `restartFrame` is not enough | exact; arrow expression bodies work | exact |
| `match-arm` | no | exact textual deletion | exact |
| `struct-field` | no | exact | exact |
| fidelity | evaluator reads the frame, calls run with side effects | by construction the same as a source mutant | by construction the same |
| cost per mutant | a stop per hit | one edit then a normal run; reapply per script instance | a process start per mutant, or reapply |

Tier B on JavaScript is therefore almost vestigial; **S is the managed
tier's source-level form.** One tier value (`TIER_M` in 4.2) can cover
bytecode (JVM), IL (.NET), a code object (Python) and source (V8), because
what the plan carries (`Edit`, `site`, `operands`, `mutated`) is enough for
each and the representation is the adapter's.

### 7.4 TypeScript: where mutation happens

Tests run on transpiled JavaScript: tsc, `ts-node`, `tsx` (esbuild), `swc`,
Babel, `ts-jest`, Vitest's esbuild transform, Jest's `babel-jest`; the
debugger sees generated JavaScript and, when the transformer emits them
(inline or `sourceMappingURL`), source maps; CDP gives `sourceMapURL` in
`scriptParsed` [M].

| | Mutate the JS the runtime executes | Mutate the TS and transpile in memory |
|---|---|---|
| How | plan on the `.ts` (profile on the source), map the site to the generated script through the source map, find the operator token in the generated function text, edit, `setScriptSource` | apply the edit to the TS text, transpile with the project's own transformer, then `setScriptSource` |
| Fidelity | the code that actually runs, whatever the transformer did (ts-jest, babel plugins, istanbul instrumentation if coverage is on) | the project's transformer configuration must be reproduced (decorators, `paths`, JSX, target): differs per runner |
| Profile layer | one grammar for planning (TypeScript), one for locating the operator in generated JS (JavaScript); no new concept | one grammar |
| `mutated` text | TS text of the site, for the report; the generated text is the adapter's | TS text |
| Ids | unchanged: path, function, operator, anchor are TS (id.rs inputs) | unchanged |
| `--in-diff` | unchanged: diff lines are TS lines and `site` is a TS span | unchanged |
| Risk | source maps map identifiers and statement starts, often not operators (esbuild maps few tokens [M]); the adapter must locate the nth operator of the right kind in the generated range and refuse when it is ambiguous (`no-breakpoint-site`) | wrong transformer, slow per mutant, type errors |

**Recommendation: mutate the executed JavaScript, plan on TypeScript.**
Everything that depends on identity (ids, `--in-diff`, reports, skip
regions) stays in TS positions; the adapter turns a TS `site` into a
generated-script edit. When no source map is present (a transformer that
strips types without maps) the adapter reports `no-breakpoint-site`.
In-memory transpile with esbuild is a fallback for Vitest and `tsx` setups
whose transformer is esbuild anyway [I]. Plain `.js` projects skip the map
step.

Bun and Deno, one line each: Bun's inspector is WebKit's, not CDP, so the
setScriptSource tier does not apply [M]; Deno exposes the V8 inspector with
`--inspect-brk` and its test runner is `deno test` [M].

### 7.5 Test runners

| Runner | List | Select | Result | Test on the stack at a function's entry |
|---|---|---|---|---|
| Jest | `jest --listTests` lists files only; test names come from a run: `--json` gives `assertionResults[].fullName` [M] | `-t <name pattern>`, file paths; `--runInBand` keeps one process and lets `node --inspect-brk node_modules/jest/bin/jest.js --runInBand` be debugged [M] | `--json --outputFile`; JUnit via `jest-junit` (a dependency) | not on the stack: tests are async callbacks run by `jest-circus`. In the frame, `expect.getState().currentTestName` names it [M] |
| Vitest | `vitest list --json` [M] | `-t`, file filter; debug with `--inspect-brk` and a single worker (`--no-file-parallelism`, a single fork) [M] | `--reporter=json` / `junit` [M] | `expect.getState().currentTestName` [M] |
| Mocha | `--dry-run` with a reporter (`json`, `xunit`) [M] | `--grep`, `--fgrep`; runs in-process, debug with `--inspect-brk` [M] | `--reporter json` / `xunit` | the running test is `this.test` in the hooks; not a global [M]; use a root hook installed by a `--require` file |
| `node:test` | none apart from a run [M] | `--test-name-pattern`; each file runs in a child process by default, so `--inspect-brk` needs `--experimental-test-isolation=none` (Node 22.8+) or the child's port [M] | `--test-reporter=junit`, `tap`, `spec`, `dot` [M] | no global current-test accessor; an `AsyncLocalStorage` hook or one test per run |
| AVA | `--list-files`-style listing [M, unverified] | `--match` | `--tap`; JUnit via a reporter | each test file runs in a worker thread/process [M] |
| Playwright Test | `playwright test --list` [M] | `-g`, file filter | `--reporter=json` / `junit` | tests run in worker processes driving browsers; the code under test usually runs in the browser (Chrome's CDP, same protocol, different target) |

Test attribution: stack-frame matching, which libtest, Go, JUnit, pytest all
use, **does not work for JavaScript tests**: the callback is invoked by the
runner after promise ticks, and the synchronous stack at a function's entry
has runner frames, not the test's name. Options, in order of cost:

1. **Evaluate at entry** [new strategy for the `TestRunner` port]: at each
   function-entry breakpoint, `evaluateOnCallFrame` an expression that names
   the current test: `expect.getState().currentTestName` (Jest, Vitest; Jest
   needs it evaluated in the test context) [M]. One request per distinct
   (entry, test) with the disarm-after-first-hit trick.
2. **A hook** the adapter evaluates once per test start: a runner event
   (jest-circus `test_start`, Mocha `test` event) feeding a global the
   breakpoint reads. This needs a setup file, which writes a scratch file or
   injects through `--require`, relaxing nothing about source.
3. **Async stack traces**: `Debugger.setAsyncCallStackDepth` gives async
   parents along await chains [V: the API exists in CDP; usefulness not
   probed]; the runner's frames still carry no test name.
4. **One test per process**: exact, costs a Node plus runner start-up per
   test (Jest: about a second [M]).

Jest/Vitest transform caches matter: a cached transform hides nothing from
`setScriptSource` (the executed script is what is edited), but the file
the runtime reports (`url`) is the transformed path or a `file://` URL
that must be mapped back to the source file [I].

### 7.6 Incumbent tool: StrykerJS

Cherenkov's per-mutation tags already name Stryker; not rebuilt. Spot
checks against the StrykerJS mutator page [V]:

| Check | Result |
|---|---|
| `ror/lt->le`, `ror/eq->ne` ("same as Stryker") | right: StrykerJS has `<` -> `<=` and `===` <-> `!==`, `==` <-> `!=`; `ror` rows spelled `===` need the 7.2 token change to match |
| `aor` | StrykerJS: `+` <-> `-`, `*` <-> `/`, `%` -> `*`; ours `%` -> `/` |
| `lcr` | StrykerJS: `&&` <-> `\|\|`, `??` -> `&&`; the `??` row is missing here |
| `unary/delete-not` | StrykerJS has `!(a == b)` -> `a == b` (narrower) and `+a` <-> `-a` (ours delete the sign) |
| `bitwise`, `shift` | **none in StrykerJS** (the listing has no bitwise or shift group); ours exist |

Gaps in our catalogue that StrykerJS has (one line each, not planned):
`StringLiteral` (non-empty to `""`, `""` to `"Stryker was here!"`),
`ArrayDeclaration` (drop items), `ObjectLiteral` (drop properties; ours is
`struct-field`), `OptionalChaining` (`?.` -> `.`), `Regex` (anchors, classes,
quantifiers), `MethodExpression` (`startsWith`/`endsWith`, `trim`,
`toUpperCase`/`toLowerCase`, `some`/`every`, removals of `sort`, `filter`..),
`Update` (`++`/`--`), `BlockStatement` (ours: `body-default` and
`delete-statement`), `ConditionalExpression` (ours: `negate-condition`).
StrykerJS instruments a sandbox copy of the sources once and switches the
active mutant at run time (a mutant schema), which writes files and is the
approach Cherenkov avoids.

### 7.7 Readiness and contract changes

| | Profile | Adapter | Cheapest spike | What would show the contract is wrong |
|---|---|---|---|---|
| JavaScript | small | medium: a CDP client, a `TestRunner` per runner, per-script reapplication | the 7.3 probe against a real Jest `--runInBand` and a `node:test` file: set `setInstrumentationBreakpoint('beforeScriptExecution')`, edit the module under test, read the runner's JSON, then repeat for 50 mutants and measure | the same function loaded in many scripts (Jest contexts) makes per-mutant cost dominated by reapplication; LiveEdit refuses scope-changing edits too often; test attribution via `currentTestName` is unreliable |
| TypeScript | small | medium-large | the same, on a ts-jest and an esbuild project: map a TS operator to its generated token through the source map for 20 sites | source maps lack the operator token (ambiguous sites); the transformer differs between test and build |

Changes the other six languages did not need (the answer to "does JS need
anything the others didn't"):

1. **A second equality spelling** (4.1, item 6): `Token::Eq`/`Ne` have one
   spelling; add strict variants (`StrictEq`, `StrictNe`, with the `ror`
   rows `strict_eq -> strict_ne`) or a per-token spelling family chosen by
   the site's original spelling. Additive.
2. **A source-level managed tier**: the same `TIER_M` as the JVM and .NET, but
   its payload is source text, so the plan needs nothing new; the adapter
   reconstructs the script from `file`, `site` and `mutated`.
3. **`TestRunner.identify` gains an evaluate-in-frame strategy** (4.2, item
   A): asynchronous tests have no useful stack.
4. **Per-script reapplication** (4.2, item J): one plan site maps to many
   script instances (Jest/Vitest module registries per test file).
5. **Source-map resolution** for TypeScript (adapter only).
6. Nothing in `Concept`, `Capture`, `Edit`, `Action` or `Requires`.

Placement in the order: **after C++ and before Java** (new order: C++,
JavaScript/TypeScript, Java, Python, C#). It proves the redefinition tier
with text edits and a protocol that has no class-file writer, which the JVM
and .NET tiers need; its probe already passes.
- JavaScript: `setScriptSource` was probed on Node v22.22.2 only, with
  plain scripts and ES modules, not under a runner; Jest's module registry,
  `vm` contexts and transform cache are [M]. Whether LiveEdit refuses
  scope-changing edits (statement deletion removing the last use of a
  captured variable) is [I], unprobed. Behaviour of an edit on an
  already-optimised hot function was exercised (2000 warm-up calls, 20
  edits, all `Ok`) but the effect of the edit on the running optimised code
  was not asserted separately.
- TypeScript: whether source maps from tsc, esbuild, swc and Babel place
  operator tokens [M]; esbuild's few mappings are from memory.
- CDP method behaviour other than `setScriptSource` (`setReturnValue`,
  `restartFrame`, `setInstrumentationBreakpoint`, `setVariableValue`) is
  from memory; the CDP reference page was not retrievable this session.
- vscode-js-debug's licence is MIT from memory (its README is silent); the
  test-runner flags in 7.5 are from memory.
- StrykerJS: mutator names and transformations are from its published
  support tables [V]; which are enabled by default was not stated there.
