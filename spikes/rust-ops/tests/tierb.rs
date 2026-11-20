//! Passes only under the mutants the tier B probe applies; fails (exit 101)
//! on the original code.

#[test]
fn mutated_mul_divides() {
    // original: 12 * 4 + 1 = 49; mutant `m = a / b`: 12 / 4 + 1 = 4
    assert_eq!(ops::mul(12, 4), 4);
}

#[test]
fn mutated_mul_loop_divides_every_iteration() {
    // original: 2^3 = 8; mutant `t = t / 2` from 1: 0
    assert_eq!(ops::mul_loop(3), 0);
}

#[test]
fn mutated_pick_reads_index_zero() {
    // original: v[1] * 10 = 60; mutant `x = v[0]`: 50
    assert_eq!(ops::pick(&[5, 6, 7], 1), 50);
}

#[test]
fn mutated_mul_takes_other_variable() {
    // SVR `m = b`: 4 + 1 = 5
    assert_eq!(ops::mul(12, 4), 5);
}

#[test]
fn mutated_mul_negates() {
    // negation insertion `m = -a`: -12 + 1 = -11
    assert_eq!(ops::mul(12, 4), -11);
}

#[test]
fn mutated_box_is_default() {
    // body replaced by Box::new(0): original holds 5
    assert_eq!(*ops::r_box(), 0);
}

#[test]
fn svr_site_runs() {
    // Unmutated: c = 3, n = 2, x = 3, acc = 3 + 4 + 5 = 12, + 2 = 14.
    assert_eq!(ops::svr_site(1, 2, "hi"), 14);
}
