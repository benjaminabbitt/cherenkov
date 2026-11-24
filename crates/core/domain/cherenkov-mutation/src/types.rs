//! What a project's own source says about its types' default values: the
//! one fact about a type a debugger cannot read back. A derived `Default`
//! that nothing uses is never compiled, so the binary cannot tell it from
//! a type with none; the source can.

use std::collections::BTreeMap;

/// How a type gets a default value, by its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDefault {
    /// Derived: each field's (or the `#[default]` variant's) default.
    Derived,
    /// Written by hand: only the program's own code knows it.
    Implemented,
    /// Defined in the project with no default at all: a mutant that asks
    /// for one does not compile.
    Absent,
    /// Not the project's, or defined more than once with a default in one
    /// of them: no claim.
    Unknown,
}

/// Per type name (unqualified, without generics), what the source says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultFacts {
    by_name: BTreeMap<String, Fact>,
    /// The project's own crates: the facts are about their types only.
    crates: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Fact {
    definitions: usize,
    derived: bool,
    implemented: bool,
}

impl DefaultFacts {
    /// A type definition named `name`, deriving `Default` or not.
    pub fn defined(&mut self, name: &str, derives_default: bool) {
        let f = self.by_name.entry(name.to_string()).or_default();
        f.definitions += 1;
        f.derived |= derives_default;
    }

    /// An `impl Default for name`.
    pub fn implemented(&mut self, name: &str) {
        self.by_name
            .entry(name.to_string())
            .or_default()
            .implemented = true;
    }

    /// A crate of the project (as rustc names it, `-` as `_`).
    pub fn project_crate(&mut self, name: &str) {
        self.crates.insert(name.replace('-', "_"));
    }

    /// Whether `krate` is one of the project's, whose types these facts
    /// describe; a dependency's type may share a short name.
    pub fn owns(&self, krate: &str) -> bool {
        self.crates.contains(krate)
    }

    /// Everything `other` knows, too (another file of the project).
    pub fn merge(&mut self, other: DefaultFacts) {
        for (name, f) in other.by_name {
            let mine = self.by_name.entry(name).or_default();
            mine.definitions += f.definitions;
            mine.derived |= f.derived;
            mine.implemented |= f.implemented;
        }
        self.crates.extend(other.crates);
    }

    /// What the source says about `name`'s default. A name defined more
    /// than once could be either type: no claim, unless neither has a
    /// default.
    pub fn of(&self, name: &str) -> SourceDefault {
        let base = name.split('<').next().unwrap_or(name).trim();
        match self.by_name.get(base) {
            Some(f) if f.definitions == 1 && f.implemented => SourceDefault::Implemented,
            Some(f) if f.definitions == 1 && f.derived => SourceDefault::Derived,
            // Several definitions, none with a default: the one meant has none.
            Some(f) if !f.derived && !f.implemented => SourceDefault::Absent,
            _ => SourceDefault::Unknown,
        }
    }
}
