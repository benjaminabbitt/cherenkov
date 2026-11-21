//! cargo-mutants' mutant names, re-derived from its behaviour: the token
//! pretty-printer, the `Type::function` / `<impl Trait for Type>::function`
//! context of a function, and the struct a field initialiser belongs to.

use cherenkov_syntax::rust_names::{Names, arm_pattern, pretty, squash_lines};

#[test]
fn pretty_spells_types_and_paths_as_cargo_mutants_does() {
    let cases = [
        (
            "<impl Iterator for MergeTrees < AE , BE , AIT , BIT > > :: next -> Option < Self ::  Item >",
            "<impl Iterator for MergeTrees<AE, BE, AIT, BIT>>::next -> Option<Self::Item>",
        ),
        ("Lex < 'buf >::take", "Lex<'buf>::take"),
        (
            "impl Iterator < Item = String >",
            "impl Iterator<Item = String>",
        ),
        ("\"warning\"\n | \"error\"", "\"warning\" | \"error\""),
        ("fair || foul", "fair || foul"),
        ("a => b", "a => b"),
        ("-> Box<dyn Fn() -> u8>", "-> Box<dyn Fn()-> u8>"),
        ("Result<(), D::Refused>", "Result<(), D::Refused>"),
        ("& 'static str", "&'static str"),
    ];
    for (from, to) in cases {
        assert_eq!(pretty(from), to, "{from}");
    }
}

#[test]
fn squash_lines_joins_indented_lines() {
    assert_eq!(squash_lines("a b c"), "a b c");
    assert_eq!(squash_lines("a\n    b c \n\nd  \n  e"), "a b c d  e");
}

#[test]
fn an_arms_pattern_is_what_precedes_the_arrow() {
    assert_eq!(arm_pattern("1 | 2 => t += 1,"), "1 | 2");
    assert_eq!(arm_pattern("Some(x) => { a => b }"), "Some(x)");
    assert_eq!(arm_pattern("Kind::A { n, .. } => n,"), "Kind::A{n, ..}");
}

const SRC: &str = r#"
pub mod m {
    pub trait T { fn d(&self) -> u32 { 1 } }
    impl<'a, X> S<'a, X> {
        pub fn get(&self) -> Result<(), String> { Ok(()) }
    }
    impl std::fmt::Display for P2 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
    }
    pub fn outer(x: i32) -> i32 {
        fn inner(y: i32) -> i32 { y * 3 }
        inner(x)
    }
}
fn free() { let _ = P { a: 1 }; }
"#;

fn function(names: &Names, needle: &str) -> (String, String) {
    let at = SRC.find(needle).unwrap();
    let f = names.function_at(at, at + needle.len()).unwrap();
    (f.qualified.clone(), f.return_type.clone())
}

#[test]
fn functions_are_named_by_their_namespace() {
    let names = Names::parse(SRC);
    let want = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(function(&names, "u32 { 1 }"), want("m::T::d", "-> u32"));
    assert_eq!(
        function(&names, "String> { Ok(()) }"),
        want("m::S<'a, X>::get", "-> Result<(), String>")
    );
    assert_eq!(
        function(&names, "Result { Ok(()) }"),
        want(
            "m::<impl std::fmt::Display for P2>::fmt",
            "-> std::fmt::Result"
        )
    );
    assert_eq!(function(&names, "inner(x)"), want("m::outer", "-> i32"));
    assert_eq!(function(&names, "y * 3"), want("m::outer::inner", "-> i32"));
    assert_eq!(function(&names, "let _ = P"), want("free", ""));
}

#[test]
fn a_field_initialiser_knows_its_struct() {
    let names = Names::parse(SRC);
    let at = SRC.find("a: 1").unwrap();
    assert_eq!(names.struct_at(at, at + 4), Some("P"));
    assert_eq!(names.struct_at(0, 3), None);
}
