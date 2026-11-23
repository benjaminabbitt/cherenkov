//! The gate's function-value cases: for each result type the cargo-mutants
//! set makes trivial values for, a function with a body of its own, and the
//! mutant the planner makes of it for each value (`fn f(..) -> T { value }`).

use std::path::Path;
use std::sync::Arc;

use cherenkov_core::edit::{Edit, Lit};
use cherenkov_core::operator_set::registry;
use cherenkov_core::plan::{Action, MutantPlan, render_source};
use cherenkov_core::plan_sets::plan_file_sets;
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::profile::Profile;
use cherenkov_core::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

/// How the function receives its input `a` (a `u32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Params {
    None,
    Value,
    Reference,
    Two,
    /// A `&mut`: the original changes it, the stub and the mutant do not.
    Mutable,
}

pub const PARAMS: [Params; 5] = [
    Params::None,
    Params::Value,
    Params::Reference,
    Params::Two,
    Params::Mutable,
];

impl Params {
    fn signature(self) -> &'static str {
        match self {
            Params::None => "",
            Params::Value => "a: u32",
            Params::Reference => "a: &u32",
            Params::Two => "a: u32, b: u64",
            Params::Mutable => "a: &mut u32",
        }
    }

    /// What binds the input `a` as a `u32` in the body.
    fn prelude(self) -> &'static str {
        match self {
            Params::None => "let a = std::hint::black_box(7u32);",
            Params::Value => "",
            Params::Reference => "let a = *a;",
            Params::Two => "let a = a ^ (b as u32);",
            Params::Mutable => "*a = a.wrapping_add(1); let a = *a;",
        }
    }

    /// The call the driver makes with its input `a`, giving the result.
    pub fn call(self, module: &str) -> String {
        match self {
            Params::None => format!("{module}::f()"),
            Params::Value => format!("{module}::f(a)"),
            Params::Reference => format!("{module}::f(&a)"),
            Params::Two => format!("{module}::f(a, (a as u64) << 32)"),
            Params::Mutable => format!("{module}::f(&mut m)"),
        }
    }
}

/// A result type with a body that gives a value of it from `a`.
pub struct Ret {
    pub ty: &'static str,
    pub body: &'static str,
}

const fn r(ty: &'static str, body: &'static str) -> Ret {
    Ret { ty, body }
}

/// The result types of the gate.
pub const RESULTS: &[Ret] = &[
    r("bool", "a % 3 == 0"),
    r("i8", "a as i8"),
    r("i16", "a as i16"),
    r("i32", "a as i32"),
    r("i64", "a as i64"),
    r("isize", "a as isize"),
    r("u8", "a as u8"),
    r("u16", "a as u16"),
    r("u32", "a"),
    r("u64", "a as u64"),
    r("usize", "a as usize"),
    r("u128", "a as u128"),
    r("f64", "a as f64"),
    r(
        "Option<bool>",
        "if a % 2 == 0 { Some(a % 4 == 0) } else { None }",
    ),
    r(
        "Option<u8>",
        "if a % 2 == 0 { Some(a as u8) } else { None }",
    ),
    r(
        "Option<u16>",
        "if a % 2 == 0 { Some(a as u16) } else { None }",
    ),
    r("Option<u32>", "if a % 2 == 0 { Some(a) } else { None }"),
    r(
        "Option<i32>",
        "if a % 2 == 0 { Some(a as i32) } else { None }",
    ),
    r(
        "Option<u64>",
        "if a % 2 == 0 { Some(a as u64) } else { None }",
    ),
    r("Option<char>", "char::from_u32(a)"),
    r("Option<En>", "if a % 2 == 0 { Some(En::B) } else { None }"),
    r(
        "Option<&'static u32>",
        "if a % 2 == 0 { Some(&7) } else { None }",
    ),
    r(
        "Option<Box<u32>>",
        "if a % 2 == 0 { Some(Box::new(a)) } else { None }",
    ),
    r(
        "Option<std::num::NonZeroU32>",
        "std::num::NonZeroU32::new(a)",
    ),
    r(
        "Option<(u32, u32)>",
        "if a % 2 == 0 { Some((a, 1)) } else { None }",
    ),
    r(
        "Option<[u8; 3]>",
        "if a % 2 == 0 { Some([a as u8; 3]) } else { None }",
    ),
    r(
        "Option<f64>",
        "if a % 2 == 0 { Some(a as f64) } else { None }",
    ),
    r(
        "Option<String>",
        "if a % 2 == 0 { Some(a.to_string()) } else { None }",
    ),
    r(
        "Result<(), ()>",
        "if a % 2 == 0 { Ok(()) } else { Err(()) }",
    ),
    r(
        "Result<(), u8>",
        "if a % 2 == 0 { Ok(()) } else { Err(a as u8) }",
    ),
    r(
        "Result<(), u32>",
        "if a % 2 == 0 { Ok(()) } else { Err(a) }",
    ),
    r(
        "Result<(), u64>",
        "if a % 2 == 0 { Ok(()) } else { Err(a as u64) }",
    ),
    r(
        "Result<(), En>",
        "if a % 2 == 0 { Ok(()) } else { Err(En::C) }",
    ),
    r(
        "Result<(), bool>",
        "if a % 2 == 0 { Ok(()) } else { Err(a % 4 == 1) }",
    ),
    r(
        "Result<(), Box<u32>>",
        "if a % 2 == 0 { Ok(()) } else { Err(Box::new(a)) }",
    ),
    r(
        "Result<(), &'static str>",
        "if a % 2 == 0 { Ok(()) } else { Err(\"odd\") }",
    ),
    r(
        "Result<(), std::io::Error>",
        "if a % 2 == 0 { Ok(()) } else { Err(std::io::Error::from(std::io::ErrorKind::Other)) }",
    ),
    r(
        "Result<(), String>",
        "if a % 2 == 0 { Ok(()) } else { Err(a.to_string()) }",
    ),
];

/// One function and the mutant of it that replaces its body with `value`.
pub struct ValueCase {
    pub id: usize,
    pub result: &'static Ret,
    pub params: Params,
    pub value: String,
    pub plan: MutantPlan,
    pub original: String,
    pub mutated: String,
    /// `pub fn f() -> T { value }`: what rustc makes of the value alone.
    pub bare: String,
}

fn source(result: &Ret, params: Params) -> String {
    format!(
        "use crate::support::*;\n#[inline(never)]\npub fn f({}) -> {} {{ {} {} }}\n",
        params.signature(),
        result.ty,
        params.prelude(),
        result.body
    )
}

fn bare(result: &Ret, value: &str) -> String {
    format!(
        "use crate::support::*;\n#[inline(never)]\npub fn f() -> {} {{ {value} }}\n",
        result.ty
    )
}

/// The values the stubs are for, and the zero ones that already have a stub.
const VALUES: [&str; 10] = [
    "true", "false", "0", "1", "-1", "None", "Ok(())", "()", "1.0", "-1.0",
];

/// The value a plan replaces its body with, when the gate has a case for it.
fn value_of(plan: &MutantPlan) -> Option<&str> {
    match (&plan.edit, &plan.action) {
        (
            Edit::ReplaceWithLiteral {
                lit: Lit::Text(v), ..
            },
            Action::ReturnDefault {
                async_fn: false, ..
            },
        ) if VALUES.contains(&v.as_str()) => Some(v),
        _ => None,
    }
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
    result: &'static Ret,
    params: Params,
    next: &mut usize,
) -> Vec<ValueCase> {
    let rust = LanguageId::new("rust");
    let original = source(result, params);
    let file = syntax
        .analyze(&rust, Path::new("s.rs"), Arc::from(original.as_str()))
        .expect("a snippet parses");
    let planned = plan_file_sets(&file, profile, &registry(), false);
    let mut out = Vec::new();
    for plan in planned.plans {
        let Some(value) = value_of(&plan).map(str::to_string) else {
            continue;
        };
        let Some(mutated) = render_source(&original, &plan, profile) else {
            continue;
        };
        out.push(ValueCase {
            id: *next,
            result,
            params,
            bare: bare(result, &value),
            value,
            plan,
            original: original.clone(),
            mutated,
        });
        *next += 1;
    }
    out
}

/// Every function-value case of the gate, numbered, once per value.
pub fn all() -> Vec<ValueCase> {
    let syntax = TreeSitterSyntax::builtin().expect("the built-in profiles load");
    let profile = rust_profile(&syntax);
    let mut next = 0;
    let mut out = Vec::new();
    for result in RESULTS {
        for params in PARAMS {
            out.extend(cases_of(&syntax, &profile, result, params, &mut next));
        }
    }
    out
}
