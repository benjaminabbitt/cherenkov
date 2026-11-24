//! The `&&` of a let chain (`if let Some(x) = a && b`) is a binary
//! operation cargo-mutants swaps for `||` (the result rarely compiles, but
//! it is a mutant). The grammar has no binary expression there: the chain
//! is one node with the operands and the `&&` tokens as its children, so
//! each `&&` becomes a binary match of its own, between its neighbours.

use cherenkov_mutation::concept::{Capture, Concept};
use cherenkov_mutation::syntax::ConceptMatch;
use tree_sitter::Node;

use crate::span_of;

/// A binary match for every `&&` in a let chain under `root`.
pub fn let_chain_ands(root: Node<'_>) -> Vec<ConceptMatch> {
    let mut out = Vec::new();
    collect(root, &mut out);
    out
}

fn collect(node: Node<'_>, out: &mut Vec<ConceptMatch>) {
    if node.kind() == "let_chain" {
        out.extend(ands_of(node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, out);
    }
}

fn ands_of(chain: Node<'_>) -> Vec<ConceptMatch> {
    let mut cursor = chain.walk();
    let children: Vec<Node<'_>> = chain.children(&mut cursor).collect();
    children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind() == "&&")
        .filter_map(|(i, op)| {
            let lhs = children.get(i.checked_sub(1)?)?;
            let rhs = children.get(i + 1)?;
            Some(ConceptMatch {
                concept: Concept::Binary,
                span: crate::span_between(lhs, rhs),
                captures: vec![
                    (Capture::Lhs, span_of(lhs)),
                    (Capture::Op, span_of(op)),
                    (Capture::Rhs, span_of(rhs)),
                ],
            })
        })
        .collect()
}
