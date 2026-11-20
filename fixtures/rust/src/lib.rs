fn log(_msg: i64) {}

pub fn total(a: i64, b: i64) -> i64 {
    let s = a + b;
    log(s);
    return s;
}

pub fn is_small(n: i64) -> bool {
    if n < 10 {
        return true;
    }
    return false;
}

pub fn count_up(n: i64) -> i64 {
    let mut i = 0;
    while i < n {
        i = i + 1;
    }
    i
}

pub fn first(v: &[i64]) -> i64 {
    if let Some(x) = v.first() {
        return *x;
    }
    0
}
