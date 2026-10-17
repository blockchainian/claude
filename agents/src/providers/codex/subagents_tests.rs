use super::*;
use std::io::Write;

const ROLLOUT: &str = include_str!("../../../tests/fixtures/codex-subagents/rollout.jsonl");

#[test]
fn subagent_reader_ignores_inherited_records_while_rollout_is_copied() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let lines: Vec<_> = ROLLOUT.split_inclusive('\n').collect();
    file.write_all(lines[..2].concat().trim_end().as_bytes())
        .unwrap();
    let mut reader = rollout::Reader::new(file.path().to_owned(), false);
    assert!(reader.refresh(usize::MAX).unwrap().events.is_empty());
    file.write_all(b"\n").unwrap();
    file.write_all(lines[2..].concat().as_bytes()).unwrap();
    let update = reader.refresh(usize::MAX).unwrap();
    assert_eq!(update.events.len(), 3);
    assert_eq!(update.events[0].body, "NEW_TASK from /root");
}

#[test]
fn subagent_reader_pages_appends_and_resets_its_history_boundary() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let lines: Vec<_> = ROLLOUT.split_inclusive('\n').collect();
    let split = lines
        .iter()
        .position(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["ordinal"] == 58)
        .unwrap();
    file.write_all(lines[..split].concat().as_bytes()).unwrap();
    let mut reader = rollout::Reader::new(file.path().to_owned(), false);
    assert_eq!(
        reader.refresh(1).unwrap().events[0].body,
        "NEW_TASK from /root"
    );
    file.write_all(lines[split..].concat().as_bytes()).unwrap();
    let update = reader.refresh(usize::MAX).unwrap();
    assert_eq!(update.events.len(), 3);
    assert_eq!(update.events[1].body, "MESSAGE from /root");
    assert!(
        update
            .events
            .iter()
            .all(|event| event.kind != crate::log::Kind::User)
    );
    let parent = include_str!("../../../tests/fixtures/codex-subagents/parent.jsonl");
    std::fs::write(file.path(), parent).unwrap();
    let update = reader.refresh(usize::MAX).unwrap();
    assert!(update.reset);
    assert_eq!(update.events.len(), 1);
    assert_eq!(update.events[0].kind, crate::log::Kind::Assistant);
}

#[test]
fn subagent_reader_excludes_inherited_history_and_formats_messages() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(ROLLOUT.as_bytes()).unwrap();
    let mut reader = rollout::Reader::new(file.path().to_owned(), false);
    let update = reader.refresh(usize::MAX).unwrap();
    assert_eq!(update.events.len(), 3);
    assert_eq!(update.events[0].body, "NEW_TASK from /root");
    assert_eq!(update.events[1].body, "MESSAGE from /root");
    assert!(update.events[2].body.starts_with("报告："));
    assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
    let summary = rollout::summary(file.path()).unwrap();
    assert!(summary.prompt_at.is_none());
}

#[test]
fn subagent_first_prompt_never_uses_inherited_user_message() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(ROLLOUT.as_bytes()).unwrap();
    let records: Vec<serde_json::Value> = ROLLOUT
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let inherited = records[1]["payload"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let (images, source) = rollout::first_prompt(
        file.path(),
        inherited.chars().take(512).collect::<String>().as_str(),
    )
    .unwrap();
    assert!(images.is_empty());
    assert!(source.is_none());
}

#[test]
fn subagent_title_comes_from_spawn_path_and_prompt_stays_empty() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let meta: serde_json::Value = serde_json::from_str(ROLLOUT.lines().next().unwrap()).unwrap();
    db.execute("UPDATE threads SET source = ?1, title = '', first_user_message = '', name = NULL WHERE id = 'thread-1'", [meta["payload"]["source"].to_string()]).unwrap();
    let session = db
        .query_row("SELECT * FROM threads WHERE id = 'thread-1'", [], |row| {
            session(row, &HashMap::new())
        })
        .unwrap()
        .unwrap();
    assert_eq!(session.display_title(), "provider_adapters");
    assert!(session.prompt.is_empty());
}
