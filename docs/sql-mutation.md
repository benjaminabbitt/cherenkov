# SQL mutation testing: prior work and a future SQL language

Status: research, for a future `cherenkov-lang-sql` adapter. Nothing here
is implemented.

## Summary
SQL mutation testing has an established body of work, mostly academic
(2006–2019), and little of it has reached everyday tools. None of the
mainstream mutation tools (PIT, Stryker, cargo-mutants, mutmut,
go-mutesting) understands SQL; they treat it as an opaque string. Stryker's
regex sub-mutator (weapon-regex) is the nearest precedent for mutating a
language embedded in strings.

What matters for Cherenkov:
1. **A reference operator set exists:** the SQLMutation catalog (Tuya,
   Suárez-Cabal, de la Riva, 2007), in four families: clauses (SC),
   operators (OR), NULLs (NL), identifier replacement (IR). Later tools
   (JDAMA, QACover/TdRules, muPLSQL, XData) reuse it. It covers SELECT.
2. **Mutating at the driver boundary is established.** JDAMA instrumented
   JDBC calls, captured each executed query, mutated it and compared
   results; QACover still intercepts at the JDBC driver (via p6spy). This
   is Cherenkov's model: a breakpoint at the driver's execute/prepare entry,
   replacing the query string.
3. **Cheap unviable/equivalent checks are known:** build mutants from a
   parse tree (no syntax-level stillborn mutants), static schema checks,
   and Tuya's equivalence rules. Formal SQL equivalence provers cover too
   few real queries to be worth it.
4. **Schema/constraint mutation is a separate family** (SchemaAnalyst):
   add or remove NOT NULL, PRIMARY KEY, UNIQUE, CHECK, FOREIGN KEY.
5. **Kills depend on test data.** SQL mutants die only if the test database
   holds distinguishing rows (NULLs, duplicates, a parent without children,
   boundaries). Survivors mix weak assertions with weak fixtures, and a
   report should tell them apart.

## Operator families
From Tuya et al. 2007 unless noted.

| Family | Operator | Example (original → mutant) | Notes |
|---|---|---|---|
| SC | SEL | `SELECT` ↔ `SELECT DISTINCT` | Lowest kill score in the NIST study; most equivalents |
| SC | JOI | `INNER JOIN` ↔ `LEFT/RIGHT/FULL OUTER`, ↔ `CROSS` | Join faults are the top real-world SQL error class; only 25% killed in an industrial suite (Derezińska 2009) |
| SC | SUB | `IN` ↔ `EXISTS`, `ANY` ↔ `ALL`, `NOT IN` | Combinatorial |
| SC | GRU | drop a GROUP BY expression | Often unviable |
| SC | AGR | `MAX` ↔ `MIN` ↔ `AVG` ↔ `SUM` ↔ `COUNT`; add/remove DISTINCT | Cheap, high yield |
| SC | UNI | `UNION` ↔ `UNION ALL`; drop a branch | |
| SC | ORD | `ASC` ↔ `DESC`; drop or swap ORDER BY keys | Equivalent for single-row results or when order is ignored |
| OR | ROR | `a > ?` → `>=`, `<`, `=`, `<>`, TRUE, FALSE | Most mutants in practice |
| OR | LCR | `AND` ↔ `OR`, left/right only | Few mutants, cheap |
| OR | BTW | `a BETWEEN x AND y` → `a > x AND a <= y` | Boundaries |
| OR | LKE | `LIKE 'ab%'` → wildcard variants | |
| OR | UOI/ABS/AOR | `e` → `-e`, `e±1`, `ABS(e)`; `+` ↔ `-` | Rare in real queries |
| NL | NLF | `IS NULL` ↔ `IS NOT NULL` | Trivial |
| NL | NLS | `c` → `IFNULL(c, r)` | Scores low until NULL test data exists |
| NL | NLI/NLO | force a condition true/other when NULL | NLO is Tuya's one clear removal candidate |
| IR | IRC/IRT/IRP/IRH | replace a column/constant/parameter with a type-compatible one | Over half of all mutants in industry; needs the schema |
| Schema | CR, CRelOpE, CInListElementR | drop a CHECK; `>` → `>=` in a CHECK; drop an IN-list element | SchemaAnalyst |
| Schema | NNA/NNR | add/remove NOT NULL | SchemaAnalyst |
| Schema | PKColumnA/R/E, UColumnA/R/E | add/remove/exchange PK or UNIQUE columns | SchemaAnalyst |
| Schema | FKColumnPairR/E | remove/exchange an FK column pair | FKColumnPairA is almost always stillborn |

Results:
- **Tuya et al., NIST suite:** the mutation score rose 79.7% → 83.3% →
  85.6% as edge-case data (duplicates, negatives, orphan rows, NULLs) was
  added; hand-written tests reached 94.2%. Equivalent mutants were about
  6%, half detected automatically.
- **McCormick (2010):** half of SQLMutation's operators scored much lower on
  real-world suites than on research suites; three revised operators were
  proposed (names not verified).
- **McMinn et al. (TSE 2019), schemas:** ineffective mutants were up to 24%
  of all mutants and changed the score for 33 of 34 schemas. Removing
  stillborn mutants statically always paid off; detecting redundant ones
  rarely did.

## Tools

| Tool | Mutates | Level | Status | Link |
|---|---|---|---|---|
| SQLMutation / TdRules | SELECT queries (Tuya operators) | Query text; hosted service with Java/.NET clients | Repo active; local generation unverified | https://github.com/giis-uniovi/tdrules |
| QACover | Coverage and mutation of the queries an app runs | Driver (JDBC via p6spy; ADO.NET) | Published, CI active | https://github.com/giis-uniovi/qacover |
| JDAMA | Queries run via JDBC, compared by result set (weak mutation) | Bytecode at the JDBC call | Research prototype, not online | DOI 10.1002/stvr.462 |
| SchemaAnalyst | DDL integrity constraints; test data; mutation analysis | Schema model | GPL-3.0; recent activity unverified | https://github.com/schemaanalyst/schemaanalyst |
| muPLSQL | PL/SQL, 44 operators | Source | Paper only | DOI 10.1016/j.jss.2022.111399 |
| XData | Generates data that kills join, predicate, aggregate and NULL mutants (SMT) | Test data | Research | https://arxiv.org/abs/1411.6704 |
| EvoSQL | Search-based test data for SQL coverage | Test data | Likely dormant | https://github.com/SERG-Delft/evosql |

No SQL mutation tool was found for PL/pgSQL, NoSQL, or ORM/query-builder
ASTs (HQL, JPQL, sea-query).

## SQL as a Cherenkov language
SQL would be a language adapter (`cherenkov-lang-sql`) behind the language
port (see `support-library.md`, "Where it lives"), with one difference: it
is **embedded**. Queries arrive inside a host language's strings, so the
SQL adapter works with a host adapter (Rust, Go) rather than alone.

- **The host adapter finds where SQL runs.** Driver entry points: sqlx's
  `Executor` methods and query construction (Rust), `(*DB|*Tx|*Conn)`
  `QueryContext`/`ExecContext`/`PrepareContext` (Go). These are breakpoint
  sites, not source sites.
- **Two kinds of planning.**
  - *Static:* tree-sitter finds SQL-looking string literals and `query!`
    arguments; the SQL adapter parses and mutates them at plan time.
  - *Dynamic:* the recording run, which already stops where tests run
    code, also records each distinct query string reaching a driver
    breakpoint, per test. Built strings (`format!`, `fmt.Sprintf`,
    sea-query's `build()`) are mutated from what was observed.
- **Applying a mutant** is a new language-neutral recipe: at the driver
  call, replace the query argument with the mutated text, written to a
  scratch buffer in the program, with the (ptr, len) pair patched. A
  call site issuing many queries matches on the exact text (or its hash).
  Prepared-statement caches key on the text, so a mutant is a new
  statement.
- **Reporting** names the call site and a query fingerprint, since a built
  query has no single source span.
- **Test selection** stays per mutant: only the tests whose recording saw
  that exact query run it.
- **Query builders:** no sea-query operators. Rust's own operators already
  reach `SimpleExpr` method chains, and the final string covers the rest;
  overlapping mutants are deduplicated.

### First operator set
A named set (e.g. `sql-tuya-core`) beside cargo-mutants and go-mutesting:
- ROR (with BETWEEN bounds), LCR, NLF;
- JOI limited to `INNER` ↔ `LEFT`;
- AGR (`MIN` ↔ `MAX`, `COUNT` ↔ `COUNT DISTINCT`);
- SEL (DISTINCT toggle, with the equivalence rules below);
- ORD (`ASC` ↔ `DESC`, drop ORDER BY);
- UNI (`UNION` ↔ `UNION ALL`).

Two additions not in the catalog:
- **WHERE-conjunct removal**, including in UPDATE and DELETE: a missing
  tenant, soft-delete or ownership filter is the costliest real-world fault.
- **LIMIT/OFFSET ±1.**

### Unviable and equivalent mutants
- Parse every mutant with a real parser and drop those that fail
  (`sqlparser-rs` for several dialects; `pg_query` for PostgreSQL's exact
  grammar).
- Optionally `PREPARE` the mutant once against the test database: unknown
  columns and type errors make it **unviable**, never killed (SQLMutation
  counted runtime errors as kills, which inflates scores).
- Tuya's static equivalence rules: `DISTINCT` is equivalent inside a
  subquery predicate, in a UNION branch, when every joined table's primary
  key is selected with no GROUP BY, when every select item is an aggregate,
  or when every GROUP BY column is selected. IR is equivalent when `c1` is
  replaced by `c2` across `INNER JOIN ... ON c1 = c2`.
- No formal provers.

### Speed
- **Data-distinguishes pre-filter** (after JDAMA's weak mutation): for a
  side-effect-free SELECT, run the original and the mutant on the test's
  database state at the breakpoint. Identical rows (under a deterministic
  ORDER BY and LIMIT) mean "survived: the test data does not distinguish
  them", without rerunning the test. Likely a large saving, and an
  actionable diagnostic.

### Migrations (second phase)
SchemaAnalyst's remover operators first: NNR, UColumnR, PKColumnR, CR,
FKColumnPairR. Tests rarely assert that constraints are enforced, so these
give strong signals. Applied by intercepting migration statements at the
same driver breakpoint; `#[sqlx::test]` builds a fresh database and runs
migrations per test, which isolates them. A DDL mutant the database rejects
is unviable.

### Risks
- Mutated DML can corrupt a shared test database for later tests: require
  isolation per test (transaction rollback, a database per test) or run
  serially.
- Results depend on fixtures; expect low scores for IR- and NL-style
  mutants.
- Ordering mutants on queries without a deterministic order give false
  kills.
- Dialects differ (`ILIKE`, `ON CONFLICT`, `RETURNING`).

### Left out at first
IR column/constant replacement (most mutants, often unviable, needs the
schema); UOI/ABS/AOR; NLS/NLI/NLO; SUB rewrites, CROSS JOIN synthesis,
GRU; formal provers; LLM-generated mutants (recent LLM work targets DBMS
bug-finding, not test adequacy); NoSQL and PL/pgSQL (no established
operator sets).

## Unverified
- The names of McCormick's three revised operators.
- Whether TdRules can generate mutants locally, without its hosted service.
- Recent activity of SchemaAnalyst and QACover.
- Whether Zhou & Frankl's ICST 2009 paper is the JDAMA work.
- How PIT, mutmut and go-mutesting handle strings (from memory).

## Sources
- Tuya, Suárez-Cabal, de la Riva, "Mutating database queries", IST 2007:
  https://digibuo.uniovi.es/dspace/bitstream/10651/29255/2/ist-2006-sqlmutation.pdf
- SQLMutation: https://test4data.uniovi.es/sqlmutation ; tool paper (2006):
  https://digibuo.uniovi.es/dspace/handle/10651/30892
- Derezińska, "An Experimental Case Study to Applying Mutation Analysis for
  SQL Queries", IMCSIT 2009:
  https://archive.fedcsis.org/proceedings/2009/PTI/pliks/178.pdf
- McCormick, "Towards a Sufficient Set of Mutation Operators for SQL", 2010:
  https://vtechworks.lib.vt.edu/handle/10919/32526
- McMinn, Wright, McCurdy, Kapfhammer, "Automatic detection and removal of
  ineffective mutants for the mutation analysis of relational database
  schemas", TSE 2019:
  https://eprints.whiterose.ac.uk/126146/1/ineffective-mutants.pdf
- Wright, Kapfhammer, McMinn (2013, 2014) and Wright's thesis:
  https://www.cs.allegheny.edu/sites/gkapfham/research/papers/Wright2013 ,
  https://gregorykapfhammer.netlify.app/research/papers/wright2014 ,
  https://etheses.whiterose.ac.uk/id/eprint/12059/1/chris-wright-thesis.pdf
- Virtual mutation (McMinn et al. 2016):
  https://cs.allegheny.edu/sites/gkapfham/research/papers/McMinn2016a
- JDAMA, Zhou & Frankl, STVR 2011: DOI 10.1002/stvr.462
- SchemaAnalyst: https://github.com/schemaanalyst/schemaanalyst
- TdRules: https://github.com/giis-uniovi/tdrules ; QACover:
  https://github.com/giis-uniovi/qacover
- XData: https://arxiv.org/abs/1411.6704 ; EvoSQL:
  https://github.com/SERG-Delft/evosql
- muPLSQL: https://eresearch.ozyegin.edu.tr/handle/10679/8394
- Stryker mutators:
  https://stryker-mutator.io/docs/mutation-testing-elements/supported-mutators/
- Equivalence provers: Cosette
  (https://www.cidrdb.org/cidr2017/papers/p51-chu-cidr17.pdf), VeriEQL
  (https://par.nsf.gov/biblio/10543008-verieql-bounded-equivalence-verification-complex-sql-queries-integrity-constraints)
- Database fault-injection interceptors: https://arxiv.org/abs/2404.01886
