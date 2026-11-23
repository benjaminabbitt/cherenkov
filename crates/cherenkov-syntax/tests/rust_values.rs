//! Rust function-body replacement values, matching cargo-mutants 27.1.0
//! `src/fnvalue.rs`. The expectations are that file's own unit tests.

use cherenkov_syntax::rust_values::return_values;

fn check(return_type: Option<&str>, error_exprs: &[&str], expected: &[&str]) {
    assert_eq!(
        return_values(return_type, error_exprs),
        expected,
        "-> {return_type:?}"
    );
}

#[test]
fn test_unit_and_explicit_unit() {
    check(None, &[], &["()"]);
    check(Some("()"), &[], &["()"]);
}

#[test]
fn test_integers_floats_and_bool() {
    check(Some("u16"), &[], &["0", "1"]);
    check(Some("isize"), &[], &["0", "1", "-1"]);
    check(Some("f32"), &[], &["0.0", "1.0", "-1.0"]);
    check(Some("bool"), &[], &["true", "false"]);
}

#[test]
fn test_nonzero_integers() {
    check(
        Some("std::num::NonZeroIsize"),
        &[],
        &["1.try_into().unwrap()", "(-1).try_into().unwrap()"],
    );
    check(
        Some("std::num::NonZeroUsize"),
        &[],
        &["1.try_into().unwrap()"],
    );
    check(
        Some("std::num::NonZeroU32"),
        &[],
        &["1.try_into().unwrap()"],
    );
}

#[test]
fn test_generic_nonzero_of_an_unsigned_type_is_only_positive() {
    for t in ["NonZero<u32>", "std::num::NonZero<usize>", "NonZero<u8>"] {
        check(Some(t), &[], &["1.try_into().unwrap()"]);
    }
}

#[test]
fn test_generic_nonzero_of_a_signed_or_unknown_type_is_either_sign() {
    for t in ["NonZero<i64>", "NonZero<T>", "NonZero<std::primitive::u8>", "NonZero<&u8>"] {
        check(
            Some(t),
            &[],
            &["1.try_into().unwrap()", "(-1).try_into().unwrap()"],
        );
    }
}

#[test]
fn test_strings() {
    check(Some("String"), &[], &["String::new()", r#""xyzzy".into()"#]);
    check(Some("&str"), &[], &[r#""""#, r#""xyzzy""#]);
    check(Some("&'static str"), &[], &[r#""""#, r#""xyzzy""#]);
}

#[test]
fn test_result_recurses_into_its_ok_type_and_adds_errors() {
    check(
        Some("std::result::Result<bool>"),
        &[],
        &["Ok(true)", "Ok(false)"],
    );
    check(
        Some("std::result::Result<Result<bool>>"),
        &[r#"anyhow!("mutated")"#],
        &[
            "Ok(Ok(true))",
            "Ok(Ok(false))",
            r#"Ok(Err(anyhow!("mutated")))"#,
            r#"Err(anyhow!("mutated"))"#,
        ],
    );
    check(Some("Result<(), Error>"), &[], &["Ok(())"]);
    check(Some("Result<()>"), &[], &["Ok(())"]);
}

#[test]
fn test_http_response() {
    check(Some("HttpResponse"), &[], &["HttpResponse::Ok().finish()"]);
}

#[test]
fn test_option_box_vec_and_containers() {
    check(Some("Option<usize>"), &[], &["None", "Some(0)", "Some(1)"]);
    check(Some("Box<usize>"), &[], &["Box::new(0)", "Box::new(1)"]);
    check(
        Some("Box<MyObject>"),
        &[],
        &["Box::new(Default::default())"],
    );
    check(
        Some("std::vec::Vec<String>"),
        &[],
        &["vec![]", "vec![String::new()]", r#"vec!["xyzzy".into()]"#],
    );
    check(
        Some("alloc::sync::Arc<String>"),
        &[],
        &["Arc::new(String::new())", r#"Arc::new("xyzzy".into())"#],
    );
    check(
        Some("alloc::sync::Rc<String>"),
        &[],
        &["Rc::new(String::new())", r#"Rc::new("xyzzy".into())"#],
    );
}

#[test]
fn test_references_leak_a_box_and_slices_leak_a_vec() {
    check(
        Some("&'static String"),
        &[],
        &[
            "Box::leak(Box::new(String::new()))",
            r#"Box::leak(Box::new("xyzzy".into()))"#,
        ],
    );
    check(
        Some("&bool"),
        &[],
        &["Box::leak(Box::new(true))", "Box::leak(Box::new(false))"],
    );
    check(
        Some("&mut bool"),
        &[],
        &["Box::leak(Box::new(true))", "Box::leak(Box::new(false))"],
    );
    check(
        Some("[u8]"),
        &[],
        &[
            "Vec::leak(Vec::new())",
            "Vec::leak(vec![0])",
            "Vec::leak(vec![1])",
        ],
    );
    check(
        Some("&[u8]"),
        &[],
        &[
            "Vec::leak(Vec::new())",
            "Vec::leak(vec![0])",
            "Vec::leak(vec![1])",
        ],
    );
}

#[test]
fn test_arrays_and_tuples() {
    check(Some("[u8; 256]"), &[], &["[0; 256]", "[1; 256]"]);
    check(
        Some("(bool, usize)"),
        &[],
        &["(true, 0)", "(true, 1)", "(false, 0)", "(false, 1)"],
    );
    check(
        Some("(bool, Option<String>)"),
        &[],
        &[
            "(true, None)",
            "(true, Some(String::new()))",
            r#"(true, Some("xyzzy".into()))"#,
            "(false, None)",
            "(false, Some(String::new()))",
            r#"(false, Some("xyzzy".into()))"#,
        ],
    );
}

#[test]
fn test_collections_and_maps() {
    check(
        Some("std::collections::BTreeSet<String>"),
        &[],
        &[
            "BTreeSet::new()",
            "BTreeSet::from_iter([String::new()])",
            r#"BTreeSet::from_iter(["xyzzy".into()])"#,
        ],
    );
    check(
        Some("BTreeMap<String, bool>"),
        &[],
        &[
            "BTreeMap::new()",
            "BTreeMap::from_iter([(String::new(), true)])",
            "BTreeMap::from_iter([(String::new(), false)])",
            r#"BTreeMap::from_iter([("xyzzy".into(), true)])"#,
            r#"BTreeMap::from_iter([("xyzzy".into(), false)])"#,
        ],
    );
}

#[test]
fn test_cow_and_unknown_single_argument_containers() {
    check(
        Some("Cow<'static, str>"),
        &[],
        &[
            r#"Cow::Borrowed("")"#,
            r#"Cow::Owned("".to_owned())"#,
            r#"Cow::Borrowed("xyzzy")"#,
            r#"Cow::Owned("xyzzy".to_owned())"#,
        ],
    );
    check(
        Some("UnknownContainer<'static, str>"),
        &[],
        &[
            "UnknownContainer::new()",
            r#"UnknownContainer::from_iter([""])"#,
            r#"UnknownContainer::new("")"#,
            r#"UnknownContainer::from("")"#,
            r#"UnknownContainer::from_iter(["xyzzy"])"#,
            r#"UnknownContainer::new("xyzzy")"#,
            r#"UnknownContainer::from("xyzzy")"#,
        ],
    );
}

#[test]
fn test_impl_iterator_and_never_and_unknown() {
    check(
        Some("impl Iterator<Item = String>"),
        &[],
        &[
            "::std::iter::empty()",
            "::std::iter::once(String::new())",
            r#"::std::iter::once("xyzzy".into())"#,
        ],
    );
    check(
        Some("impl std::iter::Iterator<Item = String>"),
        &[],
        &[
            "::std::iter::empty()",
            "::std::iter::once(String::new())",
            r#"::std::iter::once("xyzzy".into())"#,
        ],
    );
    check(Some("impl Borrow<String>"), &[], &[]);
    check(Some("!"), &[], &[]);
    check(Some("MyType"), &[], &["Default::default()"]);
    // Only a bare single-segment path is a primitive or String.
    check(Some("std::string::String"), &[], &["Default::default()"]);
}

#[test]
fn test_impl_fn_trait_with_parenthesized_arguments_has_no_values() {
    // syn parses `Fn(E) -> T` sugar; an impl Trait other than Iterator gets
    // no replacement (fnvalue.rs, Type::ImplTrait).
    check(Some("impl Fn(E) -> ExecError + '_"), &[], &[]);
    check(Some("impl FnMut(u8, &str)"), &[], &[]);
}

#[test]
fn test_boxed_fn_trait_object_is_a_boxed_default() {
    check(
        Some("Box<dyn Fn(u8) -> u8 + Send>"),
        &[],
        &["Box::new(Default::default())"],
    );
}

#[test]
fn test_a_parenthesised_type_is_the_type_itself_not_a_one_element_tuple() {
    check(
        Some("Option<&(dyn Error + 'static)>"),
        &[],
        &["None", "Some(Box::leak(Box::new(Default::default())))"],
    );
}
