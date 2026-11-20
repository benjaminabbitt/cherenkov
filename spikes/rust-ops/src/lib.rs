//! One function per mutation operator family, for reading debug-profile
//! codegen (see docs/rust-operator-evaluation.md). Each site of interest is
//! on its own line, marked with the operator it exercises.

pub fn aor(a: i64, b: i64) -> i64 {
    let s = a + b; // AOR +
    let d = s - b; // AOR -
    let m = d * b; // AOR *
    let q = m / b; // AOR /
    q % b // AOR %
}

pub fn asr(mut a: i64, b: i64) -> i64 {
    a += b; // ASR +=
    a *= b; // ASR *=
    a
}

pub fn ror(a: i64, b: u64, c: u64) -> bool {
    if a == 3 {
        // ROR == (and != etc.)
        return true;
    }
    b < c // ROR as a value (setcc), unsigned
}

pub fn lcr(a: bool, b: bool) -> bool {
    a && b // LCR && -> ||
}

pub fn lcr_branch(x: i64, y: i64) -> i64 {
    if x > 0 || y > 0 {
        // LCR || in a condition
        return 1;
    }
    0
}

pub fn bor(a: u32, b: u32) -> u32 {
    let x = a & b; // BOR &
    let y = x | b; // BOR |
    y ^ a // BOR ^
}

pub fn sor(a: u32, n: u32) -> u32 {
    a << n // SOR << (with shift-overflow check)
}

pub fn uod(a: bool, n: i64) -> i64 {
    if !a {
        // UOD delete !
        return -n; // UOD delete -
    }
    n
}

pub fn crp(a: i64) -> i64 {
    a + 42 // CRP constant 42
}

pub fn svr(a: i64, _b: i64) -> i64 {
    let x = a; // SVR: replace a with b
    x + 1
}

pub fn index(v: &[i64], i: usize) -> i64 {
    v[i] // AAR/index: replace i
}

pub fn sdl(v: &mut Vec<i64>) {
    v.push(1); // SDL: delete call statement
    v.clear(); // SDL
}

pub fn call_result(v: &[i64]) -> usize {
    let n = v.len(); // non-void call result -> default
    n
}

pub fn guard(x: Option<i64>) -> i64 {
    match x {
        Some(n) if n > 10 => 1, // match guard -> true/false
        Some(_) => 2,           // match arm deletion
        None => 3,
    }
}

pub fn arms(k: u8) -> u8 {
    match k {
        0 => 10, // match arm deletion (dense: jump table?)
        1 => 11,
        2 => 12,
        _ => 0,
    }
}

pub fn loop_cond(n: u64) -> u64 {
    let mut i = 0;
    while i < n {
        // loop condition negation / boundary
        i += 1;
    }
    i
}

#[derive(Default, Debug, PartialEq)]
pub struct S {
    pub a: i64,
    pub b: i64,
}

pub fn fields(a: i64) -> S {
    S { a, ..Default::default() } // struct field deletion
}

// --- Body replacement by result type (cargo-mutants' FnValue).
pub fn r_unit(v: &mut i64) {
    *v = 1;
}
pub fn r_bool() -> bool {
    true
}
pub fn r_int() -> i64 {
    7
}
pub fn r_float() -> f64 {
    1.5
}
pub fn r_char() -> char {
    'x'
}
pub fn r_str() -> &'static str {
    "hello"
}
pub fn r_string() -> String {
    String::from("hello")
}
pub fn r_vec() -> Vec<i64> {
    vec![1, 2]
}
pub fn r_option() -> Option<i64> {
    Some(3)
}
pub fn r_result() -> Result<i64, String> {
    Ok(4)
}
pub fn r_box() -> Box<i64> {
    Box::new(5)
}
pub fn r_ref(v: &[i64]) -> &[i64] {
    &v[1..]
}
pub fn r_struct() -> S {
    S { a: 1, b: 2 }
}

// --- Tier B targets (see docs/rust-tier-b-findings.md).
pub fn mul(a: i64, b: i64) -> i64 {
    let m = a * b; // AOR * -> / under the debugger
    m + 1
}

pub fn mul_loop(n: i64) -> i64 {
    let mut t = 1;
    let mut k = 0;
    while k < n {
        t = t * 2; // hit on every iteration
        k += 1;
    }
    t
}

pub fn pick(v: &[i64], i: usize) -> i64 {
    let x = v[i]; // index replacement: v[0]
    x * 10
}

pub fn svr_site(a: i64, b: i64, s: &str) -> i64 {
    let c = a + b;
    let n = s.len() as i64;
    let x = c; // SVR: candidates for `c`
    let mut acc = 0;
    for k in 0..3 {
        let y = k + x; // SVR in a loop: same set every hit?
        acc += y;
    }
    acc + n
}
