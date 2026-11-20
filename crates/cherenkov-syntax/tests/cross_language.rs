//! Named mutations are language independent: the same program written in
//! each built-in language gets the same named mutations, through the
//! profiles alone. Only what a profile supplies may differ (FnValue needs
//! the profile's return-value generator).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use cherenkov_core::operator_set::{Rule, mutation_named, named, registry};
use cherenkov_core::plan::plan_file;
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

const RUST: &str = "\
fn f(a: i32, b: i32, c: bool, d: bool) -> i32 {
    let mut t = 0;
    if a < b { t = 1; }
    if a <= b { t = 1; }
    if a > b { t = 1; }
    if a >= b { t = 1; }
    if a == b { t = 1; }
    if a != b { t = 1; }
    if c && d { t = 1; }
    if c || d { t = 1; }
    if !c { t = 1; }
    t = -a;
    t = a + b;
    t = a - b;
    t = a * b;
    t = a / b;
    t = a % b;
    t = a << b;
    t = a >> b;
    t = a & b;
    t = a | b;
    t = a ^ b;
    t += a;
    t -= a;
    t *= a;
    t /= a;
    t %= a;
    t <<= a;
    t >>= a;
    t &= a;
    t |= a;
    t ^= a;
    g(t);
    return t;
}
";

const GO: &str = "\
package p

func f(a int, b int, c bool, d bool) int {
\tt := 0
\tif a < b { t = 1 }
\tif a <= b { t = 1 }
\tif a > b { t = 1 }
\tif a >= b { t = 1 }
\tif a == b { t = 1 }
\tif a != b { t = 1 }
\tif c && d { t = 1 }
\tif c || d { t = 1 }
\tif !c { t = 1 }
\tt = -a
\tt = a + b
\tt = a - b
\tt = a * b
\tt = a / b
\tt = a % b
\tt = a << b
\tt = a >> b
\tt = a & b
\tt = a | b
\tt = a ^ b
\tt += a
\tt -= a
\tt *= a
\tt /= a
\tt %= a
\tt <<= a
\tt >>= a
\tt &= a
\tt |= a
\tt ^= a
\tg(t)
\treturn t
}
";

const PYTHON: &str = "\
def f(a: int, b: int, c: bool, d: bool) -> int:
    t = 0
    if a < b: t = 1
    if a <= b: t = 1
    if a > b: t = 1
    if a >= b: t = 1
    if a == b: t = 1
    if a != b: t = 1
    if c and d: t = 1
    if c or d: t = 1
    if not c: t = 1
    t = -a
    t = a + b
    t = a - b
    t = a * b
    t = a / b
    t = a % b
    t = a << b
    t = a >> b
    t = a & b
    t = a | b
    t = a ^ b
    t += a
    t -= a
    t *= a
    t /= a
    t %= a
    t <<= a
    t >>= a
    t &= a
    t |= a
    t ^= a
    g(t)
    return t
";

/// How many mutants of each named mutation the set plans for `src`.
fn counts(lang: &str, src: &str, set: &str) -> BTreeMap<&'static str, usize> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let id = LanguageId::new(lang);
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == id)
        .unwrap();
    let file = syntax.analyze(&id, Path::new("t"), Arc::from(src)).unwrap();
    assert!(!file.has_errors, "{lang} sample does not parse");
    let mut out = BTreeMap::new();
    for p in plan_file(&file, &profile, &named(set).unwrap().operators()).plans {
        let name = p.mutation.unwrap_or_else(|| panic!("{}", p.describe()));
        *out.entry(name).or_insert(0) += 1;
    }
    out
}

#[test]
fn test_every_set_plans_the_same_named_mutations_in_every_language() {
    for set in registry() {
        let mut rust = counts("rust", RUST, set.name);
        let go = counts("go", GO, set.name);
        let python = counts("python", PYTHON, set.name);
        // FnValue draws on the profile's return-value generator; only the
        // Rust profile has one so far.
        rust.remove("fn-value");
        assert_eq!(go, rust, "{}: go vs rust", set.name);
        assert_eq!(python, rust, "{}: python vs rust", set.name);
    }
}

#[test]
fn test_samples_exercise_every_token_mutation_in_the_cargo_mutants_set() {
    let planned = counts("rust", RUST, "cargo-mutants");
    for name in named("cargo-mutants").unwrap().mutations {
        // Arms, guards and fields have no common form in all three
        // languages; tests/match_and_fields.rs covers them per language.
        let rule = mutation_named(name).unwrap().rule;
        if !matches!(rule, Rule::ReplaceToken { .. } | Rule::DeleteUnary { .. }) {
            continue;
        }
        assert!(
            planned.contains_key(name),
            "{name} is not exercised by the sample"
        );
    }
}
