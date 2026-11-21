use std::collections::{BTreeMap, HashMap, HashSet};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::num::{NonZeroU32, NonZeroI64};

pub const LIMIT: usize = 4 * 8;
pub static GREETING: &str = "hi";
pub const fn cf(a: u32) -> u32 { a + 1 }
pub unsafe fn uf(a: u32) -> u32 { a + 1 }

pub struct S { pub a: u32, pub b: String }
impl S {
    pub fn new() -> Self { S { a: 1, b: String::new() } }
    pub fn build(a: u32) -> Self { Self { a, b: "x".into() } }
    pub fn get(&self) -> u32 { self.a }
    pub fn me(&self) -> &Self { self }
    pub fn name(&self) -> &str { &self.b }
    pub fn owned(&self) -> String { self.b.clone() }
    pub fn gen<T: Default + Clone>(&self, t: &T) -> T { t.clone() }
    pub fn gen2<T>(&self, t: T) -> Option<T> { Some(t) }
    pub fn ch(&self) -> char { 'a' }
    pub fn tup(&self) -> (u8, bool) { (1, true) }
    pub fn arr(&self) -> [u8; 4] { [1; 4] }
    pub fn sl(&self) -> &[u8] { &[] }
    pub fn mutref(&mut self) -> &mut u32 { &mut self.a }
    pub fn mp(&self) -> HashMap<String, u32> { HashMap::new() }
    pub fn bt(&self) -> BTreeMap<u8, bool> { BTreeMap::new() }
    pub fn hs(&self) -> HashSet<u8> { HashSet::new() }
    pub fn cow(&self) -> Cow<'static, str> { Cow::Borrowed("a") }
    pub fn arc(&self) -> Arc<String> { Arc::new(String::new()) }
    pub fn rc(&self) -> Rc<u8> { Rc::new(1) }
    pub fn mtx(&self) -> Mutex<u8> { Mutex::new(1) }
    pub fn bx(&self) -> Box<dyn std::error::Error> { "e".into() }
    pub fn nz(&self) -> NonZeroU32 { NonZeroU32::new(1).unwrap() }
    pub fn nz2(&self) -> NonZeroI64 { NonZeroI64::new(1).unwrap() }
    pub fn it(&self) -> impl Iterator<Item = u8> { 0..3u8 }
    pub fn dsp(&self) -> impl std::fmt::Display { 1 }
    pub fn res(&self) -> Result<u8, String> { Ok(1) }
    pub fn res2(&self) -> std::io::Result<()> { Ok(()) }
    pub fn fl(&self) -> f64 { 1.5 }
    pub fn never(&self) -> ! { panic!() }
    pub async fn af(&self) -> u32 { 3 }
    pub fn fut(&self) -> impl std::future::Future<Output = u32> { async { 3 } }
    pub fn dynbox(&self) -> Box<dyn Fn() -> u8> { Box::new(|| 1) }
    pub fn opt_ref(&self) -> Option<&str> { None }
    pub fn unit_explicit(&self) -> () { }
    pub fn empty(&self) {}
    pub fn ptr(&self) -> *const u8 { std::ptr::null() }
}
impl Default for S { fn default() -> Self { S { a: 2 + 3, b: String::new() } } }
impl std::fmt::Display for S {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.a) }
}
pub trait T {
    fn decl(&self) -> u32;
    fn dflt(&self) -> u32 { 1 + 2 }
    fn new() -> Self where Self: Sized;
}
impl T for S { fn decl(&self) -> u32 { 7 } fn new() -> Self { S::new() } }

pub fn ops(a: i32, b: i32, c: bool, d: u32, mut e: i32) -> i32 {
    let _ = a == b; let _ = a != b; let _ = a < b; let _ = a <= b; let _ = a > b; let _ = a >= b;
    let _ = c && c; let _ = c || c; let _ = a + b; let _ = a - b; let _ = a * b; let _ = a / b; let _ = a % b;
    let _ = a & b; let _ = a | b; let _ = a ^ b; let _ = d << 1; let _ = d >> 1;
    e += a; e -= a; e *= a; e /= a; e %= a; e &= a; e |= a; e ^= a; e <<= 1; e >>= 1;
    let _ = -a; let _ = !c; let _ = *&a; let _ = !d;
    let _ = &a; let _ = (a..b); 
    let f = |x: i32| x + 1;
    fn nested(z: u32) -> u32 { z * 2 }
    f(a) + nested(d) as i32 + e
}
pub fn matches_(v: Option<u8>, w: u8) -> u8 {
    match v { Some(x) if x > 3 => x, Some(0) => 9, Some(_) => 1, None => 2 }
}
pub fn matches2(v: Option<u8>) -> u8 {
    match v { Some(x) => x, None => 0 }
}
pub fn matches3(v: u8) -> u8 {
    match v { 0 => 1, 1 => 2, _ => 3 }
}
pub fn matches4(v: u8) -> u8 {
    match v { 0 => 1, n if n > 1 => 2, _ if v == 9 => 4, _ => 3 }
}
pub fn lets(o: Option<u8>, b: bool) -> u8 {
    let Some(x) = o else { return 0 };
    if let Some(y) = o && b { return x + y; }
    if b && let Some(z) = o { return z; }
    let v = Vec::with_capacity(1 + 2);
    let _ = v.len() + 1;
    x
}
pub fn macros() -> usize { vec![1 + 1, 2].len() + format!("{}", 1 + 2).len() + { assert!(1 + 1 == 2); 1 } }
macro_rules! mk { () => { pub fn made() -> u8 { 1 + 1 } } }
mk!();
pub fn base() -> S { S { a: 1 + 1, ..Default::default() } }
pub fn base2() -> S { S { a: 1, b: "q".into(), ..S::default() } }
pub fn nobase() -> S { S { a: 1, b: "q".into() } }
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct D { pub x: u8 }
#[cfg(test)]
mod tests { use super::*; #[test] fn t() { assert_eq!(1 + 1, 2); } pub fn helper() -> u8 { 1 } }
#[cfg(not(test))]
pub fn not_test() -> u8 { 1 + 1 }
#[cfg(any(test, feature = "x"))]
pub fn any_test() -> u8 { 1 + 1 }
#[cfg(all(test, unix))]
pub fn all_test() -> u8 { 1 + 1 }
#[cfg(feature = "zzz")]
pub fn feat() -> u8 { 1 + 1 }
#[test]
fn a_test() { assert!(1 + 1 == 2) }
#[mutants::skip]
pub fn skipped() -> u8 { 1 + 1 }
#[cfg_attr(test, mutants::skip)]
pub fn skipped2() -> u8 { 1 + 1 }
#[cfg_attr(not(test), mutants::skip)]
pub fn skipped3() -> u8 { 1 + 1 }
#[inline] #[mutants::skip]
pub fn skipped4() -> u8 { 1 + 1 }
pub fn gener<T: Default>() -> T { T::default() }
pub fn where_<T>(t: T) -> T where T: Clone + std::ops::Add<Output = T> { t.clone() + t }
pub fn wrap<'a>(s: &'a str) -> Option<&'a str> { Some(s) }
pub fn gener_res<E>() -> Result<Vec<u8>, E> { Ok(vec![]) }
pub fn eqq(a: u8) -> bool { a == 0 }
pub fn tr() -> bool { true }
pub fn fl() -> bool { false }
pub fn zero() -> u32 { 0 }
pub fn one() -> u32 { 1 }
pub fn emptyvec() -> Vec<u8> { vec![] }
pub fn none() -> Option<u8> { None }
pub fn emptystr() -> String { String::new() }
pub fn unit_ret() { }
pub fn unit_call() { println!("x"); }
pub fn ok_unit() -> Result<(), String> { Ok(()) }
pub fn ret(a: u8) -> u8 { return a + 1; }
pub fn tailcomment() -> u8 { /* c */ 3 }
