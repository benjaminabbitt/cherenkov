use super::*;

const LIB: &str = "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn test_a() {\n        assert!(true);\n    }\n\n    #[test]\n    fn test_b() {\n        assert!(true);\n    }\n}\n";

fn diff(file: &str, line: u32) -> AddedLines {
    AddedLines::parse(&format!(
        "--- a/{file}\n+++ b/{file}\n@@ -{line},1 +{line},1 @@\n-x\n+y\n"
    ))
}

fn read(path: &Path) -> Option<String> {
    (path == Path::new("core/src/lib.rs") || path == Path::new("core/tests/it.rs"))
        .then(|| LIB.to_string())
}

fn paths(found: &[ChangedTest]) -> Vec<String> {
    found.iter().map(|c| c.path.clone()).collect()
}

#[test]
fn test_a_test_whose_body_a_diff_changes_is_a_changed_test() {
    let found = changed_tests(&diff("repo/core/src/lib.rs", 7), Path::new("repo"), &read);
    assert_eq!(paths(&found), ["tests::test_a"]);
    assert_eq!(found[0].file, Path::new("core/src/lib.rs"));
}

#[test]
fn test_a_change_outside_every_test_changes_no_test() {
    let found = changed_tests(&diff("repo/core/src/lib.rs", 1), Path::new("repo"), &read);
    assert!(found.is_empty());
}

#[test]
fn test_a_file_outside_the_run_s_directory_or_unreadable_changes_no_test() {
    assert!(changed_tests(&diff("other/core/src/lib.rs", 7), Path::new("repo"), &read).is_empty());
    assert!(changed_tests(&diff("repo/core/src/gone.rs", 7), Path::new("repo"), &read).is_empty());
}

#[test]
fn test_every_test_the_diff_reaches_is_changed_in_an_integration_test_file_too() {
    let found = changed_tests(&diff("core/tests/it.rs", 12), Path::new(""), &read);
    assert_eq!(paths(&found), ["tests::test_b"]);
}

#[test]
fn test_libtest_names_match_a_changed_test_by_its_path_in_its_file() {
    let listed: Vec<String> = [
        "tests::test_a",
        "other::tests::test_a",
        "tests::test_ab",
        "test_a",
    ]
    .map(String::from)
    .to_vec();
    let changed = [ChangedTest {
        file: PathBuf::from("core/src/lib.rs"),
        path: "tests::test_a".to_string(),
    }];
    assert_eq!(
        libtest_names(&listed, &changed),
        ["tests::test_a", "other::tests::test_a"]
    );
    let bare = [ChangedTest {
        file: PathBuf::from("x.rs"),
        path: "test_a".to_string(),
    }];
    assert_eq!(
        libtest_names(&listed, &bare),
        ["tests::test_a", "other::tests::test_a", "test_a"],
        "a path in a file declared as a module elsewhere is matched by its tail"
    );
}
