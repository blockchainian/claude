use super::*;

fn paired_notifications(dir: &std::path::Path) -> Vec<crate::event::Event> {
    let events = super::super::event_list::event_markdown::notification_events(dir, None);
    // Repeated enqueues for a resumed background task remain separate rows.
    [events.clone(), events].concat()
}

#[test]
fn paired_notifications_move_down_to_newer_with_all_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let events = paired_notifications(dir.path());
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].calls[0].id, events[1].calls[0].id);
    let (mut view, now) = event_fixture();
    view.set_event(events.clone());
    event_screen(&view, 200, 45, now);
    let rect = view.viewport.borrow().event_panel;
    for code in [KeyCode::Down, KeyCode::Char('j')] {
        view.select_event(Some(0));
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(view.event_list.cursor, Some(1));
        view.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(view.event_list.cursor, Some(0));
    }
    for _ in 0..12 {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 1, rect.y));
        assert_eq!(view.event_list.cursor, Some(1));
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 1, rect.y));
        assert_eq!(view.event_list.cursor, Some(0));
    }
}

#[test]
fn structural_updates_keep_selection_and_following() {
    let dir = tempfile::tempdir().unwrap();
    let events = paired_notifications(dir.path());
    let (mut view, _) = event_fixture();
    view.set_event(events.clone());
    view.select_event(Some(0));
    let selected = view.visible_event().unwrap().clone();
    let mut older = events.clone();
    older.extend(events.clone());
    view.apply_event_update(crate::event::Update {
        events: older.clone(),
        prepended: 2,
        ..Default::default()
    });
    assert_eq!(view.event_list.cursor, Some(2));
    assert_eq!(view.visible_event(), Some(&selected));
    view.apply_event_update(crate::event::Update {
        events: older.clone(),
        replaced: 1,
        ..Default::default()
    });
    assert_eq!(view.event_list.cursor, Some(2));
    let scroll = view.event_scroll();
    older.extend(events.clone());
    view.apply_event_update(crate::event::Update {
        events: older.clone(),
        appended: 2,
        ..Default::default()
    });
    assert_eq!(view.event_list.cursor, Some(2));
    assert_eq!(view.event_scroll(), scroll);
    view.select_event(None);
    older.extend(events);
    view.apply_event_update(crate::event::Update {
        events: older,
        appended: 2,
        ..Default::default()
    });
    assert_eq!(view.event_list.cursor, Some(7));
}

#[test]
fn list_search_keeps_selected_full_index() {
    let dir = tempfile::tempdir().unwrap();
    let events = paired_notifications(dir.path());
    let (mut view, _) = event_fixture();
    view.set_event(events);
    view.select_event(Some(0));
    view.active_panel = Panel::EventList;

    {
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
    };
    assert!(view.event_list.search.input.focused());
    assert_eq!(view.event_list.cursor, Some(0));
    view.event_list.search.input.set_text("等 tab 线程");
    view.rematch_event();
    assert_eq!(view.event_list.cursor, Some(0));
}
