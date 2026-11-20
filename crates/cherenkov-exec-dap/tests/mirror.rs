//! Mirror types generated from DWARF layouts: every field at its offset,
//! enums by their tag values. Pure: no compiler or debugger.

use cherenkov_exec_dap::support_lib::mirror::{Member, Shape, TypeDef, VariantDef, define};

fn member(name: &str, offset: u64, size: u64, ty: &str) -> Member {
    Member {
        name: name.into(),
        offset,
        size,
        ty: ty.into(),
    }
}

fn record(derived_default: bool) -> TypeDef {
    // As rustc may lay it out: the u64 first, the u8 after the u16.
    TypeDef {
        name: "ops::Record".into(),
        size: 24,
        align: Some(8),
        shape: Shape::Struct(vec![
            member("small", 10, 1, "u8"),
            member("big", 0, 8, "u64"),
            member("mid", 8, 2, "u16"),
            member("name", 16, 0, "()"),
        ]),
        derived_default,
    }
}

#[test]
fn test_a_struct_mirror_has_each_field_at_its_offset_in_offset_order_with_padding() {
    let src = define("M1", &record(false)).unwrap();
    assert!(
        src.contains("#[repr(C, align(8))]\npub struct M1 {"),
        "{src}"
    );
    let body: Vec<&str> = src.lines().filter(|l| l.starts_with("    pub ")).collect();
    assert_eq!(
        body,
        [
            "    pub big: u64,",
            "    pub mid: u16,",
            "    pub small: u8,",
            "    pub _cherenkov_pad0: [u8; 5],",
            "    pub name: (),",
            "    pub _cherenkov_pad1: [u8; 8],",
        ],
        "{src}"
    );
    assert!(!src.contains("impl Default"), "{src}");
}

#[test]
fn test_a_derived_default_is_each_fields_with_zero_padding() {
    let src = define("M1", &record(true)).unwrap();
    assert!(src.contains("impl Default for M1"), "{src}");
    assert!(src.contains("big: Default::default(),"), "{src}");
    assert!(src.contains("_cherenkov_pad0: [0; 5],"), "{src}");
}

#[test]
fn test_array_fields_take_their_bytes_and_an_empty_one_none() {
    let def = TypeDef {
        name: "ops::Gauge".into(),
        size: 32,
        align: Some(8),
        shape: Shape::Struct(vec![
            member("bins", 0, 16, "[u64; 2]"),
            member("none", 16, 0, "[u8; 0]"),
            member("grid", 16, 12, "[[u16; 3]; 2]"),
        ]),
        derived_default: true,
    };
    let src = define("M5", &def).unwrap();
    let body: Vec<&str> = src.lines().filter(|l| l.starts_with("    pub ")).collect();
    assert_eq!(
        body,
        [
            "    pub bins: [u64; 2],",
            "    pub none: [u8; 0],",
            "    pub grid: [[u16; 3]; 2],",
            "    pub _cherenkov_pad0: [u8; 4],",
        ],
        "{src}"
    );
    assert!(src.contains("grid: Default::default(),"), "{src}");
}

#[test]
fn test_overlapping_fields_or_a_keyword_name_are_handled() {
    let mut def = record(false);
    def.shape = Shape::Struct(vec![member("a", 0, 8, "u64"), member("b", 4, 4, "u32")]);
    let e = define("M", &def).unwrap_err();
    assert!(e.contains("overlaps"), "{e}");
    def.shape = Shape::Struct(vec![member("type", 0, 8, "u64")]);
    def.size = 8;
    assert!(define("M", &def).unwrap().contains("pub r#type: u64,"));
}

#[test]
fn test_an_enum_mirror_keeps_its_tag_values_and_each_variants_field_offsets() {
    let def = TypeDef {
        name: "ops::Shape".into(),
        size: 32,
        align: Some(8),
        shape: Shape::Enum {
            tag: Some((0, 1)),
            variants: vec![
                VariantDef {
                    name: "Dot".into(),
                    tag: Some(3),
                    members: vec![],
                },
                VariantDef {
                    name: "Line".into(),
                    tag: Some(7),
                    members: vec![member("__0", 4, 4, "u32"), member("__1", 8, 24, "String")],
                },
            ],
        },
        derived_default: false,
    };
    let src = define("M2", &def).unwrap();
    assert!(src.contains("#[repr(u8)]\npub enum M2 {"), "{src}");
    assert!(src.contains("    Dot = 3,"), "{src}");
    assert!(
        src.contains("    Line { _cherenkov_pad0: [u8; 3], __0: u32, __1: String } = 7,"),
        "{src}"
    );
}

#[test]
fn test_a_niche_encoded_enum_is_declared_as_the_original_and_a_union_is_refused() {
    let def = TypeDef {
        name: "ops::Maybe".into(),
        size: 24,
        align: Some(8),
        shape: Shape::Enum {
            tag: Some((0, 8)),
            variants: vec![
                VariantDef {
                    name: "Some".into(),
                    tag: None,
                    members: vec![member("__0", 0, 24, "String")],
                },
                VariantDef {
                    name: "None".into(),
                    tag: Some(0x8000000000000000),
                    members: vec![],
                },
            ],
        },
        derived_default: false,
    };
    // Only rustc can place the tag in the niche: the same variants of the
    // same fields, in declaration order, for the layout guard to check.
    let src = define("M3", &def).unwrap();
    assert!(
        src.contains("pub enum M3 {\n    Some(String),\n    None,\n}"),
        "{src}"
    );
    assert!(!src.contains("#[repr"), "{src}");
    let union = TypeDef {
        shape: Shape::Union,
        ..def
    };
    assert!(define("M4", &union).unwrap_err().contains("union"));
}
