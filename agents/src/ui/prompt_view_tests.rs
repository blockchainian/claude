use super::*;

#[test]
fn task_double_click_keeps_task_focus_and_enter_opens_prompt() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 120, 40, now);
    view.rows[view.selected].process = None;
    let (x, y) = view.viewport.borrow().marker.unwrap();
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x + 3, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(!view.prompt_focused());
    assert!(view.active_panel == Panel::TaskList);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.prompt_focused());
}

#[test]
fn focused_input_pastes_and_consumes_task_shortcuts() {
    let (mut view, now) = event_fixture();
    view.rows[view.selected].process = None;
    let rendered = event_screen(&view, 120, 40, now);
    let input = view.center_layout(Rect::new(0, 0, 120, 40)).prompt;
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            input.x + 3,
            input.y,
        ),
    );
    assert!(view.prompt_focused(), "{rendered}");
    let pasted = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.handle_paste(pasted);
    assert_eq!(view.input_box.editor.text(), pasted);
    assert_eq!(key(&mut view, KeyCode::Char('n')), None);
    assert!(view.input_box.editor.text().ends_with('n'));
    let before = view.selected_id().cloned();
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(view.selected_id(), before.as_ref());
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        None
    );
    assert!(view.input_box.editor.is_empty());
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(Action::Quit)
    );
}

#[test]
fn right_click_outside_preserves_input_focus_and_event_selection() {
    let (mut view, now) = event_fixture();
    view.select_fixture_event(Some(view.event_panel[0].clone()));
    let expected = view.event_list.cursor;
    event_screen(&view, 120, 40, now);
    view.active_panel = Panel::Prompt;
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), 0, 0));
    assert!(view.prompt_focused());
    assert_eq!(view.event_list.cursor, expected);
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), 0, 0));
    assert!(view.prompt_focused());
    assert_eq!(view.event_list.cursor, expected);
}

#[test]
fn whole_event_row_double_click_keeps_event_focus_and_event_detail() {
    for width in [120, 200] {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].process = None;
        event_screen(&view, width, 60, now);
        let (x, y, expected) = {
            let viewport = view.viewport.borrow();
            let row = view
                .event_panel
                .iter()
                .position(|event| !event_list::shows_calls(event))
                .unwrap();
            assert!(row < viewport.event_rows.len());
            (
                viewport.event_panel.x + 11,
                viewport.event_panel.y + row as u16,
                view.event_panel[row].clone(),
            )
        };
        let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(click_mouse(&mut view, click), None);
        assert_eq!(
            view.flush_pending_click(Instant::now() + view.double_click_interval),
            None
        );
        assert_eq!(view.visible_event(), Some(&expected));
        assert_eq!(click_mouse(&mut view, click), None);
        assert_eq!(click_mouse(&mut view, click), None);
        assert!(!view.prompt_focused());
        assert!(view.active_panel == Panel::EventList);
        assert_eq!(view.visible_event(), Some(&expected));
        event_screen(&view, width, 60, now);
        let viewport = view.viewport.borrow();
        assert!(!viewport.event_rows.is_empty());
        if expected.kind == crate::event::Kind::Tool && expected.calls.is_empty() {
            assert!(viewport.event_text.rows.is_empty());
        } else {
            assert!(!viewport.event_text.rows.is_empty());
        }
    }
}

#[test]
fn successful_input_keeps_prompt_while_another_task_is_selected() {
    let prompt = |view: &View| {
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);
        view.input_box.render(
            area,
            &mut buffer,
            "Task",
            Style::default(),
            None,
            view.prompt_focused(),
        );
        buffer
    };
    let mut view = fixture();
    let before = prompt(&view);
    let first = view.selected;
    let first_id = view.selected_id().unwrap().clone();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.input_box.editor.set_text(text);
    view.input_sending(
        &first_id,
        &crate::providers::SessionInput::Prompt(text.into()),
    );
    let second = view
        .selectable_indices()
        .into_iter()
        .find(|index| *index != first)
        .unwrap();
    view.select(second);
    view.input_box.editor.set_text("another task");
    view.input_complete(
        &first_id,
        &crate::providers::SessionInput::Prompt(text.into()),
        Ok(()),
    );
    assert_eq!(view.input_box.editor.text(), "another task");
    assert_eq!(view.input_box.state, prompt_box::SendState::Ready);
    view.select(first);
    assert!(view.input_box.editor.is_empty());
    assert_eq!(prompt(&view), before);
    assert_eq!(view.input_box.state, prompt_box::SendState::Ready);
}

#[test]
fn input_completion_keeps_text_edited_during_the_send() {
    let mut view = fixture();
    let id = view.selected_id().unwrap().clone();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.input_box.editor.set_text(text);
    view.input_sending(&id, &crate::providers::SessionInput::Prompt(text.into()));
    view.input_box.editor.insert("next");
    let edited = view.input_box.editor.text().to_owned();
    view.input_complete(
        &id,
        &crate::providers::SessionInput::Prompt(text.into()),
        Ok(()),
    );
    assert_eq!(view.input_box.editor.text(), edited);
    view.input_complete(
        &id,
        &crate::providers::SessionInput::Interrupt,
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "attach timed out",
        )),
    );
    assert_eq!(view.input_box.editor.text(), edited);
    assert_eq!(
        view.input_box.state,
        prompt_box::SendState::Failed("attach timed out".into())
    );
}

#[test]
fn failed_prompt_restores_submitted_text_without_losing_the_next_draft() {
    for next in ["", "next draft"] {
        let mut view = fixture();
        let id = view.selected_id().unwrap().clone();
        let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
        view.input_box.editor.set_text(text);
        view.input_sending(&id, &crate::providers::SessionInput::Prompt(text.into()));
        view.input_box.editor.set_text(next);
        view.input_complete(
            &id,
            &crate::providers::SessionInput::Prompt(text.into()),
            Err(std::io::Error::other("attach timed out")),
        );
        let expected = if next.is_empty() {
            text.to_owned()
        } else {
            format!("{text}\n{next}")
        };
        assert_eq!(view.input_box.editor.text(), expected);
        assert_eq!(
            view.feedback.clone().map(|(feedback, _)| feedback),
            Some(footer::Feedback::Error("attach timed out".into()))
        );
        assert_eq!(
            view.input_box.state,
            prompt_box::SendState::Failed("attach timed out".into())
        );
    }
}

#[test]
fn pending_prompt_clears_next_frame_and_blocks_repeated_enter() {
    let mut view = fixture();
    let id = view.selected_id().unwrap().clone();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.active_panel = Panel::Prompt;
    view.input_box.editor.set_text(text);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::SendPrompt {
            id: id.clone(),
            text: text.into()
        })
    );
    view.input_sending(&id, &crate::providers::SessionInput::Prompt(text.into()));
    assert!(view.input_box.editor.is_empty());
    view.input_box.paste(text);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    view.input_complete(
        &id,
        &crate::providers::SessionInput::Prompt(text.into()),
        Ok(()),
    );
    assert_eq!(view.input_box.editor.text(), text);
}

#[test]
fn model_and_effort_submission_keep_prompt_and_block_duplicate_enter() {
    for input in [
        crate::providers::SessionInput::SetModel("claude-opus-5-5".into()),
        crate::providers::SessionInput::SetEffort("medium".into()),
    ] {
        let mut view = fixture();
        let id = view.selected_id().unwrap().clone();
        view.active_panel = Panel::Prompt;
        view.handle_paste(include_str!("../../tests/fixtures/claude-peer-message.txt"));
        let area = Rect::new(0, 0, 120, 5);
        let mut before = Buffer::empty(area);
        view.input_box.render(
            area,
            &mut before,
            "Task",
            Style::default(),
            None,
            view.prompt_focused(),
        );
        view.input_sending(&id, &input);
        let mut during = Buffer::empty(area);
        view.input_box.render(
            area,
            &mut during,
            "Task",
            Style::default(),
            None,
            view.prompt_focused(),
        );
        assert_eq!(during, before);
        assert_eq!(key(&mut view, KeyCode::Enter), None);
        view.input_complete(&id, &input, Ok(()));
        assert!(matches!(
            key(&mut view, KeyCode::Enter),
            Some(Action::SendPrompt { .. })
        ));
    }
}

#[test]
fn prompt_grows_with_its_text_up_to_the_layout_mode_share() {
    let (mut view, now) = event_fixture();
    view.rows[view.selected].process = None;
    view.active_panel = Panel::Prompt;
    let paragraph = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.handle_paste(&paragraph.lines().take(7).collect::<Vec<_>>().join("\n"));
    let area = Rect::new(0, 0, 120, 40);
    let layout = view.center_layout(area);
    let lines = view.input_box.height(area.width - 2, &view.input_label());
    assert!(lines > 5 && lines < 17, "{lines}");
    assert_eq!(layout.prompt.height, lines);
    view.handle_paste(paragraph);
    view.handle_paste(paragraph);
    let layout = view.center_layout(area);
    assert_eq!(layout.prompt.height, (40 - 3 - 2) / 2);
    assert!(layout.top.height >= 6, "{:?}", layout.top);
    assert!(layout.event_detail.height >= 5, "{:?}", layout.event_detail);
    let rendered = event_screen(&view, 120, 40, now);
    assert!(rendered.contains("No reply needed."), "{rendered}");
    // Large mode puts the event list beside the panels, so the prompt may take two thirds.
    let large = view.center_layout(Rect::new(0, 0, 200, 40));
    assert_eq!(large.prompt.height, (40 - 3 - 2) * 2 / 3);
    assert!(large.top.height >= 4, "{:?}", large.top);
    assert!(large.event_detail.height >= 3, "{:?}", large.event_detail);
}

#[test]
fn prompt_paste_and_cursor_require_the_active_prompt_panel() {
    let (mut view, now) = event_fixture();
    view.rows[view.selected].process = None;
    let area = Rect::new(0, 0, 120, 40);
    event_screen(&view, 120, 40, now);
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.handle_paste(text);
    assert!(view.input_box.editor.is_empty());
    assert_eq!(view.cursor_pos(area), None);
    view.set_active_panel(Panel::Prompt);
    view.handle_paste(text);
    assert_eq!(view.input_box.editor.text(), text);
    assert!(view.cursor_pos(area).is_some());
    view.set_active_panel(Panel::TaskList);
    assert_eq!(view.cursor_pos(area), None);
}
