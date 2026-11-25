//! The support library (docs/support-library.md): values a mutant returns,
//! made by compiled constructors in a cdylib generated outside the project
//! and built with its toolchain, one export per (type, value). The library
//! is built once for all the constructors requested so far; a request after
//! a build rebuilds it with every request.
//!
//! The debugger side needs only what [`resolve`] gives: a library's path
//! and a constructor's symbol, `extern "C" fn(out: *mut T)`, which writes
//! a fresh value into `out`.

pub mod mirror;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use coded_diagnostic::Report;
use object::{Object, ObjectSymbol};
use serde_json::Value;

use cherenkov_mutation::types::{DefaultFacts, SourceDefault};

use crate::layouts::{Layout, Layouts, Refusal};
use crate::unbuildable::Unconstructed;

/// A value to construct: its type (public Rust paths, and mirrors) and its
/// source text.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Request {
    pub ty: String,
    pub value: String,
    /// The mirror types `ty` uses, by module name: their source.
    pub mirrors: BTreeMap<String, String>,
}

/// Defining paths DWARF names std types by, and the public paths that name
/// them in source. `None`: a defaulted parameter that stable Rust cannot
/// write (the global allocator), dropped from the arguments.
const PUBLIC_PATHS: &[(&str, Option<&str>)] = &[
    ("alloc::alloc::Global", None),
    ("alloc::boxed::Box", Some("Box")),
    (
        "alloc::collections::btree::map::BTreeMap",
        Some("std::collections::BTreeMap"),
    ),
    (
        "alloc::collections::btree::set::BTreeSet",
        Some("std::collections::BTreeSet"),
    ),
    (
        "alloc::collections::vec_deque::VecDeque",
        Some("std::collections::VecDeque"),
    ),
    ("alloc::rc::Rc", Some("std::rc::Rc")),
    ("alloc::string::String", Some("String")),
    ("alloc::sync::Arc", Some("std::sync::Arc")),
    ("alloc::vec::Vec", Some("Vec")),
    ("alloc::borrow::Cow", Some("std::borrow::Cow")),
    (
        "alloc::collections::binary_heap::BinaryHeap",
        Some("std::collections::BinaryHeap"),
    ),
    (
        "alloc::collections::linked_list::LinkedList",
        Some("std::collections::LinkedList"),
    ),
    ("core::cell::Cell", Some("std::cell::Cell")),
    (
        "core::marker::PhantomData",
        Some("std::marker::PhantomData"),
    ),
    ("core::num::nonzero::NonZero", Some("std::num::NonZero")),
    ("core::ptr::non_null::NonNull", Some("std::ptr::NonNull")),
    (
        "core::sync::atomic::AtomicBool",
        Some("std::sync::atomic::AtomicBool"),
    ),
    (
        "core::sync::atomic::AtomicU32",
        Some("std::sync::atomic::AtomicU32"),
    ),
    (
        "core::sync::atomic::AtomicU64",
        Some("std::sync::atomic::AtomicU64"),
    ),
    (
        "core::sync::atomic::AtomicUsize",
        Some("std::sync::atomic::AtomicUsize"),
    ),
    ("core::time::Duration", Some("std::time::Duration")),
    ("core::cell::RefCell", Some("std::cell::RefCell")),
    ("core::option::Option", Some("Option")),
    ("core::pin::Pin", Some("std::pin::Pin")),
    ("core::result::Result", Some("Result")),
    ("core::task::poll::Poll", Some("std::task::Poll")),
    (
        "std::collections::hash::map::HashMap",
        Some("std::collections::HashMap"),
    ),
    (
        "std::collections::hash::map::RandomState",
        Some("std::collections::hash_map::RandomState"),
    ),
    (
        "std::collections::hash::set::HashSet",
        Some("std::collections::HashSet"),
    ),
    ("std::ffi::os_str::OsString", Some("std::ffi::OsString")),
    (
        "std::hash::random::RandomState",
        Some("std::hash::RandomState"),
    ),
    ("std::path::PathBuf", Some("std::path::PathBuf")),
    ("std::sync::mutex::Mutex", Some("std::sync::Mutex")),
    ("std::sync::poison::mutex::Mutex", Some("std::sync::Mutex")),
    (
        "std::sync::poison::rwlock::RwLock",
        Some("std::sync::RwLock"),
    ),
    ("std::sync::rwlock::RwLock", Some("std::sync::RwLock")),
    ("std::time::Instant", Some("std::time::Instant")),
    ("std::time::SystemTime", Some("std::time::SystemTime")),
];

const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "bool",
    "char", "f32", "f64", "str", "()",
];

/// A type as DWARF names it, written with public paths; `Err` names the
/// first path outside the table.
pub fn public_type(dwarf: &str) -> Result<String, String> {
    parse_type(dwarf, None, false)
}

/// A std type as DWARF names it, written as a mirror's field: what it
/// borrows, borrowed for `'static` (a mirror has no lifetime parameters,
/// and no lifetime is part of a layout).
pub fn mirror_field_type(dwarf: &str) -> Result<String, String> {
    parse_type(dwarf, None, true)
}

fn parse_type(
    dwarf: &str,
    mirroring: Option<&mut Mirroring<'_>>,
    borrows_static: bool,
) -> Result<String, String> {
    let mut p = TypeParser {
        text: dwarf,
        at: 0,
        mirroring,
        borrows_static,
    };
    let ty = p.ty()?;
    p.skip_spaces();
    if p.at != dwarf.len() {
        return Err(format!("{dwarf}: unexpected {:?}", &dwarf[p.at..]));
    }
    ty.ok_or_else(|| format!("{dwarf}: a defaulted parameter is not a type"))
}

/// Mirrors made for one request, from the program's DWARF.
struct Mirroring<'a> {
    layouts: &'a Layouts,
    facts: &'a DefaultFacts,
    /// By DWARF name: the mirror's Rust path and whether it has a
    /// `Default`; `None` while it is being made (a recursive type).
    made: HashMap<String, Option<(String, bool)>>,
    /// By module name: the mirror's source.
    sources: BTreeMap<String, String>,
    /// Whether every mirror the type being parsed names has a `Default`.
    all_default: bool,
}

/// Marks a mirror of a project type whose source has no `Default`: a
/// constructor that asks for one does not compile as the mutated source.
const NO_DEFAULT: &str = "// No Default in the source.";

/// FNV-1a: stable across runs and programs, so a type's mirror has one
/// name everywhere.
fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Mirroring<'_> {
    /// The Rust path of the mirror of the type DWARF names `dwarf`, made
    /// (with the mirrors its fields need) if it is not yet.
    fn mirror(&mut self, dwarf: &str) -> Result<String, String> {
        let module = format!("m{:016x}", stable_hash(dwarf));
        let path = format!("crate::{module}::M{}", &module[1..]);
        match self.made.get(dwarf) {
            Some(Some((path, default))) => {
                self.all_default &= *default;
                return Ok(path.clone());
            }
            Some(None) => {
                self.all_default = false;
                return Ok(path);
            }
            None => {}
        }
        if self.made.len() > 64 {
            return Err(format!("{dwarf}: too many types to mirror"));
        }
        let def = self
            .layouts
            .definition(dwarf)
            .ok_or_else(|| format!("{dwarf}: no definition in the program's debug info"))?;
        self.made.insert(dwarf.to_string(), None);
        let outer = std::mem::replace(&mut self.all_default, true);
        let mut members = |ms: Vec<crate::layouts::Member>| -> Result<Vec<mirror::Member>, String> {
            ms.into_iter()
                .map(|m| {
                    let ty = m.ty.as_deref().ok_or_else(|| {
                        format!("{dwarf}.{}: a field type debug info gives no name", m.name)
                    })?;
                    Ok(mirror::Member {
                        ty: parse_type(ty, Some(self), true)
                            .map_err(|e| format!("{dwarf}.{}: {e}", m.name))?,
                        name: m.name,
                        offset: m.offset,
                        size: m.size,
                    })
                })
                .collect()
        };
        let shape = match def.kind {
            crate::layouts::DefinitionKind::Struct(ms) => mirror::Shape::Struct(members(ms)?),
            crate::layouts::DefinitionKind::Enum { tag, variants } => mirror::Shape::Enum {
                tag,
                variants: variants
                    .into_iter()
                    .map(|(name, tag, ms)| {
                        Ok(mirror::VariantDef {
                            name,
                            tag,
                            members: members(ms)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            },
            crate::layouts::DefinitionKind::Union => mirror::Shape::Union,
        };
        let fields_default = std::mem::replace(&mut self.all_default, outer);
        // A derived Default is each field's; a hand-written one is only
        // the program's own code, which a mirror does not have.
        let derived_default = matches!(shape, mirror::Shape::Struct(_))
            && fields_default
            && self.facts.owns(&def.krate)
            && self.facts.of(&def.short) == SourceDefault::Derived;
        let absent =
            self.facts.owns(&def.krate) && self.facts.of(&def.short) == SourceDefault::Absent;
        let source = mirror::define(
            &format!("M{}", &module[1..]),
            &mirror::TypeDef {
                name: dwarf.to_string(),
                size: def.size,
                align: def.align,
                shape,
                derived_default,
            },
        )?;
        let source = if absent {
            format!("{source}{NO_DEFAULT}\n")
        } else {
            source
        };
        self.sources.insert(module, source);
        self.made
            .insert(dwarf.to_string(), Some((path.clone(), derived_default)));
        self.all_default &= derived_default;
        Ok(path)
    }
}

/// The request for `value` as a value of the type DWARF names `dwarf` in
/// the program `layouts` describes: types outside std are mirrored from
/// its DWARF (with a `Default` where `facts` say the source derives one).
pub fn request_in(
    dwarf: &str,
    value: &str,
    layouts: &Layouts,
    facts: &DefaultFacts,
) -> Result<Request, Refusal> {
    let mut mirroring = Mirroring {
        layouts,
        facts,
        made: HashMap::new(),
        sources: BTreeMap::new(),
        all_default: true,
    };
    let ty = parse_type(dwarf, Some(&mut mirroring), true).map_err(not_a_type)?;
    requested(ty, value, mirroring.sources)
}

/// The request for `value` as a value of the type DWARF names `dwarf`.
/// A `Poll` the value is not one of is the poll of a future not known as
/// the mutated fn's body: what the value would compile as is unknown, so
/// it is refused rather than built (and found unviable).
pub fn request_for(dwarf: &str, value: &str) -> Result<Request, Refusal> {
    requested(
        public_type(dwarf).map_err(not_a_type)?,
        value,
        BTreeMap::new(),
    )
}

/// A type the support library does not name, for the parser's `detail`.
fn not_a_type(detail: String) -> Refusal {
    Refusal::Unsupported(Report::of(&Unconstructed::Type { detail }))
}

fn requested(
    ty: String,
    value: &str,
    mirrors: BTreeMap<String, String>,
) -> Result<Request, Refusal> {
    if ty.starts_with("std::task::Poll<") && !value.starts_with("Poll::") {
        return Err(Refusal::Unsupported(Report::of(
            &Unconstructed::NotItsPoll {
                value: value.to_string(),
                ty,
            },
        )));
    }
    Ok(Request {
        ty,
        value: value.to_string(),
        mirrors,
    })
}

struct TypeParser<'t, 'm, 'l> {
    text: &'t str,
    at: usize,
    /// Mirrors for types outside std; `None`: such a type is refused.
    mirroring: Option<&'m mut Mirroring<'l>>,
    /// What the type borrows, it borrows for `'static`: in a mirror's field.
    borrows_static: bool,
}

/// What the support library calls a type path.
enum PublicName {
    Named(String),
    /// A parameter the table drops.
    Dropped,
    Unknown,
}

/// The support library's name for `path`; `single` when it is one segment.
fn public_name(path: &str, single: bool) -> PublicName {
    if single && PRIMITIVES.contains(&path) {
        return PublicName::Named(path.to_string());
    }
    match PUBLIC_PATHS.iter().find(|(d, _)| *d == path) {
        Some((_, Some(p))) => PublicName::Named(p.to_string()),
        Some((_, None)) => PublicName::Dropped,
        None => PublicName::Unknown,
    }
}

impl TypeParser<'_, '_, '_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn skip_spaces(&mut self) {
        while self.rest().starts_with(' ') {
            self.at += 1;
        }
    }

    fn eat(&mut self, s: &str) -> bool {
        self.skip_spaces();
        if self.rest().starts_with(s) {
            self.at += s.len();
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> &str {
        self.skip_spaces();
        let start = self.at;
        let len = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(self.rest().len());
        self.at += len;
        &self.text[start..start + len]
    }

    /// One type: `None` for a parameter the table drops.
    fn ty(&mut self) -> Result<Option<String>, String> {
        if self.eat("&") {
            return self.reference().map(Some);
        }
        if self.eat("*") {
            return self.raw_pointer().map(Some);
        }
        if self.mirroring.is_some()
            && let Some(opaque) = self.opaque()?
        {
            return Ok(Some(opaque));
        }
        if self.eat("(") {
            return self.tuple().map(Some);
        }
        if self.eat("[") {
            return self.array_or_slice().map(Some);
        }
        self.path_type()
    }

    /// A reference, after its `&`.
    fn reference(&mut self) -> Result<String, String> {
        let mutable = self.rest().starts_with("mut ") && self.eat("mut");
        let inner = self.required()?;
        let lifetime = if self.borrows_static { "'static " } else { "" };
        Ok(format!(
            "&{lifetime}{}{inner}",
            if mutable { "mut " } else { "" }
        ))
    }

    /// A raw pointer, after its `*`.
    fn raw_pointer(&mut self) -> Result<String, String> {
        let kind = self.word().to_string();
        if kind != "mut" && kind != "const" {
            return Err(format!("{}: a raw pointer without mut or const", self.text));
        }
        let inner = self.required()?;
        Ok(format!("*{kind} {inner}"))
    }

    /// In a mirror only the layout matters: a trait object's is any trait
    /// object's, a function pointer's any function pointer's. `None` for
    /// any other type, which is left unread.
    fn opaque(&mut self) -> Result<Option<String>, String> {
        self.skip_spaces();
        let rest = self.rest();
        if rest.starts_with("dyn ") || rest.starts_with("(dyn ") {
            let parenthesized = self.eat("(");
            self.skip_type();
            if parenthesized && !self.eat(")") {
                return Err(format!("{}: unclosed trait object", self.text));
            }
            return Ok(Some("dyn std::any::Any".to_string()));
        }
        if ["fn(", "unsafe ", "extern "]
            .iter()
            .any(|p| rest.starts_with(p))
        {
            self.skip_type();
            return Ok(Some("unsafe fn()".to_string()));
        }
        Ok(None)
    }

    /// A tuple, after its `(`.
    fn tuple(&mut self) -> Result<String, String> {
        let mut items = Vec::new();
        while !self.eat(")") {
            items.push(self.required()?);
            if !self.eat(",") && !self.rest().trim_start().starts_with(')') {
                return Err(format!("{}: unclosed tuple", self.text));
            }
        }
        Ok(match items.len() {
            1 => format!("({},)", items[0]),
            _ => format!("({})", items.join(", ")),
        })
    }

    /// An array or a slice, after its `[`.
    fn array_or_slice(&mut self) -> Result<String, String> {
        let elem = self.required()?;
        if self.eat(";") {
            let len = self.word().to_string();
            if !self.eat("]") || len.is_empty() {
                return Err(format!("{}: unclosed array", self.text));
            }
            return Ok(format!("[{elem}; {len}]"));
        }
        if !self.eat("]") {
            return Err(format!("{}: unclosed slice", self.text));
        }
        Ok(format!("[{elem}]"))
    }

    /// A path, with its arguments: named by the table, dropped by it, or
    /// (not in it) mirrored.
    fn path_type(&mut self) -> Result<Option<String>, String> {
        self.skip_spaces();
        let start = self.at;
        let mut segments = vec![self.word().to_string()];
        while self.rest().starts_with("::") {
            self.at += 2;
            segments.push(self.word().to_string());
        }
        if segments.iter().any(String::is_empty) {
            return Err(format!(
                "{}: not a type path at {:?}",
                self.text,
                self.rest()
            ));
        }
        let path = segments.join("::");
        let mut args = Vec::new();
        if self.eat("<") {
            while !self.eat(">") {
                args.extend(self.ty()?);
                if !self.eat(",") && !self.rest().trim_start().starts_with('>') {
                    return Err(format!("{}: unclosed arguments", self.text));
                }
            }
        }
        let public = match public_name(&path, segments.len() == 1) {
            PublicName::Named(public) => public,
            PublicName::Dropped => return Ok(None),
            PublicName::Unknown => {
                let full = self.text[start..self.at].trim_end().to_string();
                return match self.mirroring.as_deref_mut() {
                    Some(m) => m.mirror(&full).map(Some),
                    None => Err(format!(
                        "{path} is not a std type the support library names"
                    )),
                };
            }
        };
        // The one table type with a lifetime parameter.
        if self.borrows_static && public == "std::borrow::Cow" {
            args.insert(0, "'static".to_string());
        }
        Ok(Some(if args.is_empty() {
            public
        } else {
            format!("{public}<{}>", args.join(", "))
        }))
    }

    /// Move past one type, whatever it is: to the next `,`, `>` or `)`
    /// outside brackets (an arrow's `>` is no bracket).
    fn skip_type(&mut self) {
        let mut depth = 0i32;
        let bytes = self.text.as_bytes();
        while self.at < bytes.len() {
            match bytes[self.at] {
                b'-' if bytes.get(self.at + 1) == Some(&b'>') => {
                    self.at += 2;
                    continue;
                }
                b'(' | b'[' | b'<' => depth += 1,
                b')' | b']' | b'>' if depth == 0 => break,
                b',' if depth == 0 => break,
                b')' | b']' | b'>' => depth -= 1,
                _ => {}
            }
            self.at += 1;
        }
        while self.text[..self.at].ends_with(' ') {
            self.at -= 1;
        }
    }

    fn required(&mut self) -> Result<String, String> {
        self.ty()?.ok_or_else(|| {
            format!(
                "{}: a defaulted parameter where a type is needed",
                self.text
            )
        })
    }
}

/// The names a value text uses unqualified, as a mutated source in scope
/// of the common std imports writes them.
const PRELUDE: &str = "\
#[allow(unused_imports)]
pub use std::borrow::Cow;
#[allow(unused_imports)]
pub use std::cell::{Cell, RefCell};
#[allow(unused_imports)]
pub use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, LinkedList, VecDeque};
#[allow(unused_imports)]
pub use std::path::{Path, PathBuf};
#[allow(unused_imports)]
pub use std::rc::Rc;
#[allow(unused_imports)]
pub use std::sync::{Arc, Mutex, RwLock};
#[allow(unused_imports)]
pub use std::task::Poll;
";

/// The library's export for its `k`th constructor.
pub fn symbol(k: usize) -> String {
    format!("cherenkov_v{k}")
}

/// The library crate's files (path, contents) for `requests`, the `k`th
/// in `src/v{k}.rs`, so a compile error's file names its constructor.
pub fn sources(requests: &[Request]) -> Vec<(String, String)> {
    let manifest = "[package]\nname = \"cherenkov_support\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
                    [lib]\ncrate-type = [\"cdylib\"]\npath = \"src/lib.rs\"\n\n\
                    [profile.dev]\nopt-level = 0\ndebug = 2\nstrip = false\n\n[workspace]\n";
    let mut lib = String::from("#![allow(improper_ctypes_definitions)]\npub mod prelude;\n");
    let mut files = vec![
        ("Cargo.toml".to_string(), manifest.to_string()),
        ("src/prelude.rs".to_string(), PRELUDE.to_string()),
    ];
    // Each mirror in its own file, so a compile error names it.
    let mirrors: BTreeMap<&String, &String> =
        requests.iter().flat_map(|r| r.mirrors.iter()).collect();
    for (module, source) in mirrors {
        lib.push_str(&format!("pub mod {module};\n"));
        files.push((format!("src/{module}.rs"), source.clone()));
    }
    for (k, r) in requests.iter().enumerate() {
        lib.push_str(&format!("mod v{k};\n"));
        files.push((
            format!("src/v{k}.rs"),
            format!(
                "#[allow(unused_imports)]\nuse crate::prelude::*;\n\n\
                 #[no_mangle]\npub unsafe extern \"C\" fn {}(out: *mut {}) {{\n    \
                 unsafe {{ out.write({{ {} }}) }}\n}}\n",
                symbol(k),
                r.ty,
                r.value
            ),
        ));
    }
    files.push(("src/lib.rs".to_string(), lib));
    files
}

/// Error codes of a name the library's prelude does not import: the
/// mutated source may still compile, so the constructor is unsupported,
/// not unviable.
const UNRESOLVED: &[&str] = &[
    "E0405", "E0412", "E0422", "E0423", "E0425", "E0432", "E0433",
];

/// What cargo's JSON messages say failed: per constructor index, its first
/// error (and whether the name was unresolved); and errors outside every
/// constructor's file.
pub fn attribute_errors(stdout: &str) -> (BTreeMap<usize, (bool, String)>, Vec<String>) {
    let (by_ctor, by_mirror, mut other) = attribute(stdout);
    other.extend(by_mirror.into_values());
    let by_ctor = by_ctor
        .into_iter()
        .map(|(k, (code, text))| (k, (UNRESOLVED.contains(&code.as_str()), text)))
        .collect();
    (by_ctor, other)
}

/// Where an error is: in a constructor's file, or a mirror's.
enum Origin {
    Constructor(usize),
    Mirror(String),
}

/// Each erring constructor's first error (its code and text), each erring
/// mirror module's, and the errors in neither.
#[allow(clippy::type_complexity)]
fn attribute(
    stdout: &str,
) -> (
    BTreeMap<usize, (String, String)>,
    BTreeMap<String, String>,
    Vec<String>,
) {
    let mut by_ctor = BTreeMap::new();
    let mut by_mirror = BTreeMap::new();
    let mut other = Vec::new();
    for m in stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-message")
    {
        let msg = &m["message"];
        if msg["level"] != "error" {
            continue;
        }
        let text = msg["message"].as_str().unwrap_or("").to_string();
        let code = msg["code"]["code"].as_str().unwrap_or("").to_string();
        let origin = msg["spans"]
            .as_array()
            .and_then(|spans| spans.iter().find_map(origin_of));
        match origin {
            Some(Origin::Constructor(k)) => {
                by_ctor.entry(k).or_insert((code, text));
            }
            Some(Origin::Mirror(module)) => {
                by_mirror.entry(module).or_insert(text);
            }
            // rustc's summary ("aborting due to ...") follows attributed errors.
            None if text.starts_with("aborting due to")
                || text.starts_with("could not compile") => {}
            None => other.push(text),
        }
    }
    (by_ctor, by_mirror, other)
}

/// The constructor or mirror a span is in: its file's, or, for code a
/// macro expanded (`vec![..]`), the file the expansion was written in.
fn origin_of(span: &Value) -> Option<Origin> {
    let mut span = span;
    for _ in 0..16 {
        let file = span["file_name"]
            .as_str()
            .and_then(|f| f.strip_prefix("src/"));
        let stem = file.and_then(|f| f.strip_suffix(".rs"));
        if let Some(k) = stem
            .and_then(|f| f.strip_prefix('v'))
            .and_then(|k| k.parse().ok())
        {
            return Some(Origin::Constructor(k));
        }
        if let Some(m) = stem.filter(|f| f.starts_with('m') && *f != "prelude") {
            return Some(Origin::Mirror(m.to_string()));
        }
        span = &span["expansion"]["span"];
        if span.is_null() {
            return None;
        }
    }
    None
}

/// The compared shape of two layouts of one type: size, alignment, field
/// offsets and names (recursively), and an enum's tag and each variant's
/// tag and fields, an array's length and element. `Err` names the first
/// difference.
pub fn same_layout(here: &Layout, there: &Layout) -> Result<(), String> {
    use Layout::*;
    let differ = |what: &str, a: &dyn std::fmt::Debug, b: &dyn std::fmt::Debug| {
        Err(format!(
            "{what}: {a:?} in the library, {b:?} in the program"
        ))
    };
    match (here, there) {
        (Int { size: a, signed: x }, Int { size: b, signed: y }) if (a, x) == (b, y) => Ok(()),
        (Float { size: a }, Float { size: b }) if a == b => Ok(()),
        (Pointer { size: a, align: x }, Pointer { size: b, align: y }) => {
            if (a, x) == (b, y) {
                Ok(())
            } else {
                differ("pointer (size, pointee alignment)", &(a, x), &(b, y))
            }
        }
        (
            Struct {
                name: n,
                size: a,
                align: x,
                fields: f,
                ..
            },
            Struct {
                size: b,
                align: y,
                fields: g,
                ..
            },
        ) => {
            if a != b {
                return differ(&format!("{n}'s size"), a, b);
            }
            if x != y {
                return differ(&format!("{n}'s alignment"), x, y);
            }
            same_fields(n, f, g)
        }
        (
            Enum {
                name: n,
                size: a,
                tag: s,
                variants: v,
                ..
            },
            Enum {
                size: b,
                tag: t,
                variants: w,
                ..
            },
        ) => {
            if a != b {
                return differ(&format!("{n}'s size"), a, b);
            }
            if s != t {
                return differ(&format!("{n}'s tag (offset, size)"), s, t);
            }
            if v.len() != w.len() {
                return differ(&format!("{n}'s variants"), &v.len(), &w.len());
            }
            for (p, q) in v.iter().zip(w) {
                if (&p.name, p.tag) != (&q.name, q.tag) {
                    return differ(
                        &format!("{n}'s variant"),
                        &(&p.name, p.tag),
                        &(&q.name, q.tag),
                    );
                }
                same_fields(&format!("{n}::{}", p.name), &p.fields, &q.fields)?;
            }
            Ok(())
        }
        (Array { count: a, elem: x }, Array { count: b, elem: y }) => {
            if a != b {
                return differ("array length", a, b);
            }
            same_layout(x, y).map_err(|e| format!("array element: {e}"))
        }
        (Other { name: n, size: a }, Other { size: b, .. }) if a == b => {
            let _ = n;
            Ok(())
        }
        (a, b) => differ("layout", a, b),
    }
}

fn same_fields(
    owner: &str,
    f: &[crate::layouts::Field],
    g: &[crate::layouts::Field],
) -> Result<(), String> {
    // A mirror's padding is no field of the original, and it declares its
    // fields in offset order.
    let real = |fs: &[crate::layouts::Field]| -> Vec<crate::layouts::Field> {
        let mut fs: Vec<_> = fs
            .iter()
            .filter(|f| !f.name.starts_with(mirror::PAD))
            .cloned()
            .collect();
        fs.sort_by(|a, b| (a.offset, &a.name).cmp(&(b.offset, &b.name)));
        fs
    };
    let (f, g) = (&real(f)[..], &real(g)[..]);
    if f.len() != g.len() {
        return Err(format!(
            "{owner}'s fields: {} in the library, {} in the program",
            f.len(),
            g.len()
        ));
    }
    for (a, b) in f.iter().zip(g) {
        if (&a.name, a.offset) != (&b.name, b.offset) {
            return Err(format!(
                "{owner}'s field: {}@{} in the library, {}@{} in the program",
                a.name, a.offset, b.name, b.offset
            ));
        }
        same_layout(&a.layout, &b.layout).map_err(|e| format!("{owner}.{}: {e}", a.name))?;
    }
    Ok(())
}

/// One build of the library: each request's constructor, or why not.
struct Build {
    built: HashMap<Request, Result<(PathBuf, String), Refusal>>,
    /// The library's DWARF, for the layout guard.
    layouts: Option<Arc<Layouts>>,
    /// Each constructor's entry (file address in the library) by symbol.
    entries: HashMap<String, u64>,
}

#[derive(Default)]
struct Library {
    requested: BTreeSet<Request>,
    build: Option<Build>,
    builds: usize,
}

/// Every toolchain's library in this process (`None`: whatever `cargo`
/// is). Shared by all executors, so it is built once per run.
static LIBRARIES: Mutex<BTreeMap<Option<String>, Library>> = Mutex::new(BTreeMap::new());

/// The directory every library is generated and built in: Cherenkov's own
/// scratch space, never the project's.
pub fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cherenkov-support-{}", std::process::id()))
}

/// Remove [`scratch_dir`] (at the end of a run).
pub fn remove_scratch() {
    let _ = std::fs::remove_dir_all(scratch_dir());
}

/// Remove the scratch directories (`cherenkov-support-<pid>`, and
/// `cherenkov-run-<pid>` of a Go run) that processes no longer running left
/// behind: a run killed with SIGKILL cannot remove its own, and each holds a
/// few hundred megabytes. Returns how many were removed.
pub fn sweep_stale_scratch() -> usize {
    sweep_stale_in(&std::env::temp_dir(), crate::gdb::alive)
}

/// [`sweep_stale_scratch`] in `temp`, `alive` saying whether a process runs.
fn sweep_stale_in(temp: &Path, alive: impl Fn(u32) -> bool) -> usize {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| owner_gone(&entry.file_name().to_string_lossy(), &alive))
        .filter(|entry| std::fs::remove_dir_all(entry.path()).is_ok())
        .count()
}

/// Whether directory `name` is a scratch directory of a process that is gone.
fn owner_gone(name: &str, alive: &impl Fn(u32) -> bool) -> bool {
    ["cherenkov-support-", "cherenkov-run-"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .and_then(|pid| pid.parse::<u32>().ok())
        .is_some_and(|pid| !alive(pid))
}

/// Ask for a constructor in the next build; nothing is built yet.
pub fn request(toolchain: Option<&str>, request: &Request) {
    if let Ok(mut libs) = LIBRARIES.lock() {
        libs.entry(toolchain.map(str::to_string))
            .or_default()
            .requested
            .insert(request.clone());
    }
}

/// A request the support library refuses to make, for `why`.
fn unsupported(why: Unconstructed) -> Refusal {
    Refusal::Unsupported(Report::of(&why))
}

/// The library and symbol of `request`'s constructor, building the library
/// (with every request so far) if it lacks it. `expected` is the type's
/// layout in the program: a constructor whose type is laid out otherwise
/// in the library is refused.
pub fn resolve(
    toolchain: Option<&str>,
    request: &Request,
    expected: &Layout,
) -> Result<(PathBuf, String), Refusal> {
    let mut libs = LIBRARIES
        .lock()
        .map_err(|_| unsupported(Unconstructed::Poisoned))?;
    let key = toolchain.map(str::to_string);
    let lib = libs.entry(key).or_default();
    lib.requested.insert(request.clone());
    if !lib
        .build
        .as_ref()
        .is_some_and(|b| b.built.contains_key(request))
    {
        lib.builds += 1;
        let requests: Vec<Request> = lib.requested.iter().cloned().collect();
        let dir = scratch_dir().join(toolchain.unwrap_or("default"));
        lib.build = Some(build(&dir, toolchain, &requests, lib.builds));
    }
    let Some(b) = &lib.build else {
        return Err(unsupported(Unconstructed::NoLibrary));
    };
    let (path, symbol) = b
        .built
        .get(request)
        .cloned()
        .unwrap_or_else(|| Err(unsupported(Unconstructed::NotInLibrary)))?;
    let there = b
        .layouts
        .as_ref()
        .zip(b.entries.get(&symbol))
        .and_then(|(l, &e)| l.first_param_pointee(e))
        .ok_or_else(|| {
            unsupported(Unconstructed::NoLayout {
                ty: request.ty.clone(),
            })
        })?;
    same_layout(&there, expected).map_err(|detail| {
        unsupported(Unconstructed::LaidOutDifferently {
            ty: request.ty.clone(),
            detail,
        })
    })?;
    Ok((path, symbol))
}

/// Generate and build the library for `requests`: constructors that do not
/// compile are unviable (or unsupported, for a name the prelude lacks), and
/// the rest are built again without them.
fn build(dir: &Path, toolchain: Option<&str>, requests: &[Request], generation: usize) -> Build {
    let mut refused = conflicting_mirrors(requests);
    // All, then those that compiled; rustc may report a later stage's
    // errors only once an earlier stage's are gone.
    for _ in 0..6 {
        let kept: Vec<usize> = (0..requests.len())
            .filter(|k| !refused.contains_key(k))
            .collect();
        let these: Vec<Request> = kept.iter().map(|&k| requests[k].clone()).collect();
        let (stdout, ok) = match compile(dir, toolchain, &these) {
            Ok(o) => o,
            Err(detail) => return failed(Unconstructed::Io { detail }, requests),
        };
        if ok {
            return built(dir, &stdout, generation, requests, &kept, refused);
        }
        let (by_ctor, by_mirror, other) = attribute(&stdout);
        if by_ctor.is_empty() && by_mirror.is_empty() {
            let why = other
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown error".into());
            return failed(Unconstructed::Build { detail: why }, requests);
        }
        refuse_mirrors(by_mirror, &kept, requests, &mut refused);
        for (i, (code, why)) in by_ctor {
            let Some(&k) = kept.get(i) else { continue };
            refused
                .entry(k)
                .or_insert_with(|| ctor_refusal(&requests[k], &code, &why));
        }
    }
    failed(Unconstructed::NeverBuilds, requests)
}

/// A build in which every request is refused for `why`.
fn failed(why: Unconstructed, requests: &[Request]) -> Build {
    Build {
        built: requests
            .iter()
            .map(|r| (r.clone(), Err(unsupported(why.clone()))))
            .collect(),
        layouts: None,
        entries: HashMap::new(),
    }
}

/// The requests whose mirrors are laid out differently from an earlier
/// request's, by their index. One definition per mirror: a type two
/// programs lay out differently is mirrored for the first only.
fn conflicting_mirrors(requests: &[Request]) -> HashMap<usize, Refusal> {
    let mut refused = HashMap::new();
    let mut mirrors: HashMap<&String, &String> = HashMap::new();
    for (k, r) in requests.iter().enumerate() {
        if let Some((module, _)) = r
            .mirrors
            .iter()
            .find(|(m, src)| mirrors.get(m).is_some_and(|s| s != src))
        {
            refused.insert(
                k,
                unsupported(Unconstructed::MirrorConflict {
                    ty: r.ty.clone(),
                    module: module.clone(),
                }),
            );
            continue;
        }
        mirrors.extend(r.mirrors.iter());
    }
    refused
}

/// The build once the library compiled (`stdout` is cargo's report): the
/// library copied, its layouts and exports read, each request's symbol, or
/// its refusal.
fn built(
    dir: &Path,
    stdout: &str,
    generation: usize,
    requests: &[Request],
    kept: &[usize],
    mut refused: HashMap<usize, Refusal>,
) -> Build {
    let Some(so) = cdylib_of(stdout) else {
        return failed(
            Unconstructed::Io {
                detail: "cargo named no library".into(),
            },
            requests,
        );
    };
    // A copy per build: a process may still have the last one loaded.
    let path = dir.join(format!("libcherenkov_support-{generation}.so"));
    if let Err(e) = std::fs::copy(&so, &path) {
        return failed(
            Unconstructed::Io {
                detail: format!("{}: {e}", path.display()),
            },
            requests,
        );
    }
    let mut built = HashMap::new();
    for (k, r) in requests.iter().enumerate() {
        let result = match refused.remove(&k) {
            Some(why) => Err(why),
            None => {
                let i = kept.iter().position(|&j| j == k).unwrap_or(k);
                Ok((path.clone(), symbol(i)))
            }
        };
        built.insert(r.clone(), result);
    }
    Build {
        built,
        layouts: Layouts::read(&path).ok().map(Arc::new),
        entries: exports(&path),
    }
}

/// Refuse each kept request whose mirror failed to compile (`by_mirror`:
/// module and why), unless it is refused already.
fn refuse_mirrors(
    by_mirror: std::collections::BTreeMap<String, String>,
    kept: &[usize],
    requests: &[Request],
    refused: &mut HashMap<usize, Refusal>,
) {
    for (module, why) in by_mirror {
        for &k in kept {
            let r = &requests[k];
            if let Some(source) = r.mirrors.get(&module) {
                let of = source.lines().next().unwrap_or_default();
                refused.entry(k).or_insert_with(|| {
                    unsupported(Unconstructed::MirrorDoesNotCompile {
                        ty: r.ty.clone(),
                        of: of.trim_start_matches("/// ").to_string(),
                        detail: why.clone(),
                    })
                });
            }
        }
    }
}

/// Why the constructor of `r` did not compile (rustc's error `code`).
fn ctor_refusal(r: &Request, code: &str, why: &str) -> Refusal {
    if UNRESOLVED.contains(&code) {
        unsupported(Unconstructed::NameNotImported {
            value: r.value.clone(),
            ty: r.ty.clone(),
            detail: why.to_string(),
        })
    } else if !r.mirrors.is_empty()
        && code != "E0308"
        && !r.mirrors.iter().any(|(module, source)| {
            source.contains(NO_DEFAULT) && why.contains(&format!("`M{}: Default`", &module[1..]))
        })
    {
        // A mirror has none of its original's traits but a derived
        // Default: the original may have the one asked for.
        unsupported(Unconstructed::NeedsMirror {
            value: r.value.clone(),
            ty: r.ty.clone(),
            detail: why.to_string(),
        })
    } else {
        Refusal::Unviable(Report::of(&Unconstructed::DoesNotCompile {
            value: r.value.clone(),
            ty: r.ty.clone(),
            detail: why.to_string(),
        }))
    }
}

/// Write the crate for `requests` into `dir` and build it: cargo's JSON
/// output, and whether it succeeded.
fn compile(
    dir: &Path,
    toolchain: Option<&str>,
    requests: &[Request],
) -> Result<(String, bool), String> {
    let _ = std::fs::remove_dir_all(dir.join("src"));
    std::fs::create_dir_all(dir.join("src")).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (path, text) in sources(requests) {
        let at = dir.join(path);
        std::fs::write(&at, text).map_err(|e| format!("{}: {e}", at.display()))?;
    }
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--message-format=json"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("target"));
    // The profile is the manifest's: a run's own profile overrides would
    // strip the DWARF the layout guard reads.
    for (name, _) in std::env::vars() {
        if name.starts_with("CARGO_PROFILE_") || name.starts_with("CARGO_BUILD_") {
            cmd.env_remove(name);
        }
    }
    match toolchain {
        Some(t) => cmd.env("RUSTUP_TOOLCHAIN", t),
        None => cmd.env_remove("RUSTUP_TOOLCHAIN"),
    };
    let out = cmd.output().map_err(|e| format!("running cargo: {e}"))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

/// The cdylib cargo's JSON output names.
fn cdylib_of(stdout: &str) -> Option<PathBuf> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact")
        .flat_map(|m| {
            m["filenames"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|f| f.as_str().map(PathBuf::from))
                .collect::<Vec<_>>()
        })
        .find(|f| f.extension().is_some_and(|e| e == "so"))
}

/// The library's exported constructors by symbol, at their file addresses.
fn exports(path: &Path) -> HashMap<String, u64> {
    let Ok(data) = std::fs::read(path) else {
        return HashMap::new();
    };
    let Ok(file) = object::File::parse(&*data) else {
        return HashMap::new();
    };
    file.dynamic_symbols()
        .chain(file.symbols())
        .filter_map(|s| {
            let name = s.name().ok()?;
            name.starts_with("cherenkov_v")
                .then(|| (name.to_string(), s.address()))
        })
        .collect()
}

#[cfg(test)]
#[path = "support_lib.test.rs"]
mod tests;
