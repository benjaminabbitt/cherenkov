//! Mutant IDs through a real parse: they must survive edits elsewhere in
//! the file and reindentation (design section 4.5).

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::id::MutantId;
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::plan_file;
use cherenkov_mutation::ports::SyntaxPort;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

const GO: &str = include_str!("../../../../../fixtures/go/calc.go");

fn ids(src: &str) -> Vec<(String, MutantId)> {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let go = LanguageId::new("go");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == go)
        .unwrap();
    let file = syntax
        .analyze(&go, Path::new("calc.go"), Arc::from(src))
        .unwrap();
    plan_file(&file, &profile, &catalog())
        .plans
        .into_iter()
        .map(|p| (p.operator.to_string(), p.id))
        .collect()
}

#[test]
fn test_ids_survive_a_function_inserted_above() {
    let shifted = GO.replacen(
        "func log(_ int) {}\n",
        "func log(_ int) {}\n\nfunc Extra(n int) int {\n\treturn n + 1\n}\n",
        1,
    );
    let before = ids(GO);
    let after = ids(&shifted);
    for entry in &before {
        assert!(
            after.contains(entry),
            "{entry:?} lost after unrelated insert"
        );
    }
    assert_eq!(
        after.len(),
        before.len() + 2,
        "Extra adds exactly its body-default mutant and the in-place swap of `n + 1`"
    );
}

#[test]
fn test_ids_survive_reindentation() {
    let reindented = GO.replace('\t', "    ");
    assert_eq!(ids(GO), ids(&reindented));
}
