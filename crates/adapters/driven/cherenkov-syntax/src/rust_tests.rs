//! The test functions of a Rust file: the functions marked `#[test]` (or by
//! another attribute whose last path segment is `test`, as `#[tokio::test]`),
//! with the modules they are in and the lines they span.

use tree_sitter::Node;

/// A test function: its path in the file (`tests::inner::test_two`, as
/// libtest names it below the file's own module) and its lines (1-based,
/// inclusive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFn {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// The test functions of `source`, in order.
pub fn test_functions(source: &str) -> Vec<TestFn> {
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
    let mut found = Vec::new();
    walk(tree.root_node(), source, &[], &mut found);
    found
}

fn walk(node: Node<'_>, src: &str, modules: &[String], found: &mut Vec<TestFn>) {
    let mut inner = modules.to_vec();
    match node.kind() {
        "mod_item" => inner.extend(name_of(node, src)),
        "function_item" if is_test(node, src) => {
            let mut path = modules.to_vec();
            path.extend(name_of(node, src));
            found.push(TestFn {
                path: path.join("::"),
                start_line: line(node.start_position().row),
                end_line: line(node.end_position().row),
            });
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, &inner, found);
    }
}

fn line(row: usize) -> u32 {
    u32::try_from(row + 1).unwrap_or(u32::MAX)
}

fn name_of(node: Node<'_>, src: &str) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| src[n.byte_range()].to_string())
}

/// Whether an attribute right above `function` marks it a test.
fn is_test(function: Node<'_>, src: &str) -> bool {
    let mut above = function.prev_sibling();
    while let Some(node) = above {
        match node.kind() {
            "attribute_item" if marks_test(&src[node.byte_range()]) => return true,
            "attribute_item" | "line_comment" | "block_comment" => {}
            _ => return false,
        }
        above = node.prev_sibling();
    }
    false
}

/// `#[test]`, `#[tokio::test]`, `#[test(args)]`: the attribute's path ends
/// in `test`.
fn marks_test(attribute: &str) -> bool {
    let inner = attribute
        .trim()
        .trim_start_matches("#[")
        .trim_end_matches(']');
    let path = inner.split(['(', '=']).next().unwrap_or(inner).trim();
    path.rsplit("::").next() == Some("test")
}
