//! Mirror types: Rust definitions, for the support library, of types it
//! cannot name (a project's private structs and enums, a dependency's),
//! laid out from the program's DWARF. A mirror has the original's size,
//! alignment and field offsets, its std fields the real std types, so a
//! value made as the mirror is a value of the original, byte for byte.
//! The layout guard checks every mirror against the program before use.

/// A field name the mirror adds to place the next field at its offset; the
/// layout guard does not count it.
pub const PAD: &str = "_cherenkov_pad";

const ALLOW: &str = "#[allow(dead_code, non_camel_case_types, non_snake_case)]";

/// A type to mirror, as DWARF lays it out, with its fields' types already
/// written in Rust (std types by public path, other mirrors by name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    /// The original's name, for messages.
    pub name: String,
    pub size: u64,
    /// When DWARF records it.
    pub align: Option<u64>,
    pub shape: Shape,
    /// Give the mirror the original's `Default`, composed field by field:
    /// only for a struct whose source derives it and whose fields' types
    /// all have one.
    pub derived_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Struct(Vec<Member>),
    /// An enum: its tag's offset and size (`None`: a single variant, no
    /// tag), and each variant's tag value (`None`: the variant whose
    /// payload holds the niche the other tags are written into).
    Enum {
        tag: Option<(u64, u64)>,
        variants: Vec<VariantDef>,
    },
    Union,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// From the start of the type (of the enum, for a variant's field).
    pub offset: u64,
    pub size: u64,
    /// Its type, as Rust source.
    pub ty: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDef {
    pub name: String,
    pub tag: Option<u64>,
    pub members: Vec<Member>,
}

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "try", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield",
];

fn ident(name: &str) -> Result<String, String> {
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !valid || matches!(name, "self" | "Self" | "super" | "crate" | "_") {
        return Err(format!("{name:?} is not a name a mirror can use"));
    }
    Ok(if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    })
}

/// The fields of a struct (or a variant, after a tag of `start` bytes)
/// in offset order, with padding before each that does not follow the
/// last, and after the last to `end`: `name: type` items.
fn fields(owner: &str, members: &[Member], start: u64, end: u64) -> Result<Vec<String>, String> {
    let mut sorted: Vec<&Member> = members.iter().collect();
    sorted.sort_by_key(|m| (m.offset, m.size));
    let mut out = Vec::new();
    let mut at = start;
    let mut pads = 0;
    let mut pad = |out: &mut Vec<String>, n: u64| {
        out.push(format!("{PAD}{pads}: [u8; {n}]"));
        pads += 1;
    };
    for m in sorted {
        if m.offset < at {
            return Err(format!(
                "{owner}.{} at {} overlaps what comes before it (to {at})",
                m.name, m.offset
            ));
        }
        if m.offset > at {
            pad(&mut out, m.offset - at);
        }
        out.push(format!("{}: {}", ident(&m.name)?, m.ty));
        at = m.offset + m.size;
    }
    if end < at {
        return Err(format!("{owner}'s fields end at {at}, past its size {end}"));
    }
    if end > at {
        pad(&mut out, end - at);
    }
    Ok(out)
}

/// The Rust items that define `def` as the type `rust` (a plain name):
/// `#[repr(C)]` for a struct; `#[repr(uN)]` with the original's tag
/// values for an enum with its tag first; for any other enum, its
/// variants as declared. Unions are refused.
pub fn define(rust: &str, def: &TypeDef) -> Result<String, String> {
    let name = &def.name;
    let allow = ALLOW;
    match &def.shape {
        Shape::Struct(members) => {
            let fields = fields(name, members, 0, def.size)?;
            let repr = match def.align {
                Some(a) if a > 1 => format!("#[repr(C, align({a}))]"),
                _ => "#[repr(C)]".to_string(),
            };
            let mut out = format!("/// `{name}`\n{allow}\n{repr}\npub struct {rust} {{\n");
            for f in &fields {
                out.push_str(&format!("    pub {f},\n"));
            }
            out.push_str("}\n");
            if def.derived_default {
                out.push_str(&format!(
                    "\nimpl Default for {rust} {{\n    fn default() -> Self {{\n        Self {{\n"
                ));
                for f in &fields {
                    let (field, ty) = f.split_once(": ").unwrap_or((f, ""));
                    let value = if field.starts_with(PAD) {
                        ty.replacen("u8", "0", 1)
                    } else {
                        "Default::default()".to_string()
                    };
                    out.push_str(&format!("            {field}: {value},\n"));
                }
                out.push_str("        }\n    }\n}\n");
            }
            Ok(out)
        }
        Shape::Enum { tag, variants } => {
            let repr = match tag {
                Some((0, 1)) => Some("u8"),
                Some((0, 2)) => Some("u16"),
                Some((0, 4)) => Some("u32"),
                Some((0, 8)) => Some("u64"),
                _ => None,
            };
            match (repr, tag) {
                (Some(repr), Some((_, size))) if variants.iter().all(|v| v.tag.is_some()) => {
                    tagged(rust, def, repr, *size, variants)
                }
                _ => untagged(rust, def, variants),
            }
        }
        Shape::Union => Err(format!("{name} is a union (not mirrored yet)")),
    }
}

/// An enum with a plain tag first: `#[repr(uN)]`, the original's tag
/// values, each variant's fields at their offsets after the tag.
fn tagged(
    rust: &str,
    def: &TypeDef,
    repr: &str,
    size: u64,
    variants: &[VariantDef],
) -> Result<String, String> {
    let name = &def.name;
    let mut out = format!("/// `{name}`\n{ALLOW}\n#[repr({repr})]\npub enum {rust} {{\n");
    for v in variants {
        let owner = format!("{name}::{}", v.name);
        let tag = v.tag.unwrap_or_default();
        if v.members.is_empty() {
            out.push_str(&format!("    {} = {tag},\n", ident(&v.name)?));
            continue;
        }
        // Each variant is laid out as its own struct after the tag.
        let end = v
            .members
            .iter()
            .map(|m| m.offset + m.size)
            .max()
            .unwrap_or(size);
        let fields = fields(&owner, &v.members, size, end)?;
        out.push_str(&format!(
            "    {} {{ {} }} = {tag},\n",
            ident(&v.name)?,
            fields.join(", ")
        ));
    }
    out.push_str("}\n");
    Ok(out)
}

/// Any other enum (a tag kept in a field's niche, a tag not first, a
/// single variant): its variants and fields in declaration order, laid out
/// by rustc as the original was. Nothing makes it so but the same compiler
/// and the same fields; the layout guard checks that it is.
fn untagged(rust: &str, def: &TypeDef, variants: &[VariantDef]) -> Result<String, String> {
    let name = &def.name;
    let mut out = format!("/// `{name}`\n{ALLOW}\npub enum {rust} {{\n");
    for v in variants {
        let tuple = !v.members.is_empty()
            && v.members
                .iter()
                .enumerate()
                .all(|(k, m)| m.name == format!("__{k}"));
        let fields: Vec<String> = if tuple {
            v.members.iter().map(|m| m.ty.clone()).collect()
        } else {
            v.members
                .iter()
                .map(|m| Ok(format!("{}: {}", ident(&m.name)?, m.ty)))
                .collect::<Result<_, String>>()?
        };
        let variant = ident(&v.name)?;
        out.push_str(&match (fields.is_empty(), tuple) {
            (true, _) => format!("    {variant},\n"),
            (false, true) => format!("    {variant}({}),\n", fields.join(", ")),
            (false, false) => format!("    {variant} {{ {} }},\n", fields.join(", ")),
        });
    }
    out.push_str("}\n");
    Ok(out)
}
