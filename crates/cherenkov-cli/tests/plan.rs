//! M0 exit check: `cherenkov plan <dir>` lists mutants for Python, Rust and
//! Go. Runs the real binary against fixtures with hand-labelled expectations.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn cherenkov(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .args(args)
        .output()
        .unwrap()
}

/// `cherenkov plan dir extra`, as text unless `extra` names a format.
fn plan(dir: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["plan", dir.to_str().unwrap()];
    args.extend_from_slice(extra);
    if !extra.contains(&"--format") {
        args.extend_from_slice(&["--format", "text"]);
    }
    let out = cherenkov(&args);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Each stdout line is `<id> <describe>`; split them.
fn listing(out: &Output) -> Vec<(String, String)> {
    String::from_utf8(out.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| {
            let (id, rest) = l.split_once(' ').unwrap();
            (id.to_string(), rest.to_string())
        })
        .collect()
}

fn expected(lang: &str) -> Vec<String> {
    std::fs::read_to_string(fixtures().join(lang).join("expected-plan.txt"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn assert_golden(lang: &str) {
    let got: Vec<String> = listing(&plan(&fixtures().join(lang), &["--operators", "breadth"]))
        .into_iter()
        .map(|(_, d)| d)
        .collect();
    assert_eq!(got, expected(lang));
}

#[test]
fn test_plan_python_fixture_matches_hand_labelled_listing() {
    assert_golden("python");
}

#[test]
fn test_plan_rust_fixture_matches_hand_labelled_listing() {
    assert_golden("rust");
}

#[test]
fn test_plan_go_fixture_matches_hand_labelled_listing() {
    assert_golden("go");
}

#[test]
fn test_plan_lines_start_with_sixteen_hex_digit_unique_ids() {
    let ids: Vec<String> = listing(&plan(&fixtures().join("go"), &[]))
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for id in &ids {
        assert!(
            id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()),
            "{id}"
        );
    }
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn test_plan_is_identical_across_runs() {
    let dir = fixtures().join("rust");
    assert_eq!(plan(&dir, &[]).stdout, plan(&dir, &[]).stdout);
}

#[test]
fn test_plan_over_mixed_tree_plans_every_language_with_prefixed_paths() {
    let all = listing(&plan(&fixtures(), &["--operators", "breadth"]));
    assert_eq!(
        all.len(),
        expected("go").len() + expected("python").len() + expected("rust").len()
    );
    for lang in ["go", "python", "rust"] {
        for line in expected(lang) {
            let prefixed = format!("{lang}/{line}");
            assert!(
                all.iter().any(|(_, d)| *d == prefixed),
                "missing {prefixed}"
            );
        }
    }
}

#[test]
fn test_plan_summary_on_stderr_reports_counts_and_unmapped_concepts() {
    let out = plan(&fixtures(), &["--operators", "breadth"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("python: 12 mutants"), "{stderr}");
    assert!(stderr.contains("rust: 13 mutants"), "{stderr}");
    assert!(stderr.contains("go: 15 mutants"), "{stderr}");
    assert!(stderr.contains("unmapped: expr, name, block"), "{stderr}");
    assert!(
        stderr.contains("unrepresentable: binary 1"),
        "python chained comparison: {stderr}"
    );
    assert!(stderr.contains("concepts mapped 13/16"), "{stderr}");
}

#[test]
fn test_plan_summary_omits_sections_with_nothing_to_report() {
    // The Go fixture has no unrepresentable, skipped, disabled or deduplicated edits.
    let out = plan(&fixtures().join("go"), &[]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        stderr.trim_end(),
        "operators: breadth\ngo: 15 mutants; concepts mapped 13/16; unmapped: expr, name, block"
    );
}

/// Each line of `out`'s stdout, parsed: each must be one JSON document.
fn documents(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8(out.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

#[test]
fn test_plan_json_is_the_dry_run_s_events_the_run_started_then_each_mutant_planned() {
    let out = plan(&fixtures().join("go"), &["--format", "json"]);
    let docs = documents(&out);
    assert_eq!(docs[0]["event"], "run.started");
    assert_eq!(docs[0]["mutants"], expected("go").len());
    assert_eq!(docs[0]["language"], "go");
    let planned = &docs[1..];
    assert_eq!(planned.len(), expected("go").len());
    assert!(planned.iter().all(|d| d["event"] == "mutant.planned"));
    let first = &planned[0];
    assert_eq!(first["operator"], "body-default");
    assert!(
        first["description"]
            .as_str()
            .unwrap()
            .starts_with("return-default"),
        "{first}"
    );
    assert_eq!(first["id"].as_str().unwrap().len(), 16);
    assert!(
        first["original"].as_str().unwrap().contains("return"),
        "{first}"
    );
}

#[test]
fn test_plan_json_lists_the_mutants_the_text_listing_does() {
    let dir = fixtures().join("go");
    let ids = |out: &Output| -> Vec<String> {
        let mut ids: Vec<String> = documents(out)
            .iter()
            .filter(|d| d["event"] == "mutant.planned")
            .map(|d| d["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    let mut text: Vec<String> = listing(&plan(&dir, &[]))
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    text.sort();
    assert_eq!(ids(&plan(&dir, &["--format", "json"])), text);
}

#[test]
fn test_plan_writes_json_when_stdout_is_not_a_terminal() {
    let out = cherenkov(&["plan", fixtures().join("go").to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(documents(&out)[0]["event"], "run.started");
}

/// The error document that ends `out`'s stderr, after any progress lines.
fn error_document(out: &Output) -> serde_json::Value {
    let stderr = String::from_utf8(out.stderr.clone()).unwrap();
    let start = stderr
        .find("\n{")
        .map(|i| i + 1)
        .unwrap_or(if stderr.starts_with('{') {
            0
        } else {
            stderr.len()
        });
    serde_json::from_str(&stderr[start..]).unwrap_or_else(|e| panic!("{stderr}: {e}"))
}

#[test]
fn test_an_error_under_json_is_its_code_args_and_message_on_stderr() {
    let out = cherenkov(&["plan", "/no/such/dir", "--format", "json"]);
    assert_eq!(out.status.code(), Some(10), "it tried, and something broke");
    assert!(out.stdout.is_empty());
    let error = error_document(&out);
    assert_eq!(error["code"], "cherenkov-cli-unreadable");
    assert_eq!(error["args"]["path"], "/no/such/dir");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .starts_with("/no/such/dir: "),
        "{error}"
    );
}

#[test]
fn test_a_request_the_state_forbids_is_declined() {
    let out = cherenkov(&[
        "plan",
        fixtures().join("go").to_str().unwrap(),
        "--operators",
        "nope",
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(20));
    let error = error_document(&out);
    assert_eq!(error["code"], "cherenkov-cli-unknown-operator-set");
    assert_eq!(error["args"]["name"], "nope");
}

#[test]
fn test_plan_missing_dir_fails_with_message() {
    let out = cherenkov(&["plan", "/no/such/dir", "--format", "text"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("/no/such/dir"));
}

#[test]
fn test_plan_exclude_drops_matching_files_for_every_language() {
    let all = listing(&plan(&fixtures(), &["--operators", "breadth"]));
    let some = listing(&plan(
        &fixtures(),
        &[
            "--operators",
            "breadth",
            "--exclude",
            "**/calc.go",
            "--exclude",
            "python/**",
        ],
    ));
    assert!(
        all.iter().any(|(_, d)| d.starts_with("go/"))
            && all.iter().any(|(_, d)| d.starts_with("python/"))
    );
    assert!(some.iter().all(|(_, d)| d.starts_with("rust/")), "{some:?}");
    assert_eq!(some.len(), expected("rust").len());
}

#[test]
fn test_plan_rejects_an_invalid_exclude_glob() {
    let out = cherenkov(&["plan", fixtures().to_str().unwrap(), "--exclude", "a["]);
    assert!(!out.status.success());
}

#[test]
fn test_sets_lists_every_named_set_with_its_pending_rules() {
    let out = cherenkov(&["sets"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for name in [
        "breadth (default for the other languages)",
        "cargo-mutants (default for rust)",
        "sufficient:",
    ] {
        assert!(text.contains(name), "{name} in {text}");
    }
    assert!(text.contains("pending: unary operator deletion"), "{text}");
}

#[test]
fn test_sets_and_mutations_list_other_tools_names_beside_the_canonical_ones() {
    let sets = String::from_utf8(cherenkov(&["sets"]).stdout).unwrap();
    assert!(
        sets.contains("    ror (Mothra ROR): lt->le (PIT CONDITIONALS_BOUNDARY)"),
        "{sets}"
    );
    let muts = String::from_utf8(cherenkov(&["mutations"]).stdout).unwrap();
    assert!(
        muts.contains("\nror: Replace a relational operator"),
        "{muts}"
    );
    assert!(muts.contains("  also called: Mothra ROR\n"), "{muts}");
    assert!(
        muts.contains("ror/lt->le\n  also called: PIT CONDITIONALS_BOUNDARY"),
        "{muts}"
    );
}

#[test]
fn test_plan_defaults_to_cargo_mutants_for_rust_and_breadth_for_go() {
    let rust = plan(&fixtures().join("rust"), &[]);
    assert!(String::from_utf8_lossy(&rust.stderr).starts_with("operators: cargo-mutants\n"));
    let explicit = plan(&fixtures().join("rust"), &["--operators", "cargo-mutants"]);
    assert_eq!(listing(&rust), listing(&explicit));
    let go = plan(&fixtures().join("go"), &[]);
    let breadth = plan(&fixtures().join("go"), &["--operators", "breadth"]);
    assert_eq!(listing(&go), listing(&breadth));
}

#[test]
fn test_plan_of_a_project_with_two_languages_uses_each_languages_default() {
    let dir = std::env::temp_dir().join(format!("cherenkov-twolang-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn f(a: i32) -> i32 { a + 1 }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("a.go"),
        "package a\nfunc F(a int) int { return a + 1 }\n",
    )
    .unwrap();
    let out = plan(&dir, &[]);
    std::fs::remove_dir_all(&dir).unwrap();
    let lines: Vec<String> = listing(&out).into_iter().map(|(_, d)| d).collect();
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("operators: go=breadth,rust=cargo-mutants\n"),
        "{err}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("a.go") && l.contains(" aor ")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("lib.rs") && l.contains("fn-value")),
        "{lines:?}"
    );
}

#[test]
fn test_a_custom_set_naming_an_alias_fails_naming_the_canonical_names() {
    let dir = std::env::temp_dir().join(format!("cherenkov-alias-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("s.toml");
    std::fs::write(&file, "name = \"mine\"\nmutations = [\"Mothra ROR\"]\n").unwrap();
    let out = cherenkov(&[
        "plan",
        fixtures().join("rust").to_str().unwrap(),
        "--operators",
        file.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Mothra ROR is another tool's name for ror"),
        "{err}"
    );
}

#[test]
fn test_sets_lists_each_sets_operators_with_their_tables() {
    let out = cherenkov(&["sets"]);
    let text = String::from_utf8(out.stdout).unwrap();
    // The cargo-mutants set's `lcr` table is its own; `sufficient`'s differs.
    assert!(text.contains("  operators (11):\n"), "{text}");
    assert!(
        text.contains("    lcr: and->or (Mothra LCR) or->and (Mothra LCR)\n"),
        "{text}"
    );
    assert!(text.contains("    lcr: and->eq or->ne\n"), "{text}");
    assert!(text.contains("    compound-assign: add->sub"), "{text}");
}

#[test]
fn test_plan_with_cargo_mutants_set_generates_its_relational_replacements() {
    let lines: Vec<String> = listing(&plan(
        &fixtures().join("rust"),
        &["--operators", "cargo-mutants"],
    ))
    .into_iter()
    .map(|(_, d)| d)
    .collect();
    // is_small: `if n < 10` -> `==`, `>`, `<=` (cargo-mutants src/visit.rs:580).
    for want in [
        r#"src/lib.rs:10:8 is_small ror eval-branch "n == 10""#,
        r#"src/lib.rs:10:8 is_small ror eval-branch "n > 10""#,
        r#"src/lib.rs:10:8 is_small ror eval-branch "n <= 10""#,
    ] {
        assert!(
            lines.iter().any(|l| l == want),
            "missing {want} in {lines:#?}"
        );
    }
    assert!(
        !lines.iter().any(|l| l.contains("negate-condition")),
        "not a cargo-mutants operator"
    );
}

#[test]
fn test_plan_with_an_unknown_set_fails_naming_the_known_ones() {
    let out = cherenkov(&[
        "plan",
        fixtures().join("rust").to_str().unwrap(),
        "--operators",
        "nope",
        "--format",
        "text",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nope") && stderr.contains("cargo-mutants"),
        "{stderr}"
    );
}

#[test]
fn test_mutations_lists_each_mutation_with_where_it_came_from_and_its_peers() {
    let out = cherenkov(&["mutations"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("ror/lt->le"), "{text}");
    assert!(
        text.contains("\nror: Replace a relational operator"),
        "{text}"
    );
    assert!(text.contains("operators: 15"), "{text}");
    assert!(text.contains("derived from cargo-mutants\n"), "{text}");
    assert!(text.contains("same as PIT CONDITIONALS_BOUNDARY"), "{text}");
}

#[test]
fn test_plan_summary_counts_skipped_regions_by_kind() {
    let dir = std::env::temp_dir().join(format!("cherenkov-skip-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn f(a: i32) -> i32 { a + 1 }\n#[test]\nfn t() { let _ = 1 + 1; }\n",
    )
    .unwrap();
    let out = plan(&dir, &["--operators", "cargo-mutants"]);
    std::fs::remove_dir_all(&dir).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("skipped regions: test "), "{stderr}");
    assert!(!stderr.contains("no action for context"), "{stderr}");
}

#[test]
fn test_plan_in_diff_keeps_only_the_mutants_touching_an_added_line() {
    let dir = std::env::temp_dir().join(format!("cherenkov-indiff-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn f(a: i32) -> i32 {\n    a + 1\n}\npub fn g(a: i32) -> i32 {\n    a * 2\n}\n",
    )
    .unwrap();
    let diff = dir.join("change.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -4,2 +4,3 @@\n pub fn g(a: i32) -> i32 {\n+    // a line added\n     a * 2\n",
    )
    .unwrap();
    // The diff adds line 5, inside `g`: its mutants stay, `f`'s go.
    let all = plan(&dir, &["--operators", "cargo-mutants"]);
    let kept = plan(
        &dir,
        &[
            "--operators",
            "cargo-mutants",
            "--in-diff",
            diff.to_str().unwrap(),
        ],
    );
    std::fs::remove_dir_all(&dir).unwrap();
    let (all, kept) = (listing(&all), listing(&kept));
    assert!(all.iter().any(|(_, d)| d.contains(" f ")), "{all:?}");
    assert!(!kept.is_empty());
    assert!(kept.iter().all(|(_, d)| d.contains(" g ")), "{kept:?}");
}

// --- Several operator sets at once: `--operators a,b,file.toml`.

fn mutant_count(dir: &Path, operators: &str) -> usize {
    listing(&plan(dir, &["--operators", operators])).len()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn test_plan_with_two_sets_lists_their_union_once_and_records_provenance() {
    let dir = fixtures().join("rust");
    let (a, b) = (
        mutant_count(&dir, "breadth"),
        mutant_count(&dir, "cargo-mutants"),
    );
    let out = plan(&dir, &["--operators", "breadth,cargo-mutants"]);
    let both = listing(&out).len();
    assert!(
        both > b.max(a) && both < a + b,
        "{a} + {b} overlap into {both}"
    );
    let err = stderr_of(&out);
    assert!(err.contains("operators: breadth,cargo-mutants\n"), "{err}");
    let shared = a + b - both;
    assert!(
        err.contains(&format!(
            "provenance: breadth {a}, cargo-mutants {b}; in more than one set: {shared}\n"
        )),
        "{err}"
    );
    let ids: std::collections::HashSet<_> = listing(&out).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids.len(), both, "ids stay unique");
}

#[test]
fn test_plan_with_a_custom_set_file_adds_its_mutations() {
    let dir = fixtures().join("rust");
    let file = std::env::temp_dir().join(format!("cherenkov-set-{}.toml", std::process::id()));
    std::fs::write(&file, "name = \"mine\"\nmutations = [\"ror/lt->le\"]\n").unwrap();
    let out = plan(
        &dir,
        &["--operators", &format!("breadth,{}", file.display())],
    );
    std::fs::remove_file(&file).unwrap();
    let err = stderr_of(&out);
    assert!(err.contains("provenance: breadth"), "{err}");
    assert!(err.contains("mine "), "{err}");
}

#[test]
fn test_plan_with_a_bad_custom_set_is_declined_naming_the_problem() {
    let dir = fixtures().join("rust");
    let file = std::env::temp_dir().join(format!("cherenkov-bad-{}.toml", std::process::id()));
    std::fs::write(&file, "name = \"mine\"\nmutations = [\"ror/zzz\"]\n").unwrap();
    let out = cherenkov(&[
        "plan",
        dir.to_str().unwrap(),
        "--operators",
        file.to_str().unwrap(),
        "--format",
        "json",
    ]);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(out.status.code(), Some(20));
    let error = error_document(&out);
    assert_eq!(error["code"], "cherenkov-cli-bad-operator-set");
    assert_eq!(error["args"]["reason"], "no mutation named ror/zzz");
}
