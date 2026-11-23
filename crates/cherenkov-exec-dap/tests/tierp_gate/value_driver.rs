//! The source of the function-value gate's executables: a `support`
//! module, one module per case and one per value alone, and a `main` that
//! calls each case's function over the inputs and prints a digest of what
//! it returned (and of what it left in a `&mut` argument).

use crate::values::{Params, ValueCase};

/// The types the functions' bodies and results are made of.
pub const SUPPORT: &str = r#"
#![allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum En { A, B, C }
"#;

const RUNTIME: &str = r#"
use std::io::Write;

fn fold(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x0100_0000_01b3).rotate_left(5)
}

/// A value's digest.
trait Out {
    fn dg(&self) -> u64;
}

macro_rules! ints {
    ($($t:ty),*) => {$( impl Out for $t { fn dg(&self) -> u64 { *self as u64 } } )*};
}
ints!(bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl Out for u128 {
    fn dg(&self) -> u64 { fold(*self as u64, (*self >> 64) as u64) }
}
impl Out for f64 {
    fn dg(&self) -> u64 { self.to_bits() }
}
impl Out for char {
    fn dg(&self) -> u64 { *self as u64 }
}
impl Out for () {
    fn dg(&self) -> u64 { 0x756e_6974 }
}
impl Out for support::En {
    fn dg(&self) -> u64 { *self as u64 + 77 }
}
impl Out for std::num::NonZeroU32 {
    fn dg(&self) -> u64 { self.get() as u64 }
}
impl Out for String {
    fn dg(&self) -> u64 { self.bytes().fold(5, |h, b| fold(h, b as u64)) }
}
impl Out for &str {
    fn dg(&self) -> u64 { self.bytes().fold(6, |h, b| fold(h, b as u64)) }
}
impl Out for std::io::Error {
    fn dg(&self) -> u64 { self.kind() as u64 + 900 }
}
impl<T: Out> Out for &T {
    fn dg(&self) -> u64 { fold(11, (**self).dg()) }
}
impl<T: Out> Out for Box<T> {
    fn dg(&self) -> u64 { fold(12, (**self).dg()) }
}
impl<T: Out, U: Out> Out for (T, U) {
    fn dg(&self) -> u64 { fold(self.0.dg(), self.1.dg()) }
}
impl<T: Out, const N: usize> Out for [T; N] {
    fn dg(&self) -> u64 { self.iter().fold(13, |h, x| fold(h, x.dg())) }
}
impl<T: Out> Out for Option<T> {
    fn dg(&self) -> u64 {
        match self {
            None => 0x4e4f_4e45,
            Some(x) => fold(1, x.dg()),
        }
    }
}
impl<T: Out, E: Out> Out for Result<T, E> {
    fn dg(&self) -> u64 {
        match self {
            Ok(x) => fold(2, x.dg()),
            Err(e) => fold(3, e.dg()),
        }
    }
}

fn inputs() -> Vec<u32> {
    let mut v: Vec<u32> = (0..64).collect();
    v.extend([100, 127, 128, 200, 255, 256, 1000, 65535, 65536, u32::MAX / 2, u32::MAX - 1, u32::MAX]);
    let mut x = 0x9E37_79B9u32;
    for _ in 0..30 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v.push(x);
    }
    v
}

fn sweep(f: fn(u32) -> u64) -> (u64, u64) {
    (inputs().into_iter().fold(0, |h, a| fold(h, f(a))), 0)
}

fn dump(f: fn(u32) -> u64) {
    for a in inputs() {
        println!("{a} {}", f(a));
    }
}
"#;

/// The shim that calls the case's function with the input `a`.
fn shim(case: &ValueCase) -> String {
    let n = case.id;
    let module = format!("s_{n}");
    let call = case.params.call(&module);
    match case.params {
        Params::Mutable => format!(
            "fn c_{n}(a: u32) -> u64 {{ let mut m = a; let r = {call}; fold(r.dg(), m as u64) }}\n"
        ),
        _ => format!("fn c_{n}(a: u32) -> u64 {{ {call}.dg() }}\n"),
    }
}

const MAIN: &str = r#"
fn run(id: usize) -> (u64, u64) {
    match id {
RUN_ARMS
        _ => (0, 0),
    }
}

fn dump_case(id: usize) {
    match id {
DUMP_ARMS
        _ => {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("dump") {
        dump_case(args[1].parse().unwrap());
        return;
    }
    let mut out = std::io::stdout().lock();
    for id in args.iter().map(|a| a.parse::<usize>().unwrap()) {
        let (digest, panics) = run(id);
        writeln!(out, "{id} {digest} {panics}").unwrap();
        out.flush().unwrap();
    }
}
"#;

/// The ids the harvested functions are run under, so that they are kept.
pub const BARE_BASE: usize = 1_000_000;

/// The driver's `main.rs` for `cases`, with its modules.
pub fn main_source(cases: &[ValueCase]) -> String {
    let mut src = String::from("mod support;\n");
    for c in cases {
        src.push_str(&format!("mod s_{0};\nmod h_{0};\n", c.id));
    }
    src.push_str(RUNTIME);
    let (mut run, mut dump) = (String::new(), String::new());
    for c in cases {
        let n = c.id;
        src.push_str(&shim(c));
        src.push_str(&format!("fn b_{n}() -> u64 {{ h_{n}::f().dg() }}\n"));
        run.push_str(&format!("        {n} => sweep(c_{n}),\n"));
        run.push_str(&format!("        {} => (b_{n}(), 0),\n", BARE_BASE + n));
        dump.push_str(&format!("        {n} => dump(c_{n}),\n"));
    }
    src.push_str(&MAIN.replace("RUN_ARMS", &run).replace("DUMP_ARMS", &dump));
    src
}
