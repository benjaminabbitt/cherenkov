use super::*;

fn snap(entries: &[(&str, &str, Option<u64>)]) -> Snapshot {
    Snapshot(
        entries
            .iter()
            .map(|(p, s, h)| (p.to_string(), (s.to_string(), *h)))
            .collect(),
    )
}

#[test]
fn test_a_path_new_at_the_end_is_written() {
    let before = snap(&[]);
    let after = snap(&[("a/marker", "??", Some(1))]);
    assert_eq!(written_since(&before, &after), ["a/marker"]);
}

#[test]
fn test_a_path_dirty_at_the_start_and_unchanged_is_not_written() {
    let both = snap(&[("a.rs", " M", Some(7))]);
    assert!(written_since(&both, &both.clone()).is_empty());
}

#[test]
fn test_a_path_dirty_at_the_start_whose_content_changed_is_written() {
    let before = snap(&[("a.rs", " M", Some(7))]);
    let after = snap(&[("a.rs", " M", Some(8))]);
    assert_eq!(written_since(&before, &after), ["a.rs"]);
}

#[test]
fn test_a_clean_path_that_is_deleted_or_a_dirty_one_that_changes_status_is_written() {
    let before = snap(&[("b.rs", "??", Some(1))]);
    let after = snap(&[("b.rs", "??", Some(1)), ("c.rs", " D", None)]);
    assert_eq!(written_since(&before, &after), ["c.rs"]);
    let staged = snap(&[("b.rs", "A ", Some(1))]);
    assert_eq!(written_since(&before, &staged), ["b.rs"]);
}

#[test]
fn test_a_path_that_is_clean_again_at_the_end_is_not_written() {
    let before = snap(&[("a.rs", " M", Some(7))]);
    assert!(written_since(&before, &snap(&[])).is_empty());
}

#[test]
fn test_the_porcelain_z_listing_is_read_with_a_rename_s_old_name_skipped() {
    let porcelain = b" M src/lib.rs\0?? a b/new\0R  to.rs\0from.rs\0 D gone.rs\0";
    let parsed = Snapshot::parse(porcelain, |p| Some(p.len() as u64));
    assert_eq!(
        parsed,
        snap(&[
            ("src/lib.rs", " M", Some(10)),
            ("a b/new", "??", Some(7)),
            ("to.rs", "R ", Some(5)),
            ("gone.rs", " D", Some(7)),
        ])
    );
}

#[test]
fn test_outside_a_git_repository_there_is_no_snapshot() {
    let dir = test_dir::TestDir::new("checkout-outside");
    assert_eq!(Snapshot::take(dir.path()), None);
}
