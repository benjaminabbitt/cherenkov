//! The gate's cases: every arithmetic or bitwise operator replacement the
//! planner makes in a snippet, with the source it would compile to.

use std::path::Path;
use std::sync::Arc;

use cherenkov_mutation::edit::Edit;
use cherenkov_mutation::operator_set::registry;
use cherenkov_mutation::plan::{MutantPlan, render_source};
use cherenkov_mutation::plan_sets::plan_file_sets;
use cherenkov_mutation::profile::Profile;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_mutation::token::Token;
use cherenkov_run_ports::syntax::SyntaxPort;
use cherenkov_syntax::TreeSitterSyntax;

use crate::snippets::{self, Snippet};

/// One mutant of one snippet.
pub struct Case {
    pub id: usize,
    pub snippet: Snippet,
    pub plan: MutantPlan,
    /// The snippet's file as the mutant has it.
    pub mutated: String,
}

fn arithmetic(t: Token) -> bool {
    let base = t.compound_base().unwrap_or(t);
    matches!(
        base,
        Token::Add
            | Token::Sub
            | Token::Mul
            | Token::Div
            | Token::Rem
            | Token::BitAnd
            | Token::BitOr
            | Token::BitXor
    )
}

/// A comparison or a short-circuit operator.
fn condition_operator(t: Token) -> bool {
    matches!(
        t,
        Token::Lt
            | Token::Le
            | Token::Gt
            | Token::Ge
            | Token::Eq
            | Token::Ne
            | Token::And
            | Token::Or
    )
}

/// `&&` to `==` and `||` to `!=` (`lcr/and->eq`) are made with the
/// operands as they are, `a < 3 == b > 3`: source that does not compile
/// when the operands are comparisons, so they are not cases.
fn compilable(from: Token, with: Token) -> bool {
    !matches!(from, Token::And | Token::Or) || matches!(with, Token::And | Token::Or)
}

fn operator_replacement(plan: &MutantPlan) -> bool {
    matches!(
        &plan.edit,
        Edit::ReplaceToken { from, with, .. }
            if arithmetic(*from) || (condition_operator(*from) && compilable(*from, *with))
    )
}

fn rust_profile(syntax: &TreeSitterSyntax) -> Profile {
    let rust = LanguageId::new("rust");
    syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .expect("a Rust profile")
}

fn cases_of(
    syntax: &TreeSitterSyntax,
    profile: &Profile,
    snippet: &Snippet,
    next: &mut usize,
) -> Vec<Case> {
    let rust = LanguageId::new("rust");
    let file = syntax
        .analyze(&rust, Path::new("s.rs"), Arc::from(snippet.source.as_str()))
        .expect("a snippet parses");
    let planned = plan_file_sets(&file, profile, &registry(), false);
    let mut out = Vec::new();
    for plan in planned.plans.into_iter().filter(operator_replacement) {
        let Some(mutated) = render_source(&snippet.source, &plan, profile) else {
            continue;
        };
        out.push(Case {
            id: *next,
            snippet: snippet.clone(),
            plan,
            mutated,
        });
        *next += 1;
    }
    out
}

/// Every case of every snippet, numbered.
pub fn all() -> Vec<Case> {
    let syntax = TreeSitterSyntax::builtin().expect("the built-in profiles load");
    let profile = rust_profile(&syntax);
    let mut next = 0;
    let mut out = Vec::new();
    for snippet in snippets::all() {
        out.extend(cases_of(&syntax, &profile, &snippet, &mut next));
    }
    out
}
