use super::*;
use crate::providers::{Provider, codex::CodexProvider};

#[test]
fn claude_client_recorded_input_and_reply_snapshot() {
    for (tool, source) in [
        (
            "ask_claude",
            include_str!("../../tests/fixtures/claude-client/ask_claude.jsonl"),
        ),
        (
            "notify_claude",
            include_str!("../../tests/fixtures/claude-client/notify_claude.jsonl"),
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let transcript = directory.path().join("client.jsonl");
        std::fs::write(&transcript, source).unwrap();
        let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
        db.execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
            .unwrap();
        db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
            .unwrap();
        db.execute(
            "UPDATE threads SET rollout_path=?1 WHERE id='thread-0'",
            [transcript.to_str().unwrap()],
        )
        .unwrap();
        let mut provider =
            CodexProvider::new(directory.path().into(), directory.path().join("manager"));
        let session = provider.refresh(1).sessions.remove(0);
        let events = provider.log(&session, usize::MAX, false).unwrap();
        let record: serde_json::Value = serde_json::from_str(source).unwrap();
        let input = record["payload"]["item"]["arguments"]["text"]
            .as_str()
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, input);
        let now = events[0].ended_at.unwrap();
        let (mut view, _) = log_fixture();
        view.set_log(events);
        let rendered = log_screen(&view, 160, 65, now);
        assert!(!rendered.contains("\"text\":"), "{rendered}");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/ui_snapshots/claude_client_{tool}.txt"));
        if std::env::var_os("UPDATE_CLAUDE_CLIENT_SNAPSHOTS").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    }
}
