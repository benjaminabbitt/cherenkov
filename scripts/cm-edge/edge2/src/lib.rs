pub mod inner;
#[cfg(test)]
mod tests_mod;
#[cfg(test)]
#[path = "other.test.rs"]
mod other;
#[cfg(feature = "f")]
mod gated;
mod unreferenced_ok;
#[mutants::skip]
mod skipmod;
pub mod dir_mod;

#[cfg(test)]
pub const C1: u32 = 1 + 2;
pub const C2: u32 = 1 + 2;
#[mutants::skip]
pub const C3: u32 = 1 + 2;
#[mutants::skip]
pub static S3: u32 = 1 + 2;
#[cfg(test)]
pub struct Ts { pub a: [u8; 1 + 1] }

pub struct P { pub a: u32, pub b: u32 }
impl Default for P { fn default() -> Self { P { a: 0, b: 0 } } }
pub fn attrs_on_stmts(x: u32, y: u32) -> u32 {
    #[cfg(test)]
    let a = x + y;
    #[mutants::skip]
    let b = x * y;
    #[mutants::skip]
    println!("{}", x + y);
    #[mutants::skip]
    let _call = std::cmp::max(x + 1, y);
    let p = P {
        #[mutants::skip]
        a: x + 1,
        #[cfg(test)]
        b: y - 1,
        ..Default::default()
    };
    let q = match x {
        #[mutants::skip]
        0 => 1 + 1,
        _ => 2 - 1,
    };
    #[mutants::skip]
    { let z = x - y; }
    let r = unsafe { x + y };
    p.a + q + r
}
pub mod nested_mod {
    #[mutants::skip]
    impl super::P { pub fn m(&self) -> u32 { 1 + 1 } }
    impl super::P { #[mutants::skip] pub fn m2(&self) -> u32 { 1 + 1 } pub fn m3(&self) -> u32 { 1 + 1 } }
    #[mutants::skip]
    pub trait Tr { fn d(&self) -> u32 { 1 + 1 } }
    pub trait Tr2 { #[mutants::skip] fn d(&self) -> u32 { 1 + 1 } }
}
pub fn loops(mut n: u32) -> u32 {
    while n < 10 { n += 1; }
    for i in 0..n { n = n - i; }
    let c = if n > 3 { 1 } else { 2 };
    let _ = vec![0u8; (n + 1) as usize];
    let _s = String::with_capacity(n as usize + 1);
    let _t = Vec::<u8>::with_capacity(n as usize * 2);
    c
}
