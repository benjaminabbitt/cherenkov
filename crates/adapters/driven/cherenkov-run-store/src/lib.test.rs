use test_dir::TestDir;

use super::*;

#[tokio::test]
async fn a_store_is_made_in_a_file_that_is_not_there() {
    let dir = TestDir::new("cherenkov-store-made");
    let file = dir.join("runs.sqlite");
    let store = open(&file).await.unwrap();
    store.close().await.unwrap();
    assert!(file.is_file());
}

#[tokio::test]
async fn a_store_reopened_is_the_one_that_was_closed() {
    use angzarr_ports::{EventStore, StreamRef};
    let dir = TestDir::new("cherenkov-store-reopened");
    let file = dir.join("runs.sqlite");
    let cover = angzarr_ports::angzarr_proto::v1::Cover {
        domain: "d".to_string(),
        root: Some(angzarr_ports::angzarr_proto::v1::Uuid::from_bytes([3; 16])),
        edition: None,
    };
    let page = angzarr_ports::angzarr_proto::v1::EventPage {
        sequence: 0,
        ..Default::default()
    };
    let store = open(&file).await.unwrap();
    store
        .write(angzarr_ports::Write::append(
            StreamRef::try_from(&cover).unwrap(),
            0,
            &[page],
        ))
        .await
        .unwrap();
    store.close().await.unwrap();

    let again = open(&file).await.unwrap();
    let slice = again
        .read(StreamRef::try_from(&cover).unwrap(), 0, None)
        .await
        .unwrap();
    assert_eq!(slice.pages.len(), 1);
    again.close().await.unwrap();
}

#[tokio::test]
async fn a_file_that_is_not_a_store_is_refused_and_left_alone() {
    let dir = TestDir::new("cherenkov-store-refused");
    let file = dir.join("runs.sqlite");
    std::fs::write(
        &file,
        b"this is not a database, and is long enough to be read as a header",
    )
    .unwrap();
    assert!(open(&file).await.is_err());
    assert!(std::fs::read(&file).unwrap().starts_with(b"this is not"));
}

#[test]
fn a_projects_store_is_under_its_cherenkov_directory_and_git_ignores_itself() {
    let dir = TestDir::new("cherenkov-store-project");
    let file = in_project(&dir).unwrap();
    assert_eq!(file, dir.join(".cherenkov/store/runs.sqlite"));
    let ignore = std::fs::read_to_string(dir.join(".cherenkov/store/.gitignore")).unwrap();
    assert_eq!(ignore.trim(), "*");
    // A second call leaves what is there.
    std::fs::write(dir.join(".cherenkov/store/.gitignore"), "*\n# mine\n").unwrap();
    in_project(&dir).unwrap();
    assert!(
        std::fs::read_to_string(dir.join(".cherenkov/store/.gitignore"))
            .unwrap()
            .contains("mine")
    );
}

#[test]
fn a_projects_store_is_under_its_cherenkov_directory_and_naming_it_makes_nothing() {
    let dir = TestDir::new("cherenkov-store-project-store");
    let path = project_store(&dir);
    assert_eq!(path, dir.join(".cherenkov/store/runs.sqlite"));
    assert!(!dir.join(".cherenkov").exists());
    assert_eq!(in_project(&dir).unwrap(), path);
    assert!(dir.join(".cherenkov/store/.gitignore").is_file());
}
