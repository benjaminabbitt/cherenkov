//! `cherenkov operators`: the operator inventory as JSON (a contract: the
//! keys below are documented and stable) and as Markdown.

use std::process::{Command, Output};

use serde_json::Value;

fn cherenkov(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cherenkov"))
        .args(args)
        .output()
        .unwrap()
}

fn json(args: &[&str]) -> Value {
    let out = cherenkov(args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort();
    k
}

#[test]
fn test_operators_json_has_the_documented_keys_at_every_level() {
    let doc = json(&["operators", "--format", "json"]);
    assert_eq!(keys(&doc), ["operators", "schema", "set"]);
    assert_eq!(doc["schema"], 1);
    assert!(doc["set"].is_null());
    let ops = doc["operators"].as_array().unwrap();
    assert_eq!(ops.len(), 15);
    assert_eq!(
        keys(&ops[0]),
        ["aliases", "definition", "name", "replacements", "sources"]
    );
    let r = &ops[0]["replacements"][0];
    assert_eq!(
        keys(r),
        [
            "aliases",
            "definition",
            "detail",
            "name",
            "origins",
            "sets",
            "spellings"
        ]
    );
    assert_eq!(
        keys(&r["origins"][0]),
        ["name", "note", "relation", "source"]
    );
    assert_eq!(r["name"], "ror/lt->le");
    assert_eq!(
        keys(&r["spellings"]["rust"]),
        ["cargo_mutants", "from", "with"]
    );
    assert_eq!(r["spellings"]["rust"]["cargo_mutants"], "replace < with <=");
    assert_eq!(r["spellings"]["go"]["from"], "<");
}

#[test]
fn test_operators_json_without_spellings_has_none() {
    let doc = json(&["operators", "--format", "json", "--no-spellings"]);
    let r = &doc["operators"][0]["replacements"][0];
    assert_eq!(r["spellings"], serde_json::json!({}));
}

#[test]
fn test_operators_for_one_set_has_its_families_and_its_tables() {
    let doc = json(&["operators", "--format", "json", "--set", "sufficient"]);
    assert_eq!(keys(&doc["set"]), ["name", "pending", "source", "summary"]);
    assert_eq!(doc["set"]["name"], "sufficient");
    let lcr = doc["operators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "lcr")
        .unwrap();
    let names: Vec<&str> = lcr["replacements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["lcr/and->eq", "lcr/or->ne"]);
}

#[test]
fn test_operators_markdown_is_the_default_and_names_the_scheme() {
    let out = cherenkov(&["operators"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("| `compound-assign` |"), "{text}");
    assert!(text.contains("### `lcr`"), "{text}");
    assert!(text.contains("`family/detail`"), "{text}");
}

#[test]
fn test_operators_for_an_unknown_set_fails() {
    assert!(!cherenkov(&["operators", "--set", "nope"]).status.success());
}

#[test]
fn test_every_name_in_the_dump_is_accepted_in_a_custom_set_file() {
    let doc = json(&["operators", "--format", "json"]);
    let mut names: Vec<String> = Vec::new();
    for o in doc["operators"].as_array().unwrap() {
        names.push(o["name"].as_str().unwrap().to_string());
        for r in o["replacements"].as_array().unwrap() {
            names.push(r["name"].as_str().unwrap().to_string());
        }
    }
    let dir = std::env::temp_dir().join(format!("cherenkov-opnames-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn f(a: i32) -> i32 { a + 1 }\n",
    )
    .unwrap();
    let list: Vec<String> = names.iter().map(|n| format!("{n:?}")).collect();
    let file = dir.join("all.toml");
    std::fs::write(
        &file,
        format!("name = \"all\"\nmutations = [{}]\n", list.join(", ")),
    )
    .unwrap();
    let out = cherenkov(&[
        "plan",
        dir.to_str().unwrap(),
        "--operators",
        file.to_str().unwrap(),
        "--format",
        "text",
    ]);
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(out.status.success(), "{err}");
}

/// Every `<!-- generated: cherenkov ARGS -->` ... `<!-- /generated -->`
/// block of the docs: the arguments and the text between the markers.
fn generated_blocks(doc: &str) -> Vec<(Vec<String>, String)> {
    let mut blocks = Vec::new();
    let mut rest = doc;
    while let Some(start) = rest.find("<!-- generated: cherenkov ") {
        let after = &rest[start + "<!-- generated: cherenkov ".len()..];
        let (args, body) = after.split_once(" -->\n").unwrap();
        let (text, tail) = body.split_once("<!-- /generated -->").unwrap();
        blocks.push((
            args.split_whitespace().map(str::to_string).collect(),
            text.to_string(),
        ));
        rest = tail;
    }
    blocks
}

#[test]
fn test_the_generated_blocks_of_the_docs_equal_the_dump() {
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut found = 0;
    for file in [
        "mutation-operators.md",
        "operator-sets.md",
        "rust-operator-evaluation.md",
    ] {
        let doc = std::fs::read_to_string(docs.join(file)).unwrap();
        for (args, text) in generated_blocks(&doc) {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = cherenkov(&args);
            assert!(out.status.success(), "{file}: {args:?}");
            assert_eq!(
                text,
                String::from_utf8(out.stdout).unwrap(),
                "{file}: `cherenkov {}` differs: run `just docs`",
                args.join(" ")
            );
            found += 1;
        }
    }
    assert!(found >= 4, "{found} generated blocks");
}
