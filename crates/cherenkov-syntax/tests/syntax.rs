//! The tree-sitter adapter against the real grammars: what the profiles
//! capture, and how repeated grammar fields are folded into one concept.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::ports::{SyntaxError, SyntaxPort};
use cherenkov_mutation::syntax::{ConceptMatch, FileSyntax, LanguageId, StmtKind};
use cherenkov_syntax::{ProfileError, ProfileSource, TreeSitterSyntax, profiles};

fn analyze(lang: &str, src: &str) -> FileSyntax {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    syntax
        .analyze(&LanguageId::new(lang), Path::new("t"), Arc::from(src))
        .unwrap()
}

fn of(file: &FileSyntax, concept: Concept) -> Vec<&ConceptMatch> {
    file.matches
        .iter()
        .filter(|m| m.concept == concept)
        .collect()
}

fn text(file: &FileSyntax, m: &ConceptMatch, c: Capture) -> Vec<String> {
    m.all(c).map(|s| s.text(&file.source).to_string()).collect()
}

fn python_source(
    concepts_scm: &str,
    tokens_toml: &str,
) -> Result<cherenkov_syntax::LoadedProfile, ProfileError> {
    profiles::load(&ProfileSource {
        lang: "python",
        grammar: tree_sitter_python::LANGUAGE.into(),
        concepts_scm,
        tokens_toml,
        profile_toml: include_str!("../../../profiles/python/profile.toml"),
        return_values: None,
    })
}

// --- Profile loading.

#[test]
fn test_builtin_profiles_load_for_go_python_and_rust() {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let langs: Vec<String> = syntax.profiles().iter().map(|p| p.lang.0.clone()).collect();
    assert_eq!(langs, ["go", "python", "rust"]);
}

#[test]
fn test_builtin_profiles_map_all_concepts_but_expr_name_and_block() {
    for p in TreeSitterSyntax::builtin().unwrap().profiles() {
        let unmapped: Vec<Concept> = Concept::ALL
            .into_iter()
            .filter(|c| !p.mapped.contains(c))
            .collect();
        let mut expected = vec![Concept::Expr, Concept::Name, Concept::Block];
        // Python has no struct literal whose omitted fields take a default.
        if p.lang.0 == "python" {
            expected.push(Concept::FieldInit);
        }
        assert_eq!(unmapped, expected, "{}", p.lang);
    }
}

#[test]
fn test_builtin_profiles_carry_file_rules_and_defaults() {
    let profiles = TreeSitterSyntax::builtin().unwrap().profiles();
    let go = profiles.iter().find(|p| p.lang.0 == "go").unwrap();
    assert_eq!(go.files.extensions, ["go"]);
    assert!(go.files.exclude.contains(&"**/*_test.go".to_string()));
    assert_eq!(go.defaults.get("bool").map(String::as_str), Some("false"));
    let py = profiles.iter().find(|p| p.lang.0 == "python").unwrap();
    assert_eq!(py.tokens.wrap_not("x"), "not (x)");
    assert!(py.unit_types.contains("None"));
}

#[test]
fn test_profile_with_unknown_concept_capture_is_rejected() {
    let err = python_source(
        "(identifier) @concept.bogus",
        include_str!("../../../profiles/python/tokens.toml"),
    )
    .err()
    .unwrap();
    assert!(
        matches!(err, ProfileError::UnknownCapture { ref name, .. } if name == "concept.bogus"),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "python: concepts.scm: unknown capture @concept.bogus"
    );
}

#[test]
fn test_profile_with_unknown_role_capture_is_rejected() {
    let err = python_source(
        "(binary_operator operator: _ @opp) @concept.binary",
        include_str!("../../../profiles/python/tokens.toml"),
    )
    .err()
    .unwrap();
    assert!(
        matches!(err, ProfileError::UnknownCapture { ref name, .. } if name == "opp"),
        "{err}"
    );
}

#[test]
fn test_profile_with_unknown_token_key_is_rejected() {
    let err = python_source("", "not_template = \"not ({})\"\n[tokens]\nplus = \"+\"\n")
        .err()
        .unwrap();
    assert!(
        matches!(err, ProfileError::UnknownToken { ref name, .. } if name == "plus"),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "python: tokens.toml: unknown token \"plus\""
    );
}

#[test]
fn test_profile_with_invalid_query_is_rejected() {
    let err = python_source(
        "(no_such_node) @concept.binary",
        include_str!("../../../profiles/python/tokens.toml"),
    )
    .err()
    .unwrap();
    assert!(matches!(err, ProfileError::Query { .. }), "{err}");
}

#[test]
fn test_analyze_unknown_language_is_an_error() {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let err = syntax
        .analyze(&LanguageId::new("cobol"), Path::new("t"), Arc::from(""))
        .unwrap_err();
    assert_eq!(err, SyntaxError::UnknownLanguage(LanguageId::new("cobol")));
}

// --- Positions and statements.

#[test]
fn test_positions_are_one_based_lines_and_byte_columns() {
    let f = analyze("python", "pass\n\tx = a + b\n");
    let b = of(&f, Concept::Binary)[0];
    assert_eq!(
        (b.span.start.line, b.span.start.col, b.span.start.byte),
        (2, 6, 10)
    );
}

#[test]
fn test_python_statements_are_classified_by_kind() {
    let src = "def f():\n    log(x)\n    x = 1\n    x += 1\n    if x:\n        pass\n    while x:\n        pass\n    return x\n";
    let f = analyze("python", src);
    let kinds: Vec<StmtKind> = f.statements.iter().map(|s| s.kind).collect();
    use StmtKind::*;
    assert_eq!(kinds, [Call, Assign, Assign, If, Loop, Return]);
}

#[test]
fn test_rust_if_and_return_are_statements_with_or_without_semicolon() {
    let src = "fn f() -> bool {\n    if a { return true; }\n    return false\n}\n";
    let f = analyze("rust", src);
    let kinds: Vec<StmtKind> = f.statements.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, [StmtKind::If, StmtKind::Return, StmtKind::Return]);
}

#[test]
fn test_syntax_errors_are_flagged() {
    assert!(analyze("python", "def f(:\n").has_errors);
    assert!(!analyze("python", "def f():\n    pass\n").has_errors);
}

// --- Repeated grammar fields fold into one concept match.

#[test]
fn test_python_chained_comparison_is_one_binary_with_two_operators() {
    let f = analyze("python", "ok = a < b <= c\n");
    let bins = of(&f, Concept::Binary);
    assert_eq!(bins.len(), 1);
    assert_eq!(text(&f, bins[0], Capture::Op), ["<", "<="]);
}

#[test]
fn test_python_if_with_elif_and_else_is_one_condition_whose_first_else_is_the_elif() {
    let src = "if a:\n    x()\nelif b:\n    y()\nelse:\n    z()\n";
    let f = analyze("python", src);
    let conds = of(&f, Concept::Condition);
    assert_eq!(conds.len(), 1, "elif conditions are not mapped yet");
    let first_else = conds[0].first(Capture::Else).unwrap();
    assert!(first_else.text(&f.source).starts_with("elif b:"));
    assert_eq!(conds[0].all(Capture::Else).count(), 2);
}

// --- Per-language shapes the generic queries could get wrong.

#[test]
fn test_rust_if_let_is_not_a_condition() {
    let f = analyze(
        "rust",
        "fn f(v: Option<i32>) {\n    if let Some(x) = v { g(x); }\n    if v.is_some() { h(); }\n}\n",
    );
    let conds = of(&f, Concept::Condition);
    assert_eq!(conds.len(), 1);
    assert_eq!(text(&f, conds[0], Capture::Cond), ["v.is_some()"]);
}

#[test]
fn test_rust_let_with_initializer_is_assign_with_pattern_target() {
    let f = analyze("rust", "fn f() {\n    let mut i = a + 1;\n}\n");
    let a = of(&f, Concept::Assign)[0];
    assert_eq!(text(&f, a, Capture::Target), ["i"]);
    assert_eq!(text(&f, a, Capture::Value), ["a + 1"]);
}

#[test]
fn test_go_for_conditions_with_and_without_clause_are_conditions() {
    let src = "package p\nfunc f() {\n\tfor i < n {\n\t}\n\tfor j := 0; j < n; j++ {\n\t}\n\tfor {\n\t}\n}\n";
    let f = analyze("go", src);
    let conds: Vec<Vec<String>> = of(&f, Concept::Condition)
        .iter()
        .map(|m| text(&f, m, Capture::Cond))
        .collect();
    assert_eq!(conds, [vec!["i < n"], vec!["j < n"]]);
}

#[test]
fn test_go_compound_assignment_is_not_a_plain_assignment() {
    let f = analyze("go", "package p\nfunc f() {\n\tt += 1\n\tt = t + 1\n}\n");
    assert_eq!(of(&f, Concept::CompoundAssign).len(), 1);
    let assigns = of(&f, Concept::Assign);
    assert_eq!(assigns.len(), 1);
    assert_eq!(text(&f, assigns[0], Capture::Value), ["t + 1"]);
}

#[test]
fn test_go_multi_value_return_has_no_single_value() {
    let f = analyze(
        "go",
        "package p\nfunc f() (int, error) {\n\treturn 0, nil\n}\nfunc g() bool {\n\treturn true\n}\n",
    );
    let rets = of(&f, Concept::Return);
    assert_eq!(rets.len(), 1);
    assert_eq!(text(&f, rets[0], Capture::Value), ["true"]);
}

#[test]
fn test_functions_capture_name_result_type_and_body() {
    for (lang, src, name, returns) in [
        ("python", "def f(a) -> bool:\n    return a\n", "f", "bool"),
        ("rust", "pub fn f(a: u8) -> bool { a > 0 }\n", "f", "bool"),
        (
            "go",
            "package p\nfunc (c *C) F() bool { return true }\n",
            "F",
            "bool",
        ),
    ] {
        let f = analyze(lang, src);
        let func = of(&f, Concept::Function)[0];
        assert_eq!(text(&f, func, Capture::Name), [name], "{lang}");
        assert_eq!(text(&f, func, Capture::Returns), [returns], "{lang}");
        assert_eq!(func.all(Capture::Body).count(), 1, "{lang}");
    }
}
