use super::*;

fn said(done: usize, last: &str) -> Compiling {
    Compiling {
        done,
        last: last.into(),
    }
}

#[test]
fn test_the_first_crate_is_always_said() {
    let mut counter = Counter::new(Duration::from_secs(3600));
    assert_eq!(
        counter.crate_done("a".into(), Instant::now()),
        Some(said(1, "a"))
    );
}

#[test]
fn test_crates_within_the_interval_are_counted_but_not_said() {
    let start = Instant::now();
    let mut counter = Counter::new(Duration::from_secs(1));
    counter.crate_done("a".into(), start);
    assert_eq!(
        counter.crate_done("b".into(), start + Duration::from_millis(500)),
        None
    );
    assert_eq!(
        counter.crate_done("c".into(), start + Duration::from_secs(1)),
        Some(said(3, "c"))
    );
}

#[test]
fn test_only_compiler_artifact_lines_name_a_crate() {
    let artifact = br#"{"reason":"compiler-artifact","target":{"name":"serde"}}"#;
    assert_eq!(artifact_of(artifact), Some("serde".into()));
    assert_eq!(artifact_of(br#"{"reason":"compiler-message"}"#), None);
    assert_eq!(artifact_of(b"not json"), None);
}

#[test]
fn test_the_output_of_a_streamed_command_is_exactly_what_it_wrote() {
    let script = "echo '{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"a\"}}'; echo plain; printf 'no newline'; echo err >&2; exit 3";
    let mut told = Vec::new();
    let out = output_streaming(
        Command::new("sh").args(["-c", script]),
        Duration::ZERO,
        &mut |c| told.push(c),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"a\"}}\nplain\nno newline"
    );
    assert_eq!(out.stderr, b"err\n");
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(told, [said(1, "a")]);
}
