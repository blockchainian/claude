use super::*;
use crate::providers::Provider;

fn fixture() -> (tempfile::TempDir, View, Vec<crate::log::Event>) {
    let (dir, view, events) =
        transcript_view(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
    let events: Vec<_> = events.iter().cycle().take(24).cloned().collect();
    assert!(events.len() > 10);
    (dir, view, events)
}

/// Shows one Claude transcript as the selected inactive task, with its log events.
fn transcript_view(transcript: &str) -> (tempfile::TempDir, View, Vec<crate::log::Event>) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, transcript).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.log_session = view.selected_id().cloned();
    let events = crate::providers::claude::log::tail_session(&path, usize::MAX, true).unwrap();
    (dir, view, events)
}

fn draw(view: &View) {
    let area = Rect::new(0, 1, 100, 4);
    view.viewport.borrow_mut().log = area;
    view.render_log(
        area,
        &mut Buffer::empty(Rect::new(0, 0, 100, 6)),
        Utc::now(),
    );
}

#[test]
fn chronological_rows_follow_appends_at_bottom() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..8].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().log_rows, vec![4, 5, 6, 7]);
    assert_eq!(view.log_scroll(), 4);
    view.set_log(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().log_rows, vec![6, 7, 8, 9]);
    assert_eq!(view.log_selected, Some(9));
    assert_eq!(view.viewport.borrow().log_unseen, 0);
}

#[test]
fn sending_a_prompt_returns_the_log_to_live_tail() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..8].to_vec());
    draw(&view);
    view.navigate_log(KeyCode::Home);
    draw(&view);
    assert!(!view.following_log());
    let id = view.selected_id().unwrap().clone();
    view.input_sending(&id, &crate::providers::SessionInput::Prompt("next".into()));
    draw(&view);
    assert!(view.following_log());
    view.set_log(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().log_rows, vec![6, 7, 8, 9]);
    assert_eq!(view.viewport.borrow().log_unseen, 0);
}

#[test]
fn escape_stopping_a_running_turn_returns_its_prompt_to_the_input() {
    for (status, restored) in [
        (SessionStatus::Working, true),
        (SessionStatus::Ready, false),
    ] {
        let (_dir, mut view, events) =
            transcript_view(include_str!("../../tests/fixtures/claude-start.jsonl"));
        view.set_log(events.clone());
        let id = view.selected_id().unwrap().clone();
        view.set_capabilities(HashMap::from([(
            ProviderId::Claude,
            crate::providers::Capabilities {
                stop: true,
                ..Default::default()
            },
        )]));
        view.rows[view.selected].status = status;
        // The fixture shows the Inactive tab; a live task belongs on Active.
        view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(view.selected_id(), Some(&id));
        view.focus_input();
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Stop(id.clone()))
        );
        assert!(view.input_box.editor.is_empty());
        view.stop_complete(&id, true);
        let prompt = events
            .iter()
            .rev()
            .find(|event| event.kind == crate::log::Kind::User)
            .unwrap()
            .body
            .clone();
        let expected = if restored { prompt.as_str() } else { "" };
        assert_eq!(view.input_box.editor.text(), expected, "{status:?}");
    }
}

#[test]
fn chronological_appends_preserve_older_view_and_count_new_rows() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..8].to_vec());
    draw(&view);
    view.navigate_log(KeyCode::Home);
    draw(&view);
    let rows = view.viewport.borrow().log_rows.clone();
    view.set_log(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().log_rows, rows);
    assert_eq!(view.log_scroll(), 0);
    assert_eq!(view.log_selected, Some(0));
    assert_eq!(view.viewport.borrow().log_unseen, 2);
    let area = Rect::new(0, 5, 100, 1);
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_log_jump(area, &mut buf);
    let label: String = (0..100).map(|x| buf[(x, 5)].symbol()).collect();
    assert!(label.contains("↓ 2 New"), "{label}");
    view.show_newest_log();
    draw(&view);
    assert_eq!(view.viewport.borrow().log_rows.last(), Some(&9));
    assert_eq!(view.viewport.borrow().log_unseen, 0);
}

#[test]
fn chronological_jump_only_shows_and_accepts_clicks_for_new_events() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..8].to_vec());
    view.navigate_log(KeyCode::Home);
    let area = Rect::new(0, 0, 100, 45);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, Utc::now());
    let border = view.viewport.borrow().log_borders[1];
    let former = super::log::log_content_centered(border, " ↓ Latest ".len() as u16);
    let border_text: String = (border.x..border.right())
        .map(|x| buf[(x, border.y)].symbol())
        .collect();
    assert!(!border_text.contains("Latest"), "{border_text}");
    assert!(view.viewport.borrow().log_jump.is_empty());
    let click = |view: &mut View, target: Rect| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            view.handle_mouse(MouseEvent {
                kind,
                column: target.x + 2,
                row: target.y,
                modifiers: KeyModifiers::NONE,
            });
        }
    };
    click(&mut view, former);
    assert_eq!(view.log_selected, Some(0));
    view.set_log(events[..10].to_vec());
    view.render_at(area, &mut buf, Utc::now());
    let jump = view.viewport.borrow().log_jump;
    assert!(!jump.is_empty());
    let label: String = (jump.x..jump.right())
        .map(|x| buf[(x, jump.y)].symbol())
        .collect();
    assert_eq!(label, " ↓ 2 New ");
    view.pending_click = None;
    click(&mut view, jump);
    assert_eq!(view.log_selected, Some(9));
    assert!(view.following_log());
    view.render_at(area, &mut buf, Utc::now());
    assert!(view.viewport.borrow().log_jump.is_empty());
    view.navigate_log(KeyCode::Home);
    view.render_at(area, &mut buf, Utc::now());
    assert!(view.viewport.borrow().log_jump.is_empty());
}

#[test]
fn chronological_prepend_anchors_rows_and_selection() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[3..10].to_vec());
    draw(&view);
    view.navigate_log(KeyCode::Home);
    view.navigate_log(KeyCode::Down);
    draw(&view);
    let selected = view.visible_log_event().unwrap().clone();
    view.apply_log_update(crate::log::Update {
        events: events[..10].to_vec(),
        prepended: 3,
        ..Default::default()
    });
    draw(&view);
    assert_eq!(view.log_scroll(), 3);
    assert_eq!(view.viewport.borrow().log_rows, vec![3, 4, 5, 6]);
    assert_eq!(view.visible_log_event(), Some(&selected));
    assert_eq!(view.log_selected, Some(4));
    assert_eq!(view.viewport.borrow().log_unseen, 0);
}

#[test]
fn chronological_keys_move_in_screen_direction_and_page_at_top() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..10].to_vec());
    draw(&view);
    view.navigate_log(KeyCode::Home);
    view.navigate_log(KeyCode::Down);
    assert_eq!(view.log_selected, Some(1));
    view.navigate_log(KeyCode::PageDown);
    assert_eq!(view.log_selected, Some(5));
    view.navigate_log(KeyCode::PageUp);
    assert_eq!(view.log_selected, Some(1));
    view.navigate_log(KeyCode::Up);
    view.navigate_log(KeyCode::Up);
    assert!(view.log_page_pending());
    view.navigate_log(KeyCode::End);
    assert_eq!(view.log_selected, Some(9));
    assert!(!view.log_page_pending());
}

#[test]
fn chronological_border_double_clicks_select_their_screen_end() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..10].to_vec());
    draw(&view);
    view.viewport.borrow_mut().log_borders = [Rect::new(0, 0, 100, 1), Rect::new(0, 5, 100, 1)];
    for (row, selected) in [(0, 0), (5, 9)] {
        view.pending_click = None;
        for _ in 0..2 {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                view.handle_mouse(MouseEvent {
                    kind,
                    column: 1,
                    row,
                    modifiers: KeyModifiers::NONE,
                });
            }
        }
        assert_eq!(view.log_selected, Some(selected));
    }
}

#[test]
fn chronological_direct_render_uses_its_geometry_and_clamps_without_selection() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..8].to_vec());
    view.log_selected = None;
    assert!(view.viewport.borrow().log.is_empty());
    let area = Rect::new(3, 2, 100, 4);
    let mut buf = Buffer::empty(Rect::new(0, 0, 110, 10));
    view.render_log(area, &mut buf, Utc::now());
    assert_eq!(view.viewport.borrow().log, area);
    assert_eq!(view.log_scroll(), 4);
    assert_eq!(view.viewport.borrow().log_rows, vec![4, 5, 6, 7]);
    let area = Rect::new(3, 2, 100, 8);
    view.render_log(area, &mut buf, Utc::now());
    assert_eq!(view.log_scroll(), 0);
    assert_eq!(view.viewport.borrow().log_rows, (0..8).collect::<Vec<_>>());
}

#[test]
fn chronological_history_request_covers_queued_steps_beyond_a_page() {
    let (_dir, mut view, events) = fixture();
    view.set_log(events[..10].to_vec());
    draw(&view);
    view.navigate_log(KeyCode::Home);
    for _ in 0..12 {
        view.navigate_log(KeyCode::Up);
    }
    assert!(view.log_read_count(4, true) >= 22);
    assert_eq!(view.log_read_count(4, false), 10);
    view.apply_log_update(crate::log::Update {
        events: events[..22].to_vec(),
        prepended: 12,
        ..Default::default()
    });
    assert_eq!(view.log_selected, Some(0));
    assert!(!view.log_page_pending());
}

#[test]
fn chronological_new_border_aligns_with_time_and_preserves_model_targets() {
    use unicode_width::UnicodeWidthStr;
    let (_dir, template, events) = fixture();
    for width in [60, 80, 100, 149, 150, 200] {
        for unseen in [3, 100_000_000] {
            let mut view = View::new(Grouping::Project, false);
            view.set_sessions(template.rows.clone(), false);
            view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            view.log_session = view.selected_id().cloned();
            view.set_log(events.clone());
            view.select_log_event(Some(3));
            view.rows[view.selected].model =
                Some("provider/model-with-a-very-long-display-name".into());
            view.rows[view.selected].effort = Some("a-very-long-effort-label".into());
            view.viewport.borrow_mut().log_unseen = unseen;
            let area = Rect::new(0, 0, width, 45);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, Utc::now());
            let viewport = view.viewport.borrow();
            let jump = viewport.log_jump;
            let top = viewport.log_borders[0];
            let bottom = viewport.log_borders[1];
            let label = format!(" ↓ {unseen} New ");
            assert_eq!(jump.right() - 1, viewport.log.right(), "width {width}");
            assert_eq!(jump.width, label.width() as u16);
            assert_eq!(jump.y, if width >= 150 { bottom.y } else { top.y });
            let drawn: String = (jump.x..jump.right())
                .map(|x| buf[(x, jump.y)].symbol())
                .collect();
            assert_eq!(drawn, label, "width {width}");
            for field in [Field::Model, Field::Effort] {
                let target = viewport
                    .field_targets
                    .iter()
                    .find(|(_, candidate)| *candidate == field)
                    .expect("visible model/effort target")
                    .0;
                assert!(
                    !target.intersects(jump),
                    "{field:?} overlaps New at width {width}"
                );
            }
            assert_eq!(top.intersects(jump), width < 150);
            let border_click = (top.x, top.y);
            drop(viewport);
            // The plain top border selects oldest; the labeled button still selects latest.
            for _ in 0..2 {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    view.handle_mouse(MouseEvent {
                        kind,
                        column: border_click.0,
                        row: border_click.1,
                        modifiers: KeyModifiers::NONE,
                    });
                }
            }
            assert_eq!(view.log_selected, Some(0));
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                view.handle_mouse(MouseEvent {
                    kind,
                    column: jump.x + 1,
                    row: jump.y,
                    modifiers: KeyModifiers::NONE,
                });
            }
            assert_eq!(view.log_selected, view.matching_log_count().checked_sub(1));
            assert_eq!(view.viewport.borrow().log_unseen, 0);
        }
    }
}
