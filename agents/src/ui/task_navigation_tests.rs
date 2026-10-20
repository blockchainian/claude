use super::*;

fn tasks() -> View {
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            ..Capabilities::default()
        },
    )]));
    view.set_sessions(
        (0..4)
            .map(|i| {
                session(
                    &format!("task-{i}"),
                    &format!("Task {i}"),
                    SessionStatus::Ready,
                    i,
                )
            })
            .collect(),
        false,
    );
    view
}

#[test]
fn opening_keys_use_terminal() {
    for (code, modifiers) in [
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Right, KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::CONTROL),
        (KeyCode::Char('o'), KeyModifiers::NONE),
    ] {
        let mut view = tasks();
        let id = view.selected_id().unwrap().clone();
        assert_eq!(
            view.handle_key(KeyEvent::new(code, modifiers)),
            Some(Action::OpenTerminal(id))
        );
        view.capabilities.clear();
        assert_eq!(
            view.handle_key(KeyEvent::new(code, modifiers)),
            Some(Action::Unsupported("Terminal"))
        );
    }
}

#[test]
fn escape_walks_selection_history_without_bouncing() {
    let mut view = tasks();
    view.set_selected(view.selectable_indices()[0]);
    let first = view.selected_id().cloned();
    key(&mut view, KeyCode::Down);
    let second = view.selected_id().cloned();
    key(&mut view, KeyCode::Down);
    let third = view.selected_id().cloned();
    view.select(view.selected); // Re-selecting a task adds no history.
    view.set_sessions(view.rows.clone(), false);
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_id(), second.as_ref());
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_id(), first.as_ref());
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_id(), first.as_ref());
    assert_ne!(first, third);
}

#[test]
fn escape_search_and_other_panels_do_not_consume_task_history() {
    for panel in [
        Panel::TaskList,
        Panel::EventList,
        Panel::EventDetail,
        Panel::TaskDetails,
    ] {
        let mut view = tasks();
        let first = view.selected_id().cloned();
        key(&mut view, KeyCode::Down);
        let second = view.selected_id().cloned();
        view.active_panel = panel;
        if panel == Panel::TaskList {
            view.searching = true;
            view.search.set_text("Task");
        }
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert!(!view.searching);
        assert_eq!(view.selected_id(), second.as_ref());
        view.active_panel = Panel::TaskList;
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert_eq!(view.selected_id(), first.as_ref());
    }
}

#[test]
fn escape_skips_removed_and_hidden_tasks() {
    let mut view = tasks();
    view.set_selected(view.selectable_indices()[0]);
    let first = view.selected_id().cloned();
    key(&mut view, KeyCode::Down);
    let removed = view.selected_id().cloned();
    key(&mut view, KeyCode::Down);
    let hidden = view.selected_id().unwrap().clone();
    key(&mut view, KeyCode::Down);
    view.hidden.insert(hidden);
    view.set_sessions(
        view.rows
            .clone()
            .into_iter()
            .filter(|s| Some(&s.id) != removed.as_ref())
            .collect(),
        false,
    );
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_id(), first.as_ref());
}

#[test]
fn escape_returns_across_tabs() {
    let mut view = tasks();
    let first = view.selected_id().cloned();
    let mut rows = view.rows.clone();
    rows.push(session(
        "inactive",
        "Inactive",
        SessionStatus::Inactive,
        100,
    ));
    view.set_sessions(rows, false);
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&session_key("inactive")));
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_id(), first.as_ref());
    assert_eq!(view.status_filter, 0);
}

#[test]
fn opening_show_more_still_loads_a_page() {
    for (code, modifiers) in [
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Right, KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::CONTROL),
        (KeyCode::Char('o'), KeyModifiers::NONE),
    ] {
        let mut view = tasks();
        view.has_more = true;
        key(&mut view, KeyCode::Tab);
        assert_eq!(
            view.handle_key(KeyEvent::new(code, modifiers)),
            Some(Action::ShowMore)
        );
        assert_eq!(view.handle_key(KeyEvent::new(code, modifiers)), None);
    }
}

#[test]
fn terminal_and_history_hints_match_keys() {
    let view = tasks();
    let help = view
        .center_help_lines(120)
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(help.contains("→/⌃l") && help.contains("Terminal"));
    assert!(help.contains("Previous"));
    assert!(
        view.center_footer_hints()
            .to_string()
            .contains("enter Terminal")
    );
    assert!(
        view.center_footer_hints()
            .to_string()
            .contains("esc Previous")
    );
}

#[test]
fn escape_closes_overlays_without_consuming_history() {
    for mode in 0..5 {
        let mut view = tasks();
        let first = view.selected_id().cloned();
        key(&mut view, KeyCode::Down);
        let second = view.selected_id().cloned();
        match mode {
            0 => view.help = true,
            1 => {
                view.rename_target = second.clone();
                view.set_active_panel(Panel::TaskDetails);
            }
            2 => {
                view.confirm_dialog = Some(ConfirmDialog {
                    action: ConfirmAction::Delete(second.clone().unwrap()),
                    title: "Task".into(),
                    confirm: false,
                })
            }
            3 => view.event_search.editing = true,
            4 => view.event_detail_search.editing = true,
            _ => unreachable!(),
        }
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert_eq!(view.selected_id(), second.as_ref());
        assert!(!view.help && view.rename_target.is_none() && view.confirm_dialog.is_none());
        assert!(!view.event_search.editing && !view.event_detail_search.editing);
        view.set_active_panel(Panel::TaskList);
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert_eq!(view.selected_id(), first.as_ref());
    }
}

#[test]
fn task_list_keyboard_stops_at_both_ends() {
    for (code, modifiers, forward) in [
        (KeyCode::Up, KeyModifiers::NONE, false),
        (KeyCode::Down, KeyModifiers::NONE, true),
        (KeyCode::Char('k'), KeyModifiers::NONE, false),
        (KeyCode::Char('j'), KeyModifiers::NONE, true),
        (KeyCode::Char('p'), KeyModifiers::CONTROL, false),
        (KeyCode::Char('n'), KeyModifiers::CONTROL, true),
    ] {
        let mut view = tasks();
        key(
            &mut view,
            if forward { KeyCode::End } else { KeyCode::Home },
        );
        let selected = view.selected;
        view.handle_key(KeyEvent::new(code, modifiers));
        assert_eq!(view.selected, selected, "{code:?} at boundary");
    }
}

#[test]
fn task_list_wheel_stops_at_both_ends() {
    for (kind, boundary) in [
        (MouseEventKind::ScrollUp, KeyCode::Home),
        (MouseEventKind::ScrollDown, KeyCode::End),
    ] {
        let mut view = tasks();
        key(&mut view, boundary);
        screen(&view, 120, 40);
        let list = view.center_layout(Rect::new(0, 0, 120, 40)).list;
        let selected = view.selected;
        let scroll = view.viewport.borrow().scroll;
        view.handle_mouse(mouse(kind, list.x, list.y));
        assert_eq!(view.selected, selected, "{kind:?} at boundary");
        assert_eq!(view.viewport.borrow().scroll, scroll);
    }
}
