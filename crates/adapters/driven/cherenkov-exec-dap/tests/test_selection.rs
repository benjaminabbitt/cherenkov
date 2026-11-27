//! Selecting individual tests for a mutation run, through each runner's own
//! selection flags: libtest's name filters with `--exact`, Go's `-test.run`
//! anchored regex. No selection runs every test.

use cherenkov_exec_dap::toolchain::{go_test_args, rust_test_args};

#[test]
fn test_no_selection_passes_no_arguments() {
    assert!(rust_test_args(&[], &[]).is_empty());
    assert!(go_test_args(&[], &[]).is_empty());
}

#[test]
fn test_rust_selection_is_exact_name_filters() {
    let names = ["calc::tests::adds".to_string(), "parses".to_string()];
    assert_eq!(
        rust_test_args(&names, &[]),
        ["calc::tests::adds", "parses", "--exact"]
    );
}

#[test]
fn test_go_selection_is_one_anchored_alternation() {
    let names = ["TestAdd".to_string(), "TestSub".to_string()];
    assert_eq!(
        go_test_args(&names, &[]),
        ["-test.run", "^(TestAdd|TestSub)$"]
    );
}

#[test]
fn test_go_selection_escapes_regex_metacharacters() {
    // A subtest name is matched literally, `/` included (Go splits on it).
    let names = ["TestTable/a+b".to_string()];
    assert_eq!(
        go_test_args(&names, &[]),
        ["-test.run", r"^(TestTable)$/^(a\+b)$"]
    );
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn test_rust_skips_are_exact_libtest_skip_filters() {
    assert_eq!(
        rust_test_args(&[], &names(&["bus::slow", "bus::tail"])),
        ["--skip", "bus::slow", "--skip", "bus::tail", "--exact"]
    );
}

#[test]
fn test_rust_skips_follow_the_selected_names_under_one_exact() {
    assert_eq!(
        rust_test_args(&names(&["a"]), &names(&["b"])),
        ["a", "--skip", "b", "--exact"]
    );
}

#[test]
fn test_go_skips_are_one_anchored_test_skip_pattern() {
    assert_eq!(
        go_test_args(&[], &names(&["TestSlow", "TestTable/a+b"])),
        ["-test.skip", r"^(TestSlow|TestTable)$/^(a\+b)$"]
    );
}

#[test]
fn test_go_selection_and_skips_are_both_passed() {
    assert_eq!(
        go_test_args(&names(&["TestA"]), &names(&["TestB"])),
        ["-test.run", "^(TestA)$", "-test.skip", "^(TestB)$"]
    );
}
