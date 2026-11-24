//! The source port is read-only by construction: no port writes source
//! (design section 4.1).

use std::path::Path;

#[test]
fn test_source_reader_port_has_no_write_method() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/source.rs");
    let ports = std::fs::read_to_string(path).unwrap();
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
