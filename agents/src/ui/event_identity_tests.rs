use super::*;
use crate::providers::{Provider, ProviderId};

fn fixture() -> (tempfile::TempDir, View, Vec<Event>, Vec<Event>, usize) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut session = provider.refresh(1).sessions.remove(0);
    session.id.provider = ProviderId::Codex;
    session.status = crate::model::SessionStatus::Ready;
    session.transcript = Some(path.clone());
    let transcript = include_str!("../../tests/fixtures/codex-agent-exchange.jsonl");
    let earlier = "amsg_01a150fc-88af-7831-beee-c780bc453351";
    let recent = transcript
        .lines()
        .filter(|line| {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["payload"]["id"].as_str() != Some(earlier)
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&path, recent).unwrap();
    let before = crate::providers::event_reader(&session)
        .unwrap()
        .refresh(100)
        .unwrap()
        .events;
    std::fs::write(&path, transcript).unwrap();
    let after = crate::providers::event_reader(&session)
        .unwrap()
        .refresh(100)
        .unwrap()
        .events;
    let merged = after
        .iter()
        .position(|event| event.calls.len() == 2)
        .unwrap();
    assert_ne!(before[merged].source, after[merged].source);
    assert_eq!(before[merged].calls[0].id, after[merged].calls[1].id);
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![session], false);
    view.event_session = view.selected_id().cloned();
    view.set_event(before.clone());
    (dir, view, before, after, merged)
}

fn select(view: &mut View, index: usize) {
    view.active_panel = Panel::EventList;
    view.show_newest_event();
    draw(view);
    for _ in index + 1..view.matching_event_count() {
        view.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    assert_eq!(view.event_list.cursor, Some(index));
}

fn draw(view: &View) {
    let area = Rect::new(0, 1, 100, 4);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_event(area, &mut buffer, Utc::now());
    assert!(!view.viewport.borrow().event_rows.is_empty());
}

#[test]
fn merged_message_restores_selection_and_scroll_after_earlier_member() {
    let (_dir, mut view, _before, after, merged) = fixture();
    select(&mut view, merged);
    view.event_list.scroll.set(merged);
    view.park_event_list();
    view.event_panel = after;
    view.prepare_event_search();
    view.restore_event_list();
    assert_eq!(view.event_list.cursor, Some(merged));
    assert_eq!(view.event_scroll(), merged);
    draw(&view);
    assert_eq!(view.event_list.cursor, Some(merged));
}

#[test]
fn merged_message_tail_counts_appends_after_earlier_member() {
    let (_dir, mut view, before, after, merged) = fixture();
    view.set_event(before[..=merged].to_vec());
    select(&mut view, 0);
    view.event_list.scroll.set(0);
    view.park_event_list();
    let appended = after.len() - merged - 1;
    view.event_panel = after;
    view.prepare_event_search();
    view.restore_event_list();
    assert_eq!(view.event_list.cursor, Some(0));
    assert_eq!(view.event_list.unseen.get(), appended);
    draw(&view);
}

#[test]
fn merged_message_reset_keeps_selection_after_earlier_member() {
    let (_dir, mut view, _before, after, merged) = fixture();
    select(&mut view, merged);
    draw(&view);
    view.apply_event_update(crate::event::Update {
        events: after,
        reset: true,
        ..Default::default()
    });
    assert_eq!(view.event_list.cursor, Some(merged));
    draw(&view);
    assert_eq!(view.visible_event().unwrap().calls.len(), 2);
}
