//! The core stays pure: no I/O dependencies, no file-writing APIs, and no
//! port that writes source (design section 4.1).

use std::path::Path;

const ALLOWED_DEPENDENCIES: &[&str] = &["bitflags", "serde", "sha2"];

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn test_core_depends_only_on_pure_crates() {
    let manifest: toml::Table = std::fs::read_to_string(manifest_dir().join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let deps = manifest
        .get("dependencies")
        .and_then(|d| d.as_table())
        .cloned()
        .unwrap_or_default();
    for name in deps.keys() {
        assert!(
            ALLOWED_DEPENDENCIES.contains(&name.as_str()),
            "cherenkov-core may not depend on {name}"
        );
    }
}

#[test]
fn test_core_source_uses_no_filesystem_process_or_network_api() {
    let forbidden = [
        "std::fs",
        "std::process",
        "std::net",
        "std::env",
        "File::",
        "OpenOptions",
    ];
    for entry in std::fs::read_dir(manifest_dir().join("src")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for f in forbidden {
            assert!(!text.contains(f), "{} uses {f}", path.display());
        }
    }
}

#[test]
fn test_source_reader_port_has_no_write_method() {
    let ports = std::fs::read_to_string(manifest_dir().join("src/ports.rs")).unwrap();
    let start = ports.find("pub trait SourceReader").unwrap();
    let body = &ports[start..start + ports[start..].find("\n}").unwrap()];
    let methods: Vec<&str> = body
        .lines()
        .filter_map(|l| l.trim().strip_prefix("fn "))
        .collect();
    assert_eq!(methods.len(), 2, "{methods:?}");
    assert!(
        methods[0].starts_with("list(") && methods[1].starts_with("read("),
        "{methods:?}"
    );
}
