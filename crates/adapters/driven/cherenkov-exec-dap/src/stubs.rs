//! The shapes of function-value stubs: a function body replaced by a
//! trivial non-zero value (`true`, `1`, `-1`, `None`, `Ok(())`) is
//! patched by writing the bytes rustc makes of that value over the
//! function's entry. Which bytes depends on the result type's layout (a
//! niche's value, the integer's width), so a stub is named by a shape: the
//! layout class and the value. The bytes of each shape that passed the gate
//! (`tests/tierp_gate`) are in `isa/tierp-shapes.toml`; any other shape is
//! declined.
//!
//! Only results the System V convention returns in registers, with the
//! value in the first of them, have a shape: an integer of one to eight
//! bytes, and an enum of at most sixteen bytes whose tag is at its start.

use std::fmt;

use crate::layouts::{Layout, Variant};

/// A trivial value of a result type's layout class, on x86-64.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StubShape {
    class: String,
    value: String,
}

impl fmt::Display for StubShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stub amd64 {} {}", self.class, self.value)
    }
}

/// The widest an enum returned in registers is.
const REGISTERS: u64 = 16;

/// The shape of returning `value` from a function whose result has layout
/// `result`; `None` when no stub is known for it.
pub fn shape_of(result: &Layout, value: &str) -> Option<StubShape> {
    match result {
        Layout::Int { size, signed } => int_shape(*size, *signed, value),
        Layout::Enum {
            size,
            tag: Some((0, tag)),
            variants,
            ..
        } if *size <= REGISTERS => enum_shape(*size, *tag, variants, value),
        _ => None,
    }
}

fn int_shape(size: u64, signed: bool, value: &str) -> Option<StubShape> {
    let fits = match value {
        "true" => size == 1 && !signed,
        "1" => true,
        "-1" => signed,
        _ => false,
    };
    (fits && matches!(size, 1 | 2 | 4 | 8)).then(|| StubShape {
        class: format!("{}{}", if signed { 'i' } else { 'u' }, size * 8),
        value: value.to_string(),
    })
}

/// The variant a value of an enum is, when it has no payload.
fn variant_of(value: &str) -> Option<&'static str> {
    match value {
        "None" => Some("None"),
        "Ok(())" => Some("Ok"),
        _ => None,
    }
}

fn enum_shape(size: u64, tag_size: u64, variants: &[Variant], value: &str) -> Option<StubShape> {
    let name = variant_of(value)?;
    let variant = variants.iter().find(|v| v.name == name)?;
    let empty = variant.fields.iter().all(|f| f.layout.size() == 0);
    let tag = variant.tag.filter(|_| empty)?;
    let mask = match tag_size {
        1 | 2 | 4 => (1u64 << (tag_size * 8)) - 1,
        8 => u64::MAX,
        _ => return None,
    };
    Some(StubShape {
        class: format!("enum{size}"),
        value: format!("{value} tag{tag_size}={}", tag & mask),
    })
}
