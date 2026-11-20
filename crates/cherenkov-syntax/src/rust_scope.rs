//! Whether the names a replacement value uses resolve where the mutant is.
//!
//! A value such as `MutexGuard::new()` is a guess at a constructor. If its
//! leading name is neither defined, imported nor in the prelude in the
//! mutated function's scope, the mutated source does not compile, and the
//! mutant is unviable (as cargo-mutants finds it), whatever a debugger could
//! build. Only capitalized leading names are checked (types, variants,
//! constants), and any scope this cannot read in full (a glob import, an
//! item-level macro, which may define any name) decides nothing.

use std::collections::HashSet;
use tree_sitter::{Node, Parser};

/// What the standard prelude names, capitalized.
const PRELUDE: &[&str] = &[
    "Self",
    "Some",
    "None",
    "Ok",
    "Err",
    "Option",
    "Result",
    "Vec",
    "String",
    "Box",
    "Default",
    "Clone",
    "Copy",
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    "Drop",
    "Fn",
    "FnMut",
    "FnOnce",
    "AsRef",
    "AsMut",
    "Into",
    "From",
    "TryFrom",
    "TryInto",
    "FromIterator",
    "Iterator",
    "IntoIterator",
    "DoubleEndedIterator",
    "ExactSizeIterator",
    "Extend",
    "Eq",
    "PartialEq",
    "Ord",
    "PartialOrd",
    "ToOwned",
    "ToString",
    "Debug",
    "Hash",
    // Edition 2024's additions.
    "Future",
    "IntoFuture",
];

/// The leading names of `value` that do not resolve at byte `at` of
/// `source` (inside the function being mutated). Empty when they all do, or
/// when the scope cannot be read in full.
pub fn unresolved_names(source: &str, at: usize, value: &str) -> Vec<String> {
    let heads = heads(value);
    if heads.is_empty() {
        return Vec::new();
    }
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let Some(mut node) = tree.root_node().descendant_for_byte_range(at, at) else {
        return Vec::new();
    };
    let mut known: HashSet<&str> = PRELUDE.iter().copied().collect();
    loop {
        let module = node.kind() == "declaration_list"
            && node.parent().is_some_and(|p| p.kind() == "mod_item");
        if (module || matches!(node.kind(), "source_file" | "block"))
            && !scope_names(node, source, &mut known)
        {
            return Vec::new();
        }
        if let Some(params) = node.child_by_field_name("type_parameters") {
            generic_names(params, source, &mut known);
        }
        // A module sees its own names, not its parent's.
        if module {
            break;
        }
        match node.parent() {
            Some(parent) => node = parent,
            None => break,
        }
    }
    heads
        .into_iter()
        .filter(|h| !known.contains(h.as_str()))
        .collect()
}

/// The capitalized names `value` starts a path with (not after `::` or `.`),
/// outside string and char literals, in order, once each.
fn heads(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    let mut prev_path = false;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                prev_path = false;
            }
            b'\'' if value[i..].starts_with("'\"'") => i += 3,
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let start = i;
                while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                let word = &value[start..i];
                if !prev_path
                    && word.starts_with(|c: char| c.is_ascii_uppercase())
                    && !out.iter().any(|w| w == word)
                {
                    out.push(word.to_string());
                }
                prev_path = false;
            }
            b'0'..=b'9' => {
                while i < bytes.len() && (bytes[i] == b'_' || bytes[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                prev_path = false;
            }
            b':' if value[i..].starts_with("::") => {
                prev_path = true;
                i += 2;
            }
            b'.' => {
                prev_path = true;
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                prev_path = false;
                i += 1;
            }
        }
    }
    out
}

/// The names `scope` (a file, module body, block, function or impl) puts in
/// scope. False when it may put in more than can be read: a glob import or
/// an item-level macro.
fn scope_names<'a>(scope: Node<'_>, src: &'a str, known: &mut HashSet<&'a str>) -> bool {
    let mut cursor = scope.walk();
    for child in scope.children(&mut cursor) {
        match child.kind() {
            "struct_item"
            | "enum_item"
            | "union_item"
            | "type_item"
            | "trait_item"
            | "function_item"
            | "const_item"
            | "static_item"
            | "mod_item"
            | "macro_definition"
            | "foreign_mod_item"
            | "function_signature_item" => {
                if let Some(name) = child.child_by_field_name("name") {
                    known.insert(&src[name.byte_range()]);
                }
            }
            "extern_crate_declaration" => {
                if let Some(name) = child.child_by_field_name("name") {
                    known.insert(&src[name.byte_range()]);
                }
            }
            "use_declaration" => {
                if let Some(arg) = child.child_by_field_name("argument")
                    && !use_names(arg, src, known)
                {
                    return false;
                }
            }
            "macro_invocation" => return false,
            "expression_statement"
                if child
                    .child(0)
                    .is_some_and(|c| c.kind() == "macro_invocation")
                    && matches!(scope.kind(), "source_file" | "declaration_list") =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// The names a `use` tree brings in. False for a glob.
fn use_names<'a>(node: Node<'_>, src: &'a str, known: &mut HashSet<&'a str>) -> bool {
    match node.kind() {
        "identifier" | "type_identifier" => {
            known.insert(&src[node.byte_range()]);
            true
        }
        "scoped_identifier" => node
            .child_by_field_name("name")
            .is_none_or(|n| use_names(n, src, known)),
        "use_as_clause" => {
            if let Some(alias) = node.child_by_field_name("alias") {
                known.insert(&src[alias.byte_range()]);
            }
            true
        }
        "use_wildcard" => false,
        "scoped_use_list" | "use_list" => {
            let mut ok = true;
            let prefix = node
                .child_by_field_name("path")
                .map(|p| src[p.byte_range()].rsplit("::").next().unwrap_or(""));
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if node.child_by_field_name("path") == Some(child) {
                    continue;
                }
                if child.kind() == "self" {
                    if let Some(p) = prefix {
                        known.insert(p);
                    }
                    continue;
                }
                ok &= use_names(child, src, known);
            }
            ok
        }
        _ => true,
    }
}

/// The names generic parameters declare.
fn generic_names<'a>(params: Node<'_>, src: &'a str, known: &mut HashSet<&'a str>) {
    let mut cursor = params.walk();
    for p in params.named_children(&mut cursor) {
        let name = match p.kind() {
            "type_identifier" => Some(p),
            "constrained_type_parameter" => p.child_by_field_name("left"),
            "optional_type_parameter" | "const_parameter" => p.child_by_field_name("name"),
            _ => None,
        };
        if let Some(n) = name {
            known.insert(&src[n.byte_range()]);
        }
    }
}
