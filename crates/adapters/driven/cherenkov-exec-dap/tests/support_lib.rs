//! The support library's pure parts: naming DWARF's types by public paths,
//! the generated source, attributing compile errors to constructors, and
//! the layout guard.

use cherenkov_exec_dap::layouts::{DefaultImpl, Field, Layout, Variant};
use cherenkov_exec_dap::support_lib::{
    Request, attribute_errors, mirror_field_type, public_type, request_for, same_layout, sources,
};

#[test]
fn test_std_types_are_named_by_their_public_paths_without_the_global_allocator() {
    assert_eq!(
        public_type(
            "std::collections::hash::set::HashSet<u32, std::hash::random::RandomState, alloc::alloc::Global>"
        ),
        Ok("std::collections::HashSet<u32, std::hash::RandomState>".to_string())
    );
    assert_eq!(
        public_type(
            "core::result::Result<alloc::sync::Arc<i64, alloc::alloc::Global>, alloc::string::String>"
        ),
        Ok("Result<std::sync::Arc<i64>, String>".to_string())
    );
    assert_eq!(
        public_type("alloc::boxed::Box<i64, alloc::alloc::Global>"),
        Ok("Box<i64>".to_string())
    );
    assert_eq!(public_type("&u32"), Ok("&u32".to_string()));
    assert_eq!(
        public_type("&mut alloc::vec::Vec<u8, alloc::alloc::Global>"),
        Ok("&mut Vec<u8>".to_string())
    );
    assert_eq!(
        public_type("core::task::poll::Poll<std::path::PathBuf>"),
        Ok("std::task::Poll<std::path::PathBuf>".to_string())
    );
    assert_eq!(
        public_type("(u32, &str, [u8; 4])"),
        Ok("(u32, &str, [u8; 4])".to_string())
    );
}

#[test]
fn test_a_mirrors_field_borrows_for_static_wherever_its_type_borrows() {
    assert_eq!(
        mirror_field_type("alloc::borrow::Cow<str>"),
        Ok("std::borrow::Cow<'static, str>".to_string())
    );
    assert_eq!(
        mirror_field_type("core::option::Option<[alloc::borrow::Cow<[u8]>; 2]>"),
        Ok("Option<[std::borrow::Cow<'static, [u8]>; 2]>".to_string())
    );
    assert_eq!(
        mirror_field_type("&[&str]"),
        Ok("&'static [&'static str]".to_string())
    );
    // A constructor's out pointer elides it.
    assert_eq!(
        public_type("alloc::borrow::Cow<str>"),
        Ok("std::borrow::Cow<str>".to_string())
    );
}

#[test]
fn test_arrays_differing_in_count_or_element_do_not_match() {
    let array = |count: u64, elem: Layout| Layout::Array {
        count,
        elem: Box::new(elem),
    };
    assert_eq!(same_layout(&array(2, int(8)), &array(2, int(8))), Ok(()));
    assert_eq!(array(2, int(8)).size(), 16);
    let e = same_layout(&array(2, int(8)), &array(4, int(4))).unwrap_err();
    assert!(e.contains("array"), "{e}");
    let e = same_layout(&array(2, pair(8)), &array(2, pair(0))).unwrap_err();
    assert!(e.contains("P's field"), "{e}");
    assert_eq!(same_layout(&array(0, int(1)), &array(0, int(1))), Ok(()));
}

#[test]
fn test_a_type_outside_the_table_is_refused_by_its_path() {
    let e = public_type("core::option::Option<ops::Pair>").unwrap_err();
    assert!(e.contains("ops::Pair"), "{e}");
    assert!(public_type("ops::{closure_env#0}").is_err());
}

#[test]
fn test_a_poll_the_value_is_not_is_refused_not_found_unviable() {
    // An async block's poll, not known as the fn's body: `vec![]` would not
    // compile as the Poll, but the mutated source returns it from the fn.
    let poll = "core::task::poll::Poll<alloc::vec::Vec<u8, alloc::alloc::Global>>";
    assert!(request_for(poll, "vec![]").is_err());
    assert_eq!(
        request_for(poll, "Poll::Ready(vec![])").map(|r| r.ty),
        Ok("std::task::Poll<Vec<u8>>".to_string())
    );
}

#[test]
fn test_each_constructor_is_its_own_file_writing_the_value_through_its_out_pointer() {
    let files = sources(&[
        Request {
            ty: "std::collections::HashSet<u32, std::hash::RandomState>".into(),
            value: "HashSet::from_iter([1])".into(),
            ..Default::default()
        },
        Request {
            ty: "&u32".into(),
            value: "Box::leak(Box::new(0))".into(),
            ..Default::default()
        },
    ]);
    let file = |p: &str| &files.iter().find(|(n, _)| n == p).unwrap().1;
    assert!(file("Cargo.toml").contains("cdylib"));
    assert!(file("src/lib.rs").contains("mod v0;") && file("src/lib.rs").contains("mod v1;"));
    let v1 = file("src/v1.rs");
    assert!(v1.contains("fn cherenkov_v1(out: *mut &u32)"), "{v1}");
    assert!(v1.contains("out.write({ Box::leak(Box::new(0)) })"), "{v1}");
}

#[test]
fn test_an_error_inside_a_macro_is_attributed_to_the_constructor_expanding_it() {
    // `vec![..]` of the wrong element type: rustc's span is in alloc's
    // macros.rs, expanded from the constructor's file.
    let out = serde_json::json!({
        "reason": "compiler-message",
        "message": {
            "level": "error",
            "message": "mismatched types",
            "code": {"code": "E0308"},
            "spans": [{
                "file_name": "/rustlib/src/rust/library/alloc/src/macros.rs",
                "expansion": {"span": {
                    "file_name": "/rustlib/src/rust/library/alloc/src/macros.rs",
                    "expansion": {"span": {"file_name": "src/v2.rs"}},
                }},
            }],
        }
    })
    .to_string();
    let (by_ctor, other) = attribute_errors(&out);
    assert_eq!(
        by_ctor.get(&2),
        Some(&(false, "mismatched types".to_string()))
    );
    assert!(other.is_empty(), "{other:?}");
}

#[test]
fn test_compile_errors_are_attributed_to_their_constructors_file() {
    let msg = |file: &str, code: &str, text: &str| {
        serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "level": "error",
                "message": text,
                "code": {"code": code},
                "spans": [{"file_name": file}],
            }
        })
        .to_string()
    };
    let out = [
        msg("src/v3.rs", "E0308", "mismatched types"),
        msg("src/v3.rs", "E0308", "a second error"),
        msg(
            "src/v1.rs",
            "E0433",
            "failed to resolve: use of undeclared type `Pool`",
        ),
        serde_json::json!({"reason": "compiler-message", "message": {
            "level": "error", "message": "aborting due to 2 previous errors", "spans": []}})
        .to_string(),
        serde_json::json!({"reason": "build-finished", "success": false}).to_string(),
    ]
    .join("\n");
    let (by_ctor, other) = attribute_errors(&out);
    assert_eq!(by_ctor.len(), 2);
    assert_eq!(by_ctor[&3], (false, "mismatched types".to_string()));
    assert!(by_ctor[&1].0, "an unresolved name is not the value's fault");
    assert!(other.is_empty(), "{other:?}");
}

fn int(size: u64) -> Layout {
    Layout::Int {
        size,
        signed: false,
    }
}

fn pair(second_at: u64) -> Layout {
    Layout::Struct {
        name: "P".into(),
        size: 16,
        align: Some(8),
        fields: vec![
            Field {
                name: "a".into(),
                offset: 0,
                layout: int(8),
            },
            Field {
                name: "b".into(),
                offset: second_at,
                layout: int(8),
            },
        ],
        params: vec![],
        default: DefaultImpl::Absent,
        krate: "x".into(),
    }
}

#[test]
fn test_layouts_differing_in_a_field_offset_or_an_enum_tag_do_not_match() {
    assert_eq!(same_layout(&pair(8), &pair(8)), Ok(()));
    let e = same_layout(&pair(0), &pair(8)).unwrap_err();
    assert!(e.contains("P's field") && e.contains("b@0"), "{e}");
    let tagged = |t: u64| Layout::Enum {
        name: "E".into(),
        size: 8,
        tag: Some((0, 8)),
        variants: vec![Variant {
            name: "A".into(),
            tag: Some(t),
            fields: vec![],
        }],
        default: DefaultImpl::Absent,
        krate: "x".into(),
    };
    let e = same_layout(&tagged(1), &tagged(0)).unwrap_err();
    assert!(e.contains("E's variant"), "{e}");
}

#[test]
fn test_a_mirrors_field_order_and_padding_do_not_count_as_layout() {
    let Layout::Struct {
        name,
        size,
        align,
        mut fields,
        params,
        default,
        krate,
    } = pair(8)
    else {
        unreachable!()
    };
    fields.reverse();
    fields.push(Field {
        name: "_cherenkov_pad0".into(),
        offset: 16,
        layout: int(0),
    });
    let mirrored = Layout::Struct {
        name,
        size,
        align,
        fields,
        params,
        default,
        krate,
    };
    assert_eq!(same_layout(&mirrored, &pair(8)), Ok(()));
}

#[test]
fn test_the_programs_own_default_does_not_count_as_layout() {
    let with = |d| match pair(8) {
        Layout::Struct {
            name,
            size,
            align,
            fields,
            params,
            krate,
            ..
        } => Layout::Struct {
            name,
            size,
            align,
            fields,
            params,
            default: d,
            krate,
        },
        l => l,
    };
    assert_eq!(
        same_layout(&with(DefaultImpl::At(5)), &with(DefaultImpl::Absent)),
        Ok(())
    );
}

mod built {
    //! Slow: builds the fidelity fixture and the support library.

    use std::path::{Path, PathBuf};

    use cherenkov_exec_dap::layouts::{Layouts, Refusal};
    use cherenkov_exec_dap::support_lib::{
        Request, ScratchLease, public_type, request_in, resolve,
    };
    use cherenkov_exec_dap::toolchain::{build_rust_test_binaries, rustup_toolchain};

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../test/fidelity/cherenkov-fidelity/fixtures/rust-ops")
    }

    /// The fixture's library tests and the entry of `ops::{name}`.
    fn ops(name: &str) -> (PathBuf, u64) {
        let binary = build_rust_test_binaries(Path::new("cargo"), &fixture())
            .unwrap()
            .into_iter()
            .find(|b| {
                let out = std::process::Command::new(b)
                    .arg("--list")
                    .output()
                    .unwrap();
                String::from_utf8_lossy(&out.stdout).contains("test_tags")
            })
            .unwrap();
        let out = std::process::Command::new("nm")
            .arg("-C")
            .arg(&binary)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text
            .lines()
            .find(|l| l.ends_with(&format!(" ops::{name}")) && l.contains(" T "))
            .unwrap();
        let entry = u64::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap();
        (binary, entry)
    }

    #[test]
    #[ignore = "slow: needs cargo"]
    fn test_a_std_type_is_laid_out_alike_in_the_library_and_the_program() {
        let _lease = ScratchLease::acquire();
        let (binary, entry) = ops("tags");
        let layouts = Layouts::read(&binary).unwrap();
        let ty = public_type(&layouts.result_type_name(entry).unwrap()).unwrap();
        assert_eq!(ty, "std::collections::HashSet<u32, std::hash::RandomState>");
        let layout = layouts.result_of(entry).unwrap();
        let request = Request {
            ty,
            value: "HashSet::from_iter([1])".into(),
            ..Default::default()
        };
        let toolchain = rustup_toolchain(&fixture());
        let (library, symbol) = resolve(toolchain.as_deref(), &request, &layout).unwrap();
        assert!(library.is_file());
        assert!(symbol.starts_with("cherenkov_v"));
        eprintln!("{} {symbol}", library.display());
    }

    #[test]
    #[ignore = "slow: needs cargo"]
    fn test_a_constructor_whose_type_is_laid_out_otherwise_is_refused() {
        let _lease = ScratchLease::acquire();
        // The program's `Vec<u8>` against the library's `HashSet<u32>`.
        let (binary, entry) = ops("list");
        let layout = Layouts::read(&binary).unwrap().result_of(entry).unwrap();
        let request = Request {
            ty: "std::collections::HashSet<u32>".into(),
            value: "HashSet::new()".into(),
            ..Default::default()
        };
        let toolchain = rustup_toolchain(&fixture());
        match resolve(toolchain.as_deref(), &request, &layout) {
            Err(Refusal::Unsupported(why)) => {
                let why = why.to_string();
                assert!(why.contains("laid-out-differently"), "{why}");
                assert!(why.contains("HashSet<u32>"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    #[ignore = "slow: needs cargo"]
    fn test_a_value_that_does_not_compile_as_its_type_is_unviable_and_the_rest_build() {
        let _lease = ScratchLease::acquire();
        let (binary, entry) = ops("shared");
        let layout = Layouts::read(&binary).unwrap().result_of(entry).unwrap();
        let toolchain = rustup_toolchain(&fixture());
        let bad = Request {
            ty: "std::sync::Arc<i64>".into(),
            value: "Arc::new(\"no\")".into(),
            ..Default::default()
        };
        let good = Request {
            ty: "std::sync::Arc<i64>".into(),
            value: "Arc::new(3)".into(),
            ..Default::default()
        };
        cherenkov_exec_dap::support_lib::request(toolchain.as_deref(), &bad);
        assert!(resolve(toolchain.as_deref(), &good, &layout).is_ok());
        match resolve(toolchain.as_deref(), &bad, &layout) {
            Err(Refusal::Unviable(why)) => {
                assert!(why.to_string().contains("mismatched types"), "{why}")
            }
            other => panic!("{other:?}"),
        }
    }

    fn facts() -> cherenkov_mutation::types::DefaultFacts {
        let mut facts = cherenkov_mutation::types::DefaultFacts::default();
        facts.project_crate("ops");
        let src = std::fs::read_to_string(fixture().join("src/lib.rs")).unwrap();
        facts.merge(cherenkov_syntax::rust_types::default_facts(&src));
        facts
    }

    #[test]
    #[ignore = "slow: needs cargo"]
    fn test_private_structs_and_enums_are_mirrored_with_the_programs_layout() {
        let _lease = ScratchLease::acquire();
        let toolchain = rustup_toolchain(&fixture());
        for (name, value) in [
            ("inventory", "Default::default()"),
            ("labels", "vec![Default::default()]"),
            ("event_index", "HashMap::new()"),
            // A niche-encoded enum, laid out by rustc as the original.
            ("slot_index", "HashMap::new()"),
            // A function pointer and a trait object.
            ("hooks", "vec![Default::default()]"),
            // Arrays: plain, nested, empty, of a mirror.
            ("gauges", "vec![Default::default()]"),
            // A `Cow` and a reference, borrowed for `'static`.
            ("banners", "vec![Default::default()]"),
        ] {
            let (binary, entry) = ops(name);
            let layouts = Layouts::read(&binary).unwrap();
            let dwarf = layouts.result_type_name(entry).unwrap();
            let request = request_in(&dwarf, value, &layouts, &facts()).unwrap();
            assert!(!request.mirrors.is_empty(), "{name}: {request:?}");
            let layout = layouts.result_of(entry).unwrap();
            if let Err(e) = resolve(toolchain.as_deref(), &request, &layout) {
                panic!("{name}: {e}\n{request:?}");
            }
        }
    }
}
