# First real-crate comparison: angzarr-embedded-host-config

Run on 2026-11-17 against angzarr-embedded at `c7c95e8`, exported with
`git archive` into a scratch directory. The checkout itself has an
untracked `crates/annals-log/` that breaks its workspace, so it was not
used. Rust 1.96.1 (the repo's pin), GDB 16.3, cargo-mutants 26.2.0. Tests
in `*.test.rs` files are excluded from mutation, since the crate keeps them
beside the code.

## What each tool generated and found

| Function | cargo-mutants | Cherenkov |
|---|---|---|
| `loader() -> Loader` | `Default::default()`: unviable | body-default: unsupported (no known zero value) |
| `same_shape(..) -> Result<(), _>` | not mutated | body-default: unsupported |
| `from_yaml(..) -> Result<HostConfig, LoadError>` | `Ok(Default::default())`: caught | body-default: unsupported |
| `from_yaml` closure, `doc.remove(KEY);` | not mutated | delete-statement: **killed** (tier P) |
| `stamped(..) -> String` | `String::new()`, `"xyzzy".into()`: caught ×2 | body-default: unsupported |
| `Expose::as_str -> &'static str` | `""`, `"xyzzy"`: caught ×2 | body-default: unsupported |
| `ComponentKindConfig::as_str -> &'static str` | `""`, `"xyzzy"`: caught ×2 | body-default: unsupported |
| three `default() -> Self` | not mutated | body-default: unsupported |

- **cargo-mutants:** 8 mutants (7 caught, 1 unviable) in 98 s wall, including the initial build.
- **Cherenkov:** 10 planned, 1 tested (killed) in 5.5 s wall with the build cached (18.5 s cold).

## What it shows

- **The gap is body replacement for non-primitive result types.** Every
  cargo-mutants mutant here replaces a function body with a value built by
  the compiler (`String::new()`, `""`, `Ok(Default::default())`). Tier P's
  body stub only zeroes registers, which is exact only for primitives. A
  `String` or `Result` is returned through memory (sret), and a `&str` must
  not be a null pointer. Closing the gap needs type-aware stubs from the
  debug info's layouts (`""`, empty `String`/`Vec`, `None`), and possibly
  calls to code already in the binary. It does not need recompiling.
- **Cherenkov generates mutants cargo-mutants does not**: statement deletion
  (here `doc.remove(KEY);`, killed), and body replacement of `Default` impls.
- **Real code exposed two patcher bugs, both fixed and covered by tests.**
  The line table attributes the next statement's operand loads to a call
  statement. And a statement is split by unwinding landing pads, with the
  drop of the call's result in a second block.
- **The timings aren't comparable yet**, because Cherenkov tested one mutant.
