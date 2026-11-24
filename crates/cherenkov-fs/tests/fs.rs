//! The filesystem SourceReader over the checked-in fixtures (read-only).

use std::path::{Path, PathBuf};

use cherenkov_fs::FsSourceReader;
use cherenkov_mutation::ports::SourceReader;
use cherenkov_mutation::profile::Profile;
use cherenkov_mutation::syntax::LanguageId;
use cherenkov_syntax::TreeSitterSyntax;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn profiles() -> Vec<Profile> {
    TreeSitterSyntax::builtin().unwrap().profiles()
}

fn list(dir: &str, lang: &str) -> Vec<PathBuf> {
    FsSourceReader::new(fixtures().join(dir), &profiles())
        .unwrap()
        .list(&LanguageId::new(lang))
        .unwrap()
}

#[test]
fn test_list_returns_sources_relative_to_root_excluding_tests() {
    assert_eq!(list("python", "python"), [PathBuf::from("calc.py")]);
    assert_eq!(list("rust", "rust"), [PathBuf::from("src/lib.rs")]);
    assert_eq!(list("go", "go"), [PathBuf::from("calc.go")]);
}

#[test]
fn test_list_from_parent_dir_is_sorted_and_filters_by_language() {
    assert_eq!(list("", "go"), [PathBuf::from("go/calc.go")]);
    assert_eq!(list("", "python"), [PathBuf::from("python/calc.py")]);
    assert_eq!(list("", "rust"), [PathBuf::from("rust/src/lib.rs")]);
}

#[test]
fn test_list_unknown_language_is_invalid_input() {
    let reader = FsSourceReader::new(fixtures(), &profiles()).unwrap();
    let err = reader.list(&LanguageId::new("cobol")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn test_read_returns_file_contents() {
    let reader = FsSourceReader::new(fixtures().join("go"), &profiles()).unwrap();
    let text = reader.read(Path::new("calc.go")).unwrap();
    assert!(text.starts_with("package calc\n"));
}

#[test]
fn test_read_refuses_paths_outside_root() {
    let reader = FsSourceReader::new(fixtures().join("go"), &profiles()).unwrap();
    for escape in ["../python/calc.py", "/etc/hostname"] {
        let err = reader.read(Path::new(escape)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{escape}");
    }
}

#[test]
fn test_new_rejects_missing_root() {
    let err = FsSourceReader::new(fixtures().join("no-such-dir"), &profiles())
        .err()
        .unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_new_rejects_invalid_exclude_glob() {
    let mut ps = profiles();
    ps[0].files.exclude.push("a[".into());
    let err = FsSourceReader::new(fixtures(), &ps).err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}
