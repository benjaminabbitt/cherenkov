//! Tier P return stubs for the trivial values a function body is replaced
//! by (`true`, `1`, `-1`, `None`, `Ok(())`): the bytes are the ones rustc
//! makes of the value (`isa/tierp-shapes.toml`, written by the gate in
//! `tests/tierp_gate`), chosen by the result type's layout from the DWARF.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use cherenkov_core::edit::{Edit, Lit};
use cherenkov_core::operator_set::named;
use cherenkov_core::plan::{Action, MutantPlan, plan_file};
use cherenkov_core::ports::SyntaxPort;
use cherenkov_core::syntax::LanguageId;
use cherenkov_exec_dap::isa::Isa;
use cherenkov_exec_dap::layouts::{Layout, Variant};
use cherenkov_exec_dap::shapes::Table;
use cherenkov_exec_dap::stubs::shape_of;
use cherenkov_exec_dap::tierp::{Insn, Unpatchable, derive_returning};
use cherenkov_syntax::TreeSitterSyntax;

const FILE: &str = "src/lib.rs";

fn int(size: u64, signed: bool) -> Layout {
    Layout::Int { size, signed }
}

fn variant(name: &str, tag: Option<u64>) -> Variant {
    Variant {
        name: name.into(),
        tag,
        fields: Vec::new(),
    }
}

/// An enum of `size` bytes with its tag of `tag_size` bytes at `offset`.
fn enum_of(size: u64, tag: (u64, u64), variants: Vec<Variant>) -> Layout {
    Layout::Enum {
        name: "Option".into(),
        size,
        tag: Some(tag),
        variants,
        default: cherenkov_exec_dap::layouts::DefaultImpl::Absent,
        krate: "core".into(),
    }
}

fn option(size: u64, tag_size: u64, none: u64) -> Layout {
    enum_of(
        size,
        (0, tag_size),
        vec![variant("None", Some(none)), variant("Some", None)],
    )
}

fn shape(layout: &Layout, value: &str) -> Option<String> {
    shape_of(layout, value).map(|s| s.to_string())
}

#[test]
fn test_shape_of_a_bool_true_is_named_for_the_one_byte_integer() {
    assert_eq!(
        shape(&int(1, false), "true").as_deref(),
        Some("stub amd64 u8 true")
    );
}

#[test]
fn test_shape_of_one_and_minus_one_is_named_for_the_width_and_sign() {
    let got = [
        shape(&int(4, false), "1"),
        shape(&int(8, true), "-1"),
        shape(&int(2, true), "1"),
    ];
    assert_eq!(
        got,
        [
            Some("stub amd64 u32 1".to_string()),
            Some("stub amd64 i64 -1".to_string()),
            Some("stub amd64 i16 1".to_string()),
        ]
    );
}

#[test]
fn test_shape_of_an_integer_wider_than_a_register_is_none() {
    assert_eq!(shape(&int(16, false), "1"), None);
}

#[test]
fn test_shape_of_minus_one_for_an_unsigned_type_is_none() {
    assert_eq!(shape(&int(4, false), "-1"), None);
}

#[test]
fn test_shape_of_true_for_a_wider_integer_is_none() {
    assert_eq!(shape(&int(4, false), "true"), None);
}

#[test]
fn test_shape_of_a_value_without_a_stub_is_none() {
    assert_eq!(shape(&int(4, false), "7"), None);
    assert_eq!(shape(&Layout::Float { size: 8 }, "1.0"), None);
}

#[test]
fn test_shape_of_none_carries_the_tag_the_dwarf_gives_it() {
    assert_eq!(
        shape(&option(1, 1, 2), "None").as_deref(),
        Some("stub amd64 enum1 None tag1=2")
    );
    assert_eq!(
        shape(&option(8, 4, 0), "None").as_deref(),
        Some("stub amd64 enum8 None tag4=0")
    );
}

#[test]
fn test_shape_of_ok_unit_is_the_ok_variants_tag() {
    // `Ok(())`'s payload is a field of no size.
    let mut ok = variant("Ok", Some(0));
    ok.fields.push(cherenkov_exec_dap::layouts::Field {
        name: "__0".into(),
        offset: 0,
        layout: int(0, false),
    });
    let result = enum_of(8, (0, 8), vec![ok, variant("Err", None)]);
    assert_eq!(
        shape(&result, "Ok(())").as_deref(),
        Some("stub amd64 enum8 Ok(()) tag8=0")
    );
}

#[test]
fn test_shape_of_none_of_an_enum_bigger_than_two_registers_is_none() {
    assert_eq!(shape(&option(24, 8, 0), "None"), None);
}

#[test]
fn test_shape_of_none_whose_tag_is_not_at_the_start_is_none() {
    let layout = enum_of(
        16,
        (8, 8),
        vec![variant("None", Some(0)), variant("Some", None)],
    );
    assert_eq!(shape(&layout, "None"), None);
}

#[test]
fn test_shape_of_none_when_that_variant_holds_the_niche_payload_is_none() {
    let layout = enum_of(
        8,
        (0, 8),
        vec![variant("None", None), variant("Some", Some(0))],
    );
    assert_eq!(shape(&layout, "None"), None);
}

#[test]
fn test_shape_of_ok_whose_payload_has_fields_is_none() {
    let mut ok = variant("Ok", Some(0));
    ok.fields.push(cherenkov_exec_dap::layouts::Field {
        name: "__0".into(),
        offset: 4,
        layout: int(4, false),
    });
    let layout = enum_of(8, (0, 4), vec![ok, variant("Err", None)]);
    assert_eq!(shape(&layout, "Ok(())"), None);
}

#[test]
fn test_shape_of_the_value_of_a_different_variant_is_none() {
    assert_eq!(shape(&option(8, 4, 0), "Some(1)"), None);
    assert_eq!(shape(&option(8, 4, 0), "Ok(())"), None);
}

// --- derive_returning

fn plan_for(source: &str, value: &str) -> MutantPlan {
    let syntax = TreeSitterSyntax::builtin().unwrap();
    let rust = LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let file = syntax
        .analyze(&rust, Path::new(FILE), Arc::from(source))
        .unwrap();
    plan_file(
        &file,
        &profile,
        &named("cargo-mutants").unwrap().operators(),
    )
    .plans
    .into_iter()
    .find(|p| matches!(&p.edit, Edit::ReplaceWithLiteral { lit: Lit::Text(v), .. } if v == value))
    .unwrap_or_else(|| panic!("no mutant with {value}"))
}

/// A function of `bytes` long: `push rbp; mov rbp, rsp; ...; ret`.
fn body(len: usize) -> Vec<Insn> {
    let mut rows = vec![(0x1000u64, vec![0x55u8]), (0x1001, vec![0x48, 0x89, 0xe5])];
    let mut at = 0x1004;
    while at < 0x1000 + len as u64 - 1 {
        rows.push((at, vec![0x90]));
        at += 1;
    }
    rows.push((at, vec![0xc3]));
    rows.into_iter()
        .map(|(pc, bytes)| Insn {
            pc,
            bytes,
            file: FILE.into(),
            line: 1,
            column: 0,
        })
        .collect()
}

fn table(stubs: &[(&str, &[u8])]) -> Table {
    let map: BTreeMap<String, Vec<u8>> = stubs
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_vec()))
        .collect();
    Table::harvested(map)
}

fn isa(stubs: &[(&str, &[u8])]) -> Isa {
    Isa::amd64_unfiltered()
        .with_shapes(table(stubs))
        .with_abi("sysv")
        .unwrap()
}

const ONE: &str = "stub amd64 u32 1";
const ONE_BYTES: [u8; 6] = [0xb8, 1, 0, 0, 0, 0xc3];

fn u32_one() -> MutantPlan {
    plan_for("pub fn f(a: u32) -> u32 { a + 2 }\n", "1")
}

#[test]
fn test_derive_returning_a_shape_in_the_table_writes_its_bytes_over_the_entry() {
    let isa = isa(&[(ONE, &ONE_BYTES)]);
    let patch = derive_returning(&u32_one(), FILE, &body(12), &isa, Some(&int(4, false))).unwrap();
    assert_eq!(patch.regions.len(), 1);
    assert_eq!(patch.regions[0].addr, 0x1000);
    assert_eq!(patch.regions[0].replacement, ONE_BYTES);
}

#[test]
fn test_derive_returning_a_shape_the_table_lacks_is_declined() {
    let isa = isa(&[]);
    assert_eq!(
        derive_returning(&u32_one(), FILE, &body(12), &isa, Some(&int(4, false))),
        Err(Unpatchable::NonZeroValue("1".into()))
    );
}

#[test]
fn test_derive_returning_without_the_result_layout_is_declined() {
    let isa = isa(&[(ONE, &ONE_BYTES)]);
    assert_eq!(
        derive_returning(&u32_one(), FILE, &body(12), &isa, None),
        Err(Unpatchable::NonZeroValue("1".into()))
    );
}

#[test]
fn test_derive_returning_a_function_shorter_than_the_stub_is_declined() {
    let isa = isa(&[(ONE, &ONE_BYTES)]);
    assert_eq!(
        derive_returning(&u32_one(), FILE, &body(5), &isa, Some(&int(4, false))),
        Err(Unpatchable::FunctionTooShort { bytes: 5, stub: 6 })
    );
}

#[test]
fn test_derive_returning_for_an_async_fn_is_declined() {
    let plan = plan_for("pub async fn f(a: u32) -> u32 { a + 2 }\n", "1");
    assert!(matches!(
        plan.action,
        Action::ReturnDefault { async_fn: true, .. }
    ));
    let isa = isa(&[(ONE, &ONE_BYTES)]);
    assert_eq!(
        derive_returning(&plan, FILE, &body(12), &isa, Some(&int(4, false))),
        Err(Unpatchable::NonZeroValue("1".into()))
    );
}

#[test]
fn test_derive_returning_without_the_sysv_convention_is_declined() {
    let isa = Isa::amd64_unfiltered().with_shapes(table(&[(ONE, &ONE_BYTES)]));
    assert_eq!(
        derive_returning(&u32_one(), FILE, &body(12), &isa, Some(&int(4, false))),
        Err(Unpatchable::NoReturnStub)
    );
}

#[test]
fn test_derive_returning_a_zero_value_still_uses_the_zero_stub() {
    let plan = plan_for("pub fn f(a: u32) -> u32 { a + 2 }\n", "0");
    let isa = isa(&[]);
    let patch = derive_returning(&plan, FILE, &body(12), &isa, Some(&int(4, false))).unwrap();
    assert_eq!(
        patch.regions[0].replacement,
        [0x31, 0xc0, 0x0f, 0x57, 0xc0, 0xc3]
    );
}

#[test]
fn test_the_table_shipped_with_the_isa_passes_the_stubs_of_scalars() {
    let isa = Isa::amd64().with_abi("sysv").unwrap();
    let plan = plan_for("pub fn f(a: u32) -> u32 { a + 2 }\n", "1");
    let patch = derive_returning(&plan, FILE, &body(12), &isa, Some(&int(4, false))).unwrap();
    assert_eq!(patch.regions[0].replacement.last(), Some(&0xc3));
}

#[test]
fn test_derive_returning_a_go_plan_never_gets_a_rust_stub() {
    let mut plan = u32_one();
    plan.file = std::path::PathBuf::from("calc.go");
    let isa = isa(&[(ONE, &ONE_BYTES)]);
    assert_eq!(
        derive_returning(&plan, FILE, &body(12), &isa, Some(&int(4, false))),
        Err(Unpatchable::NonZeroValue("1".into()))
    );
}

#[test]
fn test_an_isa_names_the_calling_convention_it_was_given() {
    let plain = Isa::amd64_unfiltered();
    assert_eq!(plain.abi(), None);
    assert_eq!(plain.with_abi("sysv").unwrap().abi(), Some("sysv"));
    assert_eq!(plain.with_abi("go").unwrap().abi(), Some("go"));
}
