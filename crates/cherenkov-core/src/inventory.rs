//! The operator inventory, for documentation: every operator (family) with
//! its replacements, their language-neutral definitions, sources, the names
//! other tools use for them, and the spellings a language gives them.
//!
//! The dump is data (`Inventory`, serializable) and Markdown. Docs are
//! generated from it, so docs, source comments and `Origin` data agree.
//!
//! Naming: an operator is named by its family id (`ror`), a replacement by
//! `family/detail` (`ror/lt->le`), the name mutant IDs and set files use.
//! Aliases are what other tools call it; they document and are never
//! accepted as input, since one tool name can cover several replacements.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::operator_set::{
    Family, Mutation, OperatorSet, Relation, Rule, families, family_definition, registry,
};
use crate::token::{Token, TokenTable};

/// The JSON document's version: changes when a key is removed or renamed.
pub const SCHEMA: u32 = 1;

/// The per-language token spellings the dump may include.
pub struct Spellings<'a> {
    languages: Vec<(&'a str, &'a TokenTable)>,
}

impl<'a> Spellings<'a> {
    /// No language's spellings: only the language-neutral definitions.
    pub fn none() -> Self {
        Spellings {
            languages: Vec::new(),
        }
    }

    pub fn of(languages: &[(&'a str, &'a TokenTable)]) -> Self {
        Spellings {
            languages: languages.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub schema: u32,
    /// The set this view is of, when it is one set's.
    pub set: Option<SetEntry>,
    pub operators: Vec<OperatorEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetEntry {
    pub name: &'static str,
    pub summary: &'static str,
    pub source: &'static str,
    pub pending: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorEntry {
    /// The family id: the operator id in mutant IDs.
    pub name: &'static str,
    pub definition: &'static str,
    /// Names other tools give the operator as a whole.
    pub aliases: Vec<String>,
    /// Every source of its replacements, once each.
    pub sources: Vec<OriginEntry>,
    pub replacements: Vec<ReplacementEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplacementEntry {
    /// `family/detail`: the stable name sets and mutant IDs use.
    pub name: &'static str,
    pub detail: &'static str,
    pub definition: String,
    pub origins: Vec<OriginEntry>,
    /// Names other tools give it, as `tool NAME`.
    pub aliases: Vec<String>,
    /// The registered sets that use it.
    pub sets: Vec<&'static str>,
    /// By language id; empty unless spellings were asked for.
    pub spellings: BTreeMap<String, Spelling>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OriginEntry {
    pub source: &'static str,
    pub name: Option<&'static str>,
    /// `derived`, `same` or `similar`.
    pub relation: &'static str,
    pub note: Option<&'static str>,
}

/// A replacement as one language writes it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Spelling {
    pub from: Option<String>,
    pub with: Option<String>,
    /// cargo-mutants' name for the mutant's change (Rust only).
    pub cargo_mutants: Option<String>,
}

impl Inventory {
    /// Every operator with every replacement of every set.
    pub fn of_all(spellings: &Spellings<'_>) -> Inventory {
        let operators = families()
            .iter()
            .map(|f| operator_entry(f, &f.replacements, spellings))
            .collect();
        Inventory {
            schema: SCHEMA,
            set: None,
            operators,
        }
    }

    /// The operators `set` uses, each with `set`'s table only.
    pub fn of_set(set: &OperatorSet, spellings: &Spellings<'_>) -> Inventory {
        let all = families();
        let operators = set
            .tables()
            .iter()
            .filter_map(|t| {
                let f = all.iter().find(|f| f.id == t.family)?;
                Some(operator_entry(f, &t.replacements, spellings))
            })
            .collect();
        Inventory {
            schema: SCHEMA,
            set: Some(SetEntry {
                name: set.name,
                summary: set.summary,
                source: set.source,
                pending: set.pending.to_vec(),
            }),
            operators,
        }
    }
}

fn operator_entry(
    family: &Family,
    table: &[&'static Mutation],
    spellings: &Spellings<'_>,
) -> OperatorEntry {
    let mut sources: Vec<OriginEntry> = Vec::new();
    for o in table.iter().flat_map(|m| origin_entries(m)) {
        if !sources.contains(&o) {
            sources.push(o);
        }
    }
    OperatorEntry {
        name: family.id,
        definition: family.definition,
        // Of the whole family: a set's smaller table must not narrow what
        // the operator is called.
        aliases: family_aliases(&family.replacements),
        sources,
        replacements: table
            .iter()
            .map(|m| replacement_entry(m, spellings))
            .collect(),
    }
}

fn origin_entries(m: &Mutation) -> Vec<OriginEntry> {
    m.origins
        .iter()
        .map(|o| OriginEntry {
            source: o.source,
            name: o.name,
            relation: match o.relation {
                Relation::Derived => "derived",
                Relation::Same => "same",
                Relation::Similar(_) => "similar",
            },
            note: match o.relation {
                Relation::Similar(note) => Some(note),
                _ => None,
            },
        })
        .collect()
}

fn aliases_of(m: &Mutation) -> Vec<String> {
    m.origins
        .iter()
        .filter(|o| o.source != "Cherenkov design")
        // A similar mutation is another mutation, not another name.
        .filter(|o| !matches!(o.relation, Relation::Similar(_)))
        .filter_map(|o| o.name.map(|n| format!("{} {n}", o.source)))
        .collect()
}

/// The names every replacement of the table has: the family's own.
fn family_aliases(table: &[&Mutation]) -> Vec<String> {
    let Some(first) = table.first() else {
        return Vec::new();
    };
    aliases_of(first)
        .into_iter()
        .filter(|a| table.iter().all(|m| aliases_of(m).contains(a)))
        .collect()
}

/// The canonical names a tool's `alias` is for: an operator's name when the
/// alias names the operator as a whole, else each replacement's. Empty for
/// anything that is not an alias.
pub fn canonical_names(alias: &str) -> Vec<&'static str> {
    let mut names = Vec::new();
    for f in families() {
        if family_aliases(&f.replacements).iter().any(|a| a == alias) {
            names.push(f.id);
            continue;
        }
        names.extend(
            f.replacements
                .iter()
                .filter(|m| aliases_of(m).iter().any(|a| a == alias))
                .map(|m| m.name),
        );
    }
    names
}

fn replacement_entry(m: &'static Mutation, spellings: &Spellings<'_>) -> ReplacementEntry {
    ReplacementEntry {
        name: m.name,
        detail: m
            .name
            .strip_prefix(m.family)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(m.name),
        definition: definition_of(m),
        origins: origin_entries(m),
        aliases: aliases_of(m),
        sets: registry()
            .iter()
            .filter(|s| s.mutations.contains(&m.name))
            .map(|s| s.name)
            .collect(),
        spellings: spellings
            .languages
            .iter()
            .filter_map(|(lang, tokens)| {
                spelling_of(m, lang, tokens).map(|s| (lang.to_string(), s))
            })
            .collect(),
    }
}

/// What a token is called in a definition: no language's spelling.
fn word(t: Token) -> &'static str {
    match t {
        Token::Add => "addition",
        Token::Sub => "subtraction",
        Token::Mul => "multiplication",
        Token::Div => "division",
        Token::Rem => "remainder",
        Token::BitAnd => "bitwise and",
        Token::BitOr => "bitwise or",
        Token::BitXor => "bitwise xor",
        Token::Shl => "left shift",
        Token::Shr => "right shift",
        Token::AddAssign => "addition assignment",
        Token::SubAssign => "subtraction assignment",
        Token::MulAssign => "multiplication assignment",
        Token::DivAssign => "division assignment",
        Token::RemAssign => "remainder assignment",
        Token::BitAndAssign => "bitwise-and assignment",
        Token::BitOrAssign => "bitwise-or assignment",
        Token::BitXorAssign => "bitwise-xor assignment",
        Token::ShlAssign => "left-shift assignment",
        Token::ShrAssign => "right-shift assignment",
        Token::Lt => "less-than",
        Token::Le => "less-or-equal",
        Token::Gt => "greater-than",
        Token::Ge => "greater-or-equal",
        Token::Eq => "equal",
        Token::Ne => "not-equal",
        Token::And => "logical and",
        Token::Or => "logical or",
        Token::Assign => "assignment",
        Token::True => "true",
        Token::False => "false",
        Token::Not => "logical not",
        Token::Neg => "negation",
        Token::Sep => "separator",
    }
}

/// The replacement's definition, in terms of the concept it works on.
fn definition_of(m: &Mutation) -> String {
    match m.rule {
        Rule::ReplaceToken { from, with } => {
            format!("Replace {} with {}.", word(from), word(with))
        }
        Rule::DeleteUnary { token } => {
            format!("Delete the {} operator, keeping its operand.", word(token))
        }
        Rule::MatchGuard { value } => {
            format!("Replace a match arm's guard with the constant {value}.")
        }
        _ => family_definition(m.family).map_or_else(String::new, str::to_string),
    }
}

fn spelling_of(m: &Mutation, lang: &str, tokens: &TokenTable) -> Option<Spelling> {
    let (from, with) = match m.rule {
        Rule::ReplaceToken { from, with } => (tokens.spell(from)?, Some(tokens.spell(with)?)),
        Rule::DeleteUnary { token } => (tokens.spell(token)?, None),
        _ => return None,
    };
    let cargo_mutants = (lang == "rust").then(|| match with {
        Some(w) => format!("replace {from} with {w}"),
        None => format!("delete {from}"),
    });
    Some(Spelling {
        from: Some(from.to_string()),
        with: with.map(str::to_string),
        cargo_mutants,
    })
}

// region markdown

impl Inventory {
    /// The inventory as Markdown: the naming scheme, a table of operators
    /// and, under each, its replacements.
    pub fn markdown(&self) -> String {
        let mut out = String::new();
        if let Some(set) = &self.set {
            out.push_str(&format!("Set `{}`: {}\n\n", set.name, set.summary));
        }
        if self.set.is_none() {
            out.push_str(NAMING);
            out.push('\n');
        }
        out.push_str(&format!("{} operators.\n\n", self.operators.len()));
        out.push_str("| Operator | Definition | Derived from | Also called |\n|---|---|---|---|\n");
        for o in &self.operators {
            out.push_str(&operator_row(o));
        }
        for o in &self.operators {
            out.push_str(&operator_section(o));
        }
        out
    }
}

const NAMING: &str = "Names: an operator is its family id (`ror`); a replacement is `family/detail` \
(`ror/lt->le`), the name mutant IDs and set files use, and a set file naming a family takes its \
whole table. \"Also called\" lists the names other tools use: documentation only, never accepted \
as input, since one tool name can cover several replacements.\n";

fn derived_from(sources: &[OriginEntry]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for o in sources.iter().filter(|o| o.relation == "derived") {
        let text = match o.name {
            Some(n) => format!("{} {n}", o.source),
            None => o.source.to_string(),
        };
        if !seen.contains(&text) {
            seen.push(text);
        }
    }
    seen.join("; ")
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn operator_row(o: &OperatorEntry) -> String {
    format!(
        "| `{}` | {} | {} | {} |\n",
        o.name,
        cell(o.definition),
        cell(&derived_from(&o.sources)),
        cell(&o.aliases.join("; "))
    )
}

/// The languages that spell a replacement the same way, grouped:
/// "`<` -> `<=` (go, python, rust)".
fn spelled(r: &ReplacementEntry) -> String {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for (lang, s) in &r.spellings {
        let text = match (&s.from, &s.with) {
            (Some(f), Some(w)) => format!("`{f}` -> `{w}`"),
            (Some(f), None) => format!("delete `{f}`"),
            _ => continue,
        };
        match groups.iter_mut().find(|(t, _)| *t == text) {
            Some((_, langs)) => langs.push(lang),
            None => groups.push((text, vec![lang])),
        }
    }
    let parts: Vec<String> = groups
        .iter()
        .map(|(t, langs)| format!("{t} ({})", langs.join(", ")))
        .collect();
    cell(&parts.join("; "))
}

fn replacement_row(r: &ReplacementEntry, own_aliases: &[&String], with_spelling: bool) -> String {
    let sources: Vec<String> = r
        .origins
        .iter()
        .map(|s| {
            let name = s.name.map(|n| format!(" {n}")).unwrap_or_default();
            format!("{} {}{name}", s.relation, s.source)
        })
        .collect();
    let spelling = if with_spelling {
        format!(" {} |", spelled(r))
    } else {
        String::new()
    };
    let aliases: Vec<&str> = own_aliases.iter().map(|a| a.as_str()).collect();
    format!(
        "| `{}` | {} | {} | {} | {} |{spelling}\n",
        r.name,
        cell(&r.definition),
        cell(&sources.join("; ")),
        cell(&aliases.join("; ")),
        r.sets.join(", ")
    )
}

fn operator_section(o: &OperatorEntry) -> String {
    let with_spelling = o.replacements.iter().any(|r| !r.spellings.is_empty());
    let (head, rule) = if with_spelling {
        (" Spelling |", "---|")
    } else {
        ("", "")
    };
    let mut out = format!(
        "\n### `{}`\n\n| Replacement | Definition | Sources | Also called | Sets |{head}\n|---|---|---|---|---|{rule}\n",
        o.name
    );
    for r in &o.replacements {
        // The operator's own aliases are on the operator, not repeated here.
        let own: Vec<&String> = r
            .aliases
            .iter()
            .filter(|a| !o.aliases.contains(a))
            .collect();
        out.push_str(&replacement_row(r, &own, with_spelling));
    }
    out
}
