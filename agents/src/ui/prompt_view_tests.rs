use super::*;

#[test]
fn task_double_click_focuses_prompt_but_enter_still_opens() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 120, 40, now);
    view.rows[view.selected].process = None;
    let id = view.selected_id().unwrap().clone();
    let (x, y) = view.viewport.borrow().marker.unwrap();
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x + 3, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(view.input_box.focused);
    key(&mut view, KeyCode::Esc);
    assert!(!view.input_box.focused);
    assert_eq!(key(&mut view, KeyCode::Enter), Some(Action::Open(id)));
}

#[test]
fn focused_input_pastes_and_consumes_task_shortcuts() {
    let (mut view, now) = log_fixture();
    view.rows[view.selected].process = None;
    let rendered = log_screen(&view, 120, 40, now);
    let input = view.center_layout(Rect::new(0, 0, 120, 40)).prompt;
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            input.x + 3,
            input.y,
        ),
    );
    assert!(view.input_box.focused, "{rendered}");
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
fn right_click_leaves_input_before_closing_log_detail() {
    let (mut view, now) = log_fixture();
    view.set_log_open(Some(view.log[0].clone()));
    let expected = view.log_open.clone();
    log_screen(&view, 120, 40, now);
    view.input_box.focused = true;
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), 0, 0));
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), 0, 0));
    assert!(!view.input_box.focused);
    assert_eq!(view.log_open, expected);
}

#[test]
fn whole_log_row_double_click_focuses_input_without_opening_detail() {
    let (mut view, now) = log_fixture();
    view.rows[view.selected].process = None;
    log_screen(&view, 140, 40, now);
    let (x, y) = {
        let viewport = view.viewport.borrow();
        let row = viewport
            .log_rows
            .iter()
            .position(|(index, cut)| !cut && !log::shows_calls(&view.log[*index]))
            .unwrap();
        (
            viewport.log.x + 11,
            viewport.log.y + log::LOG_TOP + row as u16,
        )
    };
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert!(view.log_open.is_none());
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(view.input_box.focused);
    assert!(view.log_open.is_none());
}

#[test]
fn completed_input_updates_its_task_while_another_task_is_selected() {
    let mut view = fixture();
    let first = view.selected;
    let first_id = view.selected_id().unwrap().clone();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.input_box.editor.set_text(text);
    view.input_sending(&first_id);
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
    assert_eq!(view.input_box.state, prompt_box::SendState::Sent);
    assert!(view.input_box.editor.is_empty());
}

#[test]
fn input_completion_keeps_text_edited_during_the_send() {
    let mut view = fixture();
    let id = view.selected_id().unwrap().clone();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.input_box.editor.set_text(text);
    view.input_sending(&id);
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
