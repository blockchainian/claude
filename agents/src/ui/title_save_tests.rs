use super::*;

fn editing_title(width: u16) -> (tempfile::TempDir, View, SessionKey, String) {
    let (directory, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let id = view.selected_id().unwrap().clone();
    let original = view.selected_row().unwrap().display_title().to_owned();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.rename_target.as_ref(), Some(&id));
    assert_eq!(view.active_panel, Panel::TaskDetails);
    let area = Rect::new(0, 0, width, 50);
    view.render(area, &mut Buffer::empty(area));
    (directory, view, id, original)
}

fn leave_title_with_keyboard(view: &mut View) -> Option<Action> {
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        None
    );
    view.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
}

fn leave_title_with_mouse(view: &mut View) -> Option<Action> {
    let prompt = view.viewport.borrow().prompt;
    assert!(!prompt.is_empty());
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: prompt.x + 2,
        row: prompt.y,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn changed_title_keyboard_blur_parks_editor() {
    for width in [100, 200] {
        let (_directory, mut view, _id, _original) = editing_title(width);
        view.rename.set_text("  Saved by keyboard  ");
        assert_eq!(leave_title_with_keyboard(&mut view), None);
        assert_eq!(view.active_panel, Panel::TaskList);
        assert!(view.rename_target.is_some());
        assert_eq!(view.take_pending_action(), None);
    }
}

#[test]
fn changed_title_mouse_down_parks_editor() {
    for width in [100, 200] {
        let (_directory, mut view, _id, _original) = editing_title(width);
        view.rename.set_text("  Saved by mouse  ");
        assert_eq!(leave_title_with_mouse(&mut view), None);
        assert_eq!(view.active_panel, Panel::Prompt);
        assert!(view.rename_target.is_some());
        assert_eq!(view.take_pending_action(), None);
    }
}

#[test]
fn changed_title_automatic_panel_change_parks_editor() {
    for width in [100, 200] {
        for terminal in [false, true] {
            let (_directory, mut view, id, _original) = editing_title(width);
            view.rename.set_text("  Saved automatically  ");
            if terminal {
                view.show_terminal(
                    id.clone(),
                    std::sync::Arc::new(std::sync::Mutex::new(
                        crate::session_terminal::TerminalScreen::new(10, 80),
                    )),
                );
            } else {
                view.set_active_panel(Panel::EventDetail);
            }
            assert_eq!(view.active_panel, Panel::EventDetail);
            assert!(view.rename_target.is_some());
            assert_eq!(view.take_pending_action(), None);
        }
    }
}

#[test]
fn unchanged_title_enter_closes_without_provider_request() {
    for width in [100, 200] {
        for padded in [false, true] {
            let (_directory, mut view, _id, original) = editing_title(width);
            if padded {
                view.rename.set_text(&format!("  {}  ", original.trim()));
            }
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                None
            );
            assert!(view.rename_target.is_none());
            assert_eq!(view.take_pending_action(), None);
        }
    }
}

#[test]
fn unchanged_title_blur_parks_without_provider_request() {
    for width in [100, 200] {
        for route in ["keyboard", "mouse", "automatic"] {
            let (_directory, mut view, _id, original) = editing_title(width);
            view.rename.set_text(&format!("  {}  ", original.trim()));
            let action = match route {
                "keyboard" => leave_title_with_keyboard(&mut view),
                "mouse" => leave_title_with_mouse(&mut view),
                "automatic" => {
                    view.set_active_panel(Panel::EventDetail);
                    None
                }
                _ => unreachable!(),
            };
            assert_eq!(action, None, "{route}, width={width}");
            assert!(view.rename_target.is_some(), "{route}, width={width}");
            assert_eq!(view.take_pending_action(), None, "{route}, width={width}");
        }
    }
}

#[test]
fn activating_task_details_again_keeps_title_editing() {
    for width in [100, 200] {
        let (_directory, mut view, id, _original) = editing_title(width);
        view.rename.set_text("Still editing");
        view.set_active_panel(Panel::TaskDetails);
        assert_eq!(view.rename_target.as_ref(), Some(&id));
        assert_eq!(view.rename.text(), "Still editing");
        assert_eq!(view.take_pending_action(), None);
    }
}

#[test]
fn task_title_and_focus_park_across_task_switches() {
    let (_directory, mut view, id, _) = editing_title(200);
    view.rename.set_text("Half title");
    view.rename
        .handle(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let caret = view.rename.window(80).1;
    view.set_active_panel(Panel::TaskList);
    assert_eq!(view.take_pending_action(), None);
    let other = view.rows.iter().position(|row| row.id != id).unwrap();
    view.select(other);
    assert_eq!(view.active_panel, Panel::TaskList);
    view.set_active_panel(Panel::EventDetail);
    let original = view.rows.iter().position(|row| row.id == id).unwrap();
    view.select(original);
    assert_eq!(view.active_panel, Panel::TaskDetails);
    assert_eq!(view.rename.text(), "Half title");
    assert_eq!(view.rename.window(80).1, caret);
    assert!(view.title_editor().is_some());
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::Rename {
            id,
            name: "Half title".into()
        })
    );
    assert!(view.title_editor().is_none());
}

#[test]
fn parked_title_escape_discards_without_rename() {
    let (_directory, mut view, _, original) = editing_title(200);
    view.rename.set_text("Unsubmitted title");
    view.set_active_panel(Panel::Prompt);
    assert_eq!(view.take_pending_action(), None);
    assert!(view.title_editor().is_some());
    view.set_active_panel(Panel::TaskDetails);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
    assert!(view.title_editor().is_none());
    assert_eq!(view.selected_row().unwrap().display_title(), original);
}

#[test]
fn task_list_navigation_preserves_each_task_focus() {
    let (_dir, mut view, first, _) = editing_title(200);
    let second = view
        .rows
        .iter()
        .find(|row| row.id != first)
        .unwrap()
        .id
        .clone();
    view.rename.set_text("parked");
    view.set_active_panel(Panel::TaskList);
    view.select(view.rows.iter().position(|row| row.id == second).unwrap());
    assert_eq!(view.active_panel, Panel::TaskList);
    view.set_active_panel(Panel::EventDetail);
    view.set_active_panel(Panel::TaskList);
    view.select(view.rows.iter().position(|row| row.id == first).unwrap());
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(view.task_focus(), task_ui::Focus::Title);
    view.set_active_panel(Panel::TaskDetails);
    assert_eq!(view.rename.text(), "parked");
    view.select(view.rows.iter().position(|row| row.id == second).unwrap());
    assert_eq!(view.active_panel, Panel::EventDetail);
}

#[test]
fn attention_returns_to_parked_title_without_submitting() {
    for from_list in [false, true] {
        let (_dir, mut view, first, _) = editing_title(200);
        view.rename.set_text("interrupted title");
        if from_list {
            view.set_active_panel(Panel::TaskList);
            view.handle_key(KeyCode::Char('/').into());
            assert!(view.searching);
        }
        let mut rows = view.rows.clone();
        let second = rows.iter_mut().find(|row| row.id != first).unwrap();
        second.status = SessionStatus::NeedsInput;
        let second_key = second.id.clone();
        view.set_sessions(rows, false);
        assert_eq!(view.selected_id(), Some(&second_key));
        assert_eq!(view.active_panel, Panel::Prompt);
        assert!(!view.searching);
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|row| row.id == second_key)
            .unwrap()
            .status = SessionStatus::Ready;
        view.set_sessions(rows, false);
        assert_eq!(view.selected_id(), Some(&first));
        assert_eq!(view.active_panel, Panel::TaskDetails);
        assert_eq!(view.rename.text(), "interrupted title");
        assert_eq!(view.take_pending_action(), None);
    }
}

fn key(view: &mut View, code: KeyCode) -> Option<Action> {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn click_task_row(view: &mut View, id: &SessionKey) {
    let area = Rect::new(0, 0, 200, 50);
    view.render(area, &mut Buffer::empty(area));
    let entry = view
        .center_rows(&view.selectable_indices(), view.grouping)
        .iter()
        .position(|row| matches!(row, super::navigation::CenterRow::Task(index) if &view.rows[*index].id == id))
        .unwrap();
    let viewport = view.viewport.borrow();
    let x = viewport.list_rows.x + 2;
    let y = viewport.list_rows.y + (entry - viewport.first_entry) as u16;
    drop(viewport);
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        assert_eq!(view.handle_mouse(mouse(kind, x, y)), None);
    }
}

#[test]
fn task_return_routes_restore_parked_title_and_caret() {
    for route in [
        "mouse", "previous", "tab", "backtab", "enter", "right", "down", "details", "prompt",
        "events", "detail",
    ] {
        let (_dir, mut view, first, _) = editing_title(200);
        view.rename.set_text("AlphaBeta");
        for _ in 0..4 {
            key(&mut view, KeyCode::Left);
        }
        let caret = view.rename.window(80).1;
        let second = view
            .rows
            .iter()
            .find(|row| row.id != first)
            .unwrap()
            .id
            .clone();
        view.set_active_panel(Panel::TaskList);
        view.select(view.rows.iter().position(|row| row.id == second).unwrap());
        if route == "mouse" {
            click_task_row(&mut view, &first);
            assert_eq!(view.active_panel, Panel::TaskList);
            assert_eq!(key(&mut view, KeyCode::Enter), None);
        } else if route == "previous" {
            assert_eq!(key(&mut view, KeyCode::Esc), None);
        } else {
            view.select(view.rows.iter().position(|row| row.id == first).unwrap());
            assert_eq!(view.active_panel, Panel::TaskList);
            let area = Rect::new(0, 0, 200, 50);
            view.render(area, &mut Buffer::empty(area));
            match route {
                "tab" | "backtab" => {
                    let filter = view.status_filter;
                    let code = if route == "tab" {
                        KeyCode::Tab
                    } else {
                        KeyCode::BackTab
                    };
                    assert_eq!(key(&mut view, code), None);
                    assert_eq!(view.status_filter, 1 - filter, "{route}");
                    assert_eq!(view.active_panel, Panel::TaskList, "{route}");
                    assert_eq!(key(&mut view, code), None);
                    assert_eq!(view.status_filter, filter, "{route}");
                    assert_eq!(view.active_panel, Panel::TaskList, "{route}");
                    assert_eq!(key(&mut view, KeyCode::Enter), None);
                }
                "enter" => {
                    assert_eq!(key(&mut view, KeyCode::Enter), None);
                }
                "right" | "down" => {
                    view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                    key(
                        &mut view,
                        KeyCode::Char(if route == "right" { 'l' } else { 'j' }),
                    );
                }
                _ => {
                    let viewport = view.viewport.borrow();
                    let rect = match route {
                        "details" => viewport.task_details,
                        "prompt" => viewport.prompt,
                        "events" => viewport.event_panel,
                        "detail" => viewport.event_detail,
                        _ => unreachable!(),
                    };
                    let x = rect.x + 3;
                    let y = rect.y + 3.min(rect.height.saturating_sub(1));
                    drop(viewport);
                    for kind in [
                        MouseEventKind::Down(MouseButton::Left),
                        MouseEventKind::Up(MouseButton::Left),
                    ] {
                        assert_eq!(view.handle_mouse(mouse(kind, x, y)), None);
                    }
                }
            }
        }
        assert_eq!(view.selected_id(), Some(&first), "{route}");
        let expected_panel = match route {
            "previous" => Panel::TaskList,
            "down" | "prompt" => Panel::Prompt,
            "events" => Panel::EventList,
            "detail" => Panel::EventDetail,
            _ => Panel::TaskDetails,
        };
        assert_eq!(view.active_panel, expected_panel, "{route}");
        assert_eq!(view.rename.text(), "AlphaBeta", "{route}");
        assert_eq!(view.rename.window(80).1, caret, "{route}");
        view.set_active_panel(Panel::TaskDetails);
        assert_eq!(key(&mut view, KeyCode::Char('X')), None);
        assert_eq!(view.rename.text(), "AlphaXBeta", "{route}");
    }
}

fn revisited_prompt_task(width: u16) -> (tempfile::TempDir, View, SessionKey) {
    let (directory, mut view) = child_view(ProviderId::Codex, ProviderId::Codex);
    key(&mut view, KeyCode::Home);
    let id = view.selected_id().unwrap().clone();
    let area = Rect::new(0, 0, width, 50);
    view.render(area, &mut Buffer::empty(area));
    view.prepare_prompt();
    assert_eq!(view.active_panel, Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(key(&mut view, KeyCode::Char('j')), None);
    assert_ne!(view.selected_id(), Some(&id));
    assert_eq!(key(&mut view, KeyCode::Char('k')), None);
    assert_eq!(view.selected_id(), Some(&id));
    assert!(view.task_entry_pending);
    assert_eq!(view.task_focus(), task_ui::Focus::Prompt);
    view.render(area, &mut Buffer::empty(area));
    (directory, view, id)
}

fn assert_revisited_task_restores_prompt(code: KeyCode) {
    for width in [100, 200] {
        let (_directory, mut view, id) = revisited_prompt_task(width);
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.selected_id(), Some(&id));
        assert_eq!(view.active_panel, Panel::Prompt);
        assert!(!view.task_entry_pending);
    }
}

#[test]
fn revisited_task_enter_restores_prompt() {
    assert_revisited_task_restores_prompt(KeyCode::Enter);
}

#[test]
fn revisited_task_o_restores_prompt() {
    assert_revisited_task_restores_prompt(KeyCode::Char('o'));
}

#[test]
fn revisited_task_right_restores_prompt() {
    assert_revisited_task_restores_prompt(KeyCode::Right);
}

#[test]
fn revisited_task_row_click_keeps_delete() {
    let (_directory, mut view, id) = revisited_prompt_task(200);
    click_task_row(&mut view, &id);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.confirm_dialog.is_some());
}

#[test]
fn revisited_task_ctrl_w_l_moves_to_adjacent_panel() {
    for width in [100, 200] {
        let (_directory, mut view, _) = revisited_prompt_task(width);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(key(&mut view, KeyCode::Char('l')), None);
        assert_eq!(view.active_panel, Panel::TaskDetails);
    }
}
