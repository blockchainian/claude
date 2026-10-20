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
        panel.right() - 1
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

#[test]
fn time_padding_keeps_clock_selection_and_brackets_apart() {
    for width in [100, 149, 150, 200] {
        for sort in [EventSort::Time, EventSort::Duration] {
            let (_dir, mut view) = completed_fixture();
            view.event_list.sort = sort;
            view.prepare_event_search();
            view.event_list.cursor = Some(0);
            let area = Rect::new(0, 0, width, 60);
            let mut buffer = Buffer::empty(area);
            view.render_at(area, &mut buffer, Utc::now());
            let panel = view.viewport.borrow().event_panel;
            let y = (panel.y..panel.bottom())
                .find(|&y| buffer[(width - 3, y)].symbol() != " ")
                .expect("visible clock");
            let index = usize::from(y - panel.y);
            view.event_list.cursor = Some(index);
            view.render_at(area, &mut buffer, Utc::now());
            let (_, time) = super::event_list::event_row_areas(panel);
            assert_eq!(time.right(), width - 2, "width={width}");
            assert_ne!(buffer[(time.right() - 1, y)].symbol(), " ");
            assert_eq!(buffer[(width - 2, y)].symbol(), " ");
            if sort == EventSort::Time {
                assert!(["┐", "│", "┘"].contains(&buffer[(width - 1, y)].symbol()));
            }
            assert_eq!(buffer[(width - 2, y)].bg, buffer[(time.right() - 1, y)].bg);
            assert_eq!(
                view.viewport.borrow().event_list_text.area.right(),
                time.right()
            );
            view.begin_text_selection(width - 2, y);
            assert!(
                view.text_selection.borrow().is_none(),
                "padding is not text"
            );
            view.begin_text_selection(time.right() - 2, y);
            assert!(
                view.text_selection.borrow().is_some(),
                "clock remains selectable"
            );
            view.extend_text_selection(width - 1, y);
            view.render_at(area, &mut buffer, Utc::now());
            assert!(
                !buffer[(width - 2, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            );
            assert!(
                buffer[(time.right() - 1, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            );
            let Some(Action::Copy { text, .. }) = view.finish_text_selection(width - 1, y) else {
                panic!("clock copy");
            };
            assert_eq!(text.width(), 2);
            assert!(!text.contains(['┐', '│', '┘']));
        }
    }
}

#[test]
fn child_session_event_list_has_no_turn_brackets() {
    let (_dir, mut view) = completed_fixture();
    let mut parent = view.rows[view.selected].clone();
    parent.id.id = "parent".into();
    view.rows[view.selected].parent = Some(parent.id.clone());
    view.rows.push(parent);
    for provider in [
        crate::providers::ProviderId::Claude,
        crate::providers::ProviderId::Codex,
    ] {
        view.rows[view.selected].id.provider = provider;
        view.event_session = view.selected_id().cloned();
        for width in [100, 200] {
            let buffer = draw(&view, width, 20);
            assert!(!view.viewport.borrow().event_rows.is_empty());
            assert!(
                buffer
                    .content
                    .iter()
                    .all(|cell| !["┐", "│", "┘"].contains(&cell.symbol()))
            );
        }
    }
}
