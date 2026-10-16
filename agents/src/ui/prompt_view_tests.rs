use super::*;

#[test]
fn input_sits_between_top_panels_and_full_width_log() {
    let (view, _) = log_fixture();
    for width in [120, 90, 80, 40] {
        let layout = view.center_layout(Rect::new(0, 0, width, 40));
        assert!(!layout.list.is_empty());
        assert!(!layout.details.is_empty());
        assert!(layout.list.right() <= layout.details.x);
        assert!(layout.list.bottom() <= layout.prompt.y);
        assert!(layout.details.bottom() <= layout.prompt.y);
        assert_eq!(layout.prompt.height, 1);
        assert_eq!(layout.prompt.bottom(), layout.log.y);
        assert_eq!(layout.log.width, width);
    }
}

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
fn terminal_session_disables_input_with_open_hint() {
    let (mut view, now) = log_fixture();
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/registry.json")).unwrap();
    assert_eq!(registry["kind"], "interactive");
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: std::process::id(),
        label: registry["kind"].as_str().map(str::to_owned),
        started_at: None,
    });
    let rendered = log_screen(&view, 160, 40, now);
    assert!(rendered.contains("Running in a terminal; open it to type"));
    let input = view.center_layout(Rect::new(0, 0, 160, 40)).prompt;
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            input.x + 2,
            input.y,
        ),
    );
    assert!(!view.input_box.focused);
    view.handle_paste("must not type");
    assert!(view.input_box.editor.is_empty());
}

#[test]
fn whole_log_row_double_click_focuses_input_without_opening_detail() {
    let (mut view, now) = log_fixture();
    view.rows[view.selected].process = None;
    log_screen(&view, 160, 40, now);
    let (x, y) = {
        let viewport = view.viewport.borrow();
        let row = viewport
            .log_rows
            .iter()
            .position(|(index, cut)| !cut && !log::shows_calls(&view.log[*index]))
            .unwrap();
        (
            viewport.log.x + 21,
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
fn offset_layout_keeps_prompt_above_log() {
    let (view, _) = log_fixture();
    let layout = view.center_layout(Rect::new(5, 7, 120, 40));
    assert_eq!(layout.prompt.bottom(), layout.log.y);
    assert!(layout.list.bottom() <= layout.prompt.y);
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

#[test]
fn paste_routes_to_focused_text_field() {
    let pasted = include_str!("../../tests/fixtures/claude-peer-message.txt");
    for field in 0..6 {
        let (mut view, _) = log_fixture();
        match field {
            0 => view.input_box.focused = true,
            1 => {
                view.searching = true;
            }
            2 => view.rename_target = view.selected_id().cloned(),
            3 => {
                view.prepare_log_search();
                view.log_search.editing = true;
            }
            4 => {
                view.set_log_open(Some(view.log[0].clone()));
                view.log_detail_search.editing = true;
            }
            5 => {
                view.branch_editor = Some(BranchEditor {
                    draft: 1,
                    editor: crate::ui::line_edit::LineEdit::default(),
                })
            }
            _ => unreachable!(),
        }
        view.handle_paste(pasted);
        let actual = match field {
            0 => view.input_box.editor.text(),
            1 => view.search.text(),
            2 => view.rename.text(),
            3 => view.log_search.query.text(),
            4 => view.log_detail_search.query.text(),
            5 => view.branch_editor.as_ref().unwrap().editor.text(),
            _ => unreachable!(),
        };
        assert_eq!(actual, pasted, "focused field {field}");
    }
}
