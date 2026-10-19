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
                text.push_str(cell_text(cell.symbol()));
                for _ in 1..cell_text(cell.symbol()).width() {
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
    assert!(!text(&buf).contains("Memory:"));
    let area = Rect::new(0, 0, 160, 65);
    let mut buf = Buffer::empty(area);
    view.render_log_detail(area, &mut buf, now);
    let rendered = text(&buf);
    assert_clean_complete(&rendered);
    assert!(rendered.contains("Memory: MEMORY.md:136 · source conditions validation context"));
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/codex_memory_log_detail.txt")
    );
}

#[test]
fn codex_memory_note_link_opens_the_recorded_file_line() {
    let (dir, mut view, now) = provider_view();
    let memory = dir.path().join("memories/MEMORY.md");
    std::fs::create_dir(memory.parent().unwrap()).unwrap();
    std::fs::write(&memory, "# Memory\n").unwrap();
    let mut events = view.log.clone();
    events[0].detail_body.as_mut().unwrap().notes[0].links[0].path = memory.clone();
    view.set_log(events);
    let area = Rect::new(0, 0, 160, 65);
    view.viewport.borrow_mut().log_detail = area;
    let mut buf = Buffer::empty(area);
    view.render_log_detail(area, &mut buf, now);
    let target = PathBuf::from(super::super::vscode::file_line_link(&memory, 136).unwrap());
    let (rect, _) = view
        .viewport
        .borrow()
        .log_links
        .iter()
        .find(|(_, path)| *path == target)
        .cloned()
        .expect("memory file link");
    assert_eq!(rect.width, "MEMORY.md:136".len() as u16);
    assert!(
        buf[(rect.x, rect.y)]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x,
            rect.y
        ))
        .is_none()
    );
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), rect.x, rect.y))
            .is_none()
    );
    assert_eq!(
        view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1)),
        Some(Action::OpenPath(target))
    );
}
