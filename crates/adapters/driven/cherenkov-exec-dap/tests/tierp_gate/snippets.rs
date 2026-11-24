//! The source snippets of the gate: one function per (type, operand form,
//! operator), each in a file of its own so that the planner's positions are
//! the compiled line table's.

/// The integer types the gate covers.
pub const INTS: [&str; 10] = [
    "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize",
];
/// The floating-point types.
pub const FLOATS: [&str; 2] = ["f32", "f64"];

/// How the operands of the operator come to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    RegReg,
    Imm,
    ImmOne,
    ImmBig,
    ImmFirst,
    Field,
    CallArg,
    Chain,
    MutRef,
    Entry,
    Idx,
    /// A condition (a comparison or `&&`/`||`): see `conds`.
    Cond,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::RegReg => "regreg",
            Form::Imm => "imm",
            Form::ImmOne => "imm1",
            Form::ImmBig => "immbig",
            Form::ImmFirst => "immfirst",
            Form::Field => "field",
            Form::CallArg => "callarg",
            Form::Chain => "chain",
            Form::MutRef => "mutref",
            Form::Entry => "entry",
            Form::Idx => "idx",
            Form::Cond => "cond",
        }
    }

    /// Whether the function takes one input (the other operand is a literal).
    pub fn unary(self) -> bool {
        matches!(
            self,
            Form::Imm | Form::ImmOne | Form::ImmBig | Form::ImmFirst | Form::Entry
        )
    }
}

/// One snippet: a function `f` over `ty` applying an operator.
#[derive(Debug, Clone)]
pub struct Snippet {
    pub ty: &'static str,
    pub form: Form,
    pub compound: bool,
    pub source: String,
    /// The function's result type (the operand type for arithmetic).
    pub ret: &'static str,
    /// What the snippet is, for a report.
    pub label: String,
    /// Whether the function takes one input of `ty` (else two).
    pub one_input: bool,
    /// The body of the driver's shim over inputs `a` and `b`, where the
    /// function does not take them as they are.
    pub shim: Option<String>,
}

fn literal(ty: &str, form: Form) -> String {
    let float = ty.starts_with('f');
    let digits = match (form, ty) {
        (Form::ImmOne | Form::Entry, _) => "1",
        (Form::ImmBig, "i8" | "u8") => "100",
        (Form::ImmBig, _) => "1000",
        _ => "3",
    };
    if float {
        format!("{digits}.0")
    } else {
        digits.to_string()
    }
}

/// The function's text for a plain operator, or a compound assignment.
fn function(ty: &str, form: Form, op: &str, compound: bool) -> Option<String> {
    let eq = if compound { "=" } else { "" };
    let lit = literal(ty, form);
    let text = match (compound, form) {
        (false, Form::RegReg) => format!("pub fn f(a: {ty}, b: {ty}) -> {ty} {{ a {op} b }}"),
        (false, Form::Imm | Form::ImmOne | Form::ImmBig) => {
            format!("pub fn f(a: {ty}) -> {ty} {{ a {op} {lit} }}")
        }
        (false, Form::ImmFirst) => format!("pub fn f(a: {ty}) -> {ty} {{ {lit} {op} a }}"),
        (false, Form::Field) => {
            format!("pub fn f(s: &S<{ty}>, k: {ty}) -> {ty} {{ s.x {op} k }}")
        }
        (false, Form::CallArg) => {
            format!("pub fn f(a: {ty}, b: {ty}) -> {ty} {{ id(a {op} b) }}")
        }
        (false, Form::Chain) => {
            format!("pub fn f(a: {ty}, b: {ty}) -> {ty} {{ id(a) {op} id(b) }}")
        }
        (true, Form::RegReg) => {
            format!("pub fn f(a: {ty}, b: {ty}) -> {ty} {{ let mut a = a; a {op}{eq} b; a }}")
        }
        (true, Form::Imm | Form::ImmOne | Form::ImmBig) => {
            format!("pub fn f(a: {ty}) -> {ty} {{ let mut a = a; a {op}{eq} {lit}; a }}")
        }
        (true, Form::Field) => format!("pub fn f(s: &mut S<{ty}>, k: {ty}) {{ s.x {op}{eq} k; }}"),
        (true, Form::MutRef) => format!("pub fn f(m: &mut {ty}, k: {ty}) {{ *m {op}{eq} k; }}"),
        (true, Form::Entry) => format!(
            "pub fn f(m: &mut std::collections::HashMap<u32, {ty}>, k: u32) {{ \
             *m.entry(k).or_default() {op}{eq} {lit}; }}"
        ),
        (true, Form::Idx) => {
            format!("pub fn f(v: &mut [{ty}], i: usize, k: {ty}) {{ v[i] {op}{eq} k; }}")
        }
        _ => return None,
    };
    Some(text)
}

const PLAIN: [Form; 8] = [
    Form::RegReg,
    Form::Imm,
    Form::ImmOne,
    Form::ImmBig,
    Form::ImmFirst,
    Form::Field,
    Form::CallArg,
    Form::Chain,
];
const COMPOUND: [Form; 8] = [
    Form::RegReg,
    Form::Imm,
    Form::ImmOne,
    Form::ImmBig,
    Form::Field,
    Form::MutRef,
    Form::Entry,
    Form::Idx,
];

fn snippet(ty: &'static str, form: Form, op: &'static str, compound: bool) -> Option<Snippet> {
    let body = function(ty, form, op, compound)?;
    Some(Snippet {
        ty,
        form,
        compound,
        source: format!("use crate::support::*;\n#[inline(never)]\n{body}\n"),
        ret: ty,
        label: form.name().to_string(),
        one_input: form.unary(),
        shim: None,
    })
}

fn of_type(ty: &'static str, ops: &[&'static str]) -> Vec<Snippet> {
    let mut out = Vec::new();
    for &op in ops {
        for (compound, forms) in [(false, &PLAIN), (true, &COMPOUND)] {
            out.extend(forms.iter().filter_map(|&f| snippet(ty, f, op, compound)));
        }
    }
    out
}

/// Every snippet of the gate.
pub fn all() -> Vec<Snippet> {
    let mut out = Vec::new();
    for ty in INTS {
        out.extend(of_type(ty, &["+", "-", "*"]));
        for &form in &COMPOUND {
            for op in ["&", "|", "^"] {
                out.extend(snippet(ty, form, op, true));
            }
        }
    }
    for ty in FLOATS {
        out.extend(of_type(ty, &["+", "-", "*", "/"]));
    }
    out.extend(crate::conds::all());
    out
}
