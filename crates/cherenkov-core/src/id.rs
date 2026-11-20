//! Stable mutant IDs derived from source structure, not line numbers
//! (design section 4.5).

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct MutantId(pub String);

/// The structural inputs an ID is derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdParts<'a> {
    pub path: &'a str,
    pub function: Option<&'a str>,
    pub operator: &'a str,
    /// The statement or condition text the mutant is anchored to.
    pub anchor: &'a str,
    pub edit: &'a str,
    /// Index among mutants with identical other parts, in source order.
    pub occurrence: usize,
}

impl MutantId {
    /// 16 lowercase hex digits. Whitespace runs in `anchor` are collapsed
    /// first, so reformatting does not change the ID.
    pub fn derive(parts: &IdParts<'_>) -> MutantId {
        let anchor = parts
            .anchor
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let occurrence = parts.occurrence.to_string();
        let mut h = Sha256::new();
        // Length-prefix every field so adjacent fields cannot run together;
        // a missing function is distinct from an empty name.
        let fields: [Option<&str>; 6] = [
            Some(parts.path),
            parts.function,
            Some(parts.operator),
            Some(&anchor),
            Some(parts.edit),
            Some(&occurrence),
        ];
        for field in fields {
            match field {
                Some(f) => {
                    h.update([1u8]);
                    h.update((f.len() as u64).to_le_bytes());
                    h.update(f.as_bytes());
                }
                None => h.update([0u8]),
            }
        }
        let digest = h.finalize();
        MutantId(digest[..8].iter().map(|b| format!("{b:02x}")).collect())
    }
}

impl std::fmt::Display for MutantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
