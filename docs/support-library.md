# Support library: compiled constructors for mutant values

## Problem
A `fn-value` mutant replaces a function's body with a value:
`Ok(Default::default())`, `HashSet::from_iter([1])`,
`Box::leak(Box::new(Arc::new(..)))`. Cherenkov never compiles the mutated
source, so the value must reach the program some other way. Today tier B
builds it byte by byte from the type's DWARF layout and calls the program's
own `Default` where one is linked. That falls short in three ways:

- **No code for the value.** Generic constructors the program never
  instantiated (`HashSet::<u32>::from_iter`), and `Default`s inlined away.
  On angzarr: about 120 refusals, mostly std types (`HashSet`, `PathBuf`,
  `Box`, `Arc`, `RwLock`, `HashMap`) plus some dependency types
  (`chrono::DateTime`, `opentelemetry::KeyValue`).
- **Hand-built heap values.** Reproducing a `HashMap`'s or `Vec`'s internals
  from layouts is fragile. Some built values crash the program (SIGSEGV),
  which likely counts mutants as killed that the real mutant would not
  crash.
- **Complexity.** The byte builder grows with every std type it learns.

## Decision
Values come from **compiled constructors**: a support library that
Cherenkov generates outside the project, builds once per run, and loads
into the test process through the debugger. No code is written into the
project, its scratch copies, or its build.

Constructors, not instances: a returned value is owned and dropped by its
caller, so each mutated call needs a fresh value. A shared or shallowly
copied instance would be dropped once per call (double free).

## Mechanism
1. **Collect.** For each `fn-value` plan whose value is not built by the
   program's own code: the result type (from DWARF, as today) and the
   value's source text (from the plan).
2. **Generate.** A cdylib crate in Cherenkov's scratch directory, one export
   per (type, value):

   ```rust
   #[no_mangle]
   pub unsafe extern "C" fn cherenkov_v7(out: *mut std::collections::HashSet<u32>) {
       unsafe { out.write(HashSet::from_iter([1])) }
   }
   ```

   The value text compiles against the declared type, so inference picks
   the constructor as rustc would in the mutated source. A value that does
   not compile is reported **unviable**, exactly as cargo-mutants would.
3. **Name the type.** DWARF gives defining paths
   (`std::collections::hash::set::HashSet`), which run through private
   modules. Std types map to their public re-exports by a table
   (`alloc::vec::Vec` -> `Vec`, `std::collections::hash::set::HashSet` ->
   `std::collections::HashSet`, ...). A type outside the table is refused.
4. **Build** with the project's toolchain (`rustup` override, as for test
   programs). Same compiler and std, so std types have the program's
   layout.
5. **Load.** GDB calls `dlopen` on the library in the test process (glibc
   provides `dlopen` itself) after the program reaches `main`, then
   `dlsym`s each constructor by name.
6. **Apply.** At the mutant's stop, tier B calls the constructor with the
   result slot (or a scratch slot whose bytes it then returns in
   registers). For a private project type composed from its layout, each
   std field is filled by calling its constructor into that field's slot.

## Limits
- **Allocator.** The library allocates, the program frees. Both must use
  the system allocator, which is Rust's default. A program with a custom
  `#[global_allocator]` (detectable: `__rg_alloc` is defined) has its
  library-built values refused.
- **Private project types** cannot be named by separately compiled code:
  Rust's privacy is resolved in rustc, and no linker or debugger step
  changes that. The library defines **mirrors** of them from the program's
  DWARF instead (`support_lib/mirror.rs`): a struct as a `#[repr(C)]`
  struct with its fields in offset order, explicit padding (`_cherenkov_pad`
  fields) before any field not at the next offset, the original's
  alignment, std fields as the real std types and project fields as their
  own mirrors; an enum whose tag comes first as a `#[repr(uN)]` enum with
  the original's tag values; any other enum (its tag in a field's niche)
  as its variants and fields in declaration order, for rustc to lay out as
  it laid out the original. In a mirror a function pointer is any
  function pointer (`unsafe fn()`) and a trait object any trait object
  (`dyn Any`): only their layout matters. An array field is `[T; N]`,
  named from its DWARF element type and subrange count (nested for each
  dimension). A mirror has no lifetime parameters: what a field borrows
  (`&T`, `Cow<T>`) it borrows for `'static`, which no layout depends on.
  The layout guard compares each mirror with
  the original (padding and field order aside), so a mirror whose niches or
  size differ is refused, never used. A mirror gets a `Default` only where
  the source derives one for a struct (each field's, padding zero); a
  hand-written `Default` stays the program's own code (the byte builder
  calls it), and a mutant asking a mirror for any other trait is refused.
  Not yet mirrored: unions, and fields DWARF gives no type name
  (closures).
- **Dependency types** (`chrono`, `tonic`, ...) are mirrored like project
  types, without a `Default`. Naming them through the dependency at the
  project's `Cargo.lock` version would compile the dependency tree into the
  library (minutes), so it waits for the build cache.
- **Generic functions** stay refused, as today.

## Finding what to build
A static pass per test binary, before any run: no debugger is needed until
the library is loaded.
- **Types come from DWARF**, read from the binary file as `layouts.rs` does:
  the concrete result type, with aliases (`type Result<T> = ...`), `Self`,
  generics and async (`Poll<T>`) already resolved. The source's names mean
  something only inside the project's scope; resolving them is rustc's job.
- **Values come from the plan:** its value text, generated from the
  signature as cargo-mutants generates it.
- **The walk** goes through each value text together with its DWARF type.
  Project types (a `Result` around a private error, project structs and
  enums) are composed from their layouts. Std leaves become library
  constructors, keyed by (public type, value text) and shared by every plan
  with the same leaf. Leaves the program builds itself (its own linked
  `Default`) keep using its code. Each plan keeps its composition recipe.

## Where it lives: a language port
Everything language-specific lives in a language adapter
(`cherenkov-lang-rust`, `cherenkov-lang-go`, ...) behind one port in
`cherenkov-mutation`, so each language does the compatible thing in its own
terms. The debugger adapter (`cherenkov-exec-dap`) carries out language-
neutral recipes and knows no language's semantics.

```rust
trait LanguageRuntime {
    fn test_programs(&self, project: &Path) -> Result<Vec<TestProgram>>; // cargo test --no-run | go test -c
    fn select(&self, tests: Option<&[String]>) -> Vec<String>;          // libtest --exact | -test.run
    fn failures(&self, output: &str) -> Vec<Failure>;                    // libtest | go test output
    fn values(&self, plans: &[MutantPlan], program: &ProgramInfo) -> ValuePlan; // the pre-walk
}

enum Recipe {
    Bytes(Vec<u8>),                                   // scalars, field-less enums
    CallProgram { symbol: String },                   // the program's own code (its Default)
    CallSupport { symbol: String },                   // a constructor in the support artifact
    Compose { layout: LayoutId, parts: Vec<(u64, Recipe)> }, // types composed from debug info
    Unviable(String),
    Unsupported(String),
}

struct ValuePlan { support: Option<Artifact>, recipes: Vec<Recipe> } // one per plan
```

- **Rust:** all recipes; the support artifact is the cdylib described here.
- **Go:** no second Go runtime can be loaded into a Go process, so no
  support artifact: `CallProgram` (Delve calls the program's functions),
  `Compose` and `Bytes`; anything needing new compiled code is
  `Unsupported`. Test discovery, selection and failure parsing map one to
  one.
- **Others** satisfy the recipes in their own terms (a Python adapter could
  meet `CallSupport` by importing a generated module through its debugger).

### The debugger port
Debuggers are substitutable behind their own port, chosen at run time:
`dyn Debugger` (sessions, memory, breakpoints, evaluation, running), with
no language knowledge. GDB (DAP), Delve (DAP + JSON-RPC) and later LLDB
implement it.
- **Language knowledge moves out of the sessions** into the language port:
  test selection arguments (libtest `--exact` vs `-test.run`), the
  test-frame recogniser the recording uses (libtest's `test::run_test`
  today, hard-coded in `gdb_record.py`), value recipes, runtime checks
  (custom allocator).
- **Capabilities are declared** (tier B evaluation, recording, inferior
  calls, library loading) instead of implied by default trait bodies;
  unsupported pairings are refused up front.
- **Selection** by language, platform and an optional `--debugger`: Rust
  -> GDB (LLDB later on macOS); Go -> Delve, or GDB if it passes the
  conformance suite; C/C++ later -> GDB or LLDB.
- **Conformance suite:** one set of session contract tests (locate,
  read/write, arm/disarm byte-identical, run to exit, crash, timeout,
  signals, recording) runs against every implementation on every fixture
  it claims; substitution is whatever passes.

Today the Rust pieces are spread over `cherenkov-mutation` (`DefaultFacts`),
`cherenkov-syntax` (`rust_types.rs`), `cherenkov-exec-dap` (`toolchain.rs`,
`libtest.rs`, Rust semantics inside `layouts.rs` and `gdb_tierb.py`) and
`cherenkov` (`run_rust`). Extracting them behind the port is a refactor
of its own, with no behaviour change, after the spike.

## Guards
Required before the library replaces the byte builder.
- **Layout equality, per type.** The library is a binary with DWARF too.
  Before using a constructor, compare its type's layout there with the test
  binary's: size, alignment, field offsets, enum discriminants and niches.
  Any difference refuses that constructor. Same compiler and std make std
  types equal; dependency types depend on version *and* enabled features,
  and this turns "should match" into a check.
- **Mutant-list parity.** A test runs `cargo mutants --list --json` on the
  fixtures and compares (file, line, function, replacement text) with
  Cherenkov's plans, exactly. Rule drift fails a test.
- **Sampled fidelity on real projects.** The fidelity harness compiles a
  random sample of library-built mutants from source in a scratch copy and
  compares per-test outcomes, beyond the fixture's known shapes.
- **Build cache.** The library's target directory lives in Cherenkov's
  cache, keyed by the project's `Cargo.lock` and toolchain: dependency types
  cost minutes to compile the first time, and only changed constructors
  rebuild after that.

## Plan
1. Spike, std only, on the rust-ops fidelity fixture: `HashSet::from_iter`,
   `Box::leak(Box::new(..))`, `Arc::new(..)`, `PathBuf`, each faithful to
   the from-source build.
2. Measure on angzarr: refusals and crashes against the byte builder.
3. If it holds, replace the byte builder's heap construction with it.
