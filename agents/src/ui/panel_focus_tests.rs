use super::*;

fn at(view: &mut View, panel: usize, kind: MouseEventKind) {
    screen(view, 200, 45);
    let rect = {
        let viewport = view.viewport.borrow();
        [
            viewport.list,
            viewport.log,
            viewport.log_detail,
            viewport.details,
            viewport.prompt,
        ][panel]
    };
    assert!(!rect.is_empty());
    view.handle_mouse(mouse(kind, rect.x, rect.y));
}

fn editing(view: &View) -> [bool; 3] {
    [
        view.searching,
        view.log_search.editing,
        view.log_detail_search.editing,
    ]
}

#[test]
fn panel_focus_slash_tracks_last_mouse_event() {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::ScrollDown,
        MouseEventKind::ScrollUp,
        MouseEventKind::Moved,
        MouseEventKind::Down(MouseButton::Right),
    ] {
        for panel in 0..5 {
            let (mut view, _) = log_fixture();
            at(&mut view, (panel + 1) % 5, kind);
            at(&mut view, panel, kind);
            key(&mut view, KeyCode::Char('/'));
            assert_eq!(
                editing(&view),
                [panel == 0, panel == 1, panel == 2],
                "{kind:?}, {panel}"
            );
        }
    }
}

#[test]
fn panel_focus_startup_and_keyboard() {
    let mut view = fixture();
    key(&mut view, KeyCode::Char('/'));
    assert!(view.searching);
    let (mut view, _) = log_fixture();
    at(&mut view, 2, MouseEventKind::Down(MouseButton::Left));
    key(&mut view, KeyCode::Down);
    key(&mut view, KeyCode::Char('/'));
    assert!(view.log_detail_search.editing);
}

#[test]
fn panel_focus_mouse_in_another_panel_cancels_search_like_escape() {
    for panel in 0..3 {
        for destination in 0..5 {
            let (mut view, _) = log_fixture();
            at(&mut view, panel, MouseEventKind::Down(MouseButton::Left));
            key(&mut view, KeyCode::Char('/'));
            key(&mut view, KeyCode::Char('a'));
            at(
                &mut view,
                destination,
                MouseEventKind::Down(MouseButton::Left),
            );
            assert_eq!(
                editing(&view),
                [
                    panel == 0 && destination == panel,
                    panel == 1 && destination == panel,
                    panel == 2 && destination == panel
                ]
            );
            if panel != destination {
                assert!(view.search.is_empty());
                assert!(view.log_search.query.is_empty());
                assert!(view.log_detail_search.query.is_empty());
                key(&mut view, KeyCode::Char('/'));
                assert_eq!(
                    editing(&view),
                    [destination == 0, destination == 1, destination == 2]
                );
            } else {
                key(&mut view, KeyCode::Char('/'));
                let query = match panel {
                    0 => view.search.text(),
                    1 => view.log_search.query.text(),
                    _ => view.log_detail_search.query.text(),
                };
                assert_eq!(query, "a/");
            }
        }
    }
}

#[test]
fn panel_focus_slash_does_not_type_into_other_editors() {
    for panel in 1..5 {
        let (mut view, _) = log_fixture();
        view.input_box.focused = true;
        at(&mut view, panel, MouseEventKind::ScrollUp);
        key(&mut view, KeyCode::Char('/'));
        assert_eq!(editing(&view), [false, panel == 1, panel == 2]);
        assert!(view.input_box.editor.is_empty());
    }
}

#[test]
fn panel_focus_ctrl_r_never_opens_search() {
    for panel in 0..5 {
        let (mut view, _) = log_fixture();
        at(&mut view, panel, MouseEventKind::Down(MouseButton::Left));
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(editing(&view), [false; 3]);
    }
}
