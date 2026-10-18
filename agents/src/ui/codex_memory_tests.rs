use super::*;
use crate::providers::{Provider, codex::CodexProvider};

fn provider_view() -> (tempfile::TempDir, View, chrono::DateTime<Utc>) {
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("reply.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../../tests/fixtures/codex-memory-citation/reply.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, name='Memory citation reply' WHERE id='thread-0'",
        [transcript.to_str().unwrap()],
    )
    .unwrap();
    let mut provider = CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let mut sessions = provider.refresh(1).sessions;
    assert_eq!(sessions.len(), 1);
    let now = "2026-10-12T02:13:00Z".parse().unwrap();
    sessions[0].updated_at = now;
    let events = provider.log(&sessions[0], usize::MAX, false).unwrap();
    let id = sessions[0].id.clone();
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 1;
    view.set_sessions(sessions, false);
    view.log_session = Some(id);
    view.set_log(events);
    view.select_log_event(Some(0));
    (dir, view, now)
}

fn text(buf: &Buffer) -> String {
    buf.content
        .chunks(usize::from(buf.area.width))
        .map(|row| {
            let mut text = String::new();
            let mut cells = row.iter();
            while let Some(cell) = cells.next() {
                text.push_str(cell.symbol());
                for _ in 1..cell.symbol().width() {
                    cells.next();
                }
            }
            text.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_owned()
}

fn assert_clean_complete(body: &str) {
    assert!(!body.contains("<oai-mem-citation>"), "{body}");
    assert!(!body.contains("citation_entries"), "{body}");
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    for expected in [
        "完成，未提交。cache 和 streamer",
        "270→270",
        "packages/cache/scripts/runTests.ts",
        "website/src/lib/chadwallet/api/__fixtures__/tradingActivityItems.ts",
        "负责范围内的旧路径已清理",
        "未改。",
    ] {
        let expected: String = expected.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains(&expected), "missing {expected}: {body}");
    }
    assert_eq!(body.matches("完成，未提交。").count(), 1, "{body}");
}

#[test]
fn codex_memory_citation_details_use_processed_body() {
    let (_dir, view, now) = provider_view();
    let area = Rect::new(0, 0, 160, 65);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let rendered = text(&buf);
    assert_clean_complete(&rendered);
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/codex_memory_details.txt")
    );
}

#[test]
fn codex_memory_citation_log_list_and_detail_use_processed_body() {
    let (_dir, view, now) = provider_view();
    assert_eq!(view.log.len(), 1, "reply must appear once");
    let area = Rect::new(0, 0, 2400, 2);
    let mut buf = Buffer::empty(area);
    view.render_log(area, &mut buf, now);
    assert_clean_complete(&text(&buf));
    let area = Rect::new(0, 0, 160, 65);
    let mut buf = Buffer::empty(area);
    view.render_log_detail(area, &mut buf, now);
    let rendered = text(&buf);
    assert_clean_complete(&rendered);
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/codex_memory_log_detail.txt")
    );
}
