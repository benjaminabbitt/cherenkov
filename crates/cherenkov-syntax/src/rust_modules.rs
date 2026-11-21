//! The out-of-line modules (`mod name;`) a Rust file declares, and which of
//! them cargo-mutants (visit.rs, `find_mod_source` and `attrs_excluded`)
//! would not enter: a module with `#[cfg(test)]`, an attribute whose path
//! ends in `test`, or `#[mutants::skip]`, or one inside such a module. The
//! attribute rules, quirks included, are re-derived from its behaviour:
//! `cfg(not(test))`, `cfg(all(test, ..))` and `cfg(feature = ..)` do not
//! count as test code, because nothing but plain names may stand in a `cfg`.

use std::path::{Path, PathBuf};

use tree_sitter::Node;

/// One `mod name;` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFile {
    /// The files the module can be in, relative to the project root.
    pub candidates: Vec<PathBuf>,
    /// The module is test code or marked `#[mutants::skip]` (or lies inside
    /// such a module), so its file is not mutated.
    pub skipped: bool,
}

/// The module declarations of the Rust file `rel` with text `source`.
pub fn module_files(rel: &Path, source: &str) -> Vec<ModuleFile> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let dir = module_dir(rel);
    let beside = rel.parent().unwrap_or(Path::new("")).to_path_buf();
    let mut out = Vec::new();
    let place = Place {
        dir: &dir,
        beside: Some(&beside),
    };
    declarations(tree.root_node(), source, &place, false, &mut out);
    out
}

/// The directory the file's `mod` declarations are relative to: beside a
/// crate root (`lib.rs`, `main.rs`, `build.rs`, a file in `src/bin`) or a
/// `mod.rs`, else a directory named for the file.
fn module_dir(rel: &Path) -> PathBuf {
    let parent = rel.parent().unwrap_or(Path::new("")).to_path_buf();
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let in_bin = parent.file_name().is_some_and(|d| d == "bin");
    if matches!(name, "lib.rs" | "main.rs" | "mod.rs" | "build.rs") || in_bin {
        parent
    } else {
        rel.with_extension("")
    }
}

/// Where a module's children are looked for. `beside` is the file's own
/// directory, where a `#[path]` of a module declared directly in the file
/// is relative to; inside an inline module it is `None`.
struct Place<'a> {
    dir: &'a Path,
    beside: Option<&'a Path>,
}

fn declarations(
    node: Node<'_>,
    src: &str,
    place: &Place<'_>,
    skipped: bool,
    out: &mut Vec<ModuleFile>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "mod_item" {
            continue;
        }
        let attrs = attributes(child, src);
        let here = skipped || attrs.iter().any(|a| attribute_excludes(a));
        let name = child
            .child_by_field_name("name")
            .map(|n| text(n, src))
            .unwrap_or_default();
        match child.child_by_field_name("body") {
            Some(body) => {
                let inner = Place {
                    dir: &place.dir.join(name),
                    beside: None,
                };
                declarations(body, src, &inner, here, out);
            }
            None => out.push(ModuleFile {
                candidates: candidates(place, name, &attrs),
                skipped: here,
            }),
        }
    }
}

fn candidates(place: &Place<'_>, name: &str, attrs: &[&str]) -> Vec<PathBuf> {
    let dir = place.dir;
    match path_attribute(attrs) {
        Some(path) => vec![place.beside.unwrap_or(dir).join(path)],
        None => vec![
            dir.join(format!("{name}.rs")),
            dir.join(name).join("mod.rs"),
        ],
    }
}

fn text<'a>(node: Node<'_>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// The outer attributes of a module, as written: the siblings just before
/// it that are attributes (comments between them are skipped).
fn attributes<'a>(item: Node<'_>, src: &'a str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut prev = item.prev_named_sibling();
    while let Some(node) = prev {
        match node.kind() {
            "attribute_item" => out.push(text(node, src)),
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        prev = node.prev_named_sibling();
    }
    out
}

/// The attribute's text without `#[`, `]` and whitespace.
fn compact(attr: &str) -> String {
    let inner = attr.trim().trim_start_matches("#[").trim_end_matches(']');
    inner.chars().filter(|c| !c.is_whitespace()).collect()
}

fn path_attribute(attrs: &[&str]) -> Option<String> {
    attrs.iter().find_map(|a| {
        let inner = a.trim().trim_start_matches("#[").trim_end_matches(']');
        let (name, value) = inner.split_once('=')?;
        let value = value.trim();
        (name.trim() == "path" && value.starts_with('"') && value.ends_with('"'))
            .then(|| value[1..value.len() - 1].to_string())
            .filter(|p| !p.starts_with('/'))
    })
}

/// Whether an attribute makes cargo-mutants skip what it is on: a `test`
/// path (`#[test]`, `#[tokio::test(..)]`), `#[mutants::skip]`, a `cfg`
/// naming `test`, or a `cfg_attr` naming `mutants::skip`.
pub fn attribute_excludes(attr: &str) -> bool {
    let text = compact(attr);
    let (path, args) = match text.split_once('(') {
        Some((path, rest)) => (path, Some(rest.trim_end_matches(')'))),
        None => (text.split('=').next().unwrap_or_default(), None),
    };
    if path.rsplit("::").next() == Some("test") || path == "mutants::skip" {
        return true;
    }
    match (path, args) {
        ("cfg", Some(args)) => plain_names(args).is_some_and(|n| n.contains(&"test")),
        ("cfg_attr", Some(args)) => plain_names(args).is_some_and(|n| n.contains(&"mutants::skip")),
        _ => false,
    }
}

/// The comma separated names, or `None` when one is not a plain path (it
/// has arguments or a value), which cargo-mutants cannot read.
fn plain_names(args: &str) -> Option<Vec<&str>> {
    let names: Vec<&str> = args.split(',').collect();
    names
        .iter()
        .all(|n| !n.contains(['(', ')', '=', '"']))
        .then_some(names)
}
