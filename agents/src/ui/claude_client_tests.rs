use super::*;
use crate::providers::{Provider, codex::CodexProvider};

#[test]
fn claude_client_recorded_input_and_reply_snapshot() {
    for (tool, source) in [
        (
            "ask",
            include_str!("../../tests/fixtures/claude-client/ask.jsonl"),
        ),
        (
            "notify",
            include_str!("../../tests/fixtures/claude-client/notify.jsonl"),
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
            CodexProvider::new(directory.path().into());
        let session = provider.refresh(1).sessions.remove(0);
        let events = provider.event_panel(&session, usize::MAX, false).unwrap();
        let record: serde_json::Value = serde_json::from_str(source).unwrap();
        let input = record["payload"]["item"]["arguments"]["text"]
            .as_str()
            .unwrap();
        assert_eq!(events.len(), 1);
        let call = &events[0].calls[0];
        let display_name = crate::providers::tool::display_tool_name(&call.name);
        assert_eq!(events[0].body, format!("{display_name} {input}"));
        assert_eq!(call.detail_left(), "");
        assert_eq!(super::tool_detail::call_title(call).1, display_name);
        let reply = record["payload"]["item"]["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(
            call.output,
            Some(vec![crate::event::Section::Markdown {
                label: "output".into(),
                text: reply.into(),
            }])
        );
        let now = events[0].ended_at.unwrap();
        let (mut view, _) = event_fixture();
        view.set_event(events);
        let rendered = event_screen(&view, 160, 65, now);
        assert!(!rendered.contains("\"text\":"), "{rendered}");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/ui_snapshots/claude_client_{tool}.txt"));
        if std::env::var_os("UPDATE_CLAUDE_CLIENT_SNAPSHOTS").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    }
}

#[test]
fn chrome_javascript_recorded_input_and_output_blocks_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("chrome.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../../tests/fixtures/claude-chrome-javascript.jsonl"),
    )
    .unwrap();
    let events = crate::providers::claude::event_reader::tail(&transcript, usize::MAX).unwrap();
    assert_eq!(events.len(), 1);
    let now = events[0].ended_at.unwrap();
    assert_eq!(events[0].calls[0].output.as_ref().unwrap().len(), 2);
    let (mut view, _) = event_fixture();
    view.set_event(events);
    for width in [120, 180] {
        let rendered = event_screen(&view, width, 90, now);
        assert!(
            rendered.contains(".getEntriesByType('resource')"),
            "{rendered}"
        );
        assert!(!rendered.contains("\"action\""), "{rendered}");
        assert!(!rendered.contains("\"tabId\""), "{rendered}");
        assert!(rendered.contains("- Available tabs:"), "{rendered}");
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/ui_snapshots/chrome_javascript_{width}.txt"));
        if std::env::var_os("UPDATE_CHROME_JAVASCRIPT_SNAPSHOTS").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    }
}
