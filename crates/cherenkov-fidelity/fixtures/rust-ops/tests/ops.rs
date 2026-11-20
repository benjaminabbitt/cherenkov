use ops::*;

#[test]
fn test_compare() {
    assert_eq!(compare(1, 2), 1 + 2 + 32);
    assert_eq!(compare(2, 2), 2 + 8 + 16);
    assert_eq!(compare(3, 2), 4 + 8 + 32);
}

#[test]
fn test_logic() {
    assert_eq!(logic(true, true), 1);
    assert_eq!(logic(true, false), 2);
    assert_eq!(logic(false, false), 3);
}

#[test]
fn test_arith() {
    assert_eq!(arith(7, 3), 10 * 10000 + 4 * 1000 + 21 * 100 + 2 * 10 + 1);
}

#[test]
fn test_bits() {
    assert_eq!(bits(6, 3), 2 + 7 * 16 + 5 * 256 + 24 * 4096 + 3 * 65536);
}

#[test]
fn test_compound() {
    assert_eq!(compound(5, 3), ((5 + 3 - 1) * 3 / 2) % 1000);
}

#[test]
fn test_shifts() {
    assert_eq!(shifts(5), (((5u32 << 3) >> 1) & 0xff | 0x100) ^ 0x3);
}

#[test]
fn test_unary() {
    assert_eq!(unary(4, false), -4);
    assert_eq!(unary(4, true), -8);
}

#[test]
fn test_inline() {
    assert_eq!(inline(5, 2), 7 * 100 + 10 * 10 + 3);
}

#[test]
fn test_classify() {
    assert_eq!(classify(Some(20)), 1);
    assert_eq!(classify(Some(5)), 2);
    assert_eq!(classify(None), 3);
}

#[test]
fn test_digit() {
    assert_eq!(digit(0), 10);
    assert_eq!(digit(1), 11);
    assert_eq!(digit(2), 12);
    assert_eq!(digit(9), 0);
}

#[test]
fn test_pair() {
    assert_eq!(pair(4), Pair { a: 4, b: 0 });
}

#[test]
fn test_pair_set() {
    assert_eq!(Pair { a: 4, b: 0 }.set(), ["a"]);
    assert_eq!(Pair { a: 0, b: 2 }.set(), ["b"]);
}

#[test]
fn test_values() {
    assert_eq!(name(), "ops");
    assert_eq!(maybe(3), Some(3));
    assert_eq!(maybe(-3), None);
    assert_eq!(list(2), vec![1, 1]);
    assert_eq!(parse("12"), Ok(12));
    assert!(parse("x").is_err());
    assert!(flag(4));
    assert!(!flag(3));
}

#[test]
fn test_chains() {
    // Every combination: the regrouped and the tree-swapped mutants
    // differ on some of them.
    for k in 0..8 {
        let (a, b, c) = (k & 4 != 0, k & 2 != 0, k & 1 != 0);
        assert_eq!(all3(a, b, c), a && b && c, "all3{:?}", (a, b, c));
        assert_eq!(any3(a, b, c), i64::from(a || b || c), "any3{:?}", (a, b, c));
    }
}

// Inputs on which the source's grouping and the tree's grouping of a
// swapped chain differ, each in a test of its own so the two mutants
// fail different tests.

#[test]
fn test_all3_needs_c() {
    // `a || b && c` (source) is true here; `(a || b) && c` (tree) is not.
    assert!(!all3(true, true, false));
}

#[test]
fn test_any3_by_a_alone() {
    // `a || b && c` (source) is 1 here; `(a || b) && c` (tree) is 0.
    assert_eq!(any3(true, false, false), 1);
}

#[test]
fn test_mix() {
    assert_eq!(mix(12, 2, 3), 6);
}

#[test]
fn test_mix_where_the_groupings_differ() {
    // With `-` as `/`: `(a / b) * c` (source) is -4, as the original is;
    // `a / (b * c)` (tree) is -1.
    assert_eq!(mix(-2, 1, 2), -4);
}

#[test]
fn test_check() {
    assert_eq!(check(1), Ok(()));
    assert!(check(-1).is_err());
}

#[test]
fn test_evens() {
    assert_eq!(evens(&[1, 2, 3, 4, 6]), 3);
}

#[test]
fn test_plain() {
    assert_eq!(plain(3), Plain { n: 3 });
}

#[test]
fn test_shade() {
    assert_eq!(shade(1), Shade::Dark);
    assert_eq!(shade(0), Shade::Light);
}

#[test]
fn test_absent() {
    assert_eq!(absent(None), 1);
    assert_eq!(absent(Some(2)), 0);
}

#[test]
fn test_more_and_less_at_and_around_equality() {
    assert!(more(3, 2));
    assert!(!more(2, 2));
    assert!(!more(1, 2));
    assert!(less(-3, 2));
    assert!(!less(2, 2));
    assert!(!less(3, -2));
}

#[test]
fn test_unused() {
    assert_eq!(unused(2).n, 2);
    assert_eq!(unused(2).s, "set");
}

#[test]
fn test_pick_u8() {
    let _ = pick(1u8, 2u8, true);
}

#[test]
fn test_pick_u32() {
    assert_eq!(pick(1u32, 2u32, true), 1);
    assert_eq!(pick(1u32, 2u32, false), 2);
}

#[test]
fn test_noted() {
    assert_eq!(noted("5"), Ok(1));
}

#[test]
fn test_invert() {
    assert_eq!(invert(0xF1) & 0xF0, 0);
}

#[test]
fn test_pool() {
    assert_eq!(pool().limit, 3);
    assert_eq!(Pool::<u8>::default().limit, 10);
}

#[test]
fn test_wide() {
    assert!(wide(1 << 70, 2) > 0);
}

#[test]
fn test_guarded_false_only() {
    assert_eq!(guarded(false), 1);
}

#[test]
fn test_bumped() {
    let mut c = Counter { n: 10 };
    assert_eq!(bumped(&mut c), 21);
}

#[test]
fn test_inc_is_positive() {
    // `x - 1` passes this; `x * 1` too. Only an overflow check misapplied
    // to a swapped operator fails it.
    assert!(inc(5) > 0);
}

#[test]
fn test_total_is_positive() {
    // `a - b` underflows here: the real mutant panics.
    assert!(total(1, 2) > 0);
}

#[test]
fn test_pool_has_room() {
    // Passes for pool()'s own value and for Pool's Default (limit 10);
    // fails for a zeroed one.
    assert!(pool().limit >= 3);
}

#[test]
fn test_cache_has_room() {
    assert!(cache().limit >= 3);
}

/// Polls a future that never waits to its end.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

#[test]
fn test_twice() {
    assert_eq!(block_on(twice(3)), 6);
}

// Tests that pass under a fn-value mutant: only a faithfully built
// `Poll::Ready(value)` lets them pass.

#[test]
fn test_twice_zero() {
    assert_eq!(block_on(twice(0)), 0);
}

#[test]
fn test_validate_accepts() {
    assert_eq!(block_on(validate(1)), Ok(()));
}

#[test]
fn test_fetch_none() {
    assert_eq!(block_on(fetch(0)), Ok(vec![]));
}

#[test]
fn test_paired_is_ok() {
    assert!(block_on(paired(0)).is_ok_and(|p| p.a == 0));
}

#[test]
fn test_twice_plus_negative() {
    assert_eq!(block_on(twice_plus(-1)), -1);
}

#[test]
fn test_validate() {
    assert_eq!(block_on(validate(1)), Ok(()));
    assert!(block_on(validate(-1)).is_err());
}

#[test]
fn test_fetch() {
    assert_eq!(block_on(fetch(2)), Ok(vec![7, 7]));
}

#[test]
fn test_fetch_refuses_too_many() {
    assert!(block_on(fetch(101)).is_err());
}

#[test]
fn test_twice_plus() {
    assert_eq!(block_on(twice_plus(3)), 7);
}

#[test]
fn test_paired() {
    assert_eq!(block_on(paired(2)), Ok(Pair { a: 2, b: 1 }));
}

// Std values: each test passes under some fn-value mutant, so only a
// faithfully built value (and one the program can drop) gives the same
// outcome as the mutant compiled from source.

#[test]
fn test_tags() {
    assert_eq!(tags(1), std::collections::HashSet::from([1, 2, 3]));
}

#[test]
fn test_tags_hold_one() {
    assert!(tags(1).contains(&1));
}

#[test]
fn test_tags_are_few() {
    assert!(tags(0).len() <= 3);
}

#[test]
fn test_shared() {
    assert_eq!(*shared(2), 4);
}

#[test]
fn test_shared_zero_is_unshared() {
    let s = shared(0);
    let t = s.clone();
    assert_eq!(*t, 0);
    assert_eq!(std::sync::Arc::strong_count(&s), 2);
}

#[test]
fn test_home() {
    assert_eq!(home("ops"), std::path::PathBuf::from("/home/ops"));
}

#[test]
fn test_home_is_utf8() {
    let mut h = home("ops");
    h.push("x");
    assert!(h.to_str().is_some_and(|s| s.ends_with('x')));
}

#[test]
fn test_limit() {
    assert_eq!(*limit(), 7);
}

#[test]
fn test_limit_is_small() {
    assert!(*limit() <= 7);
}

#[test]
fn test_boxed() {
    assert_eq!(*boxed(1), 2);
}

#[test]
fn test_boxed_minus_one() {
    let mut b = boxed(-2);
    *b += 1;
    assert_eq!(*b, 0);
}

#[test]
fn test_shared_later() {
    assert_eq!(*block_on(shared_later(1)), 6);
}

#[test]
fn test_shared_later_small() {
    assert!(*block_on(shared_later(-5)) <= 1);
}

// Async fns compiled into async blocks: each test passes under some
// fn-value mutant.

#[test]
fn test_checked_later() {
    assert_eq!(block_on(checked_later(2)), Ok(3));
    assert!(block_on(checked_later(-1)).is_err());
}

#[test]
fn test_checked_later_small() {
    assert!(block_on(checked_later(0)).is_ok_and(|v| v <= 1));
}

#[test]
fn test_named_later() {
    assert_eq!(block_on(named_later(2)).as_deref(), Some("n2"));
}

#[test]
fn test_named_later_none() {
    assert_eq!(block_on(named_later(0)), None);
}

#[test]
fn test_named_later_short() {
    assert!(block_on(named_later(1)).is_none_or(|s| s.len() <= 5));
}

#[test]
fn test_listed_later() {
    assert_eq!(block_on(listed_later(2)), Ok(vec![3, 3]));
    assert!(block_on(listed_later(101)).is_err());
}

#[test]
fn test_listed_later_empty() {
    assert_eq!(block_on(listed_later(0)), Ok(vec![]));
}

// Private project types, mirrored: each test passes under some fn-value
// mutant, and only under a faithfully built value.

#[test]
fn test_inventory() {
    let i = inventory(2);
    assert_eq!(i.labels.len(), 2);
    assert_eq!(i.counts.get("l1"), Some(&1));
    assert_eq!(i.id, 2);
}

#[test]
fn test_inventory_empty_is_default() {
    assert_eq!(inventory(0), Inventory::default());
}

#[test]
fn test_labels() {
    assert_eq!(labels(2)[1], Label { text: "l1".into(), weight: 1 });
}

#[test]
fn test_labels_few_and_light() {
    let l = labels(1);
    assert!(l.len() <= 1 && l.iter().all(|l| l.weight == 0 && l.text.len() <= 2));
}

#[test]
fn test_stock() {
    assert_eq!(stock(1).map(|i| i.id), Ok(1));
    assert!(stock(101).is_err());
}

#[test]
fn test_stock_empty() {
    assert!(stock(0).is_ok_and(|i| i.labels.is_empty() && i.counts.is_empty()));
}

#[test]
fn test_shared_label() {
    assert_eq!(shared_label(2).text, "xx");
}

#[test]
fn test_shared_label_empty() {
    let l = shared_label(0);
    assert_eq!((l.text.as_str(), l.weight), ("", 0));
    assert_eq!(std::sync::Arc::strong_count(&l), 1);
}

#[test]
fn test_stock_later() {
    assert_eq!(block_on(stock_later(3)).map(|i| i.labels.len()), Ok(3));
}

#[test]
fn test_stock_later_empty() {
    assert_eq!(block_on(stock_later(0)), Ok(Inventory::default()));
}

#[test]
fn test_tuned_all() {
    assert_eq!(tuned_all(2).len(), 2);
}

#[test]
fn test_tuned_all_are_tuned() {
    assert!(tuned_all(1).iter().all(|t| t.level == 7 && t.name == "tuned"));
}

#[test]
fn test_event_index() {
    assert_eq!(event_index(2).get(&1), Some(&Event::Moved { x: 1, y: 2 }));
}

#[test]
fn test_event_index_empty() {
    assert!(event_index(0).is_empty());
}

#[test]
fn test_slot_index() {
    assert_eq!(slot_index(2).get(&1), Some(&Slot::Full("1".into())));
}

#[test]
fn test_slot_index_empty() {
    assert!(slot_index(0).is_empty());
}

#[test]
fn test_generated_letter() {
    assert_eq!(generated_letter(2).id, 2);
    assert_eq!(generated::Letter::default().id, 0);
}

#[test]
fn test_letter() {
    assert_eq!(letter("a").to, "a");
}

#[test]
fn test_letter_short() {
    assert!(letter("").to.len() <= 5);
}

#[test]
fn test_hooks() {
    assert_eq!(hooks(2)[1].on.map(|f| f(3)), Some(6));
}

#[test]
fn test_hooks_one_is_empty() {
    assert!(hooks(1).iter().all(|h| h.on.is_none() && h.name.is_empty() && h.extra.is_none()));
}

#[test]
fn test_gauges() {
    let g = gauges(1);
    assert_eq!(g[0].bins, [1, 2]);
    assert_eq!(g[0].grid[1][2], 3);
    assert_eq!(g[0].marks[1].at, 7);
}

#[test]
fn test_banners() {
    assert_eq!(banners(1)[0].text, "hi");
    assert_eq!(banners(1)[0].by, "ops");
}
