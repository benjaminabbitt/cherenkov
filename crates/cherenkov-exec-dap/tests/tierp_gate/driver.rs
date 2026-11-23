//! The source of the executables the gate runs: a `support` module, one
//! module per case, and a `main` that sweeps every case over its inputs and
//! prints what each did (a digest of the results, and how many panicked).

use crate::cases::Case;
use crate::snippets::Form;

/// The prelude every snippet imports.
pub const SUPPORT: &str = r#"
#![allow(unused)]
pub struct S<T> { pub x: T }
#[inline(never)]
pub fn id<T>(a: T) -> T { a }
"#;

/// Inputs and result digests of every type the gate sweeps.
const RUNTIME: &str = r#"
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::io::Write;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

trait V: Copy {
    fn inputs() -> Vec<Self>;
    fn digest(self) -> u64;
}

macro_rules! ints {
    ($($t:ty),*) => {$(
        impl V for $t {
            fn inputs() -> Vec<$t> {
                if <$t>::BITS == 8 {
                    return (0..=255u8).map(|v| v as $t).collect();
                }
                let (min, max) = (<$t>::MIN, <$t>::MAX);
                let mut v = vec![0, 1, 2, 3, 100, min, min + 1, min + 2, max, max - 1, max - 2,
                    max / 2, max / 2 + 1, min / 2, (0 as $t).wrapping_sub(1), (0 as $t).wrapping_sub(3),
                    1000u16 as $t, (0 as $t).wrapping_sub(1000u16 as $t)];
                let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
                v.extend((0..40).map(|_| rng.next() as $t));
                v
            }
            fn digest(self) -> u64 { self as u64 }
        }
    )*};
}
ints!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl V for bool {
    fn inputs() -> Vec<bool> { vec![false, true] }
    fn digest(self) -> u64 { self as u64 }
}

macro_rules! floats {
    ($($t:ty),*) => {$(
        impl V for $t {
            fn inputs() -> Vec<$t> {
                let mut v: Vec<$t> = vec![0.0, -0.0, 1.0, -1.0, 2.0, 3.0, -3.0, 6.0, 0.5, 1e-30,
                    <$t>::MIN_POSITIVE, <$t>::MAX, -<$t>::MAX, <$t>::INFINITY, <$t>::NEG_INFINITY,
                    <$t>::NAN, <$t>::EPSILON];
                let mut rng = Rng(0xD1B5_4A32_D192_ED03);
                v.extend((0..40).map(|_| (rng.next() % 2_000_001) as $t / 1000.0 - 1000.0));
                v
            }
            fn digest(self) -> u64 {
                if self.is_nan() { 0x7ff8_dead_beef } else { self.to_bits() as u64 }
            }
        }
    )*};
}
floats!(f32, f64);

fn fold(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x0100_0000_01b3).rotate_left(5)
}

/// What one call did: its digest, or a panic.
fn call<R: V>(f: impl FnOnce() -> R) -> Option<u64> {
    catch_unwind(AssertUnwindSafe(f)).ok().map(V::digest)
}

fn tally(h: &mut u64, panics: &mut u64, out: Option<u64>) {
    match out {
        Some(d) => *h = fold(*h, d),
        None => {
            *h = fold(*h, 0xdead);
            *panics += 1;
        }
    }
}

fn sweep2<T: V, R: V>(f: fn(T, T) -> R) -> (u64, u64) {
    let (mut h, mut panics) = (0u64, 0u64);
    let ins = T::inputs();
    for &a in &ins {
        for &b in &ins {
            tally(&mut h, &mut panics, call(|| f(a, b)));
        }
    }
    (h, panics)
}

fn sweep1<T: V, R: V>(f: fn(T) -> R) -> (u64, u64) {
    let (mut h, mut panics) = (0u64, 0u64);
    for &a in &T::inputs() {
        tally(&mut h, &mut panics, call(|| f(a)));
    }
    (h, panics)
}

/// With `a`, `b` the inputs: the dump of one case, input by input.
fn dump2<T: V + std::fmt::Debug, R: V>(f: fn(T, T) -> R) {
    let ins = T::inputs();
    for &a in &ins {
        for &b in &ins {
            println!("{a:?} {b:?} {:?}", call(|| f(a, b)));
        }
    }
}

fn dump1<T: V + std::fmt::Debug, R: V>(f: fn(T) -> R) {
    for &a in &T::inputs() {
        println!("{a:?} - {:?}", call(|| f(a)));
    }
}
"#;

/// The shim that calls the case's function with inputs `a` and `b`.
fn shim(case: &Case) -> String {
    let s = &case.snippet;
    let (ty, ret, n) = (s.ty, s.ret, case.id);
    let call = match (&s.shim, s.compound, s.form) {
        (Some(body), _, _) => body.replace("@F", &format!("s_{n}::f")),
        (None, false, Form::Field) => format!("s_{n}::f(&S {{ x: a }}, b)"),
        (None, true, Form::Field) => {
            format!("let mut s = S {{ x: a }}; s_{n}::f(&mut s, b); s.x")
        }
        (None, true, Form::MutRef) => format!("let mut m = a; s_{n}::f(&mut m, b); m"),
        (None, true, Form::Entry) => {
            format!("let mut m = HashMap::new(); m.insert(7u32, a); s_{n}::f(&mut m, 7); m[&7]")
        }
        (None, true, Form::Idx) => format!("let mut v = [a]; s_{n}::f(&mut v, 0, b); v[0]"),
        (None, _, _) if s.one_input => format!("s_{n}::f(a)"),
        _ => format!("s_{n}::f(a, b)"),
    };
    if s.one_input {
        format!("fn c_{n}(a: {ty}) -> {ret} {{ {call} }}\n")
    } else {
        format!("fn c_{n}(a: {ty}, b: {ty}) -> {ret} {{ {call} }}\n")
    }
}

fn arms(cases: &[Case]) -> (String, String) {
    let (mut run, mut dump) = (String::new(), String::new());
    for c in cases {
        let (n, ty, ret) = (c.id, c.snippet.ty, c.snippet.ret);
        let k = if c.snippet.one_input { 1 } else { 2 };
        run.push_str(&format!("        {n} => sweep{k}::<{ty}, {ret}>(c_{n}),\n"));
        dump.push_str(&format!("        {n} => dump{k}::<{ty}, {ret}>(c_{n}),\n"));
    }
    (run, dump)
}

const MAIN: &str = r#"
fn run(id: usize) -> (u64, u64) {
    match id {
RUN_ARMS
        _ => (0, 0),
    }
}

fn dump(id: usize) {
    match id {
DUMP_ARMS
        _ => {}
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("dump") {
        dump(args[1].parse().unwrap());
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

/// The driver's `main.rs` for `cases`, with its modules.
pub fn main_source(cases: &[Case]) -> String {
    let mut src = String::from("mod support;\nuse support::S;\n");
    for c in cases {
        src.push_str(&format!("mod s_{};\n", c.id));
    }
    src.push_str(RUNTIME);
    for c in cases {
        src.push_str(&shim(c));
    }
    let (run, dump) = arms(cases);
    src.push_str(&MAIN.replace("RUN_ARMS", &run).replace("DUMP_ARMS", &dump));
    src
}
