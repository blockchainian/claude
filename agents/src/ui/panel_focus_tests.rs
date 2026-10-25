use super::*;

fn at(view: &mut View, panel: usize, kind: MouseEventKind) {
    screen(view, 200, 45);
    let rect = {
        let viewport = view.viewport.borrow();
        [
            viewport.list,
            viewport.event_panel,
            viewport.event_detail,
            viewport.task_details,
            viewport.prompt,
        ][panel]
    };
    assert!(!rect.is_empty());
    view.handle_mouse(mouse(kind, rect.x, rect.y));
}

fn editing(view: &View) -> [bool; 3] {
    [
        view.search.focused(),
        view.event_list.search.input.focused(),
        view.event_detail.search.input.focused(),
    ]
}

#[test]
fn panel_focus_slash_tracks_wheel_but_ignores_hover() {
    for kind in [
        MouseEventKind::ScrollDown,
        MouseEventKind::ScrollUp,
        MouseEventKind::Moved,
    ] {
        for panel in 0..5 {
            let (mut view, _) = event_fixture();
            click_panel(&mut view, panel);
            at(&mut view, (panel + 1) % 5, kind);
            let active = if kind == MouseEventKind::Moved {
                panel
            } else {
                (panel + 1) % 5
            };
            key(&mut view, KeyCode::Char('/'));
            assert_eq!(
                editing(&view),
                [active == 0, active == 1, active == 2],
                "{kind:?}, {panel}"
            );
        }
    }
}

#[test]
fn panel_focus_startup_and_keyboard() {
    let mut view = fixture();
    key(&mut view, KeyCode::Char('/'));
    assert!(view.search.focused());
    let (mut view, _) = event_fixture();
    at(&mut view, 2, MouseEventKind::Down(MouseButton::Left));
    key(&mut view, KeyCode::Down);
    key(&mut view, KeyCode::Char('/'));
    assert!(view.event_detail.search.input.focused());
}

#[test]
fn panel_focus_click_blurs_search_and_preserves_all_queries() {
    for panel in 0..3 {
        for destination in 0..5 {
            let (mut view, _) = event_fixture();
            at(&mut view, panel, MouseEventKind::Down(MouseButton::Left));
            key(&mut view, KeyCode::Char('/'));
            key(&mut view, KeyCode::Char('a'));
            click_search_body(&mut view, destination);
            assert_eq!(editing(&view), [false; 3], "{panel} -> {destination}");
            assert!(!view.search_focused());
            assert_eq!(view.search.text(), if panel == 0 { "a" } else { "" });
            assert_eq!(
                view.event_list.search.input.text(),
                if panel == 1 { "a" } else { "" }
            );
            assert_eq!(
                view.event_detail.search.input.text(),
                if panel == 2 { "a" } else { "" }
            );
            key(&mut view, KeyCode::Char('/'));
            assert_eq!(
                editing(&view),
                [destination == 0, destination == 1, destination == 2]
            );
            if destination < 3 {
                assert!(view.search_focused());
                assert_eq!(
                    search_text(&view, destination),
                    if destination == panel { "a" } else { "" }
                );
            }
        }
    }
}

fn search_panel(panel: usize) -> Panel {
    [Panel::TaskList, Panel::EventList, Panel::EventDetail][panel]
}

fn search_text(view: &View, panel: usize) -> &str {
    match panel {
        0 => view.search.text(),
        1 => view.event_list.search.input.text(),
        2 => view.event_detail.search.input.text(),
        _ => unreachable!(),
    }
}

fn search_applied(view: &View, panel: usize) -> bool {
    match panel {
        0 => view.search.applied(),
        1 => view.event_list.search.input.applied(),
        2 => view.event_detail.search.input.applied(),
        _ => unreachable!(),
    }
}

fn search_payload(fixture: &str) -> Vec<crate::event::Event> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    crate::providers::claude::event_reader::tail_session(&path, 200, true).unwrap()
}

fn search_fixture() -> View {
    let (mut view, _) = event_fixture();
    let events = view.event_panel.clone();
    let mut current = view.rows[0].clone();
    current.cwd = PathBuf::from("/");
    let mut another = current.clone();
    another.id = session_key("another");
    another.title = "Another task".into();
    another.prompt = another.title.clone();
    let mut hidden = another.clone();
    hidden.id = session_key("hidden");
    hidden.title = "zzz".into();
    hidden.prompt = hidden.title.clone();
    view.set_sessions(vec![current, another, hidden], false);
    view.set_pinned(&crate::state::TaskPins {
        active: vec![session_key("log")],
        inactive: vec![],
    });
    let selected = view
        .rows
        .iter()
        .position(|row| row.id == session_key("log"))
        .unwrap();
    view.set_selected(selected);
    view.event_session = Some(session_key("log"));
    view.set_event(events);
    view.select_event(Some(0));
    view
}

fn start_search(view: &mut View, panel: usize) {
    at(view, panel, MouseEventKind::Down(MouseButton::Left));
    key(view, KeyCode::Char('/'));
    key(view, KeyCode::Char('a'));
    assert!(editing(view)[panel]);
    assert!(view.search_focused());
    assert_eq!(search_text(view, panel), "a");
}

// The selected task row and event text regions come from the preceding render.
fn click_search_body(view: &mut View, panel: usize) {
    screen(view, 200, 45);
    let (x, y) = {
        let viewport = view.viewport.borrow();
        match panel {
            0 => viewport.marker.expect("selected task row"),
            1..=3 => {
                let region = match panel {
                    1 => &viewport.event_list_text,
                    2 => &viewport.event_text,
                    3 => &viewport.task_details_text,
                    _ => unreachable!(),
                };
                let row = region
                    .rows
                    .iter()
                    .find(|row| !row.cells.is_empty())
                    .expect("rendered panel content row");
                (row.x + row.cells[0].0.start, row.y)
            }
            4 => {
                let row = viewport.prompt.inner(Margin::new(1, 0));
                assert!(!row.is_empty());
                (row.x, row.y)
            }
            _ => unreachable!(),
        }
    };
    if panel < 3 {
        assert!(
            !view
                .search_rect(search_panel(panel))
                .contains((x, y).into())
        );
    }
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
}

fn click_search_input(view: &mut View, panel: usize, query_column: u16) -> (u16, u16) {
    screen(view, 200, 45);
    let row = {
        let viewport = view.viewport.borrow();
        match panel {
            0 => viewport.search,
            1 | 2 => {
                let area = if panel == 1 {
                    viewport.event_panel
                } else {
                    viewport.event_detail
                };
                assert!(area.height >= 2);
                Rect::new(area.x, area.bottom() - 1, area.width, 1)
            }
            _ => unreachable!(),
        }
    };
    assert!(!row.is_empty(), "panel {panel} search row");
    assert_eq!(row.height, 1);
    assert_eq!(view.search_rect(search_panel(panel)), row);
    let x = row.x + "Search ❯ ".chars().count() as u16 + query_column;
    assert!(x < row.right());
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, row.y));
    (x, row.y)
}

fn search_results(view: &View, panel: usize) -> Vec<usize> {
    match panel {
        0 => view.selectable_indices(),
        1 => view
            .shown_event_rows()
            .filter_map(|(row, _)| match row {
                super::super::task_ui::EventRow::Timeline(index) => Some(index),
                super::super::task_ui::EventRow::Pinned(_) => None,
            })
            .collect(),
        2 => view
            .event_detail
            .search
            .indices(&view.visible_event().expect("selected event").body)
            .into_iter()
            .map(|index| index as usize)
            .collect(),
        _ => unreachable!(),
    }
}

#[test]
fn search_click_outside_keeps_query_and_frees_hotkeys() {
    for panel in 0..3 {
        let mut view = search_fixture();
        if panel != 0 {
            let event = search_payload("claude-tool-calls.jsonl")
                .into_iter()
                .find(|event| {
                    event.kind == crate::event::Kind::Tool
                        && event.body.contains('a')
                        && event.source.is_some()
                        && !event.source_records.is_empty()
                })
                .expect("real pinnable matching event");
            let mut events = view.event_panel.clone();
            events.insert(0, event);
            view.set_event(events);
            view.select_event(Some(0));
        }
        let unfiltered = search_results(&view, panel);
        start_search(&mut view, panel);
        let filtered = search_results(&view, panel);
        assert!(!filtered.is_empty(), "panel {panel}");
        if panel < 2 {
            assert!(
                filtered.len() < unfiltered.len(),
                "panel {panel} must filter rows"
            );
        }
        click_search_body(&mut view, panel);
        assert_eq!(editing(&view), [false; 3]);
        assert!(!view.search_focused());
        assert!(search_applied(&view, panel));
        assert_eq!(search_text(&view, panel), "a");
        assert_eq!(search_results(&view, panel), filtered);
        if panel == 0 {
            let selected = view.selected_id().cloned();
            key(&mut view, KeyCode::Char('j'));
            assert_ne!(view.selected_id(), selected.as_ref());
        } else {
            let pinned = view.event_list.pinned.len();
            assert!(matches!(
                key(&mut view, KeyCode::Char('p')),
                Some(Action::ToggleEventPin { .. })
            ));
            assert_eq!(view.event_list.pinned.len(), pinned + 1);
            if panel == 1 {
                let sort = view.event_list.sort;
                key(&mut view, KeyCode::Char('t'));
                assert_ne!(view.event_list.sort, sort);
            }
        }
        assert_eq!(search_text(&view, panel), "a");
    }
}

#[test]
fn search_click_on_input_refocuses_and_types() {
    for panel in 0..3 {
        let mut view = search_fixture();
        start_search(&mut view, panel);
        click_search_body(&mut view, panel);
        assert!(!editing(&view)[panel]);
        let caret = click_search_input(&mut view, panel, 1);
        assert!(editing(&view)[panel]);
        assert!(view.search_focused());
        assert_eq!(view.cursor_pos(Rect::new(0, 0, 200, 45)), Some(caret));
        key(&mut view, KeyCode::Char('b'));
        assert_eq!(search_text(&view, panel), "ab");
        let caret = click_search_input(&mut view, panel, 0);
        assert_eq!(
            view.cursor_pos(Rect::new(0, 0, 200, 45)),
            Some((caret.0 + 1, caret.1)),
            "panel {panel} places the caret after the clicked character"
        );
        key(&mut view, KeyCode::Char('c'));
        assert_eq!(search_text(&view, panel), "acb");
    }
}

#[test]
fn search_click_on_inactive_panel_input_focuses_it() {
    for panel in 0..3 {
        let destination = (panel + 1) % 3;
        let mut view = search_fixture();
        start_search(&mut view, destination);
        assert!(key(&mut view, KeyCode::Enter).is_none());
        start_search(&mut view, panel);
        assert!(!editing(&view)[destination]);
        assert_eq!(search_text(&view, destination), "a");
        click_search_input(&mut view, destination, 1);
        assert!(view.active_panel == search_panel(destination));
        assert_eq!(
            editing(&view),
            [destination == 0, destination == 1, destination == 2]
        );
        assert!(view.search_focused());
        assert_eq!(search_text(&view, panel), "a");
        assert_eq!(search_text(&view, destination), "a");
    }
}

#[test]
fn search_panel_switch_blurs_and_keeps_query() {
    for panel in 0..3 {
        let mut view = search_fixture();
        start_search(&mut view, panel);
        let filtered = search_results(&view, panel);
        click_search_body(&mut view, (panel + 1) % 3);
        assert_eq!(editing(&view), [false; 3]);
        assert!(!view.search_focused());
        assert!(view.active_panel == search_panel((panel + 1) % 3));
        assert_eq!(search_text(&view, panel), "a");
        assert!(search_applied(&view, panel));
        assert_eq!(search_results(&view, panel), filtered);
    }
}

#[test]
fn search_slash_refocuses_blurred_query() {
    for panel in 0..3 {
        let mut view = search_fixture();
        start_search(&mut view, panel);
        key(&mut view, KeyCode::Home);
        assert!(key(&mut view, KeyCode::Enter).is_none());
        assert!(!editing(&view)[panel]);
        key(&mut view, KeyCode::Char('/'));
        assert!(editing(&view)[panel]);
        assert!(view.search_focused());
        assert_eq!(search_text(&view, panel), "a");
        key(&mut view, KeyCode::Char('/'));
        assert_eq!(search_text(&view, panel), "a/");
    }
}

#[test]
fn search_blur_with_empty_query_closes() {
    for panel in 0..3 {
        let mut view = search_fixture();
        at(&mut view, panel, MouseEventKind::Down(MouseButton::Left));
        screen(&view, 200, 45);
        let body_height = |view: &View| {
            let viewport = view.viewport.borrow();
            match panel {
                0 => viewport.list.height,
                1 => viewport.event_list_text.area.height,
                2 => viewport.event_text.area.height,
                _ => unreachable!(),
            }
        };
        let before = body_height(&view);
        key(&mut view, KeyCode::Char('/'));
        assert!(search_applied(&view, panel));
        assert!(
            screen(&view, 200, 45).contains("Search ❯"),
            "panel {panel}: selected {:?}, events {:?}, focused {:?}",
            view.selected_id(),
            view.event_session,
            editing(&view)
        );
        click_search_body(&mut view, panel);
        assert_eq!(editing(&view), [false; 3]);
        assert!(!view.search_focused());
        assert!(!search_applied(&view, panel));
        assert_eq!(search_text(&view, panel), "");
        let after = screen(&view, 200, 45);
        assert!(!after.contains("Search ❯"));
        assert!(view.search_rect(search_panel(panel)).is_empty());
        assert_eq!(
            body_height(&view),
            before,
            "panel {panel} regains its input row"
        );
    }
}

#[test]
fn search_escape_keeps_event_selection() {
    for panel in 0..3 {
        let mut view = search_fixture();
        if panel == 2 {
            let event = search_payload("transcript.jsonl")
                .into_iter()
                .find(|event| {
                    event.kind == crate::event::Kind::User
                        && event.body.lines().count() > 10
                        && event.body.contains('a')
                })
                .expect("real scrollable detail");
            view.set_event(vec![event]);
            view.select_event(Some(0));
        }
        start_search(&mut view, panel);
        match panel {
            0 => {
                key(&mut view, KeyCode::Down);
                let selected = view.selected_id().cloned().expect("matching task");
                key(&mut view, KeyCode::Esc);
                assert_eq!(view.selected_id(), Some(&selected));
            }
            1 => {
                key(&mut view, KeyCode::Down);
                key(&mut view, KeyCode::Down);
                assert_eq!(view.event_list.cursor, Some(2));
                let selected = view.visible_event().cloned().expect("third matching event");
                key(&mut view, KeyCode::Esc);
                assert_eq!(view.visible_event(), Some(&selected));
            }
            2 => {
                screen(&view, 200, 45);
                key(&mut view, KeyCode::Down);
                screen(&view, 200, 45);
                let offset = view.event_detail.scroll.borrow().offset;
                assert!(offset > 0, "detail must be scrolled before Escape");
                key(&mut view, KeyCode::Esc);
                screen(&view, 200, 45);
                assert_eq!(view.event_detail.scroll.borrow().offset, offset);
            }
            _ => unreachable!(),
        }
        assert_eq!(editing(&view), [false; 3]);
        assert!(!view.search_focused());
        assert!(!search_applied(&view, panel));
        assert_eq!(search_text(&view, panel), "");
    }
}

#[test]
fn panel_focus_slash_does_not_type_into_other_editors() {
    for panel in 1..5 {
        let (mut view, _) = event_fixture();
        view.rows[view.selected].process = None;
        view.active_panel = Panel::Prompt;
        click_panel(&mut view, panel);
        key(&mut view, KeyCode::Char('/'));
        assert_eq!(editing(&view), [false, panel == 1, panel == 2]);
        assert_eq!(
            view.input_box.editor.text(),
            if panel == 4 { "/" } else { "" }
        );
    }
}

#[test]
fn panel_focus_ctrl_r_never_opens_search() {
    for panel in 0..5 {
        let (mut view, _) = event_fixture();
        at(&mut view, panel, MouseEventKind::Down(MouseButton::Left));
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(editing(&view), [false; 3]);
    }
}

fn click_panel(view: &mut View, panel: usize) {
    at(view, panel, MouseEventKind::Down(MouseButton::Left));
    at(view, panel, MouseEventKind::Up(MouseButton::Left));
}

#[test]
fn panel_click_event_list_takes_prompt_focus_and_opens_search() {
    let (mut view, _) = event_fixture();
    view.active_panel = Panel::Prompt;
    click_panel(&mut view, 1);
    key(&mut view, KeyCode::Char('/'));
    assert!(view.event_list.search.input.focused());
    assert!(view.active_panel == Panel::EventList);
    assert!(!(view.active_panel == Panel::EventDetail));
    assert!(!view.prompt_focused());
    assert!(view.input_box.editor.is_empty());
}

#[test]
fn panel_click_event_detail_takes_prompt_focus_and_opens_search() {
    let (mut view, _) = event_fixture();
    view.active_panel = Panel::Prompt;
    click_panel(&mut view, 2);
    key(&mut view, KeyCode::Char('/'));
    assert!(view.event_detail.search.input.focused());
    assert!(view.active_panel == Panel::EventDetail);
    assert!(!(view.active_panel == Panel::EventList));
    assert!(!view.prompt_focused());
    assert!(view.input_box.editor.is_empty());
}

#[test]
fn panel_click_non_input_takes_prompt_focus() {
    for panel in [0, 3] {
        let (mut view, _) = event_fixture();
        view.active_panel = Panel::EventList;
        view.active_panel = Panel::EventDetail;
        click_panel(&mut view, panel);
        assert!(!view.prompt_focused(), "panel {panel}");
        assert!(!(view.active_panel == Panel::EventList), "panel {panel}");
        assert!(!(view.active_panel == Panel::EventDetail), "panel {panel}");
    }
}

#[test]
fn panel_click_prompt_regains_focus_and_types_slash() {
    let (mut view, _) = event_fixture();
    view.rows[view.selected].process = None;
    click_panel(&mut view, 2);
    click_panel(&mut view, 4);
    assert!(view.prompt_focused());
    assert!(!(view.active_panel == Panel::EventList));
    assert!(!(view.active_panel == Panel::EventDetail));
    key(&mut view, KeyCode::Char('/'));
    assert_eq!(view.input_box.editor.text(), "/");
    assert_eq!(editing(&view), [false; 3]);
}

#[test]
fn search_enter_space_escape_and_f3_share_panel_behavior() {
    for panel in 0..3 {
        let mut view = search_fixture();
        start_search(&mut view, panel);
        key(&mut view, KeyCode::Char(' '));
        assert_eq!(search_text(&view, panel), "a ");
        key(&mut view, KeyCode::F(3));
        assert!(view.search_focused());
        key(&mut view, KeyCode::Enter);
        assert!(!view.search_focused());
        assert_eq!(search_text(&view, panel), "a ");
        assert!(view.active_panel == search_panel(panel));
        key(&mut view, KeyCode::Esc);
        assert!(!search_applied(&view, panel));
        assert_eq!(search_text(&view, panel), "");
    }
}

#[test]
fn search_right_click_only_blurs() {
    for panel in 0..3 {
        let mut view = search_fixture();
        start_search(&mut view, panel);
        let filtered = search_results(&view, panel);
        screen(&view, 200, 45);
        let rect = {
            let viewport = view.viewport.borrow();
            [viewport.list, viewport.event_panel, viewport.event_detail][panel]
        };
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Right),
        ] {
            assert_eq!(
                view.handle_mouse(mouse(kind, rect.x, rect.y)),
                None,
                "panel {panel}"
            );
        }
        assert!(!view.search_focused(), "panel {panel}");
        assert_eq!(search_text(&view, panel), "a");
        assert_eq!(search_results(&view, panel), filtered);
    }
}

#[test]
fn event_search_tab_parks_focused_input_with_task() {
    for (panel, focus) in [
        (Panel::EventList, super::super::task_ui::Focus::EventSearch),
        (
            Panel::EventDetail,
            super::super::task_ui::Focus::EventDetailSearch,
        ),
    ] {
        let mut view = search_fixture();
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .for_each(|row| row.status = SessionStatus::Ready);
        rows.iter_mut()
            .find(|row| row.id == session_key("another"))
            .unwrap()
            .status = SessionStatus::Inactive;
        view.set_sessions(rows, false);
        let task = view.selected_task_key().expect("selected task");
        view.set_active_panel(panel);
        key(&mut view, KeyCode::Char('/'));
        key(&mut view, KeyCode::Char('a'));
        key(&mut view, KeyCode::Tab);
        assert!(view.selected_task_key().as_ref() != Some(&task));
        let parked = &view.task_uis[&task];
        assert_eq!(parked.focus, Some(focus));
        let input = if panel == Panel::EventList {
            &parked.event_list.search.input
        } else {
            &parked.event_detail.search.input
        };
        assert!(input.focused());
        assert_eq!(input.text(), "a");

        view.set_active_panel(Panel::TaskList);
        let index = view
            .rows
            .iter()
            .position(|row| super::super::draft_controls::TaskKey::Session(row.id.clone()) == task)
            .unwrap();
        view.select(index);
        assert_eq!(view.active_panel, Panel::TaskList);
        for panel in [Panel::TaskList, Panel::EventList, Panel::EventDetail] {
            assert!(!view.panel_search(panel).unwrap().focused());
        }
        assert_eq!(view.panel_search(panel).unwrap().text(), "a");
    }
}
