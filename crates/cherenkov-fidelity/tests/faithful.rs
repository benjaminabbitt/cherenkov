//! Fidelity is a test of Cherenkov: every run-time mutant must behave, test
//! by test, like the same mutant compiled from source. A route that fails
//! here must not be enabled. Slow; run with `just test-slow`.

use std::path::{Path, PathBuf};

use cherenkov_core::execution::MutantExecutor;
use cherenkov_core::operator::Operator;
use cherenkov_core::operator_set::named;
use cherenkov_core::plan::MutantPlan;
use cherenkov_core::session::plan_project;
use cherenkov_exec_dap::gdb::{GdbConfig, GdbExecutor};
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::toolchain::{build_rust_test_binaries, rustup_toolchain};
use cherenkov_fidelity::check::{Checked, Debugger, check};
use cherenkov_fidelity::judge::Judgement;
use cherenkov_fidelity::source::ScratchProject;
use cherenkov_fs::FsSourceReader;
use cherenkov_syntax::TreeSitterSyntax;

/// A shared fixture (`fixtures/`) or one of this crate's own.
fn fixture(name: &str) -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let own = here.join("fixtures").join(name);
    let dir = if own.exists() {
        own
    } else {
        here.join("../../fixtures").join(name)
    };
    dir.canonicalize().unwrap()
}

/// Every mutant of a Rust project under the named set, with what checking
/// it found.
fn check_rust(root: &Path, set: &str) -> Vec<(MutantPlan, Checked)> {
    let operators: Vec<Box<dyn Operator>> = named(set).unwrap().operators();
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let profiles: Vec<_> = syntax
        .profiles()
        .into_iter()
        .filter(|p| p.lang.as_str() == "rust")
        .collect();
    let reader = FsSourceReader::new(root, &profiles).unwrap();
    let plans = plan_project(&reader, &syntax, &profiles, &operators)
        .unwrap()
        .plans;
    let cargo = Path::new("cargo");
    let mut binaries = build_rust_test_binaries(cargo, root).unwrap();
    binaries.sort();
    // What the fixture's source says about its types' defaults, as the CLI
    // gathers it for a workspace.
    let mut facts = cherenkov_core::types::DefaultFacts::default();
    facts.project_crate(root.file_name().unwrap().to_str().unwrap());
    facts.project_crate("ops");
    if let Ok(src) = std::fs::read_to_string(root.join("src/lib.rs")) {
        facts.merge(cherenkov_syntax::rust_types::default_facts(&src));
    }
    let executors = binaries
        .into_iter()
        .map(|test_binary| {
            let config = GdbConfig {
                gdb: PathBuf::from("gdb"),
                test_binary,
                test_args: Vec::new(),
                cwd: None,
                env: Vec::new(),
                source_root: root.canonicalize().ok(),
            };
            Box::new(
                GdbExecutor::new(config, Isa::amd64())
                    .with_default_facts(facts.clone())
                    .with_toolchain(rustup_toolchain(root)),
            ) as Box<dyn MutantExecutor>
        })
        .collect();
    let mut debugger = Debugger::new(executors).unwrap();
    let scratch = ScratchProject::copy(root, cargo).unwrap();
    let mut out = Vec::new();
    check(
        &plans,
        &profiles[0],
        &|p| std::fs::read_to_string(root.join(&p.file)),
        &mut debugger,
        &scratch,
        &[],
        |plan, checked| out.push((plan.clone(), checked.clone())),
    )
    .unwrap();
    out
}

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_every_rust_fixture_mutant_is_faithful_to_its_source() {
    let root = fixture("rust");
    let before = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let checked = check_rust(&root, "breadth");
    assert_eq!(checked.len(), 13);
    for (plan, c) in &checked {
        assert_eq!(
            c,
            &Checked::Judged(Judgement::Faithful),
            "{}",
            plan.describe()
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        before,
        "the working tree is never written"
    );
}

/// Mutants the debugger may refuse, by (function, mutation), with why.
/// Every other refusal fails the test, and so does an entry here that is no
/// longer refused: the list says what is true now.
const ACCEPTED_REFUSALS: &[(&str, &str, &str)] = &[
    (
        "pair",
        "struct-field/delete",
        "deleting a field of a struct literal has no patch or effect (accepted gap)",
    ),
    (
        "pick",
        "fn-value",
        "Default::default() of a generic T without a Default bound does not compile",
    ),
    (
        "pool",
        "fn-value",
        "cargo-mutants' Pool::new() and Pool::from(..) do not exist, so do not compile",
    ),
    (
        "wide",
        "aor/add->sub",
        "GDB cannot evaluate a 128-bit operation for Python",
    ),
    (
        "wide",
        "aor/add->mul",
        "GDB cannot evaluate a 128-bit operation for Python",
    ),
    (
        "inc",
        "aor/add->sub",
        "`let y = x - 1`: no `y` to assign at the statement's start, and rustc's unsigned check \
         (cmp; jb) makes a tier P swap unfaithful",
    ),
    (
        "inc",
        "aor/add->mul",
        "`let y = x * 1`: no `y` to assign at the statement's start",
    ),
    (
        "evens",
        "aor/rem->div",
        "GDB cannot evaluate `*x / 2` through `&&i64` inside the closure",
    ),
    (
        "twice",
        "aor/mul->add",
        "an async fn's poll returns Poll<i64>: GDB returns only scalars",
    ),
    (
        "twice",
        "aor/mul->div",
        "an async fn's poll returns Poll<i64>: GDB returns only scalars",
    ),
    (
        "twice_plus",
        "aor/add->sub",
        "an async fn's poll returns Poll<i64>: GDB returns only scalars",
    ),
    (
        "twice_plus",
        "aor/add->mul",
        "an async fn's poll returns Poll<i64>: GDB returns only scalars",
    ),
    (
        "tags",
        "aor/add->mul",
        "a mutated expression returning a heap value: GDB returns only scalars",
    ),
    (
        "shared",
        "aor/mul->add",
        "a mutated expression returning a heap value: GDB returns only scalars",
    ),
    (
        "shared",
        "aor/mul->div",
        "a mutated expression returning a heap value: GDB returns only scalars",
    ),
    (
        "boxed",
        "aor/add->sub",
        "a mutated expression returning a heap value: GDB returns only scalars",
    ),
    (
        "boxed",
        "aor/add->mul",
        "a mutated expression returning a heap value: GDB returns only scalars",
    ),
    (
        "shared_later",
        "aor/add->sub",
        "an async fn's poll returns Poll<Arc<i64>>: GDB returns only scalars",
    ),
    (
        "shared_later",
        "aor/add->mul",
        "an async fn's poll returns Poll<Arc<i64>>: GDB returns only scalars",
    ),
    (
        "checked_later",
        "aor/add->sub",
        "an async block's poll returns Poll<Result<u32, String>>: GDB returns only scalars",
    ),
    (
        "checked_later",
        "aor/add->mul",
        "an async block's poll returns Poll<Result<u32, String>>: GDB returns only scalars",
    ),
    (
        "named_later",
        "ror/gt->lt",
        "an async block's poll returns Poll<Option<String>>: GDB returns only scalars",
    ),
    (
        "named_later",
        "ror/gt->ge",
        "an async block's poll returns Poll<Option<String>>: GDB returns only scalars",
    ),
    (
        "named_later",
        "ror/gt->eq",
        "an async block's poll returns Poll<Option<String>>: GDB returns only scalars",
    ),
];

#[test]
#[ignore = "slow: needs gdb and cargo"]
fn test_every_cargo_mutants_mutant_of_rust_ops_is_faithful_unviable_or_an_accepted_refusal() {
    // Unviable: the source mutant does not compile, as cargo-mutants finds.
    let checked = check_rust(&fixture("rust-ops"), "cargo-mutants");
    let key = |p: &cherenkov_core::plan::MutantPlan| {
        (
            p.function.clone().unwrap_or_default(),
            p.mutation.unwrap_or("?").to_string(),
        )
    };
    let accepted = |p: &cherenkov_core::plan::MutantPlan| {
        let (f, m) = key(p);
        ACCEPTED_REFUSALS
            .iter()
            .any(|&(af, am, _)| af == f && am == m)
    };
    let bad: Vec<String> = checked
        .iter()
        .filter(|(p, c)| match c {
            Checked::Judged(Judgement::Faithful | Judgement::Unviable) => false,
            Checked::Unsupported(_) => !accepted(p),
            _ => true,
        })
        .map(|(p, c)| format!("{} [{}]: {c:?}", p.describe(), p.mutation.unwrap_or("?")))
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} mutants:\n{}",
        bad.len(),
        checked.len(),
        bad.join("\n")
    );
    let refused: Vec<(String, String)> = checked
        .iter()
        .filter(|(_, c)| matches!(c, Checked::Unsupported(_)))
        .map(|(p, _)| key(p))
        .collect();
    let stale: Vec<&str> = ACCEPTED_REFUSALS
        .iter()
        .filter(|&&(f, m, _)| !refused.iter().any(|(rf, rm)| rf == f && rm == m))
        .map(|&(_, _, why)| why)
        .collect();
    assert!(
        stale.is_empty(),
        "no longer refused; drop from the list: {stale:?}"
    );
}
