//! The source port: read-only by construction.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cherenkov_mutation::syntax::LanguageId;

/// Read-only by construction: no port writes source.
pub trait SourceReader {
    /// Source files of `lang` under the project root, relative to it, sorted.
    fn list(&self, lang: &LanguageId) -> io::Result<Vec<PathBuf>>;
    /// Contents of a file given relative to the project root.
    fn read(&self, rel: &Path) -> io::Result<Arc<str>>;
}
