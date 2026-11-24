mod common;

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::id::{IdParts, MutantId};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::plan::{MutantPlan, plan_file};
use cherenkov_mutation::syntax::StmtKind;
use common::{TOY_SRC, Toy, toy_file, toy_profile};

fn parts<'a>() -> IdParts<'a> {
    IdParts {
        path: "a.src",
        function: Some("f"),
        operator: "aor",
        anchor: "x := a + b;",
        edit: "e",
        occurrence: 0,
    }
}

fn plans(src: &str) -> Vec<MutantPlan> {
    plan_file(&toy_file("toy.src", src), &toy_profile(), &catalog()).plans
}

#[test]
fn test_mutant_id_is_sixteen_lowercase_hex_digits() {
    let id = MutantId::derive(&parts());
    assert_eq!(id.0.len(), 16);
    assert!(
        id.0.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{id}"
    );
}

#[test]
fn test_mutant_id_changes_with_every_part() {
    let base = MutantId::derive(&parts());
    let variants = [
        IdParts {
            path: "b.src",
            ..parts()
        },
        IdParts {
            function: Some("g"),
            ..parts()
        },
        IdParts {
            function: None,
            ..parts()
        },
        IdParts {
            operator: "ror",
            ..parts()
        },
        IdParts {
            anchor: "y := a + b;",
            ..parts()
        },
        IdParts {
            edit: "e2",
            ..parts()
        },
        IdParts {
            occurrence: 1,
            ..parts()
        },
    ];
    for v in variants {
        assert_ne!(MutantId::derive(&v), base, "{v:?}");
    }
}

#[test]
fn test_mutant_id_parts_do_not_run_together() {
    // ("ab", "c") and ("a", "bc") must not hash the same.
    let a = IdParts {
        path: "ab",
        operator: "c",
        ..parts()
    };
    let b = IdParts {
        path: "a",
        operator: "bc",
        ..parts()
    };
    assert_ne!(MutantId::derive(&a), MutantId::derive(&b));
}

#[test]
fn test_mutant_id_ignores_whitespace_in_anchor() {
    let tight = IdParts {
        anchor: "x := a + b;",
        ..parts()
    };
    let loose = IdParts {
        anchor: "x  :=\n\ta +  b;",
        ..parts()
    };
    assert_eq!(MutantId::derive(&tight), MutantId::derive(&loose));
}

#[test]
fn test_planned_ids_survive_code_inserted_above() {
    let shifted = format!("fn pre() void {{ }}\n\n{TOY_SRC}");
    let before: Vec<_> = plans(TOY_SRC).into_iter().map(|p| p.id).collect();
    let after: Vec<_> = plans(&shifted).into_iter().map(|p| p.id).collect();
    assert_eq!(before, after);
}

#[test]
fn test_planned_ids_are_unique_within_a_file() {
    let ps = plans(TOY_SRC);
    let mut ids: Vec<_> = ps.iter().map(|p| &p.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), ps.len());
}

#[test]
fn test_identical_statements_in_one_function_get_distinct_ids() {
    let src = "fn f() void {\n    log(x);\n    log(x);\n}\n";
    let file = Toy::new("dup.src", src)
        .concept(
            Concept::Function,
            src.trim_end(),
            0,
            &[(Capture::Name, "f", 1), (Capture::Returns, "void", 0)],
        )
        .stmt(StmtKind::Call, "log(x);", 0)
        .stmt(StmtKind::Call, "log(x);", 1)
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(out.plans.len(), 2);
    assert_ne!(out.plans[0].id, out.plans[1].id);
}

#[test]
fn test_same_statement_text_in_different_functions_gets_distinct_ids() {
    let src = "fn f() void { log(x); }\nfn g() void { log(x); }\n";
    let file = Toy::new("two.src", src)
        .concept(
            Concept::Function,
            "fn f() void { log(x); }",
            0,
            &[(Capture::Name, "f", 1), (Capture::Returns, "void", 0)],
        )
        .concept(
            Concept::Function,
            "fn g() void { log(x); }",
            0,
            &[(Capture::Name, "g", 1), (Capture::Returns, "void", 1)],
        )
        .stmt(StmtKind::Call, "log(x);", 0)
        .stmt(StmtKind::Call, "log(x);", 1)
        .build();
    let out = plan_file(&file, &toy_profile(), &catalog());
    assert_eq!(out.plans.len(), 2);
    assert_eq!(out.plans[0].function.as_deref(), Some("f"));
    assert_eq!(out.plans[1].function.as_deref(), Some("g"));
    assert_ne!(out.plans[0].id, out.plans[1].id);
}
