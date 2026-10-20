use super::*;
use crate::providers::Provider;

fn transcript_fixture(transcript: &str) -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, transcript).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    let events =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    view.set_event(events);
    (dir, view)
}

fn draw(view: &View, width: u16, height: u16) -> Buffer {
    let area = Rect::new(1, 1, width - 2, height);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height + 2));
    view.render_event(area, &mut buffer, Utc::now());
    buffer
}

#[test]
fn real_prompt_and_response_have_an_open_turn_bracket() {
    let (_dir, view) = transcript_fixture(include_str!("../../tests/fixtures/transcript.jsonl"));
    let buffer = draw(&view, 80, 10);
    let panel = view.viewport.borrow().event_panel;
    assert_eq!(buffer[(panel.right(), panel.y)].symbol(), "┐");
    assert_eq!(buffer[(panel.right(), panel.y + 1)].symbol(), "│");
    assert_eq!(
        view.viewport.borrow().event_list_text.area.right(),
        panel.right()
    );
    assert!(!panel.contains((panel.right(), panel.y).into()));
}

fn completed_fixture() -> (tempfile::TempDir, View) {
    transcript_fixture(include_str!(
        "../../tests/fixtures/turn-groups/claude-steer.jsonl"
    ))
}

fn brackets(view: &View, width: u16, height: u16) -> String {
    let buffer = draw(view, width, height);
    let viewport = view.viewport.borrow();
    (0..viewport.event_rows.len())
        .map(|row| {
            buffer[(
                viewport.event_panel.right(),
                viewport.event_panel.y + row as u16,
            )]
                .symbol()
        })
        .collect()
}

#[test]
fn real_completed_turn_uses_corners_and_keeps_selection_background() {
    let (_dir, view) = completed_fixture();
    for width in [80, 180] {
        let buffer = draw(&view, width, 20);
        let viewport = view.viewport.borrow();
        assert!(viewport.event_rows.len() >= 3);
        assert_eq!(buffer[(width - 1, 1)].symbol(), "┐");
        let last = viewport.event_rows.len() as u16;
        assert_eq!(buffer[(width - 1, last)].symbol(), "┘");
        assert_eq!(buffer[(width - 1, last)].bg, buffer[(width - 2, last)].bg);
        assert_eq!(buffer[(width - 1, last)].fg, ratatui::style::Color::Reset);
        assert!(
            buffer[(width - 1, last)]
                .modifier
                .contains(ratatui::style::Modifier::DIM)
        );
    }
}

#[test]
fn clipped_real_turn_keeps_vertical_lines_at_viewport_edges() {
    let (_dir, mut view) = completed_fixture();
    assert!(view.event_panel.len() >= 3);
    view.event_list.cursor = Some(0);
    assert_eq!(brackets(&view, 80, 2), "┐│");
    view.event_list.cursor = Some(1);
    view.event_list.scroll.set(1);
    let shown = brackets(&view, 80, 1);
    assert_eq!(shown, "│");
    view.event_list.cursor = Some(view.event_panel.len() - 1);
    view.event_list.scroll.set(view.event_panel.len() - 2);
    assert_eq!(brackets(&view, 80, 2), "│┘");
}

#[test]
fn single_real_event_and_duration_sort_have_no_bracket() {
    let (_dir, mut view) = completed_fixture();
    view.set_event(vec![view.event_panel[0].clone()]);
    assert_eq!(brackets(&view, 80, 20), " ");
    let (_dir, mut view) = completed_fixture();
    view.event_list.sort = EventSort::Duration;
    view.prepare_event_search();
    assert!(brackets(&view, 80, 20).chars().all(|symbol| symbol == ' '));
}

#[test]
fn single_task_notification_reply_keeps_its_turn_without_a_bracket() {
    let (_dir, view) = transcript_fixture(include_str!(
        "../../tests/fixtures/turn-groups/claude-wakeup.jsonl"
    ));
    assert!(
        view.event_panel
            .iter()
            .all(|event| event.kind != Kind::User)
    );
    assert_eq!(view.event_panel.len(), 1);
    assert!(
        view.event_panel[0]
            .turn
            .is_some_and(|turn| turn.started && turn.complete)
    );
    assert_eq!(brackets(&view, 80, 20), " ");
}

#[test]
fn search_draws_corners_on_visible_rows_and_leaves_input_unmarked() {
    let (_dir, mut view) = completed_fixture();
    view.event_list.search.query.insert("仪表盘");
    view.prepare_event_search();
    assert!(view.matching_event_count() >= 2);
    assert!(view.matching_event_count() < view.event_panel.len());
    let buffer = draw(&view, 80, 20);
    let viewport = view.viewport.borrow();
    assert_eq!(buffer[(79, 1)].symbol(), "┐");
    assert_eq!(buffer[(79, viewport.event_rows.len() as u16)].symbol(), "┘");
    assert_eq!(buffer[(79, 20)].symbol(), " ");
}
