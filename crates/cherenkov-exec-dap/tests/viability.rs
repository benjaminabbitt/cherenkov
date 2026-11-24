//! Whether a mutant's source compiles, decided by rustc: the pure parts
//! (batching, applying edits, reading rustc's diagnostics, settling a batch)
//! and, on a scratch crate, the whole check against `cargo check`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cherenkov_exec_dap::viability::{
    Blamed, Candidate, Diag, apply_edits, batches, blame, diagnostics, settle,
};
use cherenkov_mutation::execution::{ExecError, MutantExecutor, RawOutcome};

fn candidate(file: &str, start: usize, end: usize) -> Candidate {
    Candidate {
        id: cherenkov_mutation::id::MutantId(format!("{file}:{start}")),
        file: PathBuf::from(file),
        start,
        end,
        with: "()".into(),
    }
}

#[test]
fn test_edits_are_applied_in_order_and_each_mutant_s_text_is_found_in_the_result() {
    let (text, at) = apply_edits(
        "fn a() { 1 } fn b() { 2 }",
        &[(9, 10, "xx"), (22, 23, "yyyy")],
    );
    assert_eq!(text, "fn a() { xx } fn b() { yyyy }");
    assert_eq!(at, [9..11, 23..27]);
    assert_eq!(&text[at[1].clone()], "yyyy");
}

#[test]
fn test_mutants_that_overlap_never_share_a_batch() {
    let all = [
        candidate("a.rs", 0, 100),
        candidate("a.rs", 10, 20),
        candidate("a.rs", 30, 40),
        candidate("b.rs", 10, 20),
        candidate("a.rs", 100, 120),
    ];
    let got = batches(&all);
    // The nested ones wait for a later batch; touching ones may share.
    assert_eq!(got, vec![vec![0, 3, 4], vec![1, 2]]);
}

#[test]
fn test_no_candidates_make_no_batch() {
    assert!(batches(&[]).is_empty());
}

const ERROR: &str = r#"{"reason":"compiler-artifact","target":{"name":"x"}}
{"reason":"compiler-message","message":{"level":"warning","message":"unused","spans":[{"file_name":"src/lib.rs","byte_start":5,"byte_end":6,"is_primary":true,"expansion":null}]}}
{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","code":{"code":"E0308"},"spans":[{"file_name":"src/lib.rs","byte_start":30,"byte_end":34,"is_primary":true,"expansion":null},{"file_name":"src/lib.rs","byte_start":1,"byte_end":3,"is_primary":false,"expansion":null}]}}
{"reason":"compiler-message","message":{"level":"error","message":"in macro","code":null,"spans":[{"file_name":"/reg/m.rs","byte_start":1,"byte_end":9,"is_primary":true,"expansion":{"span":{"file_name":"src/b.rs","byte_start":50,"byte_end":60,"is_primary":false,"expansion":null}}}]}}
{"reason":"compiler-message","message":{"level":"error","message":"aborting due to 2 previous errors","code":null,"spans":[]}}
{"reason":"build-finished","success":false}
not json
"#;

#[test]
fn test_diagnostics_are_the_errors_with_their_primary_spans_and_macro_call_sites() {
    let got = diagnostics(ERROR);
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].message, "error[E0308]: mismatched types");
    assert_eq!(got[0].spans, [(PathBuf::from("src/lib.rs"), 30..34)]);
    assert_eq!(got[1].message, "error: in macro");
    assert_eq!(
        got[1].spans,
        [
            (PathBuf::from("/reg/m.rs"), 1..9),
            (PathBuf::from("src/b.rs"), 50..60)
        ]
    );
}

fn at(file: &str, range: std::ops::Range<usize>) -> (PathBuf, std::ops::Range<usize>) {
    (PathBuf::from(file), range)
}

#[test]
fn test_an_error_inside_a_mutants_text_is_its_and_one_outside_is_nobody_s() {
    let diags = diagnostics(ERROR);
    let regions = [at("src/lib.rs", 28..36), at("src/b.rs", 70..80)];
    assert_eq!(
        blame(&diags, &regions),
        Blamed {
            by_mutant: vec![(0, "error[E0308]: mismatched types".into())],
            elsewhere: Some("error: in macro".into()),
        }
    );
}

#[test]
fn test_an_error_only_partly_inside_a_mutants_text_is_nobody_s() {
    let diags = diagnostics(ERROR);
    let regions = [at("src/lib.rs", 32..40)];
    let got = blame(&diags, &regions);
    assert!(got.by_mutant.is_empty(), "{got:?}");
    assert!(got.elsewhere.is_some());
}

#[test]
fn test_a_macro_s_call_site_inside_a_mutants_text_blames_that_mutant() {
    let diags = diagnostics(ERROR);
    let got = blame(&diags, &[at("src/b.rs", 45..65)]);
    assert_eq!(got.by_mutant, [(0, "error: in macro".to_string())]);
}

/// A crate in which exactly `bad` do not compile, on its own or together.
fn settled(n: usize, bad: &[usize], calls: &mut Vec<Vec<usize>>) -> Vec<(usize, String)> {
    settle(n, &mut |subset: &[usize]| {
        calls.push(subset.to_vec());
        let broken: Vec<(usize, String)> = subset
            .iter()
            .filter(|i| bad.contains(i))
            .map(|i| (*i, format!("bad {i}")))
            .collect();
        Ok(if broken.is_empty() {
            Blamed::default()
        } else {
            Blamed {
                by_mutant: broken,
                elsewhere: None,
            }
        })
    })
    .unwrap()
}

#[test]
fn test_a_batch_that_compiles_is_one_compile_and_no_rejection() {
    let mut calls = vec![];
    assert!(settled(5, &[], &mut calls).is_empty());
    assert_eq!(calls, [vec![0, 1, 2, 3, 4]]);
}

#[test]
fn test_the_blamed_are_rejected_and_the_rest_checked_again_without_them() {
    let mut calls = vec![];
    let got = settled(5, &[1, 3], &mut calls);
    assert_eq!(got, [(1, "bad 1".to_string()), (3, "bad 3".to_string())]);
    assert_eq!(calls, [vec![0, 1, 2, 3, 4], vec![0, 2, 4]]);
}

#[test]
fn test_an_error_nobody_owns_bisects_the_batch_down_to_the_one_that_causes_it() {
    // Mutant 2 alone breaks the crate, but its error lies outside its text.
    let mut calls = vec![];
    let got = settle(4, &mut |subset: &[usize]| {
        calls.push(subset.to_vec());
        Ok(if subset.contains(&2) {
            Blamed {
                by_mutant: vec![],
                elsewhere: Some("error: elsewhere".into()),
            }
        } else {
            Blamed::default()
        })
    })
    .unwrap();
    assert_eq!(got, [(2, "error: elsewhere".to_string())]);
    assert_eq!(
        calls,
        [vec![0, 1, 2, 3], vec![0, 1], vec![2, 3], vec![2], vec![3]]
    );
}

#[test]
fn test_a_failed_check_ends_the_settling() {
    let got = settle(2, &mut |_: &[usize]| Err(ExecError("no cargo".into())));
    assert_eq!(got, Err(ExecError("no cargo".into())));
}

// --- The executor that reports what rustc rejects.

use cherenkov_mutation::execution::Budget;

struct Fixed(RawOutcome, std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl MutantExecutor for Fixed {
    fn run(
        &mut self,
        _: &cherenkov_mutation::plan::MutantPlan,
        _: Budget,
    ) -> Result<RawOutcome, ExecError> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.0.clone())
    }
    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(self.0.clone())
    }
}

fn plan_of(id: &str) -> cherenkov_mutation::plan::MutantPlan {
    use cherenkov_mutation::ports::SyntaxPort;
    let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
    let rust = cherenkov_mutation::syntax::LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let src = "pub fn f() -> bool { true }\n";
    let file = syntax
        .analyze(
            &rust,
            std::path::Path::new("lib.rs"),
            std::sync::Arc::from(src),
        )
        .unwrap();
    let mut plan = cherenkov_mutation::plan::plan_file(
        &file,
        &profile,
        &cherenkov_mutation::operator::catalog(),
    )
    .plans
    .remove(0);
    plan.id = cherenkov_mutation::id::MutantId(id.into());
    plan
}

#[test]
fn test_what_rustc_rejects_is_unviable_and_never_run_and_the_rest_is_left_alone() {
    use cherenkov_exec_dap::viability::RustcRejects;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let rejected = std::sync::Arc::new(HashMap::from([(
        cherenkov_mutation::id::MutantId("bad".into()),
        "error[E0599]: no function".to_string(),
    )]));
    let killed = RawOutcome::Exited {
        code: 1,
        tier: cherenkov_mutation::execution::Tier::P,
        wall: std::time::Duration::ZERO,
    };
    let runs = std::sync::Arc::new(AtomicUsize::new(0));
    let mut executor = RustcRejects::new(Fixed(killed.clone(), runs.clone()), rejected);
    let budget = Budget::for_mutant(std::time::Duration::ZERO);
    let got = executor.run(&plan_of("bad"), budget).unwrap();
    assert!(
        matches!(&got, RawOutcome::Unviable { reason } if reason.contains("rustc rejects") && reason.contains("E0599")),
        "{got:?}"
    );
    let (got, output) = executor.run_with_output(&plan_of("bad"), budget).unwrap();
    assert!(matches!(got, RawOutcome::Unviable { .. }) && output.is_empty());
    assert_eq!(runs.load(Ordering::SeqCst), 0, "no debugger for it");
    // A mutant rustc accepts is the inner executor's.
    assert_eq!(executor.run(&plan_of("fine"), budget).unwrap(), killed);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

// --- The whole check, on a scratch workspace and the real `cargo check`.

const LIB: &str = "#![deny(warnings)]
pub struct Foo(pub u8);
pub fn a() -> u32 { 1 }
pub fn b() -> Foo { Foo(1) }
pub fn c() -> String { String::from(\"c\") }
pub fn d() -> u32 { 4 }
";

fn candidate_at(lib: &str, body: &str, with: &str) -> Candidate {
    let start = lib.find(body).unwrap();
    Candidate {
        id: cherenkov_mutation::id::MutantId(body.to_string()),
        file: PathBuf::from("src/lib.rs"),
        start,
        end: start + body.len(),
        with: with.to_string(),
    }
}

fn id(s: &str) -> cherenkov_mutation::id::MutantId {
    cherenkov_mutation::id::MutantId(s.to_string())
}

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cherenkov-via-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("member/src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("member/Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("member/src/lib.rs"), LIB).unwrap();
    dir
}

#[test]
fn test_cargo_check_rejects_only_the_mutants_that_do_not_compile() {
    use cherenkov_exec_dap::viability::check;
    let dir = workspace("check");
    let cs = [
        candidate_at(LIB, "{ 1 }", "{ Default::default() }"),
        candidate_at(LIB, "{ Foo(1) }", "{ Default::default() }"),
        candidate_at(LIB, "{ String::from(\"c\") }", "{ String::new() }"),
        candidate_at(LIB, "{ 4 }", "{ Foo::new() }"),
        // Warnings that the project denies do not make a mutant unviable.
        candidate_at(LIB, "{ 4 }\n", "{ let unused = 5; 2 }\n"),
    ];
    let root = dir.join("member");
    let got = check(Path::new("cargo"), &dir, &root, &[], &cs[..4]).unwrap();
    let mut rejected: Vec<_> = got.rejected.keys().map(|k| k.0.as_str()).collect();
    rejected.sort();
    assert_eq!(rejected, ["{ 4 }", "{ Foo(1) }"], "{:?}", got.rejected);
    assert!(
        got.rejected[&id("{ Foo(1) }")].contains("E0277"),
        "{:?}",
        got.rejected
    );
    assert!(
        got.rejected[&id("{ 4 }")].contains("E0599"),
        "{:?}",
        got.rejected
    );
    // One compile with all four, one without the two blamed, and the check
    // that the unmutated crate passes.
    assert_eq!(got.compiles, 3);
    // The project itself is untouched, and a mutant is no mutant afterwards.
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        LIB
    );
    let warned = check(Path::new("cargo"), &dir, &root, &[], &cs[4..]).unwrap();
    assert!(warned.rejected.is_empty(), "{:?}", warned.rejected);
    assert_eq!(warned.compiles, 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_project_that_does_not_pass_cargo_check_is_not_checked() {
    use cherenkov_exec_dap::viability::check;
    let dir = workspace("broken");
    let broken = format!("{LIB}pub fn e() -> u8 {{ nope }}\n");
    std::fs::write(dir.join("member/src/lib.rs"), &broken).unwrap();
    let cs = [candidate_at(&broken, "{ 4 }", "{ 5 }")];
    let err = check(Path::new("cargo"), &dir, &dir.join("member"), &[], &cs).unwrap_err();
    assert!(err.0.contains("does not pass cargo check"), "{err}");
    assert!(err.0.contains("could not compile"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- Which plans are checked: a function body's replacement whose text is
// still in the file where the plan found it.

fn fn_value_plan(src: &str) -> cherenkov_mutation::plan::MutantPlan {
    use cherenkov_mutation::ports::SyntaxPort;
    let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
    let rust = cherenkov_mutation::syntax::LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let file = syntax
        .analyze(&rust, Path::new("lib.rs"), std::sync::Arc::from(src))
        .unwrap();
    cherenkov_mutation::plan::plan_file(
        &file,
        &profile,
        &cherenkov_mutation::operator_set::named("cargo-mutants")
            .unwrap()
            .operators(),
    )
    .plans
    .into_iter()
    .find(|p| p.operator == "fn-value")
    .unwrap()
}

#[test]
fn test_only_a_body_replacement_still_in_its_file_is_a_candidate() {
    use cherenkov_exec_dap::viability::candidates;
    let src = "pub fn f() -> u32 { 1 + 2 }\n";
    let plan = fn_value_plan(src);
    let dir = std::env::temp_dir().join(format!("cherenkov-via-cand-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.rs"), src).unwrap();
    let mut other = plan.clone();
    other.operator = "aor";
    other.id = id("other");
    let mut unspelled = plan.clone();
    unspelled.mutated = None;
    let got = candidates(&dir, &[plan.clone(), other, unspelled]);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].id, plan.id);
    assert_eq!(got[0].file, PathBuf::from("lib.rs"));
    assert_eq!(Some(got[0].with.as_str()), plan.mutated.as_deref());
    assert_eq!(&src[got[0].start..got[0].end], plan.original);
    // The file has changed since the plan: its offsets mean nothing.
    std::fs::write(dir.join("lib.rs"), "pub fn f() -> u32 { 2 + 2 }\n").unwrap();
    assert!(candidates(&dir, &[plan]).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_two_errors_in_one_mutants_text_blame_it_once() {
    let at = |m: &str| Diag {
        message: m.into(),
        spans: vec![(PathBuf::from("a.rs"), 5..6)],
    };
    let got = blame(
        &[at("first"), at("second")],
        &[(PathBuf::from("a.rs"), 0..9)],
    );
    assert_eq!(got.by_mutant, [(0, "first".to_string())]);
    assert_eq!(got.elsewhere, None);
}

/// An executor that remembers what it was asked.
#[derive(Default)]
struct Asked {
    selected: Option<Vec<String>>,
    closed: bool,
}

struct Spy(std::sync::Arc<std::sync::Mutex<Asked>>);

impl MutantExecutor for Spy {
    fn run(
        &mut self,
        _: &cherenkov_mutation::plan::MutantPlan,
        _: Budget,
    ) -> Result<RawOutcome, ExecError> {
        Ok(RawOutcome::NotReached { reason: "r".into() })
    }
    fn baseline(&mut self, _: Budget) -> Result<RawOutcome, ExecError> {
        Ok(RawOutcome::NotReached { reason: "b".into() })
    }
    fn driven(
        &mut self,
        plans: &[cherenkov_mutation::plan::MutantPlan],
        _: Budget,
    ) -> Result<Option<Vec<cherenkov_mutation::execution::Driven>>, ExecError> {
        Ok(Some(vec![
            cherenkov_mutation::execution::Driven::Every;
            plans.len()
        ]))
    }
    fn select_tests(&mut self, tests: Option<&[String]>) {
        self.0.lock().unwrap().selected = tests.map(<[String]>::to_vec);
    }
    fn describe(&self) -> Option<String> {
        Some("spy".into())
    }
    fn close_session(&mut self) {
        self.0.lock().unwrap().closed = true;
    }
}

#[test]
fn test_everything_else_is_the_inner_executors() {
    use cherenkov_exec_dap::viability::RustcRejects;
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Asked::default()));
    let mut executor = RustcRejects::new(Spy(asked.clone()), Default::default());
    let budget = Budget::for_mutant(std::time::Duration::ZERO);
    let plan = plan_of("p");
    assert_eq!(executor.describe().as_deref(), Some("spy"));
    assert_eq!(
        executor.baseline(budget).unwrap(),
        RawOutcome::NotReached { reason: "b".into() }
    );
    assert_eq!(executor.baseline_with_output(budget).unwrap().1, "");
    assert_eq!(
        executor
            .driven(std::slice::from_ref(&plan), budget)
            .unwrap(),
        Some(vec![cherenkov_mutation::execution::Driven::Every])
    );
    assert_eq!(
        executor
            .recording(&[plan], budget)
            .unwrap()
            .driven
            .map(|d| d.len()),
        Some(1)
    );
    executor.select_tests(Some(&["t".to_string()]));
    executor.close_session();
    let asked = asked.lock().unwrap();
    assert_eq!(asked.selected, Some(vec!["t".to_string()]));
    assert!(asked.closed);
}
