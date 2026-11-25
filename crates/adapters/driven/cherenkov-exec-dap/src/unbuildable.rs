//! Why a value of a type is not built: from its layout ([`Unbuildable`]) or
//! by the support library's compiled constructor ([`Unconstructed`]). Coded,
//! with named data, never English (the messages are
//! `locales/en-US/messages.ftl`'s). What rustc said is carried as `detail`.

use coded_diagnostic::{Arg, Diagnostic, code};

/// Why a value is not built from the layout of its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unbuildable {
    /// The type has no `Default` in the source: the mutated source does not
    /// compile.
    NoDefault { name: String },
    /// The type's `Default`, if it has one, has no code in the test binary.
    DefaultNotInBinary { name: String },
    /// The type's `Default` is in the test binary's debug info but unused,
    /// so not linked.
    DefaultNotLinked { name: String },
    /// The layout (as the debugger names it) is not one this builds yet.
    NotBuiltYet { value: String, layout: String },
    /// The value is no integer.
    NotAnInteger { value: String },
    /// The enum has no such variant.
    NoVariant { name: String, variant: String },
    /// The variant has no payload to hold the value.
    NoPayload { name: String, variant: String },
    /// A variant has a tag value but the enum has no tag.
    TagWithoutTag { name: String },
    /// A tuple's elements are not the type's fields.
    DoesNotFit { value: String, name: String },
    /// The value is no string.
    NotAString { value: String },
    /// Only the program's own code makes the value.
    NeedsProgramCode { value: String, name: String },
    /// The value needs heap memory this cannot place.
    NeedsHeap { value: String },
}

impl std::fmt::Display for Unbuildable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Unbuildable {}

impl Diagnostic for Unbuildable {
    fn code(&self) -> &str {
        match self {
            Unbuildable::NoDefault { .. } => code!("no-default"),
            Unbuildable::DefaultNotInBinary { .. } => code!("default-not-in-binary"),
            Unbuildable::DefaultNotLinked { .. } => code!("default-not-linked"),
            Unbuildable::NotBuiltYet { .. } => code!("not-built-yet"),
            Unbuildable::NotAnInteger { .. } => code!("not-an-integer"),
            Unbuildable::NoVariant { .. } => code!("no-variant"),
            Unbuildable::NoPayload { .. } => code!("no-payload"),
            Unbuildable::TagWithoutTag { .. } => code!("tag-without-tag"),
            Unbuildable::DoesNotFit { .. } => code!("does-not-fit"),
            Unbuildable::NotAString { .. } => code!("not-a-string"),
            Unbuildable::NeedsProgramCode { .. } => code!("needs-program-code"),
            Unbuildable::NeedsHeap { .. } => code!("needs-heap"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let text = |name, value: &str| (name, Arg::from(value));
        match self {
            Unbuildable::NoDefault { name }
            | Unbuildable::DefaultNotInBinary { name }
            | Unbuildable::DefaultNotLinked { name }
            | Unbuildable::TagWithoutTag { name } => vec![text("name", name)],
            Unbuildable::NotBuiltYet { value, layout } => {
                vec![text("value", value), text("layout", layout)]
            }
            Unbuildable::NotAnInteger { value }
            | Unbuildable::NotAString { value }
            | Unbuildable::NeedsHeap { value } => vec![text("value", value)],
            Unbuildable::NoVariant { name, variant } | Unbuildable::NoPayload { name, variant } => {
                vec![text("name", name), text("variant", variant)]
            }
            Unbuildable::DoesNotFit { value, name }
            | Unbuildable::NeedsProgramCode { value, name } => {
                vec![text("value", value), text("name", name)]
            }
        }
    }
}

/// Why the support library does not make a value's constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unconstructed {
    /// The library's state is poisoned by a panic.
    Poisoned,
    /// No support library was built.
    NoLibrary,
    /// The request is not in the built library.
    NotInLibrary,
    /// The library holds no layout for the constructor's type.
    NoLayout { ty: String },
    /// The type is laid out differently in the library than in the program.
    LaidOutDifferently { ty: String, detail: String },
    /// A mirrored type is laid out differently in another program.
    MirrorConflict { ty: String, module: String },
    /// The type's name is not one the library has, for `detail`.
    Type { detail: String },
    /// The value is no value of a poll: the poll of a future not known as
    /// the mutated fn's body.
    NotItsPoll { value: String, ty: String },
    /// The library did not build, saying `detail`.
    Build { detail: String },
    /// The library could not be written or built at all.
    Io { detail: String },
    /// The library does not build without its failing constructors.
    NeverBuilds,
    /// The mirror of `of` does not compile.
    MirrorDoesNotCompile {
        ty: String,
        of: String,
        detail: String,
    },
    /// The constructor names something the library's prelude does not
    /// import.
    NameNotImported {
        value: String,
        ty: String,
        detail: String,
    },
    /// The constructor needs more of a mirrored type than its layout.
    NeedsMirror {
        value: String,
        ty: String,
        detail: String,
    },
    /// The constructor does not compile: the mutated source would not.
    DoesNotCompile {
        value: String,
        ty: String,
        detail: String,
    },
}

impl std::fmt::Display for Unconstructed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        coded_diagnostic::display(self, f)
    }
}

impl std::error::Error for Unconstructed {}

impl Diagnostic for Unconstructed {
    fn code(&self) -> &str {
        match self {
            Unconstructed::Poisoned => code!("support-poisoned"),
            Unconstructed::NoLibrary => code!("no-support-library"),
            Unconstructed::NotInLibrary => code!("not-in-support-library"),
            Unconstructed::NoLayout { .. } => code!("no-layout-in-support"),
            Unconstructed::LaidOutDifferently { .. } => code!("laid-out-differently"),
            Unconstructed::MirrorConflict { .. } => code!("mirror-conflict"),
            Unconstructed::Type { .. } => code!("support-type"),
            Unconstructed::NotItsPoll { .. } => code!("not-its-poll"),
            Unconstructed::Build { .. } => code!("support-build"),
            Unconstructed::Io { .. } => code!("support-io"),
            Unconstructed::NeverBuilds => code!("support-never-builds"),
            Unconstructed::MirrorDoesNotCompile { .. } => code!("mirror-does-not-compile"),
            Unconstructed::NameNotImported { .. } => code!("name-not-imported"),
            Unconstructed::NeedsMirror { .. } => code!("needs-mirror"),
            Unconstructed::DoesNotCompile { .. } => code!("does-not-compile"),
        }
    }

    fn args(&self) -> Vec<(&str, Arg)> {
        let text = |name, value: &str| (name, Arg::from(value));
        match self {
            Unconstructed::Poisoned | Unconstructed::NoLibrary | Unconstructed::NotInLibrary => {
                Vec::new()
            }
            Unconstructed::NeverBuilds => Vec::new(),
            Unconstructed::NoLayout { ty } => vec![text("ty", ty)],
            Unconstructed::LaidOutDifferently { ty, detail } => {
                vec![text("ty", ty), text("detail", detail)]
            }
            Unconstructed::MirrorConflict { ty, module } => {
                vec![text("ty", ty), text("module", module)]
            }
            Unconstructed::NotItsPoll { value, ty } => {
                vec![text("value", value), text("ty", ty)]
            }
            Unconstructed::Type { detail }
            | Unconstructed::Build { detail }
            | Unconstructed::Io { detail } => {
                vec![text("detail", detail)]
            }
            Unconstructed::MirrorDoesNotCompile { ty, of, detail } => {
                vec![text("ty", ty), text("of", of), text("detail", detail)]
            }
            Unconstructed::NameNotImported { value, ty, detail }
            | Unconstructed::NeedsMirror { value, ty, detail }
            | Unconstructed::DoesNotCompile { value, ty, detail } => {
                vec![text("value", value), text("ty", ty), text("detail", detail)]
            }
        }
    }
}

/// One value of every reason, for the catalog test.
#[cfg(test)]
pub fn diagnostic_samples() -> Vec<Box<dyn Diagnostic>> {
    let text = |s: &str| s.to_string();
    let unbuildable = vec![
        Unbuildable::NoDefault { name: text("n") },
        Unbuildable::DefaultNotInBinary { name: text("n") },
        Unbuildable::DefaultNotLinked { name: text("n") },
        Unbuildable::NotBuiltYet {
            value: text("v"),
            layout: text("l"),
        },
        Unbuildable::NotAnInteger { value: text("v") },
        Unbuildable::NoVariant {
            name: text("n"),
            variant: text("v"),
        },
        Unbuildable::NoPayload {
            name: text("n"),
            variant: text("v"),
        },
        Unbuildable::TagWithoutTag { name: text("n") },
        Unbuildable::DoesNotFit {
            value: text("v"),
            name: text("n"),
        },
        Unbuildable::NotAString { value: text("v") },
        Unbuildable::NeedsProgramCode {
            value: text("v"),
            name: text("n"),
        },
        Unbuildable::NeedsHeap { value: text("v") },
    ];
    let unconstructed = vec![
        Unconstructed::Poisoned,
        Unconstructed::NoLibrary,
        Unconstructed::NotInLibrary,
        Unconstructed::NoLayout { ty: text("t") },
        Unconstructed::LaidOutDifferently {
            ty: text("t"),
            detail: text("d"),
        },
        Unconstructed::MirrorConflict {
            ty: text("t"),
            module: text("m"),
        },
        Unconstructed::Type { detail: text("d") },
        Unconstructed::NotItsPoll {
            value: text("v"),
            ty: text("t"),
        },
        Unconstructed::Build { detail: text("d") },
        Unconstructed::Io { detail: text("d") },
        Unconstructed::NeverBuilds,
        Unconstructed::MirrorDoesNotCompile {
            ty: text("t"),
            of: text("o"),
            detail: text("d"),
        },
        Unconstructed::NameNotImported {
            value: text("v"),
            ty: text("t"),
            detail: text("d"),
        },
        Unconstructed::NeedsMirror {
            value: text("v"),
            ty: text("t"),
            detail: text("d"),
        },
        Unconstructed::DoesNotCompile {
            value: text("v"),
            ty: text("t"),
            detail: text("d"),
        },
    ];
    let boxed = |d: Box<dyn Diagnostic>| d;
    unbuildable
        .into_iter()
        .map(|d| boxed(Box::new(d)))
        .chain(unconstructed.into_iter().map(|d| boxed(Box::new(d))))
        .collect()
}
