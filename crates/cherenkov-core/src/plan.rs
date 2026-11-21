//! The planner: concept matches and operator edits to mutant plans
//! (design section 4.5). Language- and backend-independent.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use bitflags::bitflags;
use serde::Serialize;

use crate::concept::{Capture, Concept};
use crate::edit::{Edit, Lit};
use crate::id::{IdParts, MutantId};
use crate::operator::{MatchCx, Operator, innermost_statement, operator_inputs};
use crate::profile::Profile;
use crate::syntax::{ConceptMatch, FileSyntax, Pos, Span, body_contents};
use crate::token::Token;

/// Resume and branch targets are structural; the driver resolves them,
/// because only it knows the line table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "span")]
pub enum Target {
    /// Just past the statement: native, the end of its instruction range.
    AfterStatement(Span),
    /// The first statement of a then-block or else-branch.
    BlockStart(Span),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Action {
    EvalThenJump {
        /// The mutated statement, as source text (listings, IDs).
        text: String,
        /// What is assigned: the target and the mutated value, apart, so a
        /// backend need not parse `text`.
        target: String,
        value: String,
        resume: Target,
    },
    JumpOnly {
        resume: Target,
    },
    EvalBranch {
        cond_text: String,
        if_true: Target,
        if_false: Target,
    },
    EvalReturn {
        value_text: String,
    },
    /// Apply the mutation at the edited expression itself, with no stop and
    /// evaluation at the statement (an operator outside any condition or
    /// assignment, a compound assignment). `expr_text` is the mutated
    /// expression, for display; backends that patch instructions realize it.
    InPlace {
        expr_text: String,
    },
    /// Run `to` wherever `from` would run: a deleted match arm's body
    /// continues as the catch-all arm's body. `from` and `to` are the two
    /// bodies.
    Redirect {
        from: Span,
        to: Span,
    },
    /// Return a value at function entry instead of running the body. The
    /// literal is known when the profile maps the return type; otherwise the
    /// driver derives it from debug info.
    ReturnDefault {
        value_text: Option<String>,
        /// The value is the result type's all-zero bits (the profile's zero
        /// for that type), which a register-zeroing stub can return exactly.
        zero_bits: bool,
        /// The function is `async`: its body runs when its future is polled
        /// (in whatever future a macro wraps it in), and the value is what
        /// the future yields.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        async_fn: bool,
    },
}

impl Action {
    /// Kebab-case variant name, as shown in listings.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::EvalThenJump { .. } => "eval-then-jump",
            Action::JumpOnly { .. } => "jump-only",
            Action::EvalBranch { .. } => "eval-branch",
            Action::EvalReturn { .. } => "eval-return",
            Action::ReturnDefault { .. } => "return-default",
            Action::InPlace { .. } => "in-place",
            Action::Redirect { .. } => "redirect",
        }
    }
}

bitflags! {
    /// What the evaluated text needs from a backend; computed syntactically.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
    pub struct Requires: u32 {
        /// Evaluated text contains a call.
        const CALL_IN_EVAL = 1 << 0;
        /// Evaluated text contains an assignment.
        const ASSIGN_IN_EVAL = 1 << 1;
        /// The frame must be popped with a value.
        const FORCE_RETURN = 1 << 2;
    }
}

impl Requires {
    /// Short names in a fixed order, as shown in listings: `call`, `assign`, `force-return`.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Requires::CALL_IN_EVAL, "call"),
            (Requires::ASSIGN_IN_EVAL, "assign"),
            (Requires::FORCE_RETURN, "force-return"),
        ]
        .into_iter()
        .filter(|(flag, _)| self.contains(*flag))
        .map(|(_, name)| name)
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutantPlan {
    pub id: MutantId,
    pub operator: &'static str,
    /// The named mutation this is (operator_set.rs), whose origins say where
    /// its definition came from.
    pub mutation: Option<&'static str>,
    pub file: PathBuf,
    /// Name of the innermost enclosing function; `None` at module level.
    pub function: Option<String>,
    pub stop_at: Pos,
    pub action: Action,
    pub edit: Edit,
    /// Span of the capture the edit applies to.
    pub site: Span,
    pub requires: Requires,
    /// The operands of the mutated operator (left and right of a binary
    /// one, the operand of a unary one), for backends that patch the code
    /// combining them.
    pub operands: Vec<Span>,
    /// Their source text, for backends that evaluate them (to check a
    /// mutated operation for overflow as the compiled one would be).
    pub operand_texts: Vec<String>,
    /// The source text at `site`, as the file has it.
    pub original: String,
    /// The source text the mutant has at `site` instead; `None` when the
    /// mutant has no source form (a body replacement whose value the
    /// profile does not know).
    pub mutated: Option<String>,
}

impl MutantPlan {
    /// `path:line:col function operator`, the stable part of [`MutantPlan::describe`].
    pub fn site(&self) -> String {
        format!(
            "{}:{}:{} {} {}",
            display_path(&self.file),
            self.stop_at.line,
            self.stop_at.col,
            self.function.as_deref().unwrap_or("-"),
            self.operator
        )
    }

    /// One-line listing without the ID:
    /// `path:line:col function operator action ["text"] [requires]`.
    /// Module level shows the function as `-`.
    pub fn describe(&self) -> String {
        format!("{} {}", self.site(), self.change())
    }

    /// What the mutant does, the part of [`MutantPlan::describe`] after its
    /// site: `action ["text"] [requires]`.
    pub fn change(&self) -> String {
        let text = match &self.action {
            Action::EvalThenJump { text, .. } => format!(" {text:?}"),
            Action::JumpOnly { .. } => String::new(),
            Action::EvalBranch { cond_text, .. } => format!(" {cond_text:?}"),
            Action::EvalReturn { value_text } => format!(" {value_text:?}"),
            Action::InPlace { expr_text } => format!(" {expr_text:?}"),
            Action::Redirect { to, .. } => format!(" -> {}:{}", to.start.line, to.start.col),
            Action::ReturnDefault {
                value_text: Some(v),
                ..
            } => format!(" {v:?}"),
            Action::ReturnDefault {
                value_text: None, ..
            } => " <default>".to_string(),
        };
        let names = self.requires.names();
        let requires = if names.is_empty() {
            String::new()
        } else {
            format!(" [{}]", names.join(","))
        };
        format!("{}{text}{requires}", self.action.kind())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// The edit's site is in no context with a defined action
    /// (for example an arithmetic swap in a call argument).
    NoActionForContext,
    /// The match lies in a region of a kind the operator's set skips.
    Region(crate::syntax::SkipKind),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diagnostics {
    /// Matches seen per concept (statements count as `Statement`).
    pub concept_counts: BTreeMap<Concept, usize>,
    /// Concepts the language profile does not map.
    pub unmapped: BTreeSet<Concept>,
    /// Matches whose role captures do not fit the concept model
    /// (for example a chained comparison: one `Binary`, two operators).
    pub unrepresentable: BTreeMap<Concept, usize>,
    /// Edits not planned because the profile disables the operator.
    pub disabled: BTreeMap<String, usize>,
    /// Edits that produced no plan, per operator and reason.
    #[serde(serialize_with = "skipped_as_list")]
    pub skipped: BTreeMap<(String, SkipReason), usize>,
    /// Plans dropped as identical to an earlier plan at the same stop.
    pub deduplicated: usize,
    /// Plans produced.
    pub planned: usize,
}

/// JSON objects cannot have tuple keys: emit `[{operator, reason, count}]`.
fn skipped_as_list<S: serde::Serializer>(
    skipped: &BTreeMap<(String, SkipReason), usize>,
    s: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Entry<'a> {
        operator: &'a str,
        reason: SkipReason,
        count: usize,
    }
    s.collect_seq(skipped.iter().map(|((operator, reason), &count)| Entry {
        operator,
        reason: *reason,
        count,
    }))
}

impl Diagnostics {
    /// Add another file's counts into this one.
    pub fn merge(&mut self, other: &Diagnostics) {
        fn add<K: Ord + Clone>(into: &mut BTreeMap<K, usize>, from: &BTreeMap<K, usize>) {
            for (k, n) in from {
                *into.entry(k.clone()).or_default() += n;
            }
        }
        add(&mut self.concept_counts, &other.concept_counts);
        add(&mut self.unrepresentable, &other.unrepresentable);
        add(&mut self.disabled, &other.disabled);
        add(&mut self.skipped, &other.skipped);
        self.unmapped.extend(other.unmapped.iter().copied());
        self.deduplicated += other.deduplicated;
        self.planned += other.planned;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PlanOutput {
    pub plans: Vec<MutantPlan>,
    pub diagnostics: Diagnostics,
    /// Per mutant, the operator sets that produced it, in the order they
    /// were asked for. Filled when planning with sets
    /// ([`crate::plan_sets::plan_file_sets`]); a mutant that several sets
    /// produce is planned once.
    pub provenance: Provenance,
}

/// The sets that produced each mutant.
pub type Provenance = std::collections::BTreeMap<crate::id::MutantId, Vec<&'static str>>;

/// Plan every mutant of one file. Pure and deterministic: plans come back
/// sorted by stop position, then operator id, then edit.
pub fn plan_file(
    file: &FileSyntax,
    profile: &Profile,
    catalog: &[Box<dyn Operator>],
) -> PlanOutput {
    let mut diagnostics = Diagnostics {
        unmapped: Concept::ALL
            .into_iter()
            .filter(|c| !profile.mapped.contains(c))
            .collect(),
        ..Diagnostics::default()
    };
    if file.has_errors {
        return PlanOutput {
            diagnostics,
            ..PlanOutput::default()
        };
    }

    let mut drafts = Vec::new();
    for m in &operator_inputs(file) {
        *diagnostics.concept_counts.entry(m.concept).or_default() += 1;
        if !representable(m) {
            *diagnostics.unrepresentable.entry(m.concept).or_default() += 1;
            continue;
        }
        let cx = MatchCx::new(file, profile, m);
        for op in catalog
            .iter()
            .filter(|op| op.concepts().contains(&m.concept))
        {
            drafts.extend(operator_drafts(&cx, op.as_ref(), &mut diagnostics));
        }
    }
    let drafts = ordered(drafts, &mut diagnostics);
    let plans = finished_plans(drafts, file, profile);
    diagnostics.planned = plans.len();
    PlanOutput {
        plans,
        diagnostics,
        provenance: Provenance::new(),
    }
}

/// The drafts of the edits `op` makes at the match of `cx`, counting in
/// `diagnostics` each edit it disables, skips or cannot realize.
fn operator_drafts(
    cx: &MatchCx<'_>,
    op: &dyn Operator,
    diagnostics: &mut Diagnostics,
) -> Vec<Draft> {
    let (file, profile, m) = (cx.file, cx.profile, cx.m);
    let edits = op.mutate(cx);
    if profile.disabled_operators.contains(op.id()) {
        if !edits.is_empty() {
            *diagnostics.disabled.entry(op.id().to_string()).or_default() += edits.len();
        }
        return Vec::new();
    }
    let region = op.skips().iter().copied().find(|kind| {
        file.skips
            .iter()
            .any(|r| r.kind == *kind && r.span.contains(&m.span))
    });
    if let Some(kind) = region {
        if !edits.is_empty() {
            let key = (op.id().to_string(), SkipReason::Region(kind));
            *diagnostics.skipped.entry(key).or_default() += edits.len();
        }
        return Vec::new();
    }
    let mut drafts = Vec::new();
    for edit in edits {
        match realize(cx, op.id(), edit) {
            Some(draft) => drafts.push(draft),
            None => {
                let key = (op.id().to_string(), SkipReason::NoActionForContext);
                *diagnostics.skipped.entry(key).or_default() += 1;
            }
        }
    }
    drafts
}

/// `drafts` sorted by stop position, operator and edit, without the ones
/// that stop at the same place to do the same; the count dropped goes to
/// `diagnostics`.
fn ordered(mut drafts: Vec<Draft>, diagnostics: &mut Diagnostics) -> Vec<Draft> {
    drafts.sort_by(|a, b| {
        (a.stop_at.byte, a.operator, &a.edit, a.site.start.byte).cmp(&(
            b.stop_at.byte,
            b.operator,
            &b.edit,
            b.site.start.byte,
        ))
    });
    let before = drafts.len();
    let mut seen = HashSet::new();
    drafts.retain(|d| seen.insert((d.stop_at, d.action.clone())));
    diagnostics.deduplicated = before - drafts.len();
    drafts
}

/// The plans of `drafts`, each with its id (made unique among the drafts
/// that anchor and edit alike by their occurrence) and its mutated text.
fn finished_plans(drafts: Vec<Draft>, file: &FileSyntax, profile: &Profile) -> Vec<MutantPlan> {
    let path = display_path(&file.path);
    let mut occurrences: HashMap<(Option<String>, &'static str, String, String), usize> =
        HashMap::new();
    drafts
        .into_iter()
        .map(|d| {
            let anchor = d.anchor.split_whitespace().collect::<Vec<_>>().join(" ");
            let edit = d.edit.describe();
            let n = occurrences
                .entry((d.function.clone(), d.operator, anchor.clone(), edit.clone()))
                .or_default();
            let id = MutantId::derive(&IdParts {
                path: &path,
                function: d.function.as_deref(),
                operator: d.operator,
                anchor: &anchor,
                edit: &edit,
                occurrence: *n,
            });
            *n += 1;
            MutantPlan {
                id,
                operator: d.operator,
                mutation: crate::operator_set::mutation_for(d.operator, &d.edit).map(|m| m.name),
                file: file.path.clone(),
                function: d.function,
                stop_at: d.stop_at,
                action: d.action,
                edit: d.edit,
                site: d.site,
                requires: d.requires,
                operand_texts: d
                    .operands
                    .iter()
                    .map(|o| file.source[o.start.byte..o.end.byte].to_string())
                    .collect(),
                operands: d.operands,
                original: d.site.text(&file.source).to_string(),
                mutated: None,
            }
        })
        .map(|mut plan| {
            plan.mutated = replacement(&file.source, &plan, profile);
            plan
        })
        .collect()
}

/// The source file as the plan's mutant would have it: the ground truth a
/// run-time mutant is judged against. `None` when the mutant has no source
/// form yet (a body replacement whose value the profile does not know).
pub fn render_source(source: &str, plan: &MutantPlan, profile: &Profile) -> Option<String> {
    let site = plan.site;
    let replacement = replacement(source, plan, profile)?;
    Some(format!(
        "{}{replacement}{}",
        &source[..site.start.byte],
        &source[site.end.byte..]
    ))
}

/// The text `plan`'s mutant has at its site in `source`, spelled by
/// `profile`; `None` when it has no source form.
fn replacement(source: &str, plan: &MutantPlan, profile: &Profile) -> Option<String> {
    let site = plan.site;
    let replacement = match (&plan.edit, &plan.action) {
        (Edit::ReplaceToken { with, .. }, _) => profile.tokens.spell(*with)?.to_string(),
        (Edit::DeleteToken { .. } | Edit::DeleteMatch, _) => String::new(),
        (Edit::WrapNot { .. }, _) => profile.tokens.wrap_not(site.text(source)),
        (
            Edit::ReplaceWithLiteral {
                lit: Lit::Bool(b), ..
            },
            _,
        ) => profile
            .tokens
            .spell(if *b { Token::True } else { Token::False })?
            .to_string(),
        (
            Edit::ReplaceWithLiteral {
                lit: Lit::Default, ..
            },
            Action::ReturnDefault { value_text, .. },
        ) => profile
            .body_template
            .replacen("{}", value_text.as_deref()?, 1),
        (
            Edit::ReplaceWithLiteral {
                lit: Lit::Text(v), ..
            },
            _,
        ) => profile.body_template.replacen("{}", v, 1),
        (
            Edit::ReplaceWithLiteral {
                lit: Lit::Default, ..
            },
            _,
        ) => return None,
        (Edit::DeleteStatement, _) => String::new(),
    };
    Some(replacement)
}

/// A plan before its ID is assigned.
struct Draft {
    operator: &'static str,
    function: Option<String>,
    stop_at: Pos,
    action: Action,
    edit: Edit,
    site: Span,
    requires: Requires,
    /// Text the ID is anchored to (statement, or condition for branches).
    anchor: String,
    operands: Vec<Span>,
}

/// Role captures the concept model needs exactly once (or at most once).
fn representable(m: &ConceptMatch) -> bool {
    let count = |c| m.all(c).count();
    match m.concept {
        Concept::Binary => count(Capture::Op) == 1,
        Concept::Condition => count(Capture::Cond) == 1 && count(Capture::Then) <= 1,
        Concept::Assign => count(Capture::Target) == 1 && count(Capture::Value) == 1,
        _ => true,
    }
}

/// The operands of `lhs with rhs` as the mutated source parses, which the
/// original tree may not show. The new token can bind looser than the
/// operator above it, which then gives up its right operand
/// (`a && b && c` -> `a || b && c`, that is `a || (b && c)`), or tighter
/// than the one in its left operand, which keeps its own left
/// (`a || b || c` -> `a || b && c`, again `a || (b && c)`).
fn regrouped(cx: &MatchCx<'_>, with: Token, mut lhs: Span, mut rhs: Span) -> Vec<Span> {
    let src: &str = &cx.file.source;
    let binary = |m: &&ConceptMatch| m.concept == Concept::Binary;
    let op = |m: &ConceptMatch| {
        m.one(Capture::Op)
            .and_then(|s| cx.profile.tokens.token(&src[s.start.byte..s.end.byte]))
    };
    // Up: each operator whose left operand is this one.
    let mut node = cx.m.span;
    while let Some(parent) = cx
        .file
        .matches
        .iter()
        .filter(binary)
        .find(|p| p.one(Capture::Lhs) == Some(node))
    {
        match (op(parent), parent.one(Capture::Rhs)) {
            (Some(t), Some(r)) if t.binds_tighter(with) => {
                rhs.end = r.end;
                node = parent.span;
            }
            _ => break,
        }
    }
    // Down: a left operand whose own operator binds looser.
    while let Some(left) = cx
        .file
        .matches
        .iter()
        .filter(binary)
        .find(|l| l.span == lhs)
    {
        match (op(left), left.one(Capture::Rhs)) {
            (Some(t), Some(r)) if with.binds_tighter(t) => lhs = r,
            _ => break,
        }
    }
    vec![lhs, rhs]
}

/// Turn an edit into an action, by statement kind and context
/// (design section 3.3). `None` when the context has no defined action.
fn realize(cx: &MatchCx<'_>, operator: &'static str, edit: Edit) -> Option<Draft> {
    let Realization {
        stop_at,
        action,
        site,
        requires,
        anchor,
    } = realization(cx, &edit)?;
    Some(Draft {
        operator,
        function: enclosing_function(cx.file, &site),
        stop_at,
        action,
        site,
        requires,
        anchor,
        operands: edit_operands(cx, &edit),
        edit,
    })
}

/// What an edit comes to at its site, before the operator and the enclosing
/// function are known.
struct Realization {
    stop_at: Pos,
    action: Action,
    site: Span,
    requires: Requires,
    anchor: String,
}

impl Realization {
    fn of(
        stop_at: Pos,
        action: Action,
        site: Span,
        requires: Requires,
        anchor: &str,
    ) -> Option<Self> {
        Some(Self {
            stop_at,
            action,
            site,
            requires,
            anchor: anchor.to_string(),
        })
    }
}

/// The operands of the match `edit` is made on, as the mutated source
/// groups them.
fn edit_operands(cx: &MatchCx<'_>, edit: &Edit) -> Vec<Span> {
    let operands: Vec<Span> = [Capture::Lhs, Capture::Rhs, Capture::Operand]
        .into_iter()
        .filter_map(|c| cx.m.one(c))
        .collect();
    match (edit, &operands[..]) {
        (Edit::ReplaceToken { with, .. }, &[lhs, rhs]) => regrouped(cx, *with, lhs, rhs),
        _ => operands,
    }
}

/// The action, stop and site of `edit`, by its kind.
fn realization(cx: &MatchCx<'_>, edit: &Edit) -> Option<Realization> {
    let m = cx.m;
    match edit {
        Edit::DeleteStatement => delete_statement(cx),
        Edit::ReplaceToken { capture, with, .. } => token_edit(cx, *capture, Some(*with)),
        Edit::DeleteToken { capture, .. } => token_edit(cx, *capture, None),
        Edit::WrapNot { capture } => wrap_not(cx, *capture),
        Edit::DeleteMatch if m.concept == Concept::Arm => delete_arm(cx),
        Edit::DeleteMatch => delete_match(cx),
        Edit::ReplaceWithLiteral {
            capture: Capture::Guard,
            lit: Lit::Bool(b),
        } if m.one(Capture::Value).is_some() => bool_guard(cx, *b),
        Edit::ReplaceWithLiteral {
            capture,
            lit: Lit::Bool(b),
        } if m.concept != Concept::Return => bool_in_place(cx, *capture, *b),
        Edit::ReplaceWithLiteral {
            capture,
            lit: Lit::Bool(b),
        } => bool_return(cx, *capture, *b),
        Edit::ReplaceWithLiteral {
            capture,
            lit: Lit::Default,
        } => default_body(cx, *capture),
        Edit::ReplaceWithLiteral {
            capture,
            lit: Lit::Text(value),
        } => text_body(cx, *capture, value),
    }
}

/// The statement is jumped over.
fn delete_statement(cx: &MatchCx<'_>) -> Option<Realization> {
    let stmt = cx.stmt?;
    let action = Action::JumpOnly {
        resume: Target::AfterStatement(stmt.span),
    };
    Realization::of(
        stmt.span.start,
        action,
        stmt.span,
        Requires::empty(),
        stmt.span.text(&cx.file.source),
    )
}

/// No statement-level action: the expression mutated in place.
fn in_place(cx: &MatchCx<'_>, site: Span, spelled: &str) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let src: &str = &file.source;
    let stmt = innermost_statement(file, &m.span);
    let anchor = stmt.map_or(m.span, |s| s.span).text(src);
    let action = Action::InPlace {
        expr_text: splice(src, m.span, site, spelled),
    };
    Realization::of(m.span.start, action, site, Requires::empty(), anchor)
}

/// A token swapped (`with`) or deleted (none) at `capture`, realized by the
/// evaluable context around it.
fn token_edit(cx: &MatchCx<'_>, capture: Capture, with: Option<Token>) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let src: &str = &file.source;
    let site = m.one(capture)?;
    let spelled = match with {
        Some(with) => cx.profile.tokens.spell(with)?,
        None => "",
    };
    let Some(context) = token_context(file, &site) else {
        return in_place(cx, site, spelled);
    };
    match context {
        TokenContext::Condition(cond_match, cond) => {
            let (action, requires) = branch(file, cond_match, splice(src, cond, site, spelled))?;
            Realization::of(cond.start, action, site, requires, cond.text(src))
        }
        TokenContext::Assign(assign, value) => assign_token(cx, assign, value, site, spelled),
        TokenContext::CompoundAssign(assign) => {
            compound_assign_token(cx, assign, site, spelled, with)
        }
        TokenContext::Guard(arm, guard) => {
            let (action, requires) = guard_branch(file, arm, splice(src, guard, site, spelled))?;
            Realization::of(guard.start, action, site, requires, guard.text(src))
        }
        TokenContext::Return(ret, value) => {
            let action = Action::EvalReturn {
                value_text: splice(src, value, site, spelled),
            };
            let requires = Requires::FORCE_RETURN | calls_in(file, &value);
            let anchor = innermost_statement(file, &ret.span).map_or(ret.span, |s| s.span);
            Realization::of(value.start, action, site, requires, anchor.text(src))
        }
    }
}

/// A token in the `value` of an assignment: the statement is evaluated
/// with the mutated value and jumped over.
fn assign_token(
    cx: &MatchCx<'_>,
    assign: &ConceptMatch,
    value: Span,
    site: Span,
    spelled: &str,
) -> Option<Realization> {
    let (file, src) = (cx.file, &*cx.file.source);
    let stmt = innermost_statement(file, &assign.span)?;
    let target = assign.one(Capture::Target)?.text(src);
    let op = cx.profile.tokens.spell(Token::Assign)?;
    let mutated = splice(src, value, site, spelled);
    let text = format!("{target} {op} {mutated}");
    let requires = Requires::ASSIGN_IN_EVAL | calls_in(file, &value);
    let action = Action::EvalThenJump {
        text,
        target: target.to_string(),
        value: mutated,
        resume: Target::AfterStatement(stmt.span),
    };
    Realization::of(stmt.span.start, action, site, requires, stmt.span.text(src))
}

/// A token of a compound assignment (`t op= v`), which assigns `t (op) v`:
/// the swapped operator when the swap is the compound token, else the
/// mutated value.
fn compound_assign_token(
    cx: &MatchCx<'_>,
    assign: &ConceptMatch,
    site: Span,
    spelled: &str,
    with: Option<Token>,
) -> Option<Realization> {
    let (file, src) = (cx.file, &*cx.file.source);
    let Some(stmt) = innermost_statement(file, &assign.span) else {
        // A match arm's value, say: no statement to stop at.
        return in_place(cx, site, spelled);
    };
    let tokens = &cx.profile.tokens;
    let target = assign.one(Capture::Target)?.text(src);
    let op_span = assign.one(Capture::Op)?;
    let value_span = assign.one(Capture::Value)?;
    let (compound, value) = if op_span == site {
        (with?, value_span.text(src).to_string())
    } else {
        let op = tokens.token(op_span.text(src))?;
        (op, splice(src, value_span, site, spelled))
    };
    let base = tokens.spell(compound.compound_base()?)?;
    let action = Action::EvalThenJump {
        text: splice(src, assign.span, site, spelled),
        target: target.to_string(),
        value: format!("{target} {base} {value}"),
        resume: Target::AfterStatement(stmt.span),
    };
    let requires = Requires::ASSIGN_IN_EVAL | calls_in(file, &assign.span);
    Realization::of(stmt.span.start, action, site, requires, stmt.span.text(src))
}

/// The condition at `capture` negated.
fn wrap_not(cx: &MatchCx<'_>, capture: Capture) -> Option<Realization> {
    let src: &str = &cx.file.source;
    let cond = cx.m.one(capture)?;
    let (action, requires) = branch(cx.file, cx.m, cx.profile.tokens.wrap_not(cond.text(src)))?;
    Realization::of(cond.start, action, cond, requires, cond.text(src))
}

/// A match arm deleted: its body continues as the catch-all arm's body.
fn delete_arm(cx: &MatchCx<'_>) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let src: &str = &file.source;
    let site = with_separator(src, m.span, cx.profile.tokens.spell(Token::Sep));
    let scope = m.one(Capture::Scope)?;
    let from = m.one(Capture::Value)?;
    let to = file
        .matches
        .iter()
        .find(|o| {
            o.concept == Concept::Arm
                && o.one(Capture::Scope) == Some(scope)
                && o.one(Capture::CatchAll).is_some()
        })?
        .one(Capture::Value)?;
    Realization::of(
        from.start,
        Action::Redirect { from, to },
        site,
        Requires::empty(),
        m.span.text(src),
    )
}

/// A concept match (a field) deleted with its separator, in place.
fn delete_match(cx: &MatchCx<'_>) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let src: &str = &file.source;
    let site = with_separator(src, m.span, cx.profile.tokens.spell(Token::Sep));
    let scope = m.one(Capture::Scope)?;
    let stmt = innermost_statement(file, &scope);
    let anchor = stmt.map_or(scope, |s| s.span).text(src);
    let action = Action::InPlace {
        expr_text: splice(src, scope, site, ""),
    };
    Realization::of(scope.start, action, site, Requires::empty(), anchor)
}

/// The spelling of a boolean literal.
fn spell_bool<'a>(cx: &'a MatchCx<'_>, b: bool) -> Option<&'a str> {
    cx.profile
        .tokens
        .spell(if b { Token::True } else { Token::False })
}

/// A match guard replaced by a boolean literal, branching on it.
fn bool_guard(cx: &MatchCx<'_>, b: bool) -> Option<Realization> {
    let guard = cx.m.one(Capture::Guard)?;
    let spelled = spell_bool(cx, b)?;
    let (action, requires) = guard_branch(cx.file, cx.m, spelled.to_string())?;
    Realization::of(
        guard.start,
        action,
        guard,
        requires,
        guard.text(&cx.file.source),
    )
}

/// A boolean expression (a match guard) replaced where it stands.
fn bool_in_place(cx: &MatchCx<'_>, capture: Capture, b: bool) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let site = m.one(capture)?;
    let spelled = spell_bool(cx, b)?;
    let stmt = innermost_statement(file, &m.span);
    let anchor = stmt.map_or(m.span, |s| s.span).text(&file.source);
    let action = Action::InPlace {
        expr_text: spelled.to_string(),
    };
    Realization::of(site.start, action, site, Requires::empty(), anchor)
}

/// A returned value replaced by a boolean literal.
fn bool_return(cx: &MatchCx<'_>, capture: Capture, b: bool) -> Option<Realization> {
    let (file, m) = (cx.file, cx.m);
    let value = m.one(capture)?;
    let value_text = spell_bool(cx, b)?.to_string();
    let anchor = cx.stmt.map_or(m.span, |s| s.span).text(&file.source);
    let requires = Requires::FORCE_RETURN | calls_in(file, &value);
    Realization::of(
        m.span.start,
        Action::EvalReturn { value_text },
        value,
        requires,
        anchor,
    )
}

/// A function body replaced by the default value of its return type.
fn default_body(cx: &MatchCx<'_>, capture: Capture) -> Option<Realization> {
    let (src, m) = (&*cx.file.source, cx.m);
    let body = body_contents(
        src,
        m.one(capture)?,
        cx.profile.body_braces,
        &cx.profile.comments,
    );
    let returns = m.one(Capture::Returns)?.text(src).trim();
    let value_text = cx.profile.defaults.get(returns).cloned();
    let zero_bits = value_text.is_some();
    Realization::of(
        m.span.start,
        Action::ReturnDefault {
            value_text,
            zero_bits,
            async_fn: m.one(Capture::Async).is_some(),
        },
        body,
        Requires::FORCE_RETURN,
        "",
    )
}

/// A function body replaced by the literal `value`.
fn text_body(cx: &MatchCx<'_>, capture: Capture, value: &str) -> Option<Realization> {
    let (src, m) = (&*cx.file.source, cx.m);
    let body = body_contents(
        src,
        m.one(capture)?,
        cx.profile.body_braces,
        &cx.profile.comments,
    );
    let returns = m.one(Capture::Returns).map(|r| r.text(src).trim());
    let zero_bits = match returns {
        Some(r) => cx.profile.defaults.get(r).map(String::as_str) == Some(value),
        None => value == "()",
    };
    Realization::of(
        m.span.start,
        Action::ReturnDefault {
            value_text: Some(value.to_string()),
            zero_bits,
            async_fn: m.one(Capture::Async).is_some(),
        },
        body,
        Requires::FORCE_RETURN,
        "",
    )
}

enum TokenContext<'a> {
    /// The Condition match and its `cond` span.
    Condition(&'a ConceptMatch, Span),
    /// The Assign match and its `value` span.
    Assign(&'a ConceptMatch, Span),
    /// A compound assignment statement, whole (`t += k`).
    CompoundAssign(&'a ConceptMatch),
    /// A return value (of `return`, or a function's tail expression).
    Return(&'a ConceptMatch, Span),
    /// A match arm's guard.
    Guard(&'a ConceptMatch, Span),
}

/// The innermost evaluable context containing a token site: a condition's
/// `cond`, an assignment's `value`, a compound assignment, or a return value,
/// whichever span is smallest.
fn token_context<'a>(file: &'a FileSyntax, site: &Span) -> Option<TokenContext<'a>> {
    file.matches
        .iter()
        .filter(|m| representable(m))
        .filter_map(|m| match m.concept {
            Concept::Condition => m
                .one(Capture::Cond)
                .map(|s| (s, TokenContext::Condition(m, s))),
            Concept::Assign => m
                .one(Capture::Value)
                .map(|s| (s, TokenContext::Assign(m, s))),
            Concept::CompoundAssign => Some((m.span, TokenContext::CompoundAssign(m))),
            Concept::Return => m
                .one(Capture::Value)
                .map(|s| (s, TokenContext::Return(m, s))),
            Concept::Arm if m.one(Capture::Value).is_some() => m
                .one(Capture::Guard)
                .map(|s| (s, TokenContext::Guard(m, s))),
            _ => None,
        })
        .filter(|(span, _)| span.contains(site))
        .min_by_key(|(span, _)| (span.len(), span.start.byte))
        .map(|(_, ctx)| ctx)
}

/// Branch action for a match guard: true to the arm's body; false past the
/// arm, to the next one.
fn guard_branch(
    file: &FileSyntax,
    arm: &ConceptMatch,
    cond_text: String,
) -> Option<(Action, Requires)> {
    let guard = arm.one(Capture::Guard)?;
    let body = arm.one(Capture::Value)?;
    let action = Action::EvalBranch {
        cond_text,
        if_true: Target::BlockStart(body),
        if_false: Target::AfterStatement(arm.span),
    };
    Some((action, calls_in(file, &guard)))
}

/// Branch action for a condition: true to the then-block; false to the
/// first else-branch, or past the whole statement when there is none.
fn branch(
    file: &FileSyntax,
    cond_match: &ConceptMatch,
    cond_text: String,
) -> Option<(Action, Requires)> {
    let cond = cond_match.one(Capture::Cond)?;
    let then = cond_match.one(Capture::Then)?;
    let if_false = match cond_match.first(Capture::Else) {
        Some(e) => Target::BlockStart(e),
        None => Target::AfterStatement(innermost_statement(file, &cond_match.span)?.span),
    };
    let action = Action::EvalBranch {
        cond_text,
        if_true: Target::BlockStart(then),
        if_false,
    };
    Some((action, calls_in(file, &cond)))
}

fn calls_in(file: &FileSyntax, span: &Span) -> Requires {
    let any = file
        .matches
        .iter()
        .any(|m| m.concept == Concept::Call && span.contains(&m.span));
    if any {
        Requires::CALL_IN_EVAL
    } else {
        Requires::empty()
    }
}

/// `span` extended over the separator that follows it, if one does (on the
/// same line), so deleting a list element leaves a valid list.
fn with_separator(src: &str, span: Span, sep: Option<&str>) -> Span {
    let Some(sep) = sep else { return span };
    let rest = &src[span.end.byte..];
    let gap = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if !rest[gap..].starts_with(sep) {
        return span;
    }
    let end = span.end.byte + gap + sep.len();
    Span {
        start: span.start,
        end: crate::syntax::pos_at(src, end),
    }
}

/// `outer`'s text with `site` replaced by `with`.
fn splice(src: &str, outer: Span, site: Span, with: &str) -> String {
    format!(
        "{}{with}{}",
        &src[outer.start.byte..site.start.byte],
        &src[site.end.byte..outer.end.byte]
    )
}

fn enclosing_function(file: &FileSyntax, span: &Span) -> Option<String> {
    file.matches
        .iter()
        .filter(|m| m.concept == Concept::Function && m.span.contains(span))
        .min_by_key(|m| (m.span.len(), m.span.start.byte))
        .and_then(|m| m.one(Capture::Name))
        .map(|n| n.text(&file.source).to_string())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
