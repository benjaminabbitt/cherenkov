# Combinator (map/filter/reduce) mutation: prior art and a rule family

Status: research, for a future cross-language rule family. Nothing here is
implemented. Companion to `sql-mutation.md`: SQL's operators are relational
algebra, and relational algebra is map/filter/reduce, so most SQL rules
have a functional twin in iterator chains, streams, LINQ, comprehensions
and dataflow frameworks.

## Summary
Prior art exists but is scattered; no published catalog spans languages.
- **Tools with fixed swap/removal lists:** Stryker (JS/TS, .NET LINQ,
  Scala), Arcmutate (commercial PIT extension: Java streams, reactive),
  Infection (PHP array functions).
- **One evaluated academic catalog:** TRANSMUT-Spark (de Souza Neto et
  al., STVR 2022), whose operators mirror SQLMutation's clause operators:
  JTR ≈ JOI, DTD/DTI ≈ SEL, OTD/OTI ≈ ORD, STR ≈ UNI, ATR ≈ AGR.
- **Smaller sets:** MuCheck (Haskell list functions); a Java Streams fault
  study (SAST 2022: 7 operators reproducing 96.7% of 91 real stream bugs;
  most faults in intermediate stages and lambda bodies).
- **None:** cargo-mutants, go-mutesting/gremlins/ooze, cosmic-ray, PIT core
  (beyond empty stream returns), mutmut (lambda → None, argument removal,
  string-method swaps; no builtins).

Best supported by evidence or most widely implemented:
1. **Filter removal / predicate forced true.** Subsumes predicate negation
   (TRANSMUT: tests killing removal always kill negation, not vice versa).
2. **Sort removal and order inversion.** Hardest to kill in TRANSMUT (kill
   ratio 16–30%), with no equivalent mutants.
3. **any ↔ all.**
4. **min ↔ max**, including the `_by` forms.
5. **take/skip bounds** and removal.
6. **Reducer argument mutations:** `f(x, y)` → `x`, `y`, `f(x, x)`,
   `f(y, y)`, `f(y, x)` (kill ratio about 46%).

Distinct insertion/removal is widely implemented but the noisiest: 56–78%
of SQLMutation's DISTINCT mutants were equivalent, and TRANSMUT's distinct
insertion had the most equivalents.

## Prior art (verified from source or official docs unless noted)
- **StrykerJS** (`method-expression-mutator.ts`). Removals (call replaced
  by its receiver): `filter`, `sort`, `reverse`, `slice`, `charAt`,
  `substr`, `substring`, `trim`. Swaps: `endsWith` ↔ `startsWith`,
  `every` ↔ `some`, `min` ↔ `max`, `toLowerCase` ↔ `toUpperCase`,
  `trimEnd` ↔ `trimStart`, some date setters.
- **Stryker.NET LinqMutator** (36 swaps, no removals): `All` ↔ `Any`;
  `First` ↔ `FirstOrDefault`; `Single` ↔ `SingleOrDefault`; `Last` →
  `First`; `Skip` ↔ `Take` (and `While`/`Last` forms); `Min` ↔ `Max`,
  `MinBy` ↔ `MaxBy`; `Sum` → `Max`; `Count` → `Sum`; `Average` → `Min`;
  `OrderBy` ↔ `OrderByDescending` (and `ThenBy`, `Order`); `Reverse` ↔
  `AsEnumerable` (removal by identity); `Union` ↔ `Intersect`, `Concat` ↔
  `Except`, `Append` ↔ `Prepend`. Method syntax only; no type checks. The
  cross-tool "supported mutators" page is out of date.
- **Stryker4s:** `filter` ↔ `filterNot`, `exists` ↔ `forall`, `take` ↔
  `drop` (and `Right`/`While` forms), `isEmpty` ↔ `nonEmpty`, `indexOf` ↔
  `lastIndexOf`, `max` ↔ `min`, `maxBy` ↔ `minBy`.
- No effectiveness or equivalence data was found for Stryker's method
  mutators.
- **Arcmutate:** REMOVE_DISTINCT, REMOVE_FILTER (documented as overlapping
  a `return true` lambda mutant), REMOVE_LIMIT (not on clearly infinite
  streams), REMOVE_SKIP, REMOVE_SORTED, SWAP_ALL_MATCH; Reactor/Rx
  `flatMap`/`concatMap`/`switchMap` swaps.
- **Infection (PHP):** unwrap `array_filter`/`map`/`reverse`/`slice`/
  `unique`/`reduce` (about 35 functions); `ArrayAll`/`ArrayAny` → `true`;
  Spaceship `$a <=> $b` → `$b <=> $a` (a comparator reversal).
- **universalmutator:** `min ==> max`, `max ==> min`, `begin ==> end`.
- **MuCheck (Haskell):** swaps within groups, including `sum`, `product`,
  `maximum`, `minimum`, `head`, `last`.
- **mutatest (Python):** slice bound swaps (`a[:2]` → `a[2:]`).

### TRANSMUT-Spark
15 operators over Spark programs, built as one meta-mutant: transformation
replacement, swap and deletion (UTR, UTS, UTD, BTR, BTS); map result
replacement (MTR); filter deletion (FTD) and predicate negation (NFTP);
set-operation replacement (STR); distinct deletion/insertion (DTD, DTI);
aggregation replacement (ATR); join type replacement (JTR); sort deletion
and inversion (OTD, OTI).

| Operator | Kill ratio (lower is harder) | Equivalent mutants |
|---|---|---|
| OTD | 16–20% | 0 |
| JTR | 22% | 3/6 |
| DTD | 25% | 0 |
| DTI | 26–28% | 10/31, 25/49 |
| OTI | 25–30% | 0 |
| UTD | 32% | 0 |
| STR | 34% | 2/10 |
| FTD | 34–36% | 0 |
| ATR | 46% | 4/20 (commutative reducers) |
| NFTP | 66% | 0 |
| MTR | 74–76% | 5/82 |

MTR results of Max, Min, `""`, `reverse` or `null` were judged trivial.
Reduction rules (drop distinct inserted after a group/aggregate; drop the
commutative argument swap; removal subsumes negation and inversion) cut
mutants by about 34% and equivalents by about 35%.

## Executing combinator mutants on a compiled binary
In source, deleting `.filter(p)` changes the iterator's type and rustc
re-infers what follows. In the binary, the following code was compiled
for `Filter<I, P>`, so the call cannot be skipped. Each rule falls into
one of these classes:

- **C, closure result:** force or transform a closure's result (predicate
  → `true`, negated; fold closure returns the accumulator or the element).
  Uses the body-replacement machinery that already exists.
- **R, reversed comparison:** reverse a comparator's `Ordering` result.
  One operator covers sort inversion, `min_by` ↔ `max_by`,
  `binary_search_by` and the Spaceship swap.
- **A, argument value:** change an argument at the call (`take(n)` →
  `n ± 1`, `0`, max). A small tier B effect.
- **N, neutralise a stage:** skip an in-place call (`v.sort()`,
  `v.dedup()`, `v.reverse()`): statement deletion.
- **S, method swap:** call the alternative (`first` → `last`, `sum` →
  `count`) for the same types. Needs its monomorphised code in the binary;
  closure-built iterator types cannot be named by separately compiled
  code, so the support library rarely helps. Often refused.

## Rule family

| Family | SQL form | Combinator forms | Class | Notes |
|---|---|---|---|---|
| Filter removal | WHERE dropped | `filter`, `Where`, comprehension `if`, Spark `filter` | C (predicate → `true`) | Subsumes negation; 0 equivalents in TRANSMUT |
| Filter negation | NOT on WHERE | `filter` ↔ `filterNot` | C | Subsumed by removal; easy to kill |
| any ↔ all | ANY ↔ ALL | `any`/`all`, `anyMatch`/`allMatch`, `some`/`every`, `exists`/`forall` | C (`all(p) = !any(!p)`: negate predicate and result) | Equivalent on a singleton |
| min ↔ max | MIN ↔ MAX | `min_by`/`max_by`, `min_by_key`, `Min`/`Max`, `min`/`max` | R for `_by` forms; S for plain `min()` | Equivalent on length ≤ 1 |
| Other aggregate swaps | SUM ↔ COUNT ↔ AVG | `sum`/`count`/`product` | S | No evaluation data |
| Reducer mutation | AGR (loose) | `fold`/`reduce` closure | C | Commutative swap is equivalent |
| Sort removal | ORDER BY dropped | `v.sort()`, `sorted`, `OrderBy` | N | Hardest to kill |
| Sort inversion | ASC ↔ DESC | `sort_by`, `OrderByDescending`, `sortByKey(false)` | R | Subsumed by removal |
| Reverse removal | — | `v.reverse()`, `.rev()` | N in place; S for the `Rev` adapter | |
| Distinct removal | DISTINCT dropped | `dedup`, `unique`, `Distinct` | N | High equivalent rate |
| take/skip bounds | LIMIT/OFFSET ± 1 | `take`/`skip`, `limit`, slices, `Take`/`Skip` | A | take ↔ skip swap is S |
| take_while/skip_while | — | `take_while`, `TakeWhile` | C | |
| first ↔ last | — | `first`/`last`, `head`/`last`, `First`/`Last` | S | Equivalent on a singleton |
| Emptiness | — | `is_empty` ↔ not | C (negate result) | |
| Set operations | UNION ↔ UNION ALL / INTERSECT | `chain`, `Union`/`Intersect` | S | |
| Join type | INNER ↔ LEFT | Spark `join`/`leftOuterJoin` | S | Dataflow only |

### First wave
Cheap at a breakpoint and backed by evidence:
1. Predicate forced `true` (filter removal); no separate negation.
2. Comparator-result reversal.
3. Neutralise in-place `sort`/`dedup`/`reverse`.
4. `take`/`skip`/`nth`/`step_by` bounds: `n ± 1`, `0`, max.
5. any ↔ all as negated predicate plus negated result.
6. Fold-closure mutations: return the accumulator, return the element,
   swap arguments (not for known commutative reducers).
7. `take_while`/`skip_while` predicate forced.

### Later, only where the alternative's code is in the binary
`first` ↔ `last`, `sum` ↔ `count`, `union` ↔ `intersect`, `take` ↔
`skip`, removing the `Rev` adapter.

### Left out
Distinct insertion and stage reordering (new code; many equivalents);
join-type swaps (no in-language analogue outside dataflow frameworks);
map-to-constant (low value; return-value replacement already covers much
of it).

### Equivalence rules worth building
- **Dynamic, at the breakpoint:** skip the mutant for a test when the
  stage's input has length ≤ 1, or when the mutated stage's output equals
  the original's (a weak-mutation infection check).
- **Static:** sort/rev/dedup feeding an order- or multiplicity-insensitive
  sink (sum, count, min/max, any/all, collect into a set or map); dedup
  after a group-by or set collect; argument swap in a commutative reducer;
  removal subsumes negation and inversion.

## Experiment: what a Rust test binary holds
A crate with `filter`/`map`/`sum`, `max`, `min_by_key`, `any`, `all`,
`sort_by`, `rev().take(3)` and `fold`, built as a test binary at the test
profile (opt-level 0, full debug info), the build Cherenkov runs against.
- **Every closure is its own function with its own line rows** (filter
  predicate, map, the `any`/`all` predicates, the fold closure, the
  `min_by_key` key, the `sort_by` comparator). Cherenkov already mutates
  inside them: `ror` in the `any`/`all` predicates was applied at tier P,
  `aor` in the fold closure at tier B.
- **The combinators are separate monomorphised functions**
  (`Iter::any::<closure>`, `all::<closure>`, `Copied::max`, `min_by_key`,
  `fold`, `Rev`'s methods): a breakpoint can stop at them, and a call's
  result can be flipped at the user's call site, as tier B already does.
- **Unused swap partners are absent:** no `min` for the iterator type, no
  `max_by_key`, no `last`.

Consequences per class:
- **C** is feasible now: a return stub on the closure, as on any function,
  at tier P. Missing piece: planning "closure body" as a mutation site.
- **R** is feasible for comparator closures only (`sort_by`, `min_by`,
  `max_by`, `binary_search_by`): negate the closure's `Ordering`. The
  `_by_key` forms compare keys inside std through `Ord::cmp`, which the
  rest of the program shares; reversing that would mutate everything.
- **A** is feasible: `take(3)` passes `3` in a register at the user's call
  (`Rev::take(self, 3)`), a tier B argument change.
- **N** is feasible as statement deletion.
- **S** is confirmed refused unless both methods are compiled in.

## Open questions
- Unverified: the Java Streams paper's operator names (paywalled); a pitest
  blog post on `return true` lambdas as filter removal; PIT's empty-stream
  filter scope; Descartes; MuCheck's type-aware rules; Erlang mu2's
  operators; Mattos's MapReduce operators (secondary citation only).

## Sources
- Stryker: https://stryker-mutator.io/docs/mutation-testing-elements/supported-mutators/ ,
  https://stryker-mutator.io/docs/stryker-net/mutations/ ,
  https://github.com/stryker-mutator/stryker-js/blob/master/packages/instrumenter/src/mutators/method-expression-mutator.ts ,
  https://github.com/stryker-mutator/stryker-net/blob/master/src/Stryker.Core/Stryker.Core/Mutators/LinqMutator.cs ,
  https://github.com/stryker-mutator/stryker4s/blob/master/modules/core/src/main/scala/stryker4s/mutants/findmutants/MutantMatcher.scala
- PIT: https://pitest.org/quickstart/mutators/ ; Arcmutate:
  https://docs.arcmutate.com/docs/extended-operators.html
- Java Streams operators: https://sol.sbc.org.br/index.php/sast/article/view/22183 ,
  https://github.com/easy-software-ufal/MutationOperatorsForJavaStreams
- mutmut: https://github.com/boxed/mutmut/blob/main/src/mutmut/mutation/mutators.py ;
  mutatest: https://mutatest.readthedocs.io/en/latest/mutants.html
- cargo-mutants: https://mutants.rs/mutants.html ; mutagen:
  https://github.com/llogiq/mutagen/blob/master/docs/mutators.md
- Go: https://gremlins.dev/latest/usage/mutations/ ,
  https://github.com/gtramontina/ooze , https://github.com/avito-tech/go-mutesting
- Infection: https://infection.github.io/guide/mutators.html
- universalmutator: https://github.com/agroce/universalmutator ; Deb et al.,
  FSE 2024: https://par.nsf.gov/servlets/purl/10555514
- TRANSMUT-Spark: https://arxiv.org/abs/2108.02589 ; dataflow model:
  https://arxiv.org/abs/2108.02582
- MuCheck: https://hackage.haskell.org/package/MuCheck-0.3.0.4/docs/src/Test-MuCheck-Operators.html
- SQLMutation (Tuya et al., IST 2007):
  https://digibuo.uniovi.es/dspace/bitstream/10651/29255/2/ist-2006-sqlmutation.pdf
