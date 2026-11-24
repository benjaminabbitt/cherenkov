//! A function's result type, read from the test binary's DWARF, and values
//! of it built as bytes: enum tags from DWARF's discriminant values,
//! payloads at their offsets. Slow: builds the fidelity fixture.

use std::path::{Path, PathBuf};

use cherenkov_exec_dap::layouts::{Layouts, build};
use cherenkov_exec_dap::toolchain::build_rust_test_binaries;

fn ops_binary() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../test/fidelity/cherenkov-fidelity/fixtures/rust-ops");
    build_rust_test_binaries(Path::new("cargo"), &root)
        .unwrap()
        .into_iter()
        .find(|b| {
            let out = std::process::Command::new(b)
                .arg("--list")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).contains("test_classify")
        })
        .unwrap()
}

/// The file address of a function's entry, from the symbol table: `name`
/// in `ops`, or a whole demangled symbol (`<ops::Pair>::set`), named as v0
/// mangling demangles it. Which mangling a binary has is the toolchain's
/// default (legacy on stable, v0 on nightly), so the legacy spelling of the
/// same symbol is looked for too.
fn entry(binary: &Path, name: &str) -> u64 {
    let symbol = if name.starts_with('<') {
        name.to_string()
    } else {
        format!("ops::{name}")
    };
    let spellings = [symbol.clone(), legacy(&symbol)];
    let out = std::process::Command::new("nm")
        .arg("-C")
        .arg(binary)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.contains(" T ") && spellings.iter().any(|s| l.ends_with(&format!(" {s}"))))
        .unwrap_or_else(|| panic!("no symbol {symbol}"));
    u64::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap()
}

/// A v0-demangled symbol as legacy mangling demangles it: a closure is
/// `{{closure}}`, not `{closure#0}`, and an inherent method's `<T>::m` is
/// `T::m`.
fn legacy(symbol: &str) -> String {
    let symbol = symbol.replace("{closure#0}", "{{closure}}");
    match symbol.strip_prefix('<').and_then(|s| s.split_once(">::")) {
        Some((ty, rest)) => format!("{ty}::{rest}"),
        None => symbol,
    }
}

/// The niche tag and payload words of `Ok(5)` as the toolchain's own
/// compiler writes a `Result<i64, String>`, the fixture's `parse`'s result:
/// built and run beside the test, so the expectation is the compiler's and
/// not a constant copied from one version of it. The third word is padding
/// in `Ok`, left uninitialised, so it is not returned.
fn compiled_ok_five() -> [u64; 2] {
    let dir = std::env::temp_dir().join(format!("cherenkov-niche-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("niche.rs");
    std::fs::write(
        &source,
        "fn main() {\n\
         let r: Result<i64, String> = std::hint::black_box(Ok(5));\n\
         let w: [u64; 3] = unsafe { std::mem::transmute(r) };\n\
         println!(\"{} {} {}\", w[0], w[1], w[2]);\n\
         }\n",
    )
    .unwrap();
    let program = dir.join("niche");
    let built = std::process::Command::new("rustc")
        .arg("-o")
        .arg(&program)
        .arg(&source)
        .status()
        .unwrap();
    assert!(built.success(), "rustc failed on {}", source.display());
    let out = std::process::Command::new(&program).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let words: Vec<u64> = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(|w| w.parse().unwrap())
        .collect();
    [words[0], words[1]]
}

fn bytes_of(fun: &str, value: &str) -> Vec<u8> {
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let result = layouts.result_of(entry(&bin, fun)).unwrap();
    build(&result, value).unwrap()
}

fn word(b: &[u8], k: usize) -> u64 {
    u64::from_le_bytes(b[k * 8..k * 8 + 8].try_into().unwrap())
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_option_tag_and_payload() {
    let some = bytes_of("maybe", "Some(1)");
    assert_eq!((some.len(), word(&some, 0), word(&some, 1)), (16, 1, 1));
    let none = bytes_of("maybe", "None");
    assert_eq!(word(&none, 0), 0);
    assert_eq!(word(&bytes_of("maybe", "Some(-1)"), 1), u64::MAX);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_result_ok_uses_the_niche_value_the_compiler_writes() {
    // As the compiler writes it: the String's capacity word holds the niche
    // tag, the payload follows.
    let ok = bytes_of("parse", "Ok(5)");
    assert_eq!(ok.len(), 24);
    assert_eq!([word(&ok, 0), word(&ok, 1)], compiled_ok_five());
    assert_eq!(word(&ok, 1), 5, "the payload follows the niche tag");
    assert_eq!(word(&ok, 2), 0, "the padding is written as zero");
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_empty_string_and_vec_have_zero_length_and_capacity_and_a_dangling_pointer() {
    for (fun, value) in [("name", "String::new()"), ("list", "vec![]")] {
        let b = bytes_of(fun, value);
        let mut words: Vec<u64> = (0..3).map(|k| word(&b, k)).collect();
        words.sort();
        assert_eq!(words, [0, 0, 1], "{fun}: {value}");
    }
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_struct_default_of_integers_is_refused_without_the_programs_default() {
    // A user Default may not be zero; building it needs the program's code.
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let result = layouts.result_of(entry(&bin, "pair")).unwrap();
    assert!(build(&result, "Default::default()").is_err());
}

// --- Values on the heap: the bytes, and the blocks they point to, which the
// debugger allocates in the program and patches in.

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_string_value_points_at_its_bytes_on_the_heap() {
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let result = layouts.result_of(entry(&bin, "name")).unwrap();
    let built = cherenkov_exec_dap::layouts::build_value(&result, "\"xyzzy\".into()").unwrap();
    assert_eq!(built.heap.len(), 1);
    let (at, block) = &built.heap[0];
    assert_eq!(block.bytes, b"xyzzy");
    // Capacity and length 5; the pointer slot is filled in by the debugger.
    let mut words: Vec<u64> = (0..3)
        .filter(|k| k * 8 != *at)
        .map(|k| word(&built.bytes, k))
        .collect();
    words.sort();
    assert_eq!(words, [5, 5]);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_vec_value_holds_its_elements_on_the_heap() {
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let result = layouts.result_of(entry(&bin, "list")).unwrap();
    let built = cherenkov_exec_dap::layouts::build_value(&result, "vec![1]").unwrap();
    assert_eq!(built.heap.len(), 1);
    assert_eq!(built.heap[0].1.bytes, [1]);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_method_s_result_type_is_read_from_its_declaration() {
    use cherenkov_exec_dap::layouts::Layout;
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let result = layouts.result_of(entry(&bin, "<ops::Pair>::set"));
    let Some(Layout::Struct { name, .. }) = &result else {
        panic!("{result:?}");
    };
    assert!(name.starts_with("Vec<alloc::string::String"), "{name}");
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_default_of_a_number_is_zero_and_unit_has_no_bytes() {
    // `Ok(Default::default())` as `Ok(0)`; `Ok(())` as the bare Ok tag.
    assert_eq!(
        bytes_of("parse", "Ok(Default::default())"),
        bytes_of("parse", "Ok(0)")
    );
    // `Result<(), String>`: the String's size; Ok's tag is DWARF's, and the
    // fidelity test judges the value against the compiled mutant.
    assert_eq!(bytes_of("check", "Ok(())").len(), 24);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_default_of_a_vector_is_empty() {
    assert_eq!(
        bytes_of("list", "Default::default()"),
        bytes_of("list", "vec![]")
    );
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_unit_and_default_are_refused_where_they_do_not_fit() {
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let of = |fun: &str| layouts.result_of(entry(&bin, fun)).unwrap();
    // `()` is no value of an `Option<i64>`.
    assert!(build(&of("maybe"), "()").is_err());
    // A struct's default needs the program's own `Default`, and says so
    // about the value it was asked for.
    let err = build(&of("pair"), "Default::default()")
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("Default::default() for"), "{err}");
}

/// The file address of any symbol, local ones included (an `impl`'s
/// methods are not exported).
fn symbol(binary: &Path, name: &str) -> u64 {
    let out = std::process::Command::new("nm")
        .arg("-C")
        .arg(binary)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.ends_with(&format!(" {name}")))
        .unwrap_or_else(|| panic!("no symbol {name}"));
    u64::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap()
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_struct_s_default_is_the_program_s_own_called_where_the_value_needs_it() {
    use cherenkov_exec_dap::layouts::{Layout, build_value};
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let pair = layouts.result_of(entry(&bin, "pair")).unwrap();
    let default = symbol(&bin, "<ops::Pair as core::default::Default>::default");
    let Layout::Struct { default: found, .. } = &pair else {
        panic!("{pair:?}");
    };
    assert_eq!(
        *found,
        cherenkov_exec_dap::layouts::DefaultImpl::At(default)
    );
    // No bytes to build: the program's `Default` makes them, at offset 0.
    let built = build_value(&pair, "Default::default()").unwrap();
    assert_eq!(built.calls, [(0, default)]);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_default_of_a_type_with_no_default_in_the_binary_is_refused_not_unviable() {
    // DWARF cannot tell a type without Default from one whose derived
    // (inline) Default was never used: neither leaves code or a DIE.
    use cherenkov_exec_dap::layouts::{DefaultImpl, Layout, Refusal, build_value};
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let plain = layouts.result_of(entry(&bin, "plain")).unwrap();
    let Layout::Struct { default, .. } = &plain else {
        panic!("{plain:?}");
    };
    assert_eq!(*default, DefaultImpl::Absent);
    assert!(matches!(
        build_value(&plain, "Default::default()"),
        Err(Refusal::Unsupported(_))
    ));
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_field_less_enum_is_its_tag() {
    use cherenkov_exec_dap::layouts::{Refusal, build_value};
    assert_eq!(bytes_of("shade", "Shade::Dark"), [1]);
    assert_eq!(bytes_of("shade", "Shade::Light"), [0]);
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let shade = layouts.result_of(entry(&bin, "shade")).unwrap();
    assert!(matches!(
        build_value(&shade, "Default::default()"),
        Err(Refusal::Unsupported(_))
    ));
}

/// The fixture's source facts about its types' defaults.
fn ops_facts() -> cherenkov_mutation::types::DefaultFacts {
    let src =
        include_str!("../../../../test/fidelity/cherenkov-fidelity/fixtures/rust-ops/src/lib.rs");
    let mut facts = cherenkov_syntax::rust_types::default_facts(src);
    facts.project_crate("ops");
    facts
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_derived_default_with_no_code_in_the_binary_is_built_field_by_field() {
    use cherenkov_exec_dap::layouts::{DefaultImpl, Layout, build_value_with};
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let unused = layouts.result_of(entry(&bin, "unused")).unwrap();
    let Layout::Struct { default, .. } = &unused else {
        panic!("{unused:?}");
    };
    assert_ne!(*default, DefaultImpl::At(0), "never compiled");
    assert!(matches!(
        default,
        DefaultImpl::Absent | DefaultImpl::Unlinked
    ));
    let built = build_value_with(&unused, "Default::default()", &ops_facts()).unwrap();
    // n: 0, s: an empty String, as `Unused { n: 0, s: String::new() }`.
    let expect = build_value_with(&unused, "Unused { n: 0, s: String::new() }", &ops_facts());
    assert!(built.calls.is_empty() && built.heap.is_empty(), "{built:?}");
    if let Ok(e) = expect {
        assert_eq!(built, e);
    }
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_default_of_a_project_type_without_one_in_the_source_is_unviable() {
    use cherenkov_exec_dap::layouts::{Refusal, build_value, build_value_with};
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    for fun in ["plain", "shade"] {
        let l = layouts.result_of(entry(&bin, fun)).unwrap();
        assert_eq!(l.krate(), Some("ops"), "{fun}: {l:?}");
        assert!(
            matches!(
                build_value_with(&l, "Default::default()", &ops_facts()),
                Err(Refusal::Unviable(_))
            ),
            "{fun}"
        );
        // Without the source, no claim.
        assert!(matches!(
            build_value(&l, "Default::default()"),
            Err(Refusal::Unsupported(_))
        ));
    }
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_128_bit_integer_is_written_whole() {
    assert_eq!(bytes_of("wide", "0"), [0u8; 16]);
    let mut one = [0u8; 16];
    one[0] = 1;
    assert_eq!(bytes_of("wide", "1"), one);
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_an_async_fn_s_body_is_its_future_s_poll_not_its_constructor() {
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let poll = entry(&bin, "twice::{closure#0}");
    assert!(layouts.is_future_poll(poll));
    assert!(!layouts.is_future_poll(entry(&bin, "twice")));
    assert!(!layouts.is_future_poll(entry(&bin, "compare")));
    // The poll's result wraps the fn's: Poll::Ready(7) is tag 0, then 7.
    let result = layouts.result_of(poll).unwrap();
    let ready = build(&result, "Poll::Ready(7)").unwrap();
    assert_eq!((ready.len(), word(&ready, 0), word(&ready, 1)), (16, 0, 7));
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_a_types_default_is_never_another_types_of_the_same_name() {
    use cherenkov_exec_dap::layouts::{DefaultImpl, Layout};
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let default = |f: &str| match layouts.result_of(entry(&bin, f)).unwrap() {
        Layout::Struct { default, .. } => default,
        other => panic!("{other:?}"),
    };
    // `generated::Letter`'s Default is in the binary; the root `Letter` has none.
    assert!(matches!(default("generated_letter"), DefaultImpl::At(_)));
    assert!(!matches!(default("letter"), DefaultImpl::At(_)));
}

#[test]
#[ignore = "slow: needs cargo"]
fn test_an_array_field_is_named_by_its_element_and_count_nested_and_empty_too() {
    use cherenkov_exec_dap::layouts::DefinitionKind;
    let bin = ops_binary();
    let layouts = Layouts::read(&bin).unwrap();
    let def = layouts.definition("ops::Gauge").unwrap();
    let DefinitionKind::Struct(members) = def.kind else {
        panic!("{def:?}");
    };
    let field = |name: &str| {
        let m = members.iter().find(|m| m.name == name).unwrap();
        (m.ty.clone().unwrap_or_default(), m.size)
    };
    assert_eq!(field("bins"), ("[u64; 2]".into(), 16));
    assert_eq!(field("grid"), ("[[u16; 3]; 2]".into(), 12));
    assert_eq!(field("none"), ("[u8; 0]".into(), 0));
    assert_eq!(field("marks"), ("[ops::Mark; 2]".into(), 8));
}
