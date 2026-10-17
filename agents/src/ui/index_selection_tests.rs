use super::*;

#[test]
fn paired_notifications_move_newer_by_index_with_all_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let events = super::super::log::log_markdown::notification_events(dir.path(), None);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].calls[0].id, events[1].calls[0].id);
    let (mut view, now) = log_fixture();
    view.set_log(events.clone());
    log_screen(&view, 200, 45, now);
    let rect = view.viewport.borrow().log;
    for code in [KeyCode::Up, KeyCode::Char('k')] {
        view.select_log_event(Some(0));
        view.log_list_focused = true;
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(view.log_selected, Some(1));
        view.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(view.log_selected, Some(0));
    }
    for _ in 0..12 {
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 1, rect.y));
        assert_eq!(view.log_selected, Some(1));
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 1, rect.y));
        assert_eq!(view.log_selected, Some(0));
    }
}

#[test]
fn structural_updates_keep_selection_and_following() {
    let dir = tempfile::tempdir().unwrap();
    let events = super::super::log::log_markdown::notification_events(dir.path(), None);
    let (mut view, _) = log_fixture();
    view.set_log(events.clone());
    view.select_log_event(Some(0));
    let selected = view.visible_log_event().unwrap().clone();
    let mut older = events.clone();
    older.extend(events.clone());
    view.apply_log_update(crate::log::Update {
        events: older.clone(),
        prepended: 2,
        ..Default::default()
    });
    assert_eq!(view.log_selected, Some(2));
    assert_eq!(view.visible_log_event(), Some(&selected));
    view.apply_log_update(crate::log::Update {
        events: older.clone(),
        replaced: 1,
        ..Default::default()
    });
    assert_eq!(view.log_selected, Some(2));
    let scroll = view.log_scroll();
    older.extend(events.clone());
    view.apply_log_update(crate::log::Update {
        events: older.clone(),
        appended: 2,
        ..Default::default()
    });
    assert_eq!(view.log_selected, Some(2));
    assert_eq!(view.log_scroll(), scroll + 2);
    view.select_log_event(None);
    older.extend(events);
    view.apply_log_update(crate::log::Update {
        events: older,
        appended: 2,
        ..Default::default()
    });
    assert_eq!(view.log_selected, Some(7));
}

#[test]
fn list_search_keeps_selected_full_index() {
    let dir = tempfile::tempdir().unwrap();
    let events = super::super::log::log_markdown::notification_events(dir.path(), None);
    let (mut view, _) = log_fixture();
    view.set_log(events);
    view.select_log_event(Some(0));
    view.log_list_focused = true;
    view.log_detail_focused = false;
    view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(view.log_search.editing);
    assert_eq!(view.log_selected, Some(0));
    view.log_search.query.set_text("Background command");
    view.rematch_log();
    assert_eq!(view.log_selected, Some(0));
}
