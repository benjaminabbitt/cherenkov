//! Read-only `SourceReader` over a project directory. There is no write
//! path: this adapter only lists and reads.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cherenkov_mutation::ports::SourceReader;
use cherenkov_mutation::profile::Profile;
use cherenkov_mutation::syntax::LanguageId;
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

struct LanguageFiles {
    extensions: Vec<String>,
    exclude: GlobSet,
}

pub struct FsSourceReader {
    root: PathBuf,
    languages: BTreeMap<LanguageId, LanguageFiles>,
}

/// A name no real file has, for asking "would anything under this directory
/// be excluded wholesale?" (`**/target/**` matches `target/<probe>`).
const DIR_PROBE: &str = "cherenkov-dir-probe";

impl FsSourceReader {
    /// `profiles` supply each language's extensions and exclude globs.
    pub fn new(root: impl Into<PathBuf>, profiles: &[Profile]) -> io::Result<Self> {
        let root = root.into();
        let meta = std::fs::metadata(&root)?;
        if !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a directory", root.display()),
            ));
        }
        let mut languages = BTreeMap::new();
        for p in profiles {
            let mut set = GlobSetBuilder::new();
            for pattern in &p.files.exclude {
                let glob = Glob::new(pattern).map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidInput, format!("{}: {e}", p.lang))
                })?;
                set.add(glob);
            }
            let exclude = set
                .build()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            languages.insert(
                p.lang.clone(),
                LanguageFiles {
                    extensions: p.files.extensions.clone(),
                    exclude,
                },
            );
        }
        Ok(FsSourceReader { root, languages })
    }
}

impl SourceReader for FsSourceReader {
    fn list(&self, lang: &LanguageId) -> io::Result<Vec<PathBuf>> {
        let files = self.languages.get(lang).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("no profile for language {lang}"),
            )
        })?;
        let rel = |p: &Path| {
            p.strip_prefix(&self.root)
                .map(Path::to_path_buf)
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        let walk = WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Prune directories an exclude glob removes entirely (target/, vendor/, ...).
                e.depth() == 0
                    || !e.file_type().is_dir()
                    || !files.exclude.is_match(rel(e.path()).join(DIR_PROBE))
            });
        for entry in walk {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = rel(entry.path());
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if files.extensions.iter().any(|x| x == ext) && !files.exclude.is_match(&path) {
                out.push(path);
            }
        }
        out.sort();
        Ok(out)
    }

    fn read(&self, rel: &Path) -> io::Result<Arc<str>> {
        let confined = rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !confined {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is outside the project root", rel.display()),
            ));
        }
        Ok(Arc::from(std::fs::read_to_string(self.root.join(rel))?))
    }
}
