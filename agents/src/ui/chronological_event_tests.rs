use super::*;
use crate::providers::Provider;

fn fixture() -> (tempfile::TempDir, View, Vec<crate::event::Event>) {
    let (dir, view, events) =
        transcript_view(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
    let events: Vec<_> = events.iter().cycle().take(24).cloned().collect();
    assert!(events.len() > 10);
    (dir, view, events)
}

/// Shows one Claude transcript as the selected inactive task, with its events.
fn transcript_view(transcript: &str) -> (tempfile::TempDir, View, Vec<crate::event::Event>) {
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
    (dir, view, events)
}

fn draw(view: &View) {
    let area = Rect::new(0, 1, 100, 4);
    view.viewport.borrow_mut().event_panel = area;
    view.render_event(
        area,
        &mut Buffer::empty(Rect::new(0, 0, 100, 6)),
        Utc::now(),
    );
}

impl View {
    pub(crate) fn assert_event_tail(&self) {
        assert_eq!(
            self.event_selected,
            self.matching_event_count().checked_sub(1)
        );
        assert_eq!(self.event_scroll(), self.event_max_scroll());
        assert!(self.following_event());
    }
}

#[test]
fn duration_toggle_follows_slowest_before_and_after_history_load() {
    for following in [false, true] {
        let (_dir, mut view, mut events) = fixture();
        let total = events.len();
        for (index, event) in events.iter_mut().enumerate() {
            event.kind = crate::event::Kind::Tool;
            event.completed_time = true;
            event.ended_at = Some(event.started_at);
            event.duration = Some(chrono::Duration::seconds((total - index) as i64));
        }
        view.set_event(events[16..].to_vec());
        draw(&view);
        if !following {
            view.navigate_event(KeyCode::Home);
        }
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        view.assert_event_tail();
        assert_eq!(view.visible_event(), Some(&events[16]));
        draw(&view);
        view.assert_event_tail();

        view.apply_event_update(crate::event::Update {
            events: events.clone(),
            prepended: 16,
            ..Default::default()
        });
        view.assert_event_tail();
        assert_eq!(view.visible_event(), Some(&events[0]));
        draw(&view);
        view.assert_event_tail();
    }
}

#[test]
fn chronological_rows_follow_appends_at_bottom() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..8].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().event_rows, vec![4, 5, 6, 7]);
    assert_eq!(view.event_scroll(), 4);
    view.set_event(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().event_rows, vec![6, 7, 8, 9]);
    assert_eq!(view.event_selected, Some(9));
    assert_eq!(view.viewport.borrow().event_unseen, 0);
}

#[test]
fn sending_a_prompt_preserves_the_event_view_through_refresh() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..8].to_vec());
    draw(&view);
    view.select_event(Some(2));
    view.viewport.borrow_mut().event_scroll = 1;
    view.viewport.borrow_mut().event_detail_scroll = 3;
    draw(&view);
    assert!(!view.following_event());
    let id = view.selected_id().unwrap().clone();
    view.prepare_prompt();
    view.handle_paste("next");
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::SendPrompt {
            id: id.clone(),
            text: "next".into()
        })
    );
    let input = crate::providers::SessionInput::Prompt("next".into());
    let task_scroll = view.viewport.borrow().scroll;
    view.input_sending(&id, &input);
    draw(&view);
    assert!(!view.following_event());
    assert_eq!(view.event_selected, Some(2));
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.viewport.borrow().event_detail_scroll, 3);
    view.input_complete(&id, &input, Ok(()));
    let mut rows = view.rows.clone();
    rows[view.selected].status = SessionStatus::Working;
    rows[view.selected].updated_at = Utc::now();
    view.set_sessions(rows, false);
    view.set_event(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.viewport.borrow().scroll, task_scroll);
    assert_eq!(view.event_selected, Some(2));
    assert_eq!(view.visible_event(), Some(&events[2]));
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.viewport.borrow().event_detail_scroll, 3);
    assert_eq!(view.viewport.borrow().event_unseen, 2);
}

#[test]
fn transcript_reset_preserves_the_selected_event() {
    let (_dir, mut view, _) = fixture();
    let path = view.selected_row().unwrap().transcript.clone().unwrap();
    let mut reader = crate::providers::claude::event_reader::Reader::new(path.clone(), true);
    view.apply_event_update(reader.refresh(usize::MAX).unwrap());
    draw(&view);
    assert!(view.event_panel.len() > 4);
    view.select_event(Some(2));
    view.viewport.borrow_mut().event_scroll = 1;
    view.viewport.borrow_mut().event_detail_scroll = 3;
    let selected = view.visible_event().unwrap().clone();
    // Replacing the file causes the real reader to replay the same transcript.
    let replacement = path.with_extension("replacement");
    std::fs::copy(&path, &replacement).unwrap();
    std::fs::rename(replacement, path).unwrap();
    let update = reader.refresh(usize::MAX).unwrap();
    assert!(update.reset);
    view.apply_event_update(update);
    draw(&view);
    assert_eq!(view.visible_event(), Some(&selected));
    assert_eq!(view.event_selected, Some(2));
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.viewport.borrow().event_detail_scroll, 3);
}

#[test]
fn sending_a_prompt_keeps_existing_live_tail() {
    for selection in [None, Some(7)] {
        let (_dir, mut view, events) = fixture();
        view.set_event(events[..8].to_vec());
        draw(&view);
        view.event_selected = selection;
        let id = view.selected_id().unwrap().clone();
        view.input_sending(&id, &crate::providers::SessionInput::Prompt("next".into()));
        view.set_event(events[..10].to_vec());
        draw(&view);
        view.assert_event_tail();
    }
}

#[test]
fn sending_refresh_preserves_task_selection_and_scroll_after_reordering() {
    let (_dir, mut view, _) = fixture();
    let template = view.rows[0].clone();
    let rows = (0..16)
        .map(|i| {
            let mut row = template.clone();
            row.id.id = format!("task-{i}");
            row.status = SessionStatus::Ready;
            row.updated_at = template.updated_at + chrono::Duration::seconds(i);
            row
        })
        .collect();
    view.set_sessions(rows, false);
    let selected = view
        .rows
        .iter()
        .position(|row| row.id.id == "task-8")
        .unwrap();
    view.select(selected);
    let area = Rect::new(0, 0, 80, 5);
    view.render_center_rows(area, &mut Buffer::empty(area), Utc::now());
    let scroll = view.viewport.borrow().scroll;
    assert!(scroll > 0);
    let id = view.selected_id().unwrap().clone();
    view.input_sending(&id, &crate::providers::SessionInput::Prompt("next".into()));
    let mut rows = view.rows.clone();
    rows[view.selected].status = SessionStatus::Working;
    rows[view.selected].updated_at = Utc::now();
    view.set_sessions(rows, false);
    view.render_center_rows(area, &mut Buffer::empty(area), Utc::now());
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.viewport.borrow().scroll, scroll);
}

#[test]
fn sending_to_attention_task_keeps_that_task_when_it_starts_working() {
    let (_dir, mut view, events) = fixture();
    let mut original = view.rows[0].clone();
    original.status = SessionStatus::Ready;
    view.set_sessions(vec![original.clone()], false);
    let mut waiting = original.clone();
    waiting.id.id = "waiting-task".into();
    waiting.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original.clone(), waiting.clone()], false);
    assert_eq!(view.attention_selection.as_ref(), Some(&waiting.id));
    assert_eq!(view.selected_id(), Some(&waiting.id));
    view.set_event(events[..8].to_vec());
    view.select_event(Some(2));
    view.viewport.borrow_mut().event_scroll = 1;
    view.viewport.borrow_mut().event_detail_scroll = 3;
    view.prepare_prompt();
    view.input_sending(
        &waiting.id,
        &crate::providers::SessionInput::Prompt("next".into()),
    );
    waiting.status = SessionStatus::Working;
    view.set_sessions(vec![original, waiting.clone()], false);
    assert_eq!(view.selected_id(), Some(&waiting.id));
    assert_eq!(view.event_selected, Some(2));
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.viewport.borrow().event_detail_scroll, 3);
}

#[test]
fn escape_stopping_a_running_turn_returns_its_prompt_to_the_input() {
    for (status, restored) in [
        (SessionStatus::Working, true),
        (SessionStatus::Ready, false),
    ] {
        let (_dir, mut view, events) =
            transcript_view(include_str!("../../tests/fixtures/claude-start.jsonl"));
        view.set_event(events.clone());
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
        view.set_active_panel(Panel::Prompt);
        view.prepare_prompt();
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Stop(id.clone()))
        );
        assert!(view.input_box.editor.is_empty());
        view.stop_complete(&id, true);
        let prompt = events
            .iter()
            .rev()
            .find(|event| event.kind == crate::event::Kind::User)
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
    view.set_event(events[..8].to_vec());
    draw(&view);
    view.navigate_event(KeyCode::Home);
    draw(&view);
    let rows = view.viewport.borrow().event_rows.clone();
    view.set_event(events[..10].to_vec());
    draw(&view);
    assert_eq!(view.viewport.borrow().event_rows, rows);
    assert_eq!(view.event_scroll(), 0);
    assert_eq!(view.event_selected, Some(0));
    assert_eq!(view.viewport.borrow().event_unseen, 2);
    let area = Rect::new(0, 5, 100, 1);
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_event_jump(area, &mut buf);
    let label: String = (0..100).map(|x| buf[(x, 5)].symbol()).collect();
    assert!(label.contains("↓ 2 New"), "{label}");
    view.show_newest_event();
    draw(&view);
    assert_eq!(view.viewport.borrow().event_rows.last(), Some(&9));
    assert_eq!(view.viewport.borrow().event_unseen, 0);
}

#[test]
fn chronological_jump_only_shows_and_accepts_clicks_for_new_events() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..8].to_vec());
    view.navigate_event(KeyCode::Home);
    let area = Rect::new(0, 0, 100, 45);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, Utc::now());
    let border = view.viewport.borrow().event_borders[1];
    let former = super::event_list::event_content_centered(border, " ↓ Latest ".len() as u16);
    let border_text: String = (border.x..border.right())
        .map(|x| buf[(x, border.y)].symbol())
        .collect();
    assert!(!border_text.contains("Latest"), "{border_text}");
    assert!(view.viewport.borrow().event_jump.is_empty());
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
    assert_eq!(view.event_selected, Some(0));
    view.set_event(events[..10].to_vec());
    view.render_at(area, &mut buf, Utc::now());
    let jump = view.viewport.borrow().event_jump;
    assert!(!jump.is_empty());
    let label: String = (jump.x..jump.right())
        .map(|x| buf[(x, jump.y)].symbol())
        .collect();
    assert_eq!(label, " ↓ 2 New ");
    view.pending_click = None;
    click(&mut view, jump);
    assert_eq!(view.event_selected, Some(9));
    assert!(view.following_event());
    view.render_at(area, &mut buf, Utc::now());
    assert!(view.viewport.borrow().event_jump.is_empty());
    view.navigate_event(KeyCode::Home);
    view.render_at(area, &mut buf, Utc::now());
    assert!(view.viewport.borrow().event_jump.is_empty());
}

#[test]
fn chronological_prepend_anchors_rows_and_selection() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[3..10].to_vec());
    draw(&view);
    view.navigate_event(KeyCode::Home);
    view.navigate_event(KeyCode::Down);
    draw(&view);
    let selected = view.visible_event().unwrap().clone();
    view.apply_event_update(crate::event::Update {
        events: events[..10].to_vec(),
        prepended: 3,
        ..Default::default()
    });
    draw(&view);
    assert_eq!(view.event_scroll(), 3);
    assert_eq!(view.viewport.borrow().event_rows, vec![3, 4, 5, 6]);
    assert_eq!(view.visible_event(), Some(&selected));
    assert_eq!(view.event_selected, Some(4));
    assert_eq!(view.viewport.borrow().event_unseen, 0);
}

#[test]
fn chronological_keys_move_in_screen_direction_and_page_at_top() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..10].to_vec());
    draw(&view);
    view.navigate_event(KeyCode::Home);
    view.navigate_event(KeyCode::Down);
    assert_eq!(view.event_selected, Some(1));
    view.navigate_event(KeyCode::PageDown);
    assert_eq!(view.event_selected, Some(5));
    view.navigate_event(KeyCode::PageUp);
    assert_eq!(view.event_selected, Some(1));
    view.navigate_event(KeyCode::Up);
    view.navigate_event(KeyCode::Up);
    assert!(view.event_page_pending());
    view.navigate_event(KeyCode::End);
    assert_eq!(view.event_selected, Some(9));
    assert!(!view.event_page_pending());
}

#[test]
fn chronological_border_double_clicks_select_their_screen_end() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..10].to_vec());
    draw(&view);
    view.viewport.borrow_mut().event_borders = [Rect::new(0, 0, 100, 1), Rect::new(0, 5, 100, 1)];
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
        assert_eq!(view.event_selected, Some(selected));
    }
}

#[test]
fn chronological_direct_render_uses_its_geometry_and_clamps_without_selection() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..8].to_vec());
    view.event_selected = None;
    assert!(view.viewport.borrow().event_panel.is_empty());
    let area = Rect::new(3, 2, 100, 4);
    let mut buf = Buffer::empty(Rect::new(0, 0, 110, 10));
    view.render_event(area, &mut buf, Utc::now());
    assert_eq!(view.viewport.borrow().event_panel, area);
    assert_eq!(view.event_scroll(), 4);
    assert_eq!(view.viewport.borrow().event_rows, vec![4, 5, 6, 7]);
    let area = Rect::new(3, 2, 100, 8);
    view.render_event(area, &mut buf, Utc::now());
    assert_eq!(view.event_scroll(), 0);
    assert_eq!(
        view.viewport.borrow().event_rows,
        (0..8).collect::<Vec<_>>()
    );
}

#[test]
fn chronological_history_request_covers_queued_steps_beyond_a_page() {
    let (_dir, mut view, events) = fixture();
    view.set_event(events[..10].to_vec());
    draw(&view);
    view.navigate_event(KeyCode::Home);
    for _ in 0..12 {
        view.navigate_event(KeyCode::Up);
    }
    assert!(view.event_read_count(4, true) >= 22);
    assert_eq!(view.event_read_count(4, false), 10);
    view.apply_event_update(crate::event::Update {
        events: events[..22].to_vec(),
        prepended: 12,
        ..Default::default()
    });
    assert_eq!(view.event_selected, Some(0));
    assert!(!view.event_page_pending());
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
            view.event_session = view.selected_id().cloned();
            view.set_event(events.clone());
            view.select_event(Some(3));
            view.rows[view.selected].model =
                Some("provider/model-with-a-very-long-display-name".into());
            view.rows[view.selected].effort = Some("a-very-long-effort-label".into());
            view.viewport.borrow_mut().event_unseen = unseen;
            let area = Rect::new(0, 0, width, 45);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, Utc::now());
            let viewport = view.viewport.borrow();
            let jump = viewport.event_jump;
            let top = viewport.event_borders[0];
            let bottom = viewport.event_borders[1];
            let label = format!(" ↓ {unseen} New ");
            assert_eq!(
                jump.right() - 1,
                viewport.event_panel.right(),
                "width {width}"
            );
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
            assert_eq!(view.event_selected, Some(0));
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
            assert_eq!(
                view.event_selected,
                view.matching_event_count().checked_sub(1)
            );
            assert_eq!(view.viewport.borrow().event_unseen, 0);
        }
    }
}

#[test]
fn duration_updates_preserve_screen_row_and_follow_slowest() {
    let (_dir, mut view, mut events) = fixture();
    for (i, event) in events.iter_mut().enumerate() {
        event.kind = crate::event::Kind::Tool;
        event.completed_time = true;
        event.ended_at = Some(event.started_at);
        event.duration = Some(chrono::Duration::seconds(10 + i as i64));
    }
    view.set_event(events[..8].to_vec());
    view.active_panel = Panel::EventList;
    view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    view.select_event(Some(5));
    view.viewport.borrow_mut().event_scroll = 4;
    draw(&view);
    let screen_row = view.event_selected.unwrap() - view.event_scroll();
    events[8].duration = Some(chrono::Duration::seconds(1));
    view.set_event(events[..9].to_vec());
    draw(&view);
    assert_eq!(
        view.shown_events()
            .nth(view.event_selected.unwrap())
            .unwrap()
            .0,
        5
    );
    assert_eq!(
        view.event_selected.unwrap() - view.event_scroll(),
        screen_row
    );
    let scroll_before = view.event_scroll();
    // A longer duration moves this row below the selected event, reducing the scroll.
    events[8].duration = Some(chrono::Duration::seconds(50));
    view.set_event(events[..9].to_vec());
    assert_eq!(view.event_scroll(), scroll_before - 1);
    draw(&view);
    assert_eq!(
        view.event_selected.unwrap() - view.event_scroll(),
        screen_row
    );
    assert_eq!(view.viewport.borrow().event_unseen, 0);
    let area = Rect::new(0, 5, 100, 1);
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_event_jump(area, &mut buf);
    assert!(view.viewport.borrow().event_jump.is_empty());
    assert!(
        !(0..100)
            .map(|x| buf[(x, 5)].symbol())
            .collect::<String>()
            .contains("New")
    );
    view.show_newest_event();
    events[9].duration = Some(chrono::Duration::seconds(100));
    view.set_event(events[..10].to_vec());
    assert_eq!(view.visible_event(), Some(&events[9]));
    assert!(view.following_event());
    view.apply_event_update(crate::event::Update {
        events: events[..3].to_vec(),
        reset: true,
        ..Default::default()
    });
    assert_eq!(view.event_sort, EventSort::Duration);
    assert_eq!(view.event_order, [0, 1, 2]);
}

#[test]
fn duration_updates_cancel_list_drag_only_when_event_order_changes() {
    for change in ["reorder", "append", "remove", "prepend", "unchanged"] {
        let (_dir, mut view, mut events) = fixture();
        events.truncate(8);
        for (index, event) in events.iter_mut().enumerate() {
            event.kind = crate::event::Kind::Tool;
            event.completed_time = true;
            event.ended_at = Some(event.started_at);
            event.duration = Some(chrono::Duration::seconds(10 + index as i64));
        }
        view.set_event(events.clone());
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        view.select_event(Some(0));
        view.viewport.borrow_mut().event_scroll = 0;
        draw(&view);
        view.begin_text_selection(0, 1);
        view.extend_text_selection(99, 2);
        assert!(view.text_selection_dragging());
        let mut prepended = 0;
        match change {
            "reorder" => events.swap(0, 1),
            "append" => events.push(events[0].clone()),
            "remove" => {
                events.pop();
            }
            "prepend" => {
                events.insert(0, events[0].clone());
                prepended = 1;
            }
            "unchanged" => {}
            _ => unreachable!(),
        }
        view.apply_event_update(crate::event::Update {
            events,
            prepended,
            ..Default::default()
        });
        assert_eq!(
            view.text_selection_dragging(),
            change == "unchanged",
            "{change}"
        );
        draw(&view);
        let copied = view.finish_text_selection(99, 2);
        assert_eq!(copied.is_some(), change == "unchanged", "{change}");
    }
}

#[test]
fn duration_columns_and_detail_titles_follow_sort_values() {
    use crate::event::Kind;
    let (_dir, mut view, events) = fixture();
    let now = Utc::now();
    view.rows[view.selected].status = SessionStatus::Working;
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: std::process::id(),
        label: None,
        started_at: None,
    });
    // Keep a real single-call event without shell highlighting in every detail render.
    let template = events
        .iter()
        .find(|event| event.calls.len() == 1 && event.calls[0].name == "WebFetch")
        .unwrap()
        .clone();
    for kind in [
        Kind::User,
        Kind::Assistant,
        Kind::Tool,
        Kind::Error,
        Kind::Denied,
        Kind::Compaction,
        Kind::Agent,
        Kind::Session,
        Kind::QueuedUser,
        Kind::QueuedSystem,
    ] {
        for state in ["settled", "running", "missing", "clock", "recorded"] {
            let mut event = template.clone();
            event.kind = kind.clone();
            event.calls[0].started_at = None;
            event.started_at = now - chrono::Duration::seconds(5);
            event.previous_end = Some(now - chrono::Duration::seconds(12));
            event.ended_at = Some(now - chrono::Duration::seconds(2));
            event.completed_time = false;
            event.background = false;
            if state == "running" {
                event.ended_at = None;
            }
            if state == "missing" {
                event.previous_end = None;
            }
            if state == "clock" {
                event.completed_time = true;
                event.duration = None;
            }
            if state == "recorded" {
                event.completed_time = true;
                event.duration = Some(chrono::Duration::seconds(8));
                event.previous_end = None;
            }
            for sort in [EventSort::Time, EventSort::Duration] {
                view.event_sort = sort;
                if sort == EventSort::Duration {
                    event.calls[0].started_at = Some(event.started_at);
                }
                view.set_event(vec![event.clone()]);
                view.select_event(Some(0));
                view.last_update = now - chrono::Duration::seconds(3);
                let expected = if sort == EventSort::Time {
                    match kind {
                        Kind::Tool | Kind::Error | Kind::Denied => match state {
                            "recorded" => "8s".into(),
                            "clock" => super::formatting::clock_text(event.ended_at.unwrap()),
                            "missing" => super::formatting::clock_text(event.started_at),
                            "running" => "12s".into(),
                            _ => "10s".into(),
                        },
                        _ => "5s ago".into(),
                    }
                } else {
                    match kind {
                        _ if state == "missing" => String::new(),
                        Kind::User | Kind::Assistant if state == "recorded" => String::new(),
                        Kind::User | Kind::Assistant => "7s".into(),
                        Kind::Tool | Kind::Error | Kind::Denied => match state {
                            "recorded" => "8s".into(),
                            "clock" => String::new(),
                            "running" => "12s".into(),
                            _ => "10s".into(),
                        },
                        _ => String::new(),
                    }
                };
                let area = Rect::new(0, 0, 100, 4);
                let mut buf = Buffer::empty(area);
                view.render_event(area, &mut buf, now);
                let time: String = (92..100).map(|x| buf[(x, 0)].symbol()).collect();
                assert_eq!(time.trim(), expected, "{sort:?} {kind:?} {state} list");
                let detail = Rect::new(0, 0, 100, 30);
                let mut buf = Buffer::empty(detail);
                view.render_event_detail(detail, &mut buf, now);
                let time: String = (92..100).map(|x| buf[(x, 1)].symbol()).collect();
                assert_eq!(time.trim(), expected, "{sort:?} {kind:?} {state} detail");
            }
        }
    }
}

#[test]
fn duration_running_rows_redraw_only_while_visible_and_keep_update_order() {
    let (_dir, mut view, mut events) = fixture();
    let now = Utc::now();
    events.truncate(3);
    for event in &mut events {
        event.kind = crate::event::Kind::Tool;
        event.completed_time = false;
        event.background = false;
        event.started_at = now - chrono::Duration::seconds(5);
        event.previous_end = Some(now - chrono::Duration::seconds(10));
        event.ended_at = Some(now - chrono::Duration::seconds(2));
    }
    events[0].ended_at = Some(now - chrono::Duration::seconds(7));
    events[1].previous_end = Some(now - chrono::Duration::seconds(30));
    events[2].ended_at = None;
    view.rows[view.selected].status = SessionStatus::Working;
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: std::process::id(),
        label: None,
        started_at: None,
    });
    view.event_sort = EventSort::Duration;
    view.set_event(events);
    view.select_event(Some(1));
    let area = Rect::new(0, 0, 100, 1);
    let mut buf = Buffer::empty(area);
    view.render_event(area, &mut buf, now);
    let order = view.event_order.clone();
    assert_eq!(
        view.event_redraw_in(now),
        Some(std::time::Duration::from_secs(1))
    );
    view.render_event(area, &mut buf, now + chrono::Duration::seconds(1));
    assert_eq!(view.event_order, order);
    let time: String = (92..100).map(|x| buf[(x, 0)].symbol()).collect();
    assert_eq!(time.trim(), "11s");
    view.select_event(Some(0));
    view.render_event(area, &mut buf, now);
    assert_eq!(view.event_redraw_in(now), None);
}
