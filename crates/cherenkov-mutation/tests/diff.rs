use std::path::Path;

use cherenkov_mutation::diff::AddedLines;

const DIFF: &str = "\
diff --git a/crates/x/src/hub.rs b/crates/x/src/hub.rs
index 111..222 100644
--- a/crates/x/src/hub.rs
+++ b/crates/x/src/hub.rs
@@ -10,3 +10,4 @@ fn f() {
 keep
-old
+new one
+new two
 keep
@@ -40,0 +42 @@ fn g() {
+single
diff --git a/crates/y/src/new.rs b/crates/y/src/new.rs
new file mode 100644
--- /dev/null
+++ b/crates/y/src/new.rs
@@ -0,0 +1,2 @@
+a
+b
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-a
-b
";

#[test]
fn test_a_span_touching_an_added_line_is_in_the_diff() {
    let diff = AddedLines::parse(DIFF);
    let hub = Path::new("crates/x/src/hub.rs");
    // Added: 11-12 and 42 (a context line is 10, 13).
    assert!(diff.touches(hub, 12, 12));
    assert!(diff.touches(hub, 1, 11), "a span reaching the added line");
    assert!(diff.touches(hub, 42, 50));
    assert!(!diff.touches(hub, 10, 10), "context is not added");
    assert!(!diff.touches(hub, 13, 41));
}

#[test]
fn test_a_new_file_is_all_added_and_a_deleted_one_adds_nothing() {
    let diff = AddedLines::parse(DIFF);
    assert!(diff.touches(Path::new("crates/y/src/new.rs"), 2, 2));
    assert!(!diff.touches(Path::new("crates/y/src/new.rs"), 3, 3));
    assert!(!diff.touches(Path::new("gone.rs"), 1, 2));
    assert!(!diff.touches(Path::new("crates/z/src/other.rs"), 1, 100));
}

#[test]
fn test_a_plan_s_path_below_the_run_directory_matches_the_diff_s_path_from_the_repository_root() {
    let diff = AddedLines::parse(DIFF);
    let rel = Path::new("src/hub.rs");
    assert!(diff.touches_below(Path::new("crates/x"), rel, 11, 11));
    assert!(!diff.touches_below(Path::new("crates/other"), rel, 11, 11));
}

#[test]
fn test_an_added_line_that_looks_like_a_file_header_is_still_an_added_line() {
    let diff = AddedLines::parse(
        "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,3 @@\n keep\n+++ not a header\n-old\n+z\n",
    );
    assert!(diff.touches(Path::new("x.rs"), 2, 2));
    assert!(diff.touches(Path::new("x.rs"), 3, 3));
    assert!(!diff.touches(Path::new("x.rs"), 1, 1));
}

#[test]
fn test_the_files_of_a_diff_come_with_the_line_ranges_it_adds() {
    let diff =
        "--- a/x/a.rs\n+++ b/x/a.rs\n@@ -1,2 +1,3 @@\n k\n+n\n k\n@@ -9,1 +10,2 @@\n k\n+m\n";
    let added = cherenkov_mutation::diff::AddedLines::parse(diff);
    let files: Vec<_> = added.files().collect();
    assert_eq!(
        files,
        [(
            std::path::Path::new("x/a.rs"),
            [(2, 2), (11, 11)].as_slice()
        )]
    );
}
