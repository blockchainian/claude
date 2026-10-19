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
    assert_eq!(view.active_panel, Panel::Details);
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
fn changed_title_keyboard_blur_saves_and_closes_editor() {
    for width in [100, 200] {
        let (_directory, mut view, id, _original) = editing_title(width);
        view.rename.set_text("  Saved by keyboard  ");
        assert_eq!(
            leave_title_with_keyboard(&mut view),
            Some(Action::Rename {
                id,
                name: "Saved by keyboard".into(),
            })
        );
        assert_eq!(view.active_panel, Panel::TaskList);
        assert!(view.rename_target.is_none());
        assert_eq!(view.take_pending_action(), None);
    }
}

#[test]
fn changed_title_mouse_down_saves_and_closes_editor() {
    for width in [100, 200] {
        let (_directory, mut view, id, _original) = editing_title(width);
        view.rename.set_text("  Saved by mouse  ");
        assert_eq!(
            leave_title_with_mouse(&mut view),
            Some(Action::Rename {
                id,
                name: "Saved by mouse".into(),
            })
        );
        assert_eq!(view.active_panel, Panel::Prompt);
        assert!(view.rename_target.is_none());
        assert_eq!(view.take_pending_action(), None);
    }
}

#[test]
fn changed_title_automatic_panel_change_closes_editor() {
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
                view.set_active_panel(Panel::Output);
            }
            assert_eq!(view.active_panel, Panel::Output);
            assert!(view.rename_target.is_none());
            assert_eq!(
                view.take_pending_action(),
                Some(Action::Rename {
                    id,
                    name: "Saved automatically".into(),
                })
            );
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
fn unchanged_title_blur_closes_without_provider_request() {
    for width in [100, 200] {
        for route in ["keyboard", "mouse", "automatic"] {
            let (_directory, mut view, _id, original) = editing_title(width);
            view.rename.set_text(&format!("  {}  ", original.trim()));
            let action = match route {
                "keyboard" => leave_title_with_keyboard(&mut view),
                "mouse" => leave_title_with_mouse(&mut view),
                "automatic" => {
                    view.set_active_panel(Panel::Output);
                    None
                }
                _ => unreachable!(),
            };
            assert_eq!(action, None, "{route}, width={width}");
            assert!(view.rename_target.is_none(), "{route}, width={width}");
            assert_eq!(view.take_pending_action(), None, "{route}, width={width}");
        }
    }
}

#[test]
fn activating_details_again_keeps_title_editing() {
    for width in [100, 200] {
        let (_directory, mut view, id, _original) = editing_title(width);
        view.rename.set_text("Still editing");
        view.set_active_panel(Panel::Details);
        assert_eq!(view.rename_target.as_ref(), Some(&id));
        assert_eq!(view.rename.text(), "Still editing");
        assert_eq!(view.take_pending_action(), None);
    }
}
