//! Result types from the test binary's DWARF, and values of them built as
//! bytes, for returning a replacement value that needs no code to make:
//! enum tags from DWARF's discriminant values (niches included), payloads
//! at their offsets,
//! empty `String`s and `Vec`s as zero length and capacity with a dangling,
//! aligned pointer.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use cherenkov_core::execution::ExecError;
use cherenkov_core::types::{DefaultFacts, SourceDefault};
use gimli::{AttributeValue, EndianArcSlice, Reader, RunTimeEndian, UnitOffset};
use object::{Object, ObjectSection};

type R = EndianArcSlice<RunTimeEndian>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Int {
        size: u64,
        signed: bool,
    },
    Float {
        size: u64,
    },
    /// A pointer, with its pointee's alignment (for a dangling pointer).
    Pointer {
        size: u64,
        align: u64,
    },
    Struct {
        name: String,
        size: u64,
        /// Alignment, when DWARF records it.
        align: Option<u64>,
        fields: Vec<Field>,
        /// Generic type parameters (`T` of `Vec<T>`), which a type-erased
        /// buffer pointer no longer names.
        params: Vec<(String, Layout)>,
        /// The program's own `<T as Default>::default`.
        default: DefaultImpl,
        /// The crate that defines it (its outermost DWARF namespace).
        krate: String,
    },
    Enum {
        name: String,
        size: u64,
        /// Offset and size of the tag; `None` for a single-variant enum.
        tag: Option<(u64, u64)>,
        variants: Vec<Variant>,
        /// As for a struct: the program's own `Default`.
        default: DefaultImpl,
        /// As for a struct.
        krate: String,
    },
    /// `[elem; count]`: `count` elements, each of `elem`'s size.
    Array {
        count: u64,
        elem: Box<Layout>,
    },
    Other {
        name: String,
        size: u64,
    },
}

/// A type's `<T as Default>::default`, as the binary's DWARF has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultImpl {
    /// Its code, at this file address.
    At(u64),
    /// Declared, but its code is not in the binary (unused, so the linker
    /// dropped it), or not knowable: a generic type's impl exists only
    /// where it is used.
    Unlinked,
    /// No `default` for it in the binary's DWARF: either the type has no
    /// Default, or its (inline, derived) one was never used here.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub offset: u64,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub name: String,
    /// The tag value; `None` for the variant whose payload holds the niche.
    pub tag: Option<u64>,
    /// The payload's fields, at offsets within the enum.
    pub fields: Vec<Field>,
}

impl Layout {
    /// The crate that defines it, when DWARF says (structs and enums).
    pub fn krate(&self) -> Option<&str> {
        match self {
            Layout::Struct { krate, .. } | Layout::Enum { krate, .. } if !krate.is_empty() => {
                Some(krate)
            }
            _ => None,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            Layout::Int { size, .. }
            | Layout::Float { size }
            | Layout::Pointer { size, .. }
            | Layout::Struct { size, .. }
            | Layout::Enum { size, .. }
            | Layout::Other { size, .. } => *size,
            Layout::Array { count, elem } => count * elem.size(),
        }
    }

    /// Returned in integer registers by the x86-64 System V convention: at
    /// most 16 bytes and no floating point.
    pub fn in_registers(&self) -> bool {
        self.size() <= 16 && !self.has_float()
    }

    /// Holds a floating-point value anywhere inside.
    pub fn has_float(&self) -> bool {
        match self {
            Layout::Float { .. } => true,
            Layout::Struct { fields, .. } => fields.iter().any(|f| f.layout.has_float()),
            Layout::Enum { variants, .. } => variants
                .iter()
                .any(|v| v.fields.iter().any(|f| f.layout.has_float())),
            Layout::Array { count, elem } => *count > 0 && elem.has_float(),
            _ => false,
        }
    }
}

/// The binary's subprograms by entry address, with their result types
/// resolved on demand.
pub struct Layouts {
    dwarf: gimli::Dwarf<R>,
    units: Vec<gimli::Unit<R>>,
    by_entry: HashMap<u64, (usize, UnitOffset)>,
    /// Each type's `<T as Default>::default`, by the type's DIE: a
    /// subprogram named `default` and returning it.
    defaults: HashMap<(usize, UnitOffset), u64>,
    /// Types with a `default` declared, whether or not its code was kept.
    declared_defaults: std::collections::HashSet<(usize, UnitOffset)>,
    /// Each type's crate: its outermost namespace.
    crates: HashMap<(usize, UnitOffset), String>,
    /// Each type's namespaces, outermost first, joined by `::`.
    paths: HashMap<(usize, UnitOffset), String>,
    /// Each type's `default` by namespace path and type name, across units
    /// (two types of one name in one crate are two types):
    /// a generic's instance is described again in each unit that uses it,
    /// and its `default` returns the copy in its own. `None` when two
    /// differ.
    defaults_by_name: HashMap<(String, String), Option<u64>>,
    /// Each struct, enum and union defined (not only declared), by its full
    /// name: the first unit's.
    by_name: std::sync::OnceLock<HashMap<String, (usize, UnitOffset)>>,
}

/// A struct, enum or union as DWARF defines it, with its members' types by
/// their DWARF names: what a mirror of it is written from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The full name (`ops::Pair`, `ops::Pool<u8>`).
    pub name: String,
    /// Without its namespaces and generic arguments (`Pool`).
    pub short: String,
    pub krate: String,
    pub size: u64,
    pub align: Option<u64>,
    pub kind: DefinitionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionKind {
    Struct(Vec<Member>),
    /// The tag's offset and size, and each variant's name, tag value and
    /// fields (at offsets within the enum), as in [`Layout::Enum`].
    Enum {
        tag: Option<(u64, u64)>,
        variants: Vec<(String, Option<u64>, Vec<Member>)>,
    },
    Union,
}

/// A field: its name, offset, size, and its type's DWARF name (`None` for
/// a type DWARF gives no name, such as a function; an array is named
/// `[T; N]` from its element and count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub offset: u64,
    pub size: u64,
    pub ty: Option<String>,
}

fn err(binary: &Path, what: &str, e: impl std::fmt::Display) -> ExecError {
    ExecError(format!("{}: {what}: {e}", binary.display()))
}

/// What one pass over the DWARF's DIEs finds.
#[derive(Default)]
struct Scan {
    units: Vec<gimli::Unit<R>>,
    by_entry: HashMap<u64, (usize, UnitOffset)>,
    /// Subprograms with no code of their own: their declarations.
    declarations: Vec<(usize, UnitOffset)>,
    crates: HashMap<(usize, UnitOffset), String>,
    paths: HashMap<(usize, UnitOffset), String>,
}

/// The `default` functions found among the subprograms.
#[derive(Default)]
struct Defaults {
    by_type: HashMap<(usize, UnitOffset), u64>,
    declared: std::collections::HashSet<(usize, UnitOffset)>,
    by_name: HashMap<(String, String), Option<u64>>,
}

/// The DWARF of `binary`.
fn load_dwarf(binary: &Path) -> Result<gimli::Dwarf<R>, ExecError> {
    let data = std::fs::read(binary).map_err(|e| err(binary, "read", e))?;
    let file = object::File::parse(&*data).map_err(|e| err(binary, "parse", e))?;
    let endian = if file.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    let load = |id: gimli::SectionId| -> Result<R, gimli::Error> {
        let bytes: Arc<[u8]> = file
            .section_by_name(id.name())
            .and_then(|s| s.uncompressed_data().ok())
            .map(|d| Arc::from(&*d))
            .unwrap_or_else(|| Arc::from(&[][..]));
        Ok(EndianArcSlice::new(bytes, endian))
    };
    gimli::Dwarf::load(load).map_err(|e| err(binary, "dwarf", e))
}

/// Every unit of `dwarf`, with its subprograms, and its types' crates and
/// namespace paths.
fn scan(binary: &Path, dwarf: &gimli::Dwarf<R>) -> Result<Scan, ExecError> {
    let mut scan = Scan::default();
    let mut headers = dwarf.units();
    while let Some(header) = headers.next().map_err(|e| err(binary, "unit", e))? {
        let unit = dwarf.unit(header).map_err(|e| err(binary, "unit", e))?;
        scan_unit(binary, dwarf, &unit, scan.units.len(), &mut scan)?;
        scan.units.push(unit);
    }
    Ok(scan)
}

/// The DIEs of unit `k` into `scan`.
fn scan_unit(
    binary: &Path,
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    k: usize,
    scan: &mut Scan,
) -> Result<(), ExecError> {
    let mut entries = unit.entries();
    // The namespaces around the current DIE, by depth.
    let mut namespaces: Vec<(isize, String)> = Vec::new();
    while let Some(die) = entries.next_dfs().map_err(|e| err(binary, "die", e))? {
        while namespaces.last().is_some_and(|(d, _)| *d >= die.depth()) {
            namespaces.pop();
        }
        scan_die(dwarf, unit, k, die, &mut namespaces, scan);
    }
    Ok(())
}

/// One DIE: a namespace opens, a type records its crate and path, a
/// subprogram its entry or declaration.
fn scan_die(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R>,
    k: usize,
    die: &gimli::DebuggingInformationEntry<R>,
    namespaces: &mut Vec<(isize, String)>,
    scan: &mut Scan,
) {
    let tag = die.tag();
    if tag == gimli::DW_TAG_namespace {
        let name = die
            .attr_value(gimli::DW_AT_name)
            .and_then(|v| dwarf.attr_string(unit, v).ok())
            .and_then(|n| n.to_string_lossy().ok().map(|n| n.into_owned()))
            .unwrap_or_default();
        namespaces.push((die.depth(), name));
    } else if matches!(
        tag,
        gimli::DW_TAG_structure_type | gimli::DW_TAG_union_type | gimli::DW_TAG_enumeration_type
    ) {
        if let Some((_, krate)) = namespaces.first() {
            scan.crates.insert((k, die.offset()), krate.clone());
            let path: Vec<&str> = namespaces.iter().map(|(_, n)| n.as_str()).collect();
            scan.paths.insert((k, die.offset()), path.join("::"));
        }
    } else if tag == gimli::DW_TAG_subprogram {
        let Some(low) = die.attr_value(gimli::DW_AT_low_pc) else {
            scan.declarations.push((k, die.offset()));
            return;
        };
        if let Ok(Some(pc)) = dwarf.attr_address(unit, low) {
            scan.by_entry.insert(pc, (k, die.offset()));
        }
    }
}

/// Note in `by_name` that the `default` at `pc` is the one for `key`; two
/// different ones make it `None`.
fn merge_default(
    by_name: &mut HashMap<(String, String), Option<u64>>,
    key: (String, String),
    pc: u64,
) {
    by_name
        .entry(key)
        .and_modify(|v| {
            if *v != Some(pc) {
                *v = None;
            }
        })
        .or_insert(Some(pc));
}

impl Layouts {
    pub fn read(binary: &Path) -> Result<Layouts, ExecError> {
        let dwarf = load_dwarf(binary)?;
        let Scan {
            units,
            by_entry,
            declarations,
            crates,
            paths,
        } = scan(binary, &dwarf)?;
        let mut layouts = Layouts {
            dwarf,
            units,
            by_entry,
            defaults: HashMap::new(),
            declared_defaults: std::collections::HashSet::new(),
            crates,
            paths,
            defaults_by_name: HashMap::new(),
            by_name: std::sync::OnceLock::new(),
        };
        let found = layouts.find_defaults(declarations);
        layouts.defaults = found.by_type;
        layouts.declared_defaults = found.declared;
        layouts.defaults_by_name = found.by_name;
        Ok(layouts)
    }

    /// The `default` functions among the subprograms with code and the
    /// `declarations` without.
    fn find_defaults(&self, declarations: Vec<(usize, UnitOffset)>) -> Defaults {
        let subprograms: Vec<(Option<u64>, (usize, UnitOffset))> = self
            .by_entry
            .iter()
            .map(|(&pc, &e)| (Some(pc), e))
            .chain(declarations.into_iter().map(|e| (None, e)))
            .collect();
        let mut found = Defaults::default();
        for (pc, (k, off)) in subprograms {
            self.note_default(&mut found, pc, k, off);
        }
        found
    }

    /// If the subprogram at `off` is a type's `default`, record it in
    /// `found` (with its code, when it has any).
    fn note_default(&self, found: &mut Defaults, pc: Option<u64>, k: usize, off: UnitOffset) {
        let unit = &self.units[k];
        let Some(die) = self.declaration(unit, off) else {
            return;
        };
        // A generic instance is named with its parameters (`default<u8>`).
        if self.name(k, &die).split('<').next() != Some("default") {
            return;
        }
        let Some(ty) = self.type_ref(unit, &die) else {
            return;
        };
        found.declared.insert((k, ty));
        // A function the linker dropped keeps its DIE at address 0.
        let Some(pc) = pc.filter(|&pc| pc != 0) else {
            return;
        };
        found.by_type.insert((k, ty), pc);
        if let (Some(path), Ok(ty_die)) = (self.paths.get(&(k, ty)), unit.entry(ty)) {
            let key = (path.clone(), self.name(k, &ty_die));
            merge_default(&mut found.by_name, key, pc);
        }
    }

    /// The result type of the function whose code starts at `entry` (a file
    /// address); `None` for a function with no result or no debug entry.
    pub fn result_of(&self, entry: u64) -> Option<Layout> {
        let &(k, off) = self.by_entry.get(&entry)?;
        let unit = &self.units[k];
        let die = self.declaration(unit, off)?;
        let ty = self.type_ref(unit, &die)?;
        Some(self.layout(k, ty, 0))
    }

    /// The result type of the function at `entry` as DWARF names it: each
    /// path its defining one (`std::collections::hash::set::HashSet<u32,
    /// std::hash::random::RandomState, alloc::alloc::Global>`). `None` for
    /// a type DWARF gives no name to (a function); an array is `[T; N]`.
    pub fn result_type_name(&self, entry: u64) -> Option<String> {
        let &(k, off) = self.by_entry.get(&entry)?;
        let unit = &self.units[k];
        let die = self.declaration(unit, off)?;
        let ty = self.type_ref(unit, &die)?;
        self.type_name(k, ty, 0)
    }

    /// The layout of what the first parameter of the function at `entry`
    /// points to (`T` of `out: *mut T`).
    pub fn first_param_pointee(&self, entry: u64) -> Option<Layout> {
        let &(k, off) = self.by_entry.get(&entry)?;
        let unit = &self.units[k];
        let mut tree = unit.entries_tree(Some(off)).ok()?;
        let root = tree.root().ok()?;
        let mut children = root.children();
        while let Ok(Some(child)) = children.next() {
            let die = child.entry();
            if die.tag() != gimli::DW_TAG_formal_parameter {
                continue;
            }
            let pointer = unit.entry(self.type_ref(unit, die)?).ok()?;
            let pointee = self.type_ref(unit, &pointer)?;
            return Some(self.layout(k, pointee, 0));
        }
        None
    }

    fn type_name(&self, k: usize, off: UnitOffset, depth: u32) -> Option<String> {
        let unit = &self.units[k];
        let die = unit.entry(off).ok()?;
        let name = self.name(k, &die);
        match die.tag() {
            gimli::DW_TAG_typedef | gimli::DW_TAG_const_type | gimli::DW_TAG_volatile_type
                if depth < 16 =>
            {
                self.type_name(k, self.type_ref(unit, &die)?, depth + 1)
            }
            gimli::DW_TAG_base_type | gimli::DW_TAG_pointer_type | gimli::DW_TAG_reference_type
                if !name.is_empty() =>
            {
                Some(name)
            }
            gimli::DW_TAG_structure_type
            | gimli::DW_TAG_union_type
            | gimli::DW_TAG_enumeration_type
                if !name.is_empty() =>
            {
                Some(match self.paths.get(&(k, off)) {
                    Some(path) if !path.is_empty() => format!("{path}::{name}"),
                    _ => name,
                })
            }
            gimli::DW_TAG_array_type if depth < 16 => {
                let elem = self.type_name(k, self.type_ref(unit, &die)?, depth + 1)?;
                let counts = self.array_counts(k, off)?;
                Some(
                    counts
                        .iter()
                        .rev()
                        .fold(elem, |inner, n| format!("[{inner}; {n}]")),
                )
            }
            _ => None,
        }
    }

    /// An array's element counts, outermost first (one per dimension);
    /// `None` when a dimension's count is not recorded.
    fn array_counts(&self, k: usize, off: UnitOffset) -> Option<Vec<u64>> {
        let unit = &self.units[k];
        let mut tree = unit.entries_tree(Some(off)).ok()?;
        let root = tree.root().ok()?;
        let mut children = root.children();
        let mut counts = Vec::new();
        while let Ok(Some(child)) = children.next() {
            let die = child.entry();
            if die.tag() != gimli::DW_TAG_subrange_type {
                continue;
            }
            let bound = |at| {
                die.attr_value(at).and_then(|v| match v {
                    AttributeValue::Sdata(s) => Some(s),
                    other => other.udata_value().and_then(|u| i64::try_from(u).ok()),
                })
            };
            let count = match bound(gimli::DW_AT_count) {
                Some(n) => n,
                None => {
                    bound(gimli::DW_AT_upper_bound)? - bound(gimli::DW_AT_lower_bound).unwrap_or(0)
                        + 1
                }
            };
            counts.push(u64::try_from(count).ok()?);
        }
        (!counts.is_empty()).then_some(counts)
    }

    /// The definition of the struct, enum or union DWARF names `name` (a
    /// full name, as [`Layouts::result_type_name`] writes it).
    pub fn definition(&self, name: &str) -> Option<Definition> {
        let index = self.by_name.get_or_init(|| {
            let mut index = HashMap::new();
            for (&(k, off), path) in &self.paths {
                let Ok(die) = self.units[k].entry(off) else {
                    continue;
                };
                if die.attr_value(gimli::DW_AT_declaration).is_some()
                    || die.attr_value(gimli::DW_AT_byte_size).is_none()
                {
                    continue;
                }
                let short = self.name(k, &die);
                let full = if path.is_empty() {
                    short
                } else {
                    format!("{path}::{short}")
                };
                index
                    .entry(full)
                    .and_modify(|e: &mut (usize, UnitOffset)| *e = (*e).min((k, off)))
                    .or_insert((k, off));
            }
            index
        });
        let &(k, off) = index.get(name)?;
        let unit = &self.units[k];
        let die = unit.entry(off).ok()?;
        let short = self.name(k, &die);
        let base = |n: &str| n.split('<').next().unwrap_or(n).to_string();
        let mut def = Definition {
            name: name.to_string(),
            short: base(&short),
            krate: self.crates.get(&(k, off)).cloned().unwrap_or_default(),
            size: Self::udata(&die, gimli::DW_AT_byte_size).unwrap_or(0),
            align: Self::udata(&die, gimli::DW_AT_alignment),
            kind: DefinitionKind::Union,
        };
        match die.tag() {
            gimli::DW_TAG_union_type => {}
            gimli::DW_TAG_enumeration_type => {
                let Layout::Enum { tag, variants, .. } = self.enumeration(k, off, short, def.size)
                else {
                    return None;
                };
                def.kind = DefinitionKind::Enum {
                    tag,
                    variants: variants
                        .into_iter()
                        .map(|v| (v.name, v.tag, Vec::new()))
                        .collect(),
                };
            }
            _ => {
                let mut tree = unit.entries_tree(Some(off)).ok()?;
                let root = tree.root().ok()?;
                let mut members = Vec::new();
                let mut variant_part = None;
                let mut children = root.children();
                while let Ok(Some(child)) = children.next() {
                    match child.entry().tag() {
                        gimli::DW_TAG_member => {
                            members.extend(self.member(k, child.entry(), 0));
                        }
                        gimli::DW_TAG_variant_part => {
                            variant_part = Some(self.named_variants(k, child));
                        }
                        _ => {}
                    }
                }
                def.kind = match variant_part {
                    Some((tag, variants)) => DefinitionKind::Enum { tag, variants },
                    None => DefinitionKind::Struct(members),
                };
            }
        }
        Some(def)
    }

    fn member(
        &self,
        k: usize,
        die: &gimli::DebuggingInformationEntry<R>,
        base: u64,
    ) -> Option<Member> {
        let unit = &self.units[k];
        let ty = self.type_ref(unit, die)?;
        Some(Member {
            name: self.name(k, die),
            offset: base + Self::udata(die, gimli::DW_AT_data_member_location).unwrap_or(0),
            size: self.layout(k, ty, 1).size(),
            ty: self.type_name(k, ty, 0),
        })
    }

    /// A variant part's tag and variants, each with its fields' types.
    #[allow(clippy::type_complexity)]
    fn named_variants(
        &self,
        k: usize,
        part: gimli::EntriesTreeNode<'_, '_, R>,
    ) -> (Option<(u64, u64)>, Vec<(String, Option<u64>, Vec<Member>)>) {
        let unit = &self.units[k];
        let discr = match part.entry().attr_value(gimli::DW_AT_discr) {
            Some(AttributeValue::UnitRef(o)) => Some(o),
            _ => None,
        };
        let tag = discr.and_then(|o| unit.entry(o).ok()).and_then(|m| {
            let offset = Self::udata(&m, gimli::DW_AT_data_member_location).unwrap_or(0);
            let ty = self.type_ref(unit, &m)?;
            Some((offset, self.layout(k, ty, 1).size()))
        });
        let mut variants = Vec::new();
        let mut children = part.children();
        while let Ok(Some(variant)) = children.next() {
            if variant.entry().tag() != gimli::DW_TAG_variant {
                continue;
            }
            let value = variant
                .entry()
                .attr_value(gimli::DW_AT_discr_value)
                .and_then(|v| match v {
                    AttributeValue::Sdata(s) => Some(s as u64),
                    other => other.udata_value(),
                });
            let mut members = variant.children();
            while let Ok(Some(member)) = members.next() {
                let die = member.entry().clone();
                if die.tag() != gimli::DW_TAG_member {
                    continue;
                }
                let base = Self::udata(&die, gimli::DW_AT_data_member_location).unwrap_or(0);
                let fields = self.variant_fields(k, &die, base);
                variants.push((self.name(k, &die), value, fields));
            }
        }
        (tag, variants)
    }

    /// The fields of the type a variant's member `die` refers to, at
    /// `base` within the enum.
    fn variant_fields(
        &self,
        k: usize,
        die: &gimli::DebuggingInformationEntry<R>,
        base: u64,
    ) -> Vec<Member> {
        let unit = &self.units[k];
        let Some(mut tree) = self
            .type_ref(unit, die)
            .and_then(|t| unit.entries_tree(Some(t)).ok())
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Ok(root) = tree.root() {
            let mut c = root.children();
            while let Ok(Some(f)) = c.next() {
                if f.entry().tag() == gimli::DW_TAG_member {
                    out.extend(self.member(k, f.entry(), base));
                }
            }
        }
        out
    }

    /// Whether the function whose code starts at `entry` (a file address)
    /// is a future's poll: rustc's `{async_fn#0}` (an `async fn`'s own
    /// future) or `{async_block#0}` (an `async` block's, which is where a
    /// macro such as `#[async_trait]` puts an `async fn`'s body). It returns
    /// `Poll<T>` for the future's output `T`.
    pub fn is_future_poll(&self, entry: u64) -> bool {
        let Some(&(k, off)) = self.by_entry.get(&entry) else {
            return false;
        };
        self.declaration(&self.units[k], off).is_some_and(|die| {
            let name = self.name(k, &die);
            name.starts_with("{async_fn#") || name.starts_with("{async_block#")
        })
    }

    /// The DIE that describes a subprogram: a method's code names its
    /// declaration in the `impl`, which holds its name and result type (as
    /// an inlined copy names its origin).
    fn declaration(
        &self,
        unit: &gimli::Unit<R>,
        off: UnitOffset,
    ) -> Option<gimli::DebuggingInformationEntry<R>> {
        let mut die = unit.entry(off).ok()?;
        for _ in 0..4 {
            let declared = [gimli::DW_AT_specification, gimli::DW_AT_abstract_origin]
                .into_iter()
                .find_map(|at| match die.attr_value(at)? {
                    AttributeValue::UnitRef(o) => Some(o),
                    AttributeValue::DebugInfoRef(o) => o.to_unit_offset(&unit.header),
                    _ => None,
                });
            match declared {
                Some(d) if die.attr_value(gimli::DW_AT_type).is_none() => {
                    die = unit.entry(d).ok()?;
                }
                _ => return Some(die),
            }
        }
        Some(die)
    }

    fn type_ref(
        &self,
        unit: &gimli::Unit<R>,
        die: &gimli::DebuggingInformationEntry<R>,
    ) -> Option<UnitOffset> {
        match die.attr_value(gimli::DW_AT_type)? {
            AttributeValue::UnitRef(o) => Some(o),
            AttributeValue::DebugInfoRef(o) => o.to_unit_offset(&unit.header),
            _ => None,
        }
    }

    fn name(&self, k: usize, die: &gimli::DebuggingInformationEntry<R>) -> String {
        die.attr_value(gimli::DW_AT_name)
            .and_then(|v| self.dwarf.attr_string(&self.units[k], v).ok())
            .map(|s| {
                s.to_string_lossy()
                    .map(|c| c.into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    fn udata(die: &gimli::DebuggingInformationEntry<R>, at: gimli::DwAt) -> Option<u64> {
        die.attr_value(at)?.udata_value()
    }

    fn layout(&self, k: usize, off: UnitOffset, depth: u32) -> Layout {
        let unit = &self.units[k];
        let Ok(die) = unit.entry(off) else {
            return Layout::Other {
                name: "?".into(),
                size: 0,
            };
        };
        let name = self.name(k, &die);
        let size = Self::udata(&die, gimli::DW_AT_byte_size).unwrap_or(0);
        if depth > 16 {
            return Layout::Other { name, size };
        }
        match die.tag() {
            gimli::DW_TAG_base_type => {
                let enc = die.attr_value(gimli::DW_AT_encoding);
                match enc {
                    Some(AttributeValue::Encoding(gimli::DW_ATE_float)) => Layout::Float { size },
                    Some(AttributeValue::Encoding(gimli::DW_ATE_signed)) => {
                        Layout::Int { size, signed: true }
                    }
                    _ => Layout::Int {
                        size,
                        signed: false,
                    },
                }
            }
            gimli::DW_TAG_pointer_type | gimli::DW_TAG_reference_type => {
                let align = self
                    .type_ref(unit, &die)
                    .and_then(|t| unit.entry(t).ok())
                    .map(|p| {
                        Self::udata(&p, gimli::DW_AT_alignment)
                            .or_else(|| {
                                Self::udata(&p, gimli::DW_AT_byte_size).map(|s| s.clamp(1, 8))
                            })
                            .unwrap_or(1)
                    })
                    .unwrap_or(1);
                Layout::Pointer {
                    size: if size == 0 { 8 } else { size },
                    align: align.max(1),
                }
            }
            gimli::DW_TAG_typedef | gimli::DW_TAG_const_type | gimli::DW_TAG_volatile_type => {
                match self.type_ref(unit, &die) {
                    Some(t) => self.layout(k, t, depth + 1),
                    None => Layout::Other { name, size },
                }
            }
            gimli::DW_TAG_structure_type | gimli::DW_TAG_union_type => {
                let align = Self::udata(&die, gimli::DW_AT_alignment);
                self.aggregate(k, off, name, size, align, depth)
            }
            // A field-less enum: its tag is the whole value.
            gimli::DW_TAG_enumeration_type => self.enumeration(k, off, name, size),
            gimli::DW_TAG_array_type => {
                match (self.type_ref(unit, &die), self.array_counts(k, off)) {
                    (Some(t), Some(counts)) => {
                        let elem = self.layout(k, t, depth + 1);
                        counts
                            .iter()
                            .rev()
                            .fold(elem, |inner, &count| Layout::Array {
                                count,
                                elem: Box::new(inner),
                            })
                    }
                    _ => Layout::Other { name, size },
                }
            }
            _ => Layout::Other { name, size },
        }
    }

    /// A field-less enum (`DW_TAG_enumeration_type`): each enumerator's
    /// value is its tag, over the whole value.
    fn enumeration(&self, k: usize, off: UnitOffset, name: String, size: u64) -> Layout {
        let unit = &self.units[k];
        let mut variants = Vec::new();
        if let Ok(mut tree) = unit.entries_tree(Some(off))
            && let Ok(root) = tree.root()
        {
            let mut children = root.children();
            while let Ok(Some(child)) = children.next() {
                let die = child.entry();
                if die.tag() != gimli::DW_TAG_enumerator {
                    continue;
                }
                let tag = die.attr_value(gimli::DW_AT_const_value).and_then(|v| {
                    v.udata_value()
                        .or_else(|| v.sdata_value().map(|x| x as u64))
                });
                variants.push(Variant {
                    name: self.name(k, die),
                    tag,
                    fields: Vec::new(),
                });
            }
        }
        let default = match self.defaults.get(&(k, off)) {
            Some(&pc) => DefaultImpl::At(pc),
            None if self.declared_defaults.contains(&(k, off)) => DefaultImpl::Unlinked,
            None => DefaultImpl::Absent,
        };
        Layout::Enum {
            name,
            size,
            tag: Some((0, size)),
            variants,
            default,
            krate: self.crates.get(&(k, off)).cloned().unwrap_or_default(),
        }
    }

    /// A struct, or a Rust enum (a struct holding a variant part).
    fn aggregate(
        &self,
        k: usize,
        off: UnitOffset,
        name: String,
        size: u64,
        align: Option<u64>,
        depth: u32,
    ) -> Layout {
        let unit = &self.units[k];
        let mut fields = Vec::new();
        let mut params = Vec::new();
        let mut enum_parts = None;
        let Ok(mut tree) = unit.entries_tree(Some(off)) else {
            return Layout::Other { name, size };
        };
        let Ok(root) = tree.root() else {
            return Layout::Other { name, size };
        };
        let mut children = root.children();
        while let Ok(Some(child)) = children.next() {
            let die = child.entry().clone();
            match die.tag() {
                gimli::DW_TAG_member => {
                    if let Some(f) = self.field(k, &die, 0, depth) {
                        fields.push(f);
                    }
                }
                gimli::DW_TAG_variant_part => {
                    enum_parts = Some(self.variant_part(k, child, depth));
                }
                gimli::DW_TAG_template_type_parameter => {
                    if let Some(t) = self.type_ref(unit, &die) {
                        params.push((self.name(k, &die), self.layout(k, t, depth + 1)));
                    }
                }
                _ => {}
            }
        }
        let by_name = || {
            let path = self.paths.get(&(k, off))?;
            *self.defaults_by_name.get(&(path.clone(), name.clone()))?
        };
        let default = match self.defaults.get(&(k, off)).copied().or_else(by_name) {
            Some(pc) => DefaultImpl::At(pc),
            None if self.declared_defaults.contains(&(k, off)) || !params.is_empty() => {
                DefaultImpl::Unlinked
            }
            None => DefaultImpl::Absent,
        };
        let krate = self.crates.get(&(k, off)).cloned().unwrap_or_default();
        match enum_parts {
            Some((tag, variants)) => Layout::Enum {
                name,
                size,
                tag,
                variants,
                default,
                krate: krate.clone(),
            },
            None => Layout::Struct {
                name,
                size,
                align,
                fields,
                params,
                default,
                krate,
            },
        }
    }

    fn field(
        &self,
        k: usize,
        die: &gimli::DebuggingInformationEntry<R>,
        base: u64,
        depth: u32,
    ) -> Option<Field> {
        let unit = &self.units[k];
        let offset = Self::udata(die, gimli::DW_AT_data_member_location).unwrap_or(0);
        let ty = self.type_ref(unit, die)?;
        Some(Field {
            name: self.name(k, die),
            offset: base + offset,
            layout: self.layout(k, ty, depth + 1),
        })
    }

    fn variant_part(
        &self,
        k: usize,
        part: gimli::EntriesTreeNode<'_, '_, R>,
        depth: u32,
    ) -> (Option<(u64, u64)>, Vec<Variant>) {
        let unit = &self.units[k];
        let discr = match part.entry().attr_value(gimli::DW_AT_discr) {
            Some(AttributeValue::UnitRef(o)) => Some(o),
            _ => None,
        };
        let tag = discr.and_then(|o| unit.entry(o).ok()).and_then(|m| {
            let offset = Self::udata(&m, gimli::DW_AT_data_member_location).unwrap_or(0);
            let ty = self.type_ref(unit, &m)?;
            Some((offset, self.layout(k, ty, depth + 1).size()))
        });
        let mut variants = Vec::new();
        let mut children = part.children();
        while let Ok(Some(variant)) = children.next() {
            if variant.entry().tag() != gimli::DW_TAG_variant {
                continue;
            }
            let value = variant
                .entry()
                .attr_value(gimli::DW_AT_discr_value)
                .and_then(|v| match v {
                    AttributeValue::Sdata(s) => Some(s as u64),
                    other => other.udata_value(),
                });
            let mut members = variant.children();
            while let Ok(Some(member)) = members.next() {
                let die = member.entry().clone();
                if die.tag() != gimli::DW_TAG_member {
                    continue;
                }
                let base = Self::udata(&die, gimli::DW_AT_data_member_location).unwrap_or(0);
                let name = self.name(k, &die);
                let fields = match self
                    .type_ref(unit, &die)
                    .map(|t| self.layout(k, t, depth + 1))
                {
                    Some(Layout::Struct { fields, .. }) => fields
                        .into_iter()
                        .map(|f| Field {
                            offset: base + f.offset,
                            ..f
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                variants.push(Variant {
                    name,
                    tag: value,
                    fields,
                });
            }
        }
        (tag, variants)
    }
}

/// A value's bytes, and the heap blocks it points to: each at the offset
/// of its pointer slot in `bytes`, which the debugger fills in once it has
/// allocated the block in the program.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Built {
    pub bytes: Vec<u8>,
    pub heap: Vec<(usize, Built)>,
    /// Bytes the program's own code makes: at each offset, the value its
    /// `Default` function (by address) returns.
    pub calls: Vec<(usize, u64)>,
    /// Bytes a compiled constructor makes, called with a pointer to them.
    pub constructs: Vec<Construct>,
}

/// A part of a value made by `symbol` in the loaded library at `library`,
/// an `extern "C" fn(out: *mut T)` that writes `size` bytes at `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Construct {
    pub at: usize,
    pub size: u64,
    pub library: std::path::PathBuf,
    pub symbol: String,
}

impl Built {
    /// The same value with its calls' addresses moved by the load offset
    /// (file addresses to run-time ones).
    pub fn rebased(mut self, bias: u64) -> Built {
        for (_, addr) in &mut self.calls {
            *addr = addr.wrapping_add(bias);
        }
        self.heap = self
            .heap
            .into_iter()
            .map(|(at, b)| (at, b.rebased(bias)))
            .collect();
        self
    }
}

/// Why a value cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The value is no value of the type at all (`Default::default()` of a
    /// type without `Default`): the mutated source does not compile.
    Unviable(String),
    /// The value exists, but this builder cannot make it.
    Unsupported(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Unviable(why) | Refusal::Unsupported(why) => f.write_str(why),
        }
    }
}

/// Marks a refusal, inside the builder, as unviable rather than unsupported.
const UNVIABLE: &str = "unviable: ";

fn refusal(why: String) -> Refusal {
    match why.find(UNVIABLE) {
        Some(at) => Refusal::Unviable(why[at + UNVIABLE.len()..].to_string()),
        None => Refusal::Unsupported(why),
    }
}

/// The bytes of `value` (Rust source text, as cargo-mutants' value table
/// writes it) as a value of `layout`, needing no heap memory.
pub fn build(layout: &Layout, value: &str) -> Result<Vec<u8>, Refusal> {
    let built = build_value(layout, value)?;
    if !built.calls.is_empty() {
        Err(Refusal::Unsupported(format!(
            "{value} for {} needs the program's code",
            layout_name(layout)
        )))
    } else if built.heap.is_empty() {
        Ok(built.bytes)
    } else {
        Err(Refusal::Unsupported(format!("{value} needs heap memory")))
    }
}

fn layout_name(layout: &Layout) -> &str {
    match layout {
        Layout::Struct { name, .. } | Layout::Enum { name, .. } | Layout::Other { name, .. } => {
            name
        }
        Layout::Array { .. } => "an array",
        _ => "a scalar",
    }
}

/// [`build`], with any heap blocks the value needs.
pub fn build_value(layout: &Layout, value: &str) -> Result<Built, Refusal> {
    build_value_with(layout, value, &DefaultFacts::default())
}

/// [`build_value`], knowing from the source which types derive `Default`
/// (built field by field when the binary has no code for it) and which
/// have none (unviable).
pub fn build_value_with(
    layout: &Layout,
    value: &str,
    facts: &DefaultFacts,
) -> Result<Built, Refusal> {
    let mut out = Built {
        bytes: vec![0u8; layout.size() as usize],
        heap: Vec::new(),
        calls: Vec::new(),
        constructs: Vec::new(),
    };
    write(layout, value.trim(), &mut out, 0, facts).map_err(refusal)?;
    Ok(out)
}

fn put(out: &mut [u8], at: u64, size: u64, v: u64) {
    let bytes = v.to_le_bytes();
    let n = size.min(8) as usize;
    out[at as usize..at as usize + n].copy_from_slice(&bytes[..n]);
}

/// `Name(inner)` -> ("Name", Some("inner")); `Name` -> ("Name", None).
fn call(text: &str) -> (&str, Option<&str>) {
    match text.find('(') {
        Some(i) if i > 0 && text.ends_with(')') => (&text[..i], Some(&text[i + 1..text.len() - 1])),
        _ => (text, None),
    }
}

/// Split `a, b(c, d), "e,f"` at its top-level commas.
fn items(text: &str) -> Vec<&str> {
    let (mut depth, mut quoted, mut start, mut out) = (0i32, false, 0, Vec::new());
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// The text of a string value: `"x".into()`, `"x".to_string()`,
/// `String::from("x")`, or a plain literal `"x"`.
fn string_text(value: &str) -> Option<String> {
    let v = value
        .strip_suffix(".into()")
        .or_else(|| value.strip_suffix(".to_string()"))
        .or_else(|| {
            value
                .strip_prefix("String::from(")
                .and_then(|v| v.strip_suffix(')'))
        })
        .unwrap_or(value);
    let inner = v.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// The element type of a vector-like struct (`Vec<T>`, `String`'s `Vec<u8>`).
fn element(layout: &Layout) -> Option<&Layout> {
    let Layout::Struct { params, fields, .. } = layout else {
        return None;
    };
    params
        .iter()
        .find(|(n, _)| n == "T")
        .map(|(_, l)| l)
        .or_else(|| {
            fields
                .iter()
                .find(|f| f.name == "vec")
                .and_then(|f| element(&f.layout))
        })
}

fn align_of(layout: &Layout) -> u64 {
    match layout {
        Layout::Struct { align: Some(a), .. } => *a,
        Layout::Struct { fields, .. } => fields
            .iter()
            .map(|f| align_of(&f.layout))
            .max()
            .unwrap_or(1),
        Layout::Enum { size, .. } | Layout::Other { size, .. } => {
            (*size).clamp(1, 8).next_power_of_two().min(8)
        }
        Layout::Array { elem, .. } => align_of(elem),
        Layout::Int { size, .. } | Layout::Float { size } | Layout::Pointer { size, .. } => {
            (*size).max(1)
        }
    }
}

/// Fill a vector-like struct (`ptr`, `cap`, `len` somewhere inside it) for
/// `n` elements whose bytes are `contents` (on the heap when `n > 0`).
fn fill_vec(
    layout: &Layout,
    align: u64,
    n: u64,
    contents: Option<&Built>,
    out: &mut Built,
    at: u64,
) {
    let Layout::Struct { fields, .. } = layout else {
        return;
    };
    for f in fields {
        let here = at + f.offset;
        match (&f.layout, f.name.as_str()) {
            (Layout::Pointer { size, .. }, _) => match contents {
                Some(block) if n > 0 => {
                    out.heap.push((here as usize, block.clone()));
                    put(&mut out.bytes, here, *size, 0);
                }
                // Empty: dangling at the element's alignment (`NonNull::dangling`).
                _ => put(&mut out.bytes, here, *size, align),
            },
            (Layout::Int { size, .. }, "cap" | "len" | "__0") => {
                put(&mut out.bytes, here, *size, n)
            }
            (Layout::Struct { .. }, _) => fill_vec(&f.layout, align, n, contents, out, here),
            _ => {}
        }
    }
}

fn write(
    layout: &Layout,
    value: &str,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Result<(), String> {
    if let Some(done) = write_default(layout, value, out, at, facts) {
        return done;
    }
    match layout {
        Layout::Int { size, .. } => write_int(value, *size, out, at),
        Layout::Enum {
            name,
            tag,
            variants,
            ..
        } => write_enum(name, *tag, variants, value, out, at, facts),
        Layout::Struct { name, fields, .. } => {
            write_struct(layout, name, fields, value, out, at, facts)
        }
        other => Err(format!("{value} for {other:?} is not built yet")),
    }
}

/// `()` (no bytes), a default number (zero, the bytes start zeroed) and a
/// `Default::default()` of any type; `None` for any other value.
fn write_default(
    layout: &Layout,
    value: &str,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Option<Result<(), String>> {
    match (value, layout) {
        ("()", l) if l.size() == 0 => Some(Ok(())),
        ("Default::default()", Layout::Int { .. } | Layout::Float { .. }) => Some(Ok(())),
        // An empty `Vec` or `String`: only those, by name. Any struct with a
        // parameter `T` looks vector-like to `element`, and its own Default
        // (below) is what it means.
        ("Default::default()", Layout::Struct { name, .. })
            if (name.starts_with("Vec<") || name == "String") && element(layout).is_some() =>
        {
            Some(write(layout, "vec![]", out, at, facts))
        }
        (
            "Default::default()",
            Layout::Struct { name, default, .. } | Layout::Enum { name, default, .. },
        ) => Some(write_type_default(layout, name, default, out, at, facts)),
        _ => None,
    }
}

/// A type's `Default::default()`: a call of its code, else what the
/// source says of it.
fn write_type_default(
    layout: &Layout,
    name: &str,
    default: &DefaultImpl,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Result<(), String> {
    // The source's facts are about the project's types: a type
    // declared elsewhere (a dependency) may share its short name.
    let said = match layout.krate() {
        Some(krate) if facts.owns(krate) => facts.of(name),
        _ => SourceDefault::Unknown,
    };
    match (default, said, layout) {
        (DefaultImpl::At(addr), _, _) => {
            out.calls.push((at as usize, *addr));
            Ok(())
        }
        // A derived Default is each field's: built field by field
        // when its code is not in the binary.
        (_, SourceDefault::Derived, Layout::Struct { fields, .. }) => {
            for f in fields {
                write(&f.layout, "Default::default()", out, at + f.offset, facts)?;
            }
            Ok(())
        }
        (_, SourceDefault::Absent, _) => {
            Err(format!("{UNVIABLE}{name} has no Default in the source"))
        }
        // No code for it in the binary, which does not mean the type
        // has no Default: a derived one is `#[inline]`, compiled only
        // where something uses it, and leaves no DWARF otherwise.
        (DefaultImpl::Absent, ..) => Err(format!(
            "{name}'s Default, if it has one, is not in the test binary"
        )),
        (DefaultImpl::Unlinked, ..) => Err(format!(
            "{name}'s Default is not in the test binary (unused, so not linked)"
        )),
    }
}

/// An integer of `size` bytes: `true`, `false` or a number.
fn write_int(value: &str, size: u64, out: &mut Built, at: u64) -> Result<(), String> {
    let v: i128 = match value {
        "true" => 1,
        "false" => 0,
        v => v.parse().map_err(|_| format!("{v:?} is not an integer"))?,
    };
    if size > 8 {
        // A 128-bit integer: all of its bytes, two's complement.
        let n = size as usize;
        out.bytes[at as usize..at as usize + n].copy_from_slice(&v.to_le_bytes()[..n]);
    } else {
        put(&mut out.bytes, at, size, v as u64);
    }
    Ok(())
}

/// An enum's variant, as `Variant` or `Variant(payload)`.
fn write_enum(
    name: &str,
    tag: Option<(u64, u64)>,
    variants: &[Variant],
    value: &str,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Result<(), String> {
    let (variant, payload) = call(value);
    let variant = variant.rsplit("::").next().unwrap_or(variant);
    let v = variants
        .iter()
        .find(|v| v.name == variant)
        .ok_or_else(|| format!("{name} has no variant {variant}"))?;
    if let Some(p) = payload {
        let field = v
            .fields
            .first()
            .ok_or_else(|| format!("{name}::{variant} has no payload"))?;
        write(&field.layout, p, out, at + field.offset, facts)?;
    }
    match (tag, v.tag) {
        (Some((offset, size)), Some(t)) => put(&mut out.bytes, at + offset, size, t),
        // The niche-holding variant: its payload's own bytes are its tag.
        (_, None) => {}
        (None, Some(_)) => return Err(format!("{name}: a tag value without a tag")),
    }
    Ok(())
}

/// A struct: a tuple, a string slice, or a vector-like of the program's own.
fn write_struct(
    layout: &Layout,
    name: &str,
    fields: &[Field],
    value: &str,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Result<(), String> {
    // A tuple: its elements, in order.
    if !fields.is_empty()
        && fields.iter().all(|f| f.name.starts_with("__"))
        && value.starts_with('(')
        && value.ends_with(')')
    {
        let parts = items(&value[1..value.len() - 1]);
        if parts.len() != fields.len() {
            return Err(format!("{value} does not fit {name}"));
        }
        for (f, p) in fields.iter().zip(parts) {
            write(&f.layout, p, out, at + f.offset, facts)?;
        }
        return Ok(());
    }
    if name == "&str" {
        return write_str_slice(fields, value, out, at);
    }
    write_vec_like(layout, name, value, out, at, facts)
}

/// A string slice: its bytes on the heap (never freed: a `&str` does not
/// own them).
fn write_str_slice(fields: &[Field], value: &str, out: &mut Built, at: u64) -> Result<(), String> {
    let text = string_text(value).ok_or_else(|| format!("{value} is not a string"))?;
    for f in fields {
        let here = at + f.offset;
        match &f.layout {
            Layout::Pointer { size, .. } if !text.is_empty() => {
                let block = Built {
                    bytes: text.clone().into_bytes(),
                    heap: Vec::new(),
                    calls: Vec::new(),
                    constructs: Vec::new(),
                };
                out.heap.push((here as usize, block));
                put(&mut out.bytes, here, *size, 0);
            }
            Layout::Pointer { size, .. } => put(&mut out.bytes, here, *size, 1),
            Layout::Int { size, .. } => put(&mut out.bytes, here, *size, text.len() as u64),
            _ => {}
        }
    }
    Ok(())
}

/// A `String` or `Vec`: empty, a string's bytes, or `vec![..]` of elements.
fn write_vec_like(
    layout: &Layout,
    name: &str,
    value: &str,
    out: &mut Built,
    at: u64,
    facts: &DefaultFacts,
) -> Result<(), String> {
    let Some(elem) = element(layout) else {
        return Err(format!("{value} for {name} needs the program's code"));
    };
    let align = align_of(elem);
    if matches!(value, "String::new()" | "vec![]" | "Vec::new()") {
        fill_vec(layout, align, 0, None, out, at);
        return Ok(());
    }
    if let Some(text) = string_text(value) {
        let block = Built {
            bytes: text.clone().into_bytes(),
            heap: Vec::new(),
            calls: Vec::new(),
            constructs: Vec::new(),
        };
        fill_vec(layout, 1, text.len() as u64, Some(&block), out, at);
        return Ok(());
    }
    if let Some(inner) = value
        .strip_prefix("vec![")
        .and_then(|v| v.strip_suffix(']'))
    {
        let parts = items(inner);
        let size = elem.size();
        let mut block = Built {
            bytes: vec![0u8; (size as usize) * parts.len()],
            heap: Vec::new(),
            calls: Vec::new(),
            constructs: Vec::new(),
        };
        for (i, p) in parts.iter().enumerate() {
            write(elem, p, &mut block, i as u64 * size, facts)?;
        }
        fill_vec(layout, align, parts.len() as u64, Some(&block), out, at);
        return Ok(());
    }
    Err(format!("{value} for {name} needs the program's code"))
}
