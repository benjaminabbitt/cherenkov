//! A mutant's plan between the planner's types and the run layer's.
//!
//! The planner (`cherenkov-mutation`) works in its own vocabulary: `Copy` spans,
//! hashable actions and edits, an operator named by a `&'static str`. The
//! run layer records and ships the plan as the messages of
//! `proto/cherenkov/mutant/v1/plan.proto`, and what an aggregate or a runner
//! handles is their generated domain types. [`encode`] and [`decode`] are
//! the one place the two meet; a plan that goes through both comes back
//! equal (`plan.test.rs`, over every plan of every fixture).
//!
//! The core is not given these types in place of its own: it stays free of
//! the wire (its architecture test allows no dependency beyond `bitflags`,
//! `serde` and `sha2`), and the generated structs derive neither `Copy` nor
//! `Eq` nor `Hash`, which the planner uses.

use std::path::PathBuf;

use cherenkov_mutation::concept::Capture as CoreCapture;
use cherenkov_mutation::edit::{Edit as CoreEdit, Lit as CoreLit};
use cherenkov_mutation::operator::catalog;
use cherenkov_mutation::operator_set::{mutation_named, mutations};
use cherenkov_mutation::plan::{Action as CoreAction, MutantPlan as CorePlan, Requires, Target};
use cherenkov_mutation::syntax::{Pos as CorePos, Span as CoreSpan};
use cherenkov_mutation::token::Token as CoreToken;

use crate::mutant::Target as WireTarget;
use crate::mutant::{
    Action, ActionKind, Capture, DeleteToken, Edit, EditKind, EvalBranch, EvalReturn, EvalThenJump,
    InPlace, JumpOnly, Lit, LitKind, MutantPlan, Pos, Redirect, ReplaceToken, ReplaceWithLiteral,
    Requirement, ReturnDefault, Span, TargetKind, Token, Unit, WrapNot,
};
use crate::rejection::Unreadable;

/// `plan`, as the run layer records it.
pub fn encode(plan: &CorePlan) -> MutantPlan {
    MutantPlan {
        id: plan.id.0.clone(),
        operator: plan.operator.to_string(),
        mutation: plan.mutation.map(str::to_string),
        file: plan.file.to_string_lossy().into_owned(),
        function: plan.function.clone(),
        stop_at: Some(pos(plan.stop_at)),
        action: Some(action(&plan.action)),
        edit: Some(edit(&plan.edit)),
        site: Some(span(plan.site)),
        requires: requirements(plan.requires),
        operands: plan.operands.iter().copied().map(span).collect(),
        operand_texts: plan.operand_texts.clone(),
        original: plan.original.clone(),
        mutated: plan.mutated.clone(),
    }
}

/// The planner's plan for `plan`: refused when it lacks a field every plan
/// has, or names an operator or a mutation the catalog does not.
pub fn decode(plan: MutantPlan) -> Result<CorePlan, Unreadable> {
    let operator =
        operator_id(&plan.operator).ok_or_else(|| unknown("operator", &plan.operator))?;
    let mutation = plan
        .mutation
        .map(|name| {
            mutation_named(&name)
                .map(|mutation| mutation.name)
                .ok_or_else(|| unknown("mutation", &name))
        })
        .transpose()?;
    Ok(CorePlan {
        id: cherenkov_mutation::id::MutantId(plan.id),
        operator,
        mutation,
        file: PathBuf::from(plan.file),
        function: plan.function,
        stop_at: unpos(plan.stop_at, "stop_at")?,
        action: unaction(plan.action, "action")?,
        edit: unedit(plan.edit, "edit")?,
        site: unspan(plan.site, "site")?,
        requires: unrequirements(&plan.requires),
        operands: plan
            .operands
            .into_iter()
            .map(|operand| unspan(Some(operand), "operands"))
            .collect::<Result<_, _>>()?,
        operand_texts: plan.operand_texts,
        original: plan.original,
        mutated: plan.mutated,
    })
}

/// The operator called `name`, by the id the planner gives its plans: the
/// prototype catalog's, or a named mutation's family.
fn operator_id(name: &str) -> Option<&'static str> {
    let prototypes = catalog()
        .iter()
        .map(|operator| operator.id())
        .collect::<Vec<_>>();
    let families = mutations().iter().map(|mutation| mutation.family);
    prototypes
        .into_iter()
        .chain(families)
        .find(|id| *id == name)
}

fn unknown(kind: &'static str, name: &str) -> Unreadable {
    Unreadable::Unknown {
        kind,
        name: name.to_string(),
    }
}

fn missing<T>(value: Option<T>, field: &'static str) -> Result<T, Unreadable> {
    value.ok_or(Unreadable::Missing { field })
}

// region where

fn pos(at: CorePos) -> Pos {
    Pos {
        byte: at.byte as u64,
        line: at.line,
        col: at.col,
    }
}

fn unpos(at: Option<Pos>, field: &'static str) -> Result<CorePos, Unreadable> {
    let at = missing(at, field)?;
    Ok(CorePos {
        byte: usize::try_from(at.byte).unwrap_or(usize::MAX),
        line: at.line,
        col: at.col,
    })
}

fn span(range: CoreSpan) -> Span {
    Span {
        start: Some(pos(range.start)),
        end: Some(pos(range.end)),
    }
}

fn unspan(range: Option<Span>, field: &'static str) -> Result<CoreSpan, Unreadable> {
    let range = missing(range, field)?;
    Ok(CoreSpan {
        start: unpos(range.start, field)?,
        end: unpos(range.end, field)?,
    })
}

fn target(at: Target) -> WireTarget {
    WireTarget {
        kind: match at {
            Target::AfterStatement(range) => TargetKind::AfterStatement(span(range)),
            Target::BlockStart(range) => TargetKind::BlockStart(span(range)),
        },
    }
}

fn untarget(at: Option<WireTarget>, field: &'static str) -> Result<Target, Unreadable> {
    Ok(match missing(at, field)?.kind {
        TargetKind::AfterStatement(range) => Target::AfterStatement(unspan(Some(range), field)?),
        TargetKind::BlockStart(range) => Target::BlockStart(unspan(Some(range), field)?),
    })
}

// endregion where

// region vocabulary

/// The protos number the planner's tokens from 1, in `Token::ALL` order
/// (`plan.test.rs` holds the names to it).
fn token(token: CoreToken) -> Token {
    let number = CoreToken::ALL
        .iter()
        .position(|t| *t == token)
        .map_or(0, |i| i + 1);
    i32::try_from(number)
        .ok()
        .and_then(|number| Token::try_from(number).ok())
        .unwrap_or(Token::Add)
}

fn untoken(token: Token) -> CoreToken {
    let index = usize::try_from(i32::from(token)).unwrap_or(1) - 1;
    CoreToken::ALL[index]
}

fn capture(capture: CoreCapture) -> Capture {
    let number = CoreCapture::ALL
        .iter()
        .position(|c| *c == capture)
        .map_or(0, |i| i + 1);
    i32::try_from(number)
        .ok()
        .and_then(|number| Capture::try_from(number).ok())
        .unwrap_or(Capture::Op)
}

fn uncapture(capture: Capture) -> CoreCapture {
    let index = usize::try_from(i32::from(capture)).unwrap_or(1) - 1;
    CoreCapture::ALL[index]
}

const REQUIREMENTS: [(Requires, Requirement); 3] = [
    (Requires::CALL_IN_EVAL, Requirement::CallInEval),
    (Requires::ASSIGN_IN_EVAL, Requirement::AssignInEval),
    (Requires::FORCE_RETURN, Requirement::ForceReturn),
];

fn requirements(requires: Requires) -> Vec<Requirement> {
    REQUIREMENTS
        .iter()
        .filter(|(flag, _)| requires.contains(*flag))
        .map(|(_, requirement)| *requirement)
        .collect()
}

fn unrequirements(requirements: &[Requirement]) -> Requires {
    REQUIREMENTS
        .iter()
        .filter(|(_, requirement)| requirements.contains(requirement))
        .fold(Requires::empty(), |all, (flag, _)| all | *flag)
}

// endregion vocabulary

// region what

fn lit(value: &CoreLit) -> Lit {
    Lit {
        kind: match value {
            CoreLit::Bool(flag) => LitKind::Flag(*flag),
            CoreLit::Default => LitKind::Default(Unit {}),
            CoreLit::Text(text) => LitKind::Text(text.clone()),
        },
    }
}

fn unlit(value: Option<Lit>, field: &'static str) -> Result<CoreLit, Unreadable> {
    Ok(match missing(value, field)?.kind {
        LitKind::Flag(flag) => CoreLit::Bool(flag),
        LitKind::Default(Unit {}) => CoreLit::Default,
        LitKind::Text(text) => CoreLit::Text(text),
    })
}

fn edit(edit: &CoreEdit) -> Edit {
    Edit {
        kind: match edit {
            CoreEdit::ReplaceToken {
                capture: at,
                from,
                with,
            } => EditKind::ReplaceToken(ReplaceToken {
                capture: capture(*at),
                original: token(*from),
                replacement: token(*with),
            }),
            CoreEdit::DeleteToken {
                capture: at,
                token: t,
            } => EditKind::DeleteToken(DeleteToken {
                capture: capture(*at),
                token: token(*t),
            }),
            CoreEdit::WrapNot { capture: at } => EditKind::WrapNot(WrapNot {
                capture: capture(*at),
            }),
            CoreEdit::ReplaceWithLiteral {
                capture: at,
                lit: value,
            } => EditKind::ReplaceWithLiteral(ReplaceWithLiteral {
                capture: capture(*at),
                lit: Some(lit(value)),
            }),
            CoreEdit::DeleteStatement => EditKind::DeleteStatement(Unit {}),
            CoreEdit::DeleteMatch => EditKind::DeleteMatch(Unit {}),
        },
    }
}

fn unedit(wire: Option<Edit>, field: &'static str) -> Result<CoreEdit, Unreadable> {
    Ok(match missing(wire, field)?.kind {
        EditKind::ReplaceToken(edit) => CoreEdit::ReplaceToken {
            capture: uncapture(edit.capture),
            from: untoken(edit.original),
            with: untoken(edit.replacement),
        },
        EditKind::DeleteToken(edit) => CoreEdit::DeleteToken {
            capture: uncapture(edit.capture),
            token: untoken(edit.token),
        },
        EditKind::WrapNot(edit) => CoreEdit::WrapNot {
            capture: uncapture(edit.capture),
        },
        EditKind::ReplaceWithLiteral(edit) => CoreEdit::ReplaceWithLiteral {
            capture: uncapture(edit.capture),
            lit: unlit(edit.lit, field)?,
        },
        EditKind::DeleteStatement(Unit {}) => CoreEdit::DeleteStatement,
        EditKind::DeleteMatch(Unit {}) => CoreEdit::DeleteMatch,
    })
}

fn action(action: &CoreAction) -> Action {
    Action {
        kind: match action {
            CoreAction::EvalThenJump {
                text,
                target: assigned,
                value,
                resume,
            } => ActionKind::EvalThenJump(EvalThenJump {
                text: text.clone(),
                target: assigned.clone(),
                value: value.clone(),
                resume: Some(target(*resume)),
            }),
            CoreAction::JumpOnly { resume } => ActionKind::JumpOnly(JumpOnly {
                resume: Some(target(*resume)),
            }),
            CoreAction::EvalBranch {
                cond_text,
                if_true,
                if_false,
            } => ActionKind::EvalBranch(EvalBranch {
                cond_text: cond_text.clone(),
                if_true: Some(target(*if_true)),
                if_false: Some(target(*if_false)),
            }),
            CoreAction::EvalReturn { value_text } => ActionKind::EvalReturn(EvalReturn {
                value_text: value_text.clone(),
            }),
            CoreAction::InPlace { expr_text } => ActionKind::InPlace(InPlace {
                expr_text: expr_text.clone(),
            }),
            CoreAction::Redirect { from, to } => ActionKind::Redirect(Redirect {
                from: Some(span(*from)),
                to: Some(span(*to)),
            }),
            CoreAction::ReturnDefault {
                value_text,
                zero_bits,
                async_fn,
            } => ActionKind::ReturnDefault(ReturnDefault {
                value_text: value_text.clone(),
                zero_bits: *zero_bits,
                async_fn: *async_fn,
            }),
        },
    }
}

fn unaction(wire: Option<Action>, field: &'static str) -> Result<CoreAction, Unreadable> {
    Ok(match missing(wire, field)?.kind {
        ActionKind::EvalThenJump(a) => CoreAction::EvalThenJump {
            text: a.text,
            target: a.target,
            value: a.value,
            resume: untarget(a.resume, field)?,
        },
        ActionKind::JumpOnly(a) => CoreAction::JumpOnly {
            resume: untarget(a.resume, field)?,
        },
        ActionKind::EvalBranch(a) => CoreAction::EvalBranch {
            cond_text: a.cond_text,
            if_true: untarget(a.if_true, field)?,
            if_false: untarget(a.if_false, field)?,
        },
        ActionKind::EvalReturn(a) => CoreAction::EvalReturn {
            value_text: a.value_text,
        },
        ActionKind::InPlace(a) => CoreAction::InPlace {
            expr_text: a.expr_text,
        },
        ActionKind::Redirect(a) => CoreAction::Redirect {
            from: unspan(a.from, field)?,
            to: unspan(a.to, field)?,
        },
        ActionKind::ReturnDefault(a) => CoreAction::ReturnDefault {
            value_text: a.value_text,
            zero_bits: a.zero_bits,
            async_fn: a.async_fn,
        },
    })
}

// endregion what

#[cfg(test)]
#[path = "plan.test.rs"]
mod tests;
