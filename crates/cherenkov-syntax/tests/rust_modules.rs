//! The out-of-line modules a Rust file declares and which of them
//! cargo-mutants would not enter (visit.rs: a `mod` with `#[cfg(test)]`, a
//! `test`-ending attribute or `#[mutants::skip]`), with the files each can
//! be (`foo.rs`, `foo/mod.rs`, a `#[path]`).

use std::path::{Path, PathBuf};

use cherenkov_syntax::rust_modules::{ModuleFile, module_files};

fn files(rel: &str, src: &str) -> Vec<(Vec<String>, bool)> {
    module_files(Path::new(rel), src)
        .into_iter()
        .map(
            |ModuleFile {
                 candidates,
                 skipped,
             }| {
                (
                    candidates
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect(),
                    skipped,
                )
            },
        )
        .collect()
}

fn one(names: &[&str], skipped: bool) -> (Vec<String>, bool) {
    (names.iter().map(|s| s.to_string()).collect(), skipped)
}

#[test]
fn a_crate_root_looks_beside_itself() {
    assert_eq!(
        files("src/lib.rs", "mod a;\npub mod b;"),
        vec![
            one(&["src/a.rs", "src/a/mod.rs"], false),
            one(&["src/b.rs", "src/b/mod.rs"], false)
        ]
    );
}

#[test]
fn another_file_looks_in_its_own_directory() {
    assert_eq!(
        files("src/a.rs", "mod c;"),
        vec![one(&["src/a/c.rs", "src/a/c/mod.rs"], false)]
    );
    assert_eq!(
        files("src/a/mod.rs", "mod c;"),
        vec![one(&["src/a/c.rs", "src/a/c/mod.rs"], false)]
    );
}

#[test]
fn test_and_skip_attributes_mark_a_module_skipped() {
    let src = "#[cfg(test)]\nmod t;\n#[test]\nmod u;\n#[tokio::test]\nmod v;\n\
               #[mutants::skip]\nmod w;\n#[cfg_attr(test, mutants::skip)]\nmod x;\n\
               #[cfg(not(test))]\nmod y;\n#[cfg(feature = \"f\")]\nmod z;";
    let got: Vec<bool> = files("src/lib.rs", src).iter().map(|(_, s)| *s).collect();
    assert_eq!(got, [true, true, true, true, true, false, false]);
}

#[test]
fn a_path_attribute_names_the_file() {
    let src = "#[cfg(test)]\n#[path = \"x.test.rs\"]\nmod tests;";
    assert_eq!(
        files("src/lib.rs", src),
        vec![one(&["src/x.test.rs"], true)]
    );
    assert_eq!(files("src/a.rs", src), vec![one(&["src/x.test.rs"], true)]);
}

#[test]
fn inline_modules_extend_the_directory_and_pass_skips_down() {
    let src = "mod outer { mod inner; }\n#[cfg(test)]\nmod t { mod deep; }";
    assert_eq!(
        files("src/lib.rs", src),
        vec![
            one(&["src/outer/inner.rs", "src/outer/inner/mod.rs"], false),
            one(&["src/t/deep.rs", "src/t/deep/mod.rs"], true)
        ]
    );
}

#[test]
fn a_binary_in_src_bin_is_a_root() {
    let _: PathBuf = PathBuf::new();
    assert_eq!(
        files("src/bin/tool.rs", "mod m;"),
        vec![one(&["src/bin/m.rs", "src/bin/m/mod.rs"], false)]
    );
}
