//! Rust source facts about types' defaults ([`DefaultFacts`]): which types
//! a file defines, which derive `Default`, which implement it by hand.

use cherenkov_mutation::types::DefaultFacts;
use tree_sitter::{Node, Parser};

/// The type definitions and `Default` impls of one Rust file.
pub fn default_facts(source: &str) -> DefaultFacts {
    let mut facts = DefaultFacts::default();
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return facts;
    }
    let Some(tree) = parser.parse(source, None) else {
        return facts;
    };
    walk(tree.root_node(), source, &mut facts);
    facts
}

fn walk(node: Node<'_>, src: &str, facts: &mut DefaultFacts) {
    let text = |n: Node<'_>| &src[n.byte_range()];
    match node.kind() {
        "struct_item" | "enum_item" | "union_item" => {
            if let Some(name) = node.child_by_field_name("name") {
                let name = text(name);
                match attribute_default(node, src) {
                    AttrDefault::Derived => facts.defined(name, true),
                    AttrDefault::ByMacro => {
                        facts.defined(name, false);
                        facts.implemented(name);
                    }
                    AttrDefault::None => facts.defined(name, false),
                }
            }
        }
        // An item-level macro may write impls for the types it names
        // (`impl_default!(C);`): those types may have a Default, of a shape
        // only the program knows.
        "macro_invocation"
            if node
                .parent()
                .is_some_and(|p| matches!(p.kind(), "source_file" | "declaration_list"))
                || node.parent().is_some_and(|p| {
                    p.kind() == "expression_statement"
                        && p.parent()
                            .is_some_and(|g| matches!(g.kind(), "source_file" | "declaration_list"))
                }) =>
        {
            for ident in identifiers(node, src) {
                facts.implemented(ident);
            }
        }
        "impl_item" => {
            let is_default = node
                .child_by_field_name("trait")
                .map(|t| text(t).split('<').next().unwrap_or(""))
                .is_some_and(|t| t.rsplit("::").next() == Some("Default"));
            if is_default && let Some(ty) = node.child_by_field_name("type") {
                let base = text(ty).split('<').next().unwrap_or("");
                facts.implemented(base.rsplit("::").next().unwrap_or(base).trim());
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, facts);
    }
}

enum AttrDefault {
    /// `#[derive(Default)]`: each field's default.
    Derived,
    /// Another derive or attribute that writes a Default (`SmartDefault`,
    /// `#[derivative(Default)]`, `#[educe(Default)]`): of its own shape.
    ByMacro,
    None,
}

/// What the attributes just before `item` say about its Default.
fn attribute_default(item: Node<'_>, src: &str) -> AttrDefault {
    let mut prev = item.prev_sibling();
    let mut by_macro = false;
    while let Some(p) = prev {
        if p.kind() != "attribute_item" {
            break;
        }
        let attr = &src[p.byte_range()];
        if let Some(inner) = attr
            .split_once("derive(")
            .and_then(|(_, rest)| rest.rsplit_once(')'))
            .map(|(inner, _)| inner)
        {
            let names: Vec<&str> = inner
                .split(',')
                .map(|d| d.trim().rsplit("::").next().unwrap_or(""))
                .collect();
            // prost's Message derive writes `impl Default`.
            if names.contains(&"Default") || (names.contains(&"Message") && attr.contains("prost"))
            {
                return AttrDefault::Derived;
            }
            by_macro |= names.iter().any(|n| n.contains("Default"));
        }
        // Any other attribute that mentions Default as a word.
        by_macro |= !attr.contains("derive(")
            && attr
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == "Default");
        prev = p.prev_sibling();
    }
    if by_macro {
        AttrDefault::ByMacro
    } else {
        AttrDefault::None
    }
}

/// The identifiers in a macro invocation's token tree.
fn identifiers<'a>(node: Node<'_>, src: &'a str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "identifier" || n.kind() == "type_identifier" {
            out.push(&src[n.byte_range()]);
        }
        let mut cursor = n.walk();
        stack.extend(n.children(&mut cursor));
    }
    out
}
