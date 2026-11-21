pub fn chain(a: Option<u8>, b: bool, c: bool) -> u8 {
    if let Some(x) = a && b && c {
        x
    } else {
        0
    }
}
pub fn chain2(a: Option<u8>, b: bool) -> u8 {
    if b && let Some(x) = a {
        x
    } else {
        0
    }
}
pub struct R { pub handled: u32 }
pub fn arm(n: u8, run: &mut R) {
    match n {
        1 => run.handled += 1,
        2 => run.handled *= 2,
        _ => {}
    }
}
