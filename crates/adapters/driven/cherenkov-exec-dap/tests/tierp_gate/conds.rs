//! The gate's condition snippets: a comparison (`ror`) or a short circuit
//! (`lcr`) in an `if` condition or in value position (a returned bool, a
//! closure's result), whose operands are locals, fields, calls of user
//! functions and methods of `Vec`, `Option` and `BTreeMap`, for signed,
//! unsigned and float types.

use crate::snippets::{FLOATS, Form, INTS, Snippet};

const COMPARISONS: [&str; 6] = ["<", "<=", ">", ">=", "==", "!="];

/// Where the condition is: what consumes it.
#[derive(Clone, Copy)]
enum Place {
    /// `if COND { 1 } else { 2 }`
    If,
    /// The function returns the bool.
    Value,
    /// A closure returns the bool, the function calls it.
    Closure,
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Place::If => "if",
            Place::Value => "value",
            Place::Closure => "closure",
        }
    }

    fn ret(self) -> &'static str {
        match self {
            Place::If => "u8",
            Place::Value | Place::Closure => "bool",
        }
    }
}

/// An operand form: the parameters of the function, the shim's body over
/// inputs `a` and `b` (`@F` is the function), and whether it needs one
/// input only.
struct Operands {
    name: &'static str,
    /// The function's parameter list, with `ty` for the operand type.
    params: &'static str,
    shim: &'static str,
    one_input: bool,
    /// Whether only `usize` has it (a length is a `usize`).
    usize_only: bool,
}

/// The operands of a comparison `LHS op RHS`.
const COMPARED: [(Operands, &str, &str); 11] = [
    (
        Operands {
            name: "regreg",
            params: "a: @T, b: @T",
            shim: "@F(a, b)",
            one_input: false,
            usize_only: false,
        },
        "a",
        "b",
    ),
    (
        Operands {
            name: "imm",
            params: "a: @T",
            shim: "@F(a)",
            one_input: true,
            usize_only: false,
        },
        "a",
        "@3",
    ),
    (
        Operands {
            name: "immfirst",
            params: "a: @T",
            shim: "@F(a)",
            one_input: true,
            usize_only: false,
        },
        "@3",
        "a",
    ),
    (
        Operands {
            name: "field",
            params: "s: &S<@T>, k: @T",
            shim: "@F(&S { x: a }, b)",
            one_input: false,
            usize_only: false,
        },
        "s.x",
        "k",
    ),
    (
        Operands {
            name: "call",
            params: "a: @T, b: @T",
            shim: "@F(a, b)",
            one_input: false,
            usize_only: false,
        },
        "id(a)",
        "b",
    ),
    (
        Operands {
            name: "callcall",
            params: "a: @T, b: @T",
            shim: "@F(a, b)",
            one_input: false,
            usize_only: false,
        },
        "id(a)",
        "id(b)",
    ),
    (
        Operands {
            name: "option",
            params: "o: Option<@T>, k: @T",
            shim: "@F(Some(a), b)",
            one_input: false,
            usize_only: false,
        },
        "o.unwrap_or(@0)",
        "k",
    ),
    (
        Operands {
            name: "vec-len",
            params: "v: &Vec<u8>, n: usize",
            shim: "@F(&vec![0u8; (a % 6) as usize], b)",
            one_input: false,
            usize_only: true,
        },
        "v.len()",
        "n",
    ),
    (
        Operands {
            name: "vec-len-imm",
            params: "v: &Vec<u8>",
            shim: "@F(&vec![0u8; (a % 6) as usize])",
            one_input: true,
            usize_only: true,
        },
        "v.len()",
        "3",
    ),
    (
        Operands {
            name: "map-len",
            params: "m: &std::collections::BTreeMap<u8, u8>, n: usize",
            shim: "@F(&(0..(a % 6) as u8).map(|k| (k, k)).collect(), b)",
            one_input: false,
            usize_only: true,
        },
        "m.len()",
        "n",
    ),
    (
        Operands {
            name: "field-imm",
            params: "s: &S<@T>",
            shim: "@F(&S { x: a })",
            one_input: true,
            usize_only: false,
        },
        "s.x",
        "@12",
    ),
];

/// The text with `@T`, `@0`, `@3` and `@12` as the type and its literals.
fn fill(text: &str, ty: &str) -> String {
    let lit = |n: &str| {
        if ty.starts_with('f') {
            format!("{n}.0")
        } else {
            n.to_string()
        }
    };
    text.replace("@T", ty)
        .replace("@12", &lit("12"))
        .replace("@3", &lit("3"))
        .replace("@0", &lit("0"))
}

/// The function's text around the condition `cond`.
fn function(place: Place, params: &str, cond: &str, ty: &str) -> String {
    let params = fill(params, ty);
    match place {
        Place::If => format!("pub fn f({params}) -> u8 {{ if {cond} {{ 1 }} else {{ 2 }} }}"),
        Place::Value => format!("pub fn f({params}) -> bool {{ {cond} }}"),
        Place::Closure => {
            format!("pub fn f({params}) -> bool {{ let g = || {cond}; g() }}")
        }
    }
}

fn snippet(
    ty: &'static str,
    place: Place,
    label: String,
    params: &str,
    cond: &str,
    shim: &str,
    one_input: bool,
) -> Snippet {
    Snippet {
        ty,
        form: Form::Cond,
        compound: false,
        source: format!(
            "use crate::support::*;\n#[inline(never)]\n{}\n",
            function(place, params, cond, ty)
        ),
        ret: place.ret(),
        label,
        one_input,
        shim: Some(fill(shim, ty)),
    }
}

fn comparisons(ty: &'static str, places: &[Place]) -> Vec<Snippet> {
    let mut out = Vec::new();
    for (form, lhs, rhs) in &COMPARED {
        if form.usize_only && ty != "usize" {
            continue;
        }
        for &op in &COMPARISONS {
            for &place in places {
                let cond = fill(&format!("{lhs} {op} {rhs}"), ty);
                let label = format!("{} {}", place.name(), form.name);
                out.push(snippet(
                    ty,
                    place,
                    label,
                    form.params,
                    &cond,
                    form.shim,
                    form.one_input,
                ));
            }
        }
    }
    out
}

/// The left and right operands of a short circuit: bool expressions of the
/// inputs, with the parameters and shim they need.
struct Logical {
    name: &'static str,
    params: &'static str,
    shim: &'static str,
    /// The condition's text; `OP` is the connector.
    cond: &'static str,
}

const LOGICAL: [Logical; 7] = [
    Logical {
        name: "plain",
        params: "a: @T, b: @T",
        shim: "@F(a, b)",
        cond: "a < @3 OP b > @3",
    },
    Logical {
        name: "call-left",
        params: "a: @T, b: @T",
        shim: "@F(a, b)",
        cond: "id(a) < @3 OP b > @3",
    },
    Logical {
        name: "call-right",
        params: "a: @T, b: @T",
        shim: "@F(a, b)",
        cond: "a < @3 OP id(b) > @3",
    },
    Logical {
        name: "bool-call",
        params: "a: @T, b: @T",
        shim: "@F(a, b)",
        cond: "id(a < @3) OP id(b > @3)",
    },
    Logical {
        name: "vec-empty-not",
        params: "v: &Vec<u8>, f: bool",
        shim: "@F(&vec![0u8; (a as u64 % 2) as usize], b > @3)",
        cond: "v.is_empty() OP !f",
    },
    Logical {
        name: "cursor-out",
        params: "s: &S<@T>, o: &Option<u8>",
        shim: "@F(&S { x: a }, &if b > @3 { Some(1u8) } else { None })",
        cond: "s.x > @0 OP o.is_some()",
    },
    Logical {
        name: "three",
        params: "a: @T, b: @T",
        shim: "@F(a, b)",
        cond: "a < @3 OP b > @3 OP a > b",
    },
];

fn logicals(ty: &'static str, places: &[Place]) -> Vec<Snippet> {
    let mut out = Vec::new();
    for l in &LOGICAL {
        for op in ["&&", "||"] {
            for &place in places {
                let cond = fill(&l.cond.replace("OP", op), ty);
                let shim = fill(l.shim, ty);
                let label = format!("lcr {} {}", place.name(), l.name);
                out.push(snippet(ty, place, label, l.params, &cond, &shim, false));
            }
        }
    }
    out
}

/// Every condition snippet.
pub fn all() -> Vec<Snippet> {
    let mut out = Vec::new();
    let all = [Place::If, Place::Value, Place::Closure];
    let wide: &[&str] = &["i8", "i32", "i64", "u8", "u32", "usize"];
    for &ty in INTS.iter().filter(|t| wide.contains(t)) {
        out.extend(comparisons(ty, &all));
    }
    for ty in FLOATS {
        out.extend(comparisons(ty, &all));
    }
    for ty in ["i32", "u64", "u8", "f64"] {
        out.extend(logicals(ty, &all));
    }
    out
}
