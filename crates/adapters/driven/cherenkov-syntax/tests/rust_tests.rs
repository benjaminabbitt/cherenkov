use cherenkov_syntax::rust_tests::{TestFn, test_functions};

const SRC: &str = "\
pub fn code() -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helper() -> i32 {
        2
    }

    #[test]
    fn test_one() {
        assert_eq!(code(), 1);
    }

    mod inner {
        #[tokio::test]
        async fn test_two() {}
    }
}
";

#[test]
fn test_the_test_functions_of_a_file_are_those_marked_test_with_their_module_path_and_lines() {
    assert_eq!(
        test_functions(SRC),
        [
            TestFn {
                path: "tests::test_one".to_string(),
                start_line: 14,
                end_line: 16
            },
            TestFn {
                path: "tests::inner::test_two".to_string(),
                start_line: 20,
                end_line: 20
            },
        ]
    );
}

#[test]
fn test_a_function_that_is_not_marked_test_is_not_a_test() {
    assert!(test_functions("fn a() {}\n#[inline]\nfn b() {}\n").is_empty());
}

#[test]
fn test_test_functions_of_a_file_without_modules_have_a_bare_name() {
    let found = test_functions("#[test]\nfn test_x() {}\n");
    assert_eq!(found[0].path, "test_x");
}

#[test]
fn test_a_test_is_still_a_test_with_other_attributes_and_comments_between() {
    let src = "#[test]\n#[ignore]\nfn test_a() {}\n\n#[test]\n// why\n/* more */\nfn test_b() {}\n\n#[ignore]\nfn not_a_test() {}\n";
    let paths: Vec<String> = test_functions(src).into_iter().map(|t| t.path).collect();
    assert_eq!(paths, ["test_a", "test_b"]);
}
