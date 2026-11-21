pub struct P { pub a: i32, pub b: i32 }
pub mod m {
    pub trait T { fn d(&self) -> u32 { 1 + 2 } }
    pub struct S<'a, X>(&'a X);
    impl<'a, X> S<'a, X> {
        pub fn get(&self, v: i32) -> Result<(), String> { if v > 1 { Ok(()) } else { Err(String::new()) } }
    }
    impl std::fmt::Display for P2 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "x") }
    }
    pub struct P2;
    pub fn outer(x: i32) -> i32 {
        fn inner(y: i32) -> i32 { y * 3 }
        let c = |z: i32| z - 1;
        inner(x) + c(x)
    }
}
pub fn mk(a: i32, b: i32) -> P { P { a: a + 1, b } }
pub fn arms(n: u8, k: i32) -> i32 {
    let mut t = 0;
    match n {
        1 | 2 => t += 1,
        3 if k > 2 => t = 5,
        _ => {}
    }
    if !(k > 0) { -k } else { t }
}
pub const C: usize = 2 * 3;
