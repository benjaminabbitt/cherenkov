//! cargo-mutants' spelling of a mutant, so a regular expression written for
//! its mutant names (`--exclude-re`, `exclude_re`) means the same here.
//!
//! Re-derived from cargo-mutants 26.2.0's behaviour (MIT, Copyright (c)
//! 2021-2025 Martin Pool; src/pretty.rs, src/visit.rs and src/mutant.rs are
//! the references): its token pretty-printer, the namespace of a function
//! (modules, `Type` or `<impl Trait for Type>`, traits, enclosing
//! functions), and the struct of a field initialiser. No code is copied.

use tree_sitter::Node;

/// A token of Rust source, as `proc_macro2` splits it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Literal(String),
    Punct(char),
    Group(char, Vec<Tok>),
}

/// `text` as cargo-mutants prints a token stream: no space before `<`, `>`,
/// `:`, `.`, `!`, `,` and `;`, one after a comma, `;` and `->`, and around
/// `=`, `=>` and `|`.
pub fn pretty(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let toks = lex(&chars, &mut at, None);
    print(&toks)
}

fn print(toks: &[Tok]) -> String {
    let mut out = String::new();
    for (i, tok) in toks.iter().enumerate() {
        let next = toks.get(i + 1);
        match tok {
            Tok::Punct(c) => print_punct(&mut out, *c, next.is_some()),
            Tok::Ident(s) | Tok::Literal(s) => print_word(&mut out, s, next),
            Tok::Group(open, inner) => {
                out.push(*open);
                out.push_str(&print(inner));
                out.push(close_of(*open));
            }
        }
    }
    out
}

fn print_punct(out: &mut String, c: char, more: bool) {
    if c == '|' && !out.is_empty() && !out.ends_with(' ') && !out.ends_with('|') {
        out.push(' ');
    }
    out.push(c);
    if more && (out.ends_with("->") || c == ',' || c == ';') {
        out.push(' ');
    }
}

fn print_word(out: &mut String, word: &str, next: Option<&Tok>) {
    if out.ends_with('=') || out.ends_with("=>") || out.ends_with('|') {
        out.push(' ');
    }
    out.push_str(word);
    let spaced = match next {
        Some(Tok::Ident(_) | Tok::Literal(_)) => true,
        Some(Tok::Punct(p)) => !matches!(p, ',' | ';' | '<' | '>' | ':' | '.' | '!'),
        Some(Tok::Group(..)) | None => false,
    };
    if spaced {
        out.push(' ');
    }
}

fn close_of(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

/// Tokens up to the `close` delimiter (or the end of the text).
fn lex(chars: &[char], at: &mut usize, close: Option<char>) -> Vec<Tok> {
    let mut out = Vec::new();
    while let Some(&c) = chars.get(*at) {
        if Some(c) == close {
            *at += 1;
            return out;
        }
        *at += 1;
        match c {
            c if c.is_whitespace() => {}
            '(' | '[' | '{' => out.push(Tok::Group(c, lex(chars, at, Some(close_of(c))))),
            '"' => out.push(Tok::Literal(string_from(chars, at, c))),
            '\'' => out.push_lifetime_or_char(chars, at),
            c if c.is_ascii_digit() => out.push(Tok::Literal(number_from(chars, at, c))),
            c if c.is_alphabetic() || c == '_' => out.push(Tok::Ident(word_from(chars, at, c))),
            c => out.push(Tok::Punct(c)),
        }
    }
    out
}

trait Push {
    fn push_lifetime_or_char(&mut self, chars: &[char], at: &mut usize);
}

impl Push for Vec<Tok> {
    /// A character literal (`'a'`, `'\n'`) or the tick of a lifetime.
    fn push_lifetime_or_char(&mut self, chars: &[char], at: &mut usize) {
        let closes = |n: usize| chars.get(*at + n) == Some(&'\'');
        if chars.get(*at) == Some(&'\\') || closes(1) {
            self.push(Tok::Literal(string_from(chars, at, '\'')));
        } else {
            self.push(Tok::Punct('\''));
        }
    }
}

/// A string or character literal whose opening quote was just read.
fn string_from(chars: &[char], at: &mut usize, quote: char) -> String {
    let mut s = quote.to_string();
    while let Some(&c) = chars.get(*at) {
        *at += 1;
        s.push(c);
        if c == '\\' {
            if let Some(&e) = chars.get(*at) {
                s.push(e);
                *at += 1;
            }
        } else if c == quote {
            break;
        }
    }
    s
}

fn number_from(chars: &[char], at: &mut usize, first: char) -> String {
    let mut s = first.to_string();
    while let Some(&c) = chars.get(*at) {
        let fraction = c == '.' && chars.get(*at + 1).is_some_and(char::is_ascii_digit);
        if !(c.is_alphanumeric() || c == '_' || fraction) {
            break;
        }
        s.push(c);
        *at += 1;
    }
    s
}

fn word_from(chars: &[char], at: &mut usize, first: char) -> String {
    let mut s = first.to_string();
    while let Some(&c) = chars.get(*at) {
        if !(c.is_alphanumeric() || c == '_') {
            break;
        }
        s.push(c);
        *at += 1;
    }
    s
}

/// `s` with each line break and the indentation after it made one space.
pub fn squash_lines(s: &str) -> String {
    let mut out = String::new();
    let mut in_indent = false;
    for c in s.chars() {
        match c {
            ' ' | '\t' | '\n' if in_indent => {}
            '\n' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                in_indent = true;
            }
            c => {
                in_indent = false;
                out.push(c);
            }
        }
    }
    out
}

/// `text` without the attributes and comments before its first token.
pub fn strip_attributes(text: &str) -> &str {
    let mut rest = text.trim_start();
    loop {
        let next = if rest.starts_with("#[") {
            attribute_end(rest)
        } else if rest.starts_with("//") {
            rest.find('\n').unwrap_or(rest.len())
        } else if rest.starts_with("/*") {
            rest.find("*/").map_or(rest.len(), |i| i + 2)
        } else {
            return rest;
        };
        rest = rest[next..].trim_start();
    }
}

/// The length of the attribute `#[..]` at the start of `text`.
fn attribute_end(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 2;
    lex(&chars, &mut at, Some(']'));
    chars[..at.min(chars.len())]
        .iter()
        .map(|c| c.len_utf8())
        .sum()
}

/// The pattern of a match arm's source text (`pat => body`), printed.
pub fn arm_pattern(arm: &str) -> String {
    let chars: Vec<char> = arm.chars().collect();
    let mut at = 0;
    let toks = lex(&chars, &mut at, None);
    let arrow = toks
        .windows(2)
        .position(|w| w[0] == Tok::Punct('=') && w[1] == Tok::Punct('>'))
        .unwrap_or(toks.len());
    print(&toks[..arrow])
}

/// A function: where it is, its name with its namespace and its return type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnName {
    pub start: usize,
    pub end: usize,
    /// `module::Type::function`, `<impl Trait for Type>::function`.
    pub qualified: String,
    /// `-> Type`, or empty.
    pub return_type: String,
}

/// The functions and struct expressions of a Rust file.
#[derive(Debug, Default)]
pub struct Names {
    fns: Vec<FnName>,
    /// Byte range of a struct expression, and its path.
    structs: Vec<(usize, usize, String)>,
}

impl Names {
    pub fn parse(source: &str) -> Names {
        let mut parser = tree_sitter::Parser::new();
        if parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .is_err()
        {
            return Names::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Names::default();
        };
        let mut names = Names::default();
        names.walk(tree.root_node(), source, &[]);
        names
    }

    /// The innermost function containing the bytes `start..end`.
    pub fn function_at(&self, start: usize, end: usize) -> Option<&FnName> {
        self.fns
            .iter()
            .filter(|f| f.start <= start && end <= f.end)
            .min_by_key(|f| f.end - f.start)
    }

    /// The path of the innermost struct expression containing `start..end`.
    pub fn struct_at(&self, start: usize, end: usize) -> Option<&str> {
        self.structs
            .iter()
            .filter(|(s, e, _)| *s <= start && end <= *e)
            .min_by_key(|(s, e, _)| e - s)
            .map(|(_, _, path)| path.as_str())
    }

    fn walk(&mut self, node: Node<'_>, src: &str, namespace: &[String]) {
        let mut inner = namespace.to_vec();
        match node.kind() {
            "mod_item" | "trait_item" => inner.extend(field_text(node, "name", src)),
            "impl_item" => inner.push(impl_name(node, src)),
            "function_item" => {
                inner.extend(field_text(node, "name", src));
                self.record_function(node, src, &inner);
            }
            "struct_expression" => self.record_struct(node, src),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, src, &inner);
        }
    }

    fn record_function(&mut self, node: Node<'_>, src: &str, path: &[String]) {
        let return_type = node
            .child_by_field_name("return_type")
            .map(|t| format!("-> {}", pretty(&src[t.byte_range()])))
            .unwrap_or_default();
        self.fns.push(FnName {
            start: node.start_byte(),
            end: node.end_byte(),
            qualified: path.join("::"),
            return_type: return_type.replace("-> -> ", "-> "),
        });
    }

    fn record_struct(&mut self, node: Node<'_>, src: &str) {
        if let Some(name) = field_text(node, "name", src) {
            self.structs
                .push((node.start_byte(), node.end_byte(), pretty(&name)));
        }
    }
}

fn field_text(node: Node<'_>, field: &str, src: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| src[n.byte_range()].to_string())
}

/// `Type` of an inherent impl, `<impl Trait for Type>` of a trait impl.
fn impl_name(node: Node<'_>, src: &str) -> String {
    let ty = pretty(&field_text(node, "type", src).unwrap_or_default());
    match field_text(node, "trait", src) {
        Some(tr) => format!("<impl {} for {ty}>", pretty(&tr)),
        None => ty,
    }
}
