//! Abstract tokens and the per-language token table (design section 4.3).

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Token {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    RemAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShlAssign,
    ShrAssign,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Assign,
    True,
    False,
    /// Unary logical (or, in some languages, bitwise) not: `!x`, `not x`.
    Not,
    /// Unary arithmetic negation: `-x`.
    Neg,
    /// The separator between list elements (fields of a literal).
    Sep,
}

impl Token {
    pub const ALL: [Token; 34] = [
        Token::Add,
        Token::Sub,
        Token::Mul,
        Token::Div,
        Token::Rem,
        Token::BitAnd,
        Token::BitOr,
        Token::BitXor,
        Token::Shl,
        Token::Shr,
        Token::AddAssign,
        Token::SubAssign,
        Token::MulAssign,
        Token::DivAssign,
        Token::RemAssign,
        Token::BitAndAssign,
        Token::BitOrAssign,
        Token::BitXorAssign,
        Token::ShlAssign,
        Token::ShrAssign,
        Token::Lt,
        Token::Le,
        Token::Gt,
        Token::Ge,
        Token::Eq,
        Token::Ne,
        Token::And,
        Token::Or,
        Token::Assign,
        Token::True,
        Token::False,
        Token::Not,
        Token::Neg,
        Token::Sep,
    ];

    /// The key used in a profile's `tokens.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Token::Add => "add",
            Token::Sub => "sub",
            Token::Mul => "mul",
            Token::Div => "div",
            Token::Rem => "rem",
            Token::BitAnd => "bit_and",
            Token::BitOr => "bit_or",
            Token::BitXor => "bit_xor",
            Token::Shl => "shl",
            Token::Shr => "shr",
            Token::AddAssign => "add_assign",
            Token::SubAssign => "sub_assign",
            Token::MulAssign => "mul_assign",
            Token::DivAssign => "div_assign",
            Token::RemAssign => "rem_assign",
            Token::BitAndAssign => "bit_and_assign",
            Token::BitOrAssign => "bit_or_assign",
            Token::BitXorAssign => "bit_xor_assign",
            Token::ShlAssign => "shl_assign",
            Token::ShrAssign => "shr_assign",
            Token::Lt => "lt",
            Token::Le => "le",
            Token::Gt => "gt",
            Token::Ge => "ge",
            Token::Eq => "eq",
            Token::Ne => "ne",
            Token::And => "and",
            Token::Or => "or",
            Token::Assign => "assign",
            Token::True => "true",
            Token::False => "false",
            Token::Not => "not",
            Token::Neg => "neg",
            Token::Sep => "sep",
        }
    }

    pub fn from_name(name: &str) -> Option<Token> {
        Token::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The binary operator a compound assignment applies (`-=` -> `-`).
    pub fn compound_base(self) -> Option<Token> {
        use Token::*;
        Some(match self {
            AddAssign => Add,
            SubAssign => Sub,
            MulAssign => Mul,
            DivAssign => Div,
            RemAssign => Rem,
            BitAndAssign => BitAnd,
            BitOrAssign => BitOr,
            BitXorAssign => BitXor,
            ShlAssign => Shl,
            ShrAssign => Shr,
            _ => return None,
        })
    }

    /// Whether this operator binds tighter than `other`, where swapping one
    /// for the other regroups the source: logical and over logical or, as
    /// in every supported language (`&&`/`||`, `and`/`or`).
    pub fn binds_tighter(self, other: Token) -> bool {
        matches!((self, other), (Token::And, Token::Or))
    }

    /// Prefix operators of one operand. Their spellings are looked up apart
    /// from the rest, since `-` is both [`Token::Sub`] and [`Token::Neg`].
    pub fn is_unary(self) -> bool {
        matches!(self, Token::Not | Token::Neg)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenTableError {
    /// `not_template` must contain exactly one `{}` placeholder.
    BadNotTemplate(String),
    /// Two tokens share one spelling, so spelling-to-token is ambiguous.
    DuplicateSpelling(String),
}

impl std::fmt::Display for TokenTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenTableError::BadNotTemplate(t) => {
                write!(f, "not_template {t:?} must contain exactly one {{}}")
            }
            TokenTableError::DuplicateSpelling(s) => {
                write!(f, "spelling {s:?} is mapped to more than one token")
            }
        }
    }
}

impl std::error::Error for TokenTableError {}

/// Maps concrete spellings to abstract tokens and back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenTable {
    by_token: BTreeMap<Token, String>,
    by_spelling: BTreeMap<String, Token>,
    /// Spellings of unary tokens, a namespace of their own.
    unary: BTreeMap<String, Token>,
    not_template: String,
}

impl TokenTable {
    pub fn new(
        spellings: impl IntoIterator<Item = (Token, String)>,
        not_template: impl Into<String>,
    ) -> Result<Self, TokenTableError> {
        let not_template = not_template.into();
        if not_template.matches("{}").count() != 1 {
            return Err(TokenTableError::BadNotTemplate(not_template));
        }
        let mut by_token = BTreeMap::new();
        let mut by_spelling = BTreeMap::new();
        let mut unary = BTreeMap::new();
        for (token, spelling) in spellings {
            let names = if token.is_unary() {
                &mut unary
            } else {
                &mut by_spelling
            };
            if names
                .insert(spelling.clone(), token)
                .is_some_and(|prev| prev != token)
            {
                return Err(TokenTableError::DuplicateSpelling(spelling));
            }
            by_token.insert(token, spelling);
        }
        Ok(TokenTable {
            by_token,
            by_spelling,
            unary,
            not_template,
        })
    }

    pub fn token(&self, spelling: &str) -> Option<Token> {
        self.by_spelling.get(spelling).copied()
    }

    /// The unary token a prefix operator spelling denotes.
    pub fn unary(&self, spelling: &str) -> Option<Token> {
        self.unary.get(spelling).copied()
    }

    pub fn spell(&self, token: Token) -> Option<&str> {
        self.by_token.get(&token).map(String::as_str)
    }

    /// Render `inner` through the profile's negation template.
    pub fn wrap_not(&self, inner: &str) -> String {
        self.not_template.replacen("{}", inner, 1)
    }

    /// The boolean a literal spelling denotes, if it is one.
    pub fn literal_bool(&self, text: &str) -> Option<bool> {
        match self.token(text)? {
            Token::True => Some(true),
            Token::False => Some(false),
            _ => None,
        }
    }
}
