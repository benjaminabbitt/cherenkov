# Operator research: tools and literature (2026-11-17)

Gathered by two research passes that fetched tool docs, tool source and
papers. Each claim keeps its provenance:

- **[full text]**: the paper itself was read.
- **[abstract]**: only the abstract page could be reached.
- **unverified**: seen only in search snippets.

Tool rows come from the tool's current docs or source unless marked. Read
this together with docs/mutation-operators.md (the classic definitions).

## Where tools deliberately deviate, and why

| Tool | Deviation | Stated reason |
|---|---|---|
| PIT | defaults use the boundary + negation pair for ROR; full ROR is experimental | negation "overlaps to a degree with the conditionals boundary mutator, but is less stable" |
| PIT | remove-conditionals off | "a large degree of overlap in the tests required to kill these mutations" |
| PIT | constructor and non-void call removal off | "fairly unstable" (NullPointerExceptions); "may also create equivalent mutations" |
| PIT | empty/true/false returns skip methods that already return that value | avoids equivalent mutants |
| mutmut, go-mutesting | boundary-only ROR (`<` → `<=`) | "as subtle as possible"; "to catch off-by-one errors" |
| cosmic-ray | `not` is only deleted, never replaced; `+x` is never deleted | "strange results"; equivalent mutants |
| Dextool | ROR uses Kaminski's 3-mutant sets, made type-specific (no ordering mutants on bool; no `<`→`<=` boundary on floats; none at enum range edges) | Kaminski et al.: "only three are required"; fewer junk mutants |
| Dextool | AORS (counterpart only) instead of full AOR | full AOR "highly redundant" |
| Dextool | LCR adds `true`, `false`, `x`, `y`; UOI reduced to `!x` → `x`; SDL on whole blocks | Google 2018; unproductive mutants; per-line deletion gave too many compile failures |
| cargo-mutants | `<` → `==`, `>`; equality never swapped for ordering; `&=`/`\|=` never become `^=`; unary operators only deleted | false positives (e.g. unsigned compared to 0); same result on bits accumulated from zero |
| mutagen | only mutations that compile without type information | compile-once design: "we must avoid mutations that break the code" |
| mutest-rs (ICST 2026) | type-checked through rustc, so every mutant compiles; Rust-specific operators: call → `Default`, call deletion, `continue`↔`break`, range bound inclusive↔exclusive, etc. | 8–31% of cargo-mutants' mutants were invalid [full text] |

## ROR families

The tools fall into four camps on relational operators. The `same as`
peers of the named mutations in `crates/cherenkov-mutation/src/operator_set.rs`
rest on this section: the boundary swaps (`<`↔`<=`, `>`↔`>=`) for camps
1 and 2, and `==`↔`!=` for camp 2. PIT calls these mutators
`CONDITIONALS_BOUNDARY` and `NEGATE_CONDITIONALS`.

1. **Boundary only:** mutmut, go-mutesting.
2. **Boundary plus negation:** PIT defaults, Stryker, Gremlins, Mull, mutest-rs.
3. **Kaminski's sufficient sets:** Major (example script), Dextool.
4. **Full ROR:** PIT experimental, cosmic-ray, universalmutator.

cargo-mutants' `<` → `==`, `>` matches none of them.

## What the literature recommends

1. **ROR: 3 replacements per operator instead of 7.**
   - `<` → `<=`, `!=`, `false`
   - `>` → `>=`, `!=`, `false`
   - `<=` → `<`, `==`, `true`
   - `>=` → `>`, `==`, `true`
   - `==` → `<=`, `>=`, `false`
   - `!=` → `<`, `>`, `true`

   Just & Schweiggert, STVR 2014 [full text], 10 Java programs, 410K LOC:
   27% fewer mutants, and redundant mutants inflated mutation scores. They
   confirm Kaminski, Ammann & Offutt, JSS 2013 (paper unverified directly).
2. **COR/LCR: 4 of the 10 connector mutants.** `a && b` → `false`, `a`,
   `b`, `a == b`; `a || b` → `true`, `a`, `b`, `a != b`. On boolean
   expressions these subsume UOI. Same source.
3. **Drop ABS.**
   - Google: "predominantly not useful" [full text].
   - Kushigian et al., ISSTA 2024: ABS "produces many equivalent mutants" [full text].
4. **Drop `++`/`--` insertion (AOIS),** or guard it: 94.8% of TCE-detected
   equivalent mutants in Kintis et al.'s Java data (Kushigian 2024).
5. **Keep statement and block deletion, ROR and COR.**
   - Just et al., FSE 2014 [full text], 357 real faults: they couple best to real faults and "should always be employed".
   - Google: SBR is the most productive operator.
6. **Operators that real faults need and the classic set lacks** (FSE 2014):
   deleting `return`/`break`/`continue`, swapping or omitting arguments,
   replacing a library call with a similar one, dropping a chained call, and
   literal or cast type changes.
7. **Arid-code suppression and one mutant per line.** Google ICSE-SEIP 2018
   and TSE 2021 [full text]: logging, deadlines and config flags; more than
   100 rules cut unproductive mutants from 85% to 11%. Picking the operator
   from context raised survival by over 40%.
8. **Type-aware operators for Rust** (mutest-rs, ICST 2026).

## Where findings conflict

- **Minimal sets vs adding operators.**
  - Minimal and sufficient sets: Offutt 1996; Just & Schweiggert 2014.
  - No selective set works everywhere: Kurtz et al., FSE 2016 [abstract and doctoral paper].
  - Reduction barely beats random sampling (about 13%): Gopinath et al., ICSE 2016 [abstract].
  - PIT's restricted set is too weak: Laurent et al., ICST 2017 [abstract].
- **Google's goal differs from adequacy.** It keeps 5 ROR replacements and
  samples one per line, because it wants actionable mutants, not a
  non-redundant score.
- **How strongly mutants stand in for real faults.**
  - Strong correlation: Just 2014.
  - Weak once suite size is controlled: Papadakis et al., ICSE 2018 (unverified).

## What this means for Cherenkov

There is no single correct operator set. The sets differ on purpose,
because they serve different goals:

| Goal | Set |
|---|---|
| comparability with cargo-mutants | cargo-mutants' set |
| a non-redundant adequacy score | Kaminski's and Just's sufficient sets |
| fewest compile-invalid mutants | type-aware Rust operators (mutest-rs) |
| actionable mutants for developers | context-picked, one per line, arid filtering (Google) |

An operator definition must therefore name the set it follows. Mixing them
silently would make results incomparable with any of them.
