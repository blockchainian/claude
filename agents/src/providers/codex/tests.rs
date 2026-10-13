use super::*;
use crate::{model::SessionStatus, providers::SessionKey};
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, CodexProvider) {
    let dir = tempfile::tempdir().unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let provider = CodexProvider::new(dir.path().to_owned());
    (dir, provider)
}

#[test]
fn reads_named_columns_and_pages_inactive_threads() {
    let (_dir, mut provider) = fixture();
    let first = provider.refresh(1);
    assert_eq!(first.sessions.len(), 10);
    assert!(first.has_more);
    let all = provider.refresh(2);
    assert!(!all.has_more);
    assert_eq!(all.sessions.len(), 14);
    let session = all.sessions.iter().find(|s| s.id.id == "thread-0").unwrap();
    assert_eq!(session.id.provider, ProviderId::Codex);
    assert_eq!(session.cwd, PathBuf::from("/workspace/project"));
    assert_eq!(session.title, "Example task 0");
    assert_eq!(session.prompt, "Inspect the project");
    assert_eq!(session.git_branch.as_deref(), Some("example-branch"));
    assert!(session.model.is_some());
    assert!(session.started_at.is_some());
    assert_eq!(session.status, SessionStatus::Inactive);
    let child = all
        .sessions
        .iter()
        .find(|s| s.id.id == "thread-13")
        .unwrap();
    assert_eq!(
        child.parent,
        Some(SessionKey {
            provider: ProviderId::Codex,
            id: "thread-12".into()
        })
    );
    assert_eq!(provider.capabilities(), Capabilities::default());
    assert!(
        provider
            .launch(&Action::Resume(session.id.clone()), Some(session))
            .is_none()
    );
    assert!(provider.rename(session, "name").is_err());
    assert!(provider.delete(session).is_err());
}

#[test]
fn missing_home_is_empty_without_creating_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent");
    assert!(
        CodexProvider::new(path.clone())
            .refresh(1)
            .sessions
            .is_empty()
    );
    assert!(!path.exists());
}

#[test]
fn missing_optional_columns_degrade_and_database_stays_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state_5.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE threads (id TEXT, cwd TEXT, title TEXT, updated_at INTEGER); INSERT INTO threads VALUES ('minimal','/workspace','Task',1791911000);").unwrap();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let mut provider = CodexProvider::new(dir.path().to_owned());
    let snap = provider.refresh(1);
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].title, "Task");
    assert_eq!(snap.sessions[0].updated_at.timestamp(), 1791911000);
    assert_eq!(snap.sessions[0].model, None);
    assert_eq!(before, std::fs::read(path).unwrap());
}

#[test]
fn locks_require_a_holder_not_just_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("thread.lock");
    assert!(!lock_held(&path).unwrap());
    let writer = std::fs::File::create(&path).unwrap();
    assert!(!lock_held(&path).unwrap());
    writer.lock().unwrap();
    assert!(lock_held(&path).unwrap());
    drop(writer);
    assert!(!lock_held(&path).unwrap());
    assert_eq!(status(false, false), SessionStatus::Inactive);
    assert_eq!(status(false, true), SessionStatus::Inactive);
    assert_eq!(status(true, false), SessionStatus::Working);
    assert_eq!(status(true, true), SessionStatus::Ready);
}

#[test]
fn rollout_details_are_read_through_the_provider() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-rollout.jsonl"),
    )
    .unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=?",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let snapshot = provider.refresh(1);
    let session = &snapshot.sessions[0];
    assert_eq!(session.last_message.as_deref(), Some("Checks passed."));
    assert!(session.last_record_at.is_some());
    assert!(session.activity.is_some());
    let events = provider
        .log(session, 100, &log::Filter::default(), false)
        .unwrap();
    assert_eq!(events.len(), 3);
}
