use super::*;
use crate::providers::Provider;

fn fixture() -> (tempfile::TempDir, View) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("config/projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider =
        crate::providers::claude::ClaudeProvider::new(directory.path().join("config"));
    let mut sessions = provider.refresh(1).sessions;
    assert_eq!(sessions.len(), 1);
    sessions[0].cwd = directory.path().into();
    sessions[0].status = SessionStatus::Ready;
    let mut view = View::default();
    view.set_sessions(sessions, false);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    (directory, view)
}
fn click(view: &mut View, x: u16, y: u16) -> Option<Action> {
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    assert!(view.handle_mouse(event).is_none());
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..event
    })
}
#[test]
fn heading_text_opens_and_double_clicks_but_blank_sides_do_not() {
    let (directory, mut view) = fixture();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 300, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    let (path, _) = view.viewport.borrow().list_links[0].clone();
    let edge = view.viewport.borrow().list_rows.right() - 1;
    assert!(edge >= path.right());
    assert!(click(&mut view, edge, path.y).is_none());
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert!(click(&mut view, edge, path.y).is_none());
    assert!(click(&mut view, path.x, path.y).is_none());
    assert_eq!(
        click(&mut view, path.x, path.y),
        Some(Action::NewDraft {
            cwd: directory.path().into()
        })
    );
    assert!(click(&mut view, path.x, path.y).is_none());
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        Some(Action::OpenPath(directory.path().into()))
    );
}
#[test]
fn different_target_runs_pending_path_before_new_click() {
    let (directory, mut view) = fixture();
    let (path, _) = view.viewport.borrow().list_links[0].clone();
    let (_, tab) = view.viewport.borrow().tabs[1];
    assert!(click(&mut view, path.x, path.y).is_none());
    assert_eq!(
        click(&mut view, tab.x, tab.y),
        Some(Action::OpenPath(directory.path().into()))
    );
    assert_eq!(view.status_filter, 1);
}
#[test]
fn links_without_double_action_open_each_click_immediately() {
    let (_directory, mut view) = fixture();
    let (rect, target) = view
        .viewport
        .borrow()
        .task_details_links
        .iter()
        .find(|(rect, _)| {
            !view
                .viewport
                .borrow()
                .task_details_copy
                .iter()
                .any(|(rows, _, _)| rows.contains(&rect.y))
        })
        .cloned()
        .expect("directory link outside copy bodies");
    assert_eq!(
        click(&mut view, rect.x, rect.y),
        Some(Action::OpenPath(target.clone()))
    );
    assert_eq!(
        click(&mut view, rect.x, rect.y),
        Some(Action::OpenPath(target))
    );
    assert!(view.pending_click.is_none());
}

#[test]
fn pending_path_precedes_immediate_link_action() {
    let (directory, mut view) = fixture();
    let (heading, _) = view.viewport.borrow().list_links[0].clone();
    let (link, target) = view
        .viewport
        .borrow()
        .task_details_links
        .iter()
        .find(|(rect, _)| {
            !view
                .viewport
                .borrow()
                .task_details_copy
                .iter()
                .any(|(rows, _, _)| rows.contains(&rect.y))
        })
        .cloned()
        .expect("directory link outside copy bodies");
    assert!(click(&mut view, heading.x, heading.y).is_none());
    assert_eq!(
        click(&mut view, link.x, link.y),
        Some(Action::Batch(vec![
            Action::OpenPath(directory.path().into()),
            Action::OpenPath(target)
        ]))
    );
}

#[test]
fn title_editing_allows_task_row_click_and_list_wheel() {
    for wheel in [false, true] {
        let (directory, mut view) = fixture();
        let payload = include_str!("../../tests/fixtures/claude-start.jsonl");
        let session: serde_json::Value =
            serde_json::from_str(payload.lines().next().unwrap()).unwrap();
        let id = session["sessionId"].as_str().unwrap();
        std::fs::write(
            directory
                .path()
                .join("config/projects/probe")
                .join(format!("{id}.jsonl")),
            payload,
        )
        .unwrap();
        let mut provider =
            crate::providers::claude::ClaudeProvider::new(directory.path().join("config"));
        let mut sessions = provider.refresh(2).sessions;
        assert_eq!(sessions.len(), 2);
        for session in &mut sessions {
            session.status = SessionStatus::Ready;
            session.cwd = directory.path().into();
        }
        view.set_capabilities(HashMap::from([(
            crate::providers::ProviderId::Claude,
            provider.capabilities(),
        )]));
        view.set_sessions(sessions, false);
        let visible = view.selectable_indices();
        view.select(visible[0]);
        let target = visible[1];
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            None
        );
        assert!(view.rename_target.is_some());
        let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 45));
        view.render_at(buffer.area, &mut buffer, Utc::now());
        let entry = view.center_rows(&view.selectable_indices(), view.grouping).iter()
            .position(|row| matches!(row, super::super::navigation::CenterRow::Task(index) if *index == target))
            .unwrap();
        let (x, y) = {
            let viewport = view.viewport.borrow();
            (
                viewport.list_rows.x + 1,
                viewport.list_rows.y + (entry - viewport.first_entry) as u16,
            )
        };
        if wheel {
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            });
        } else {
            assert_eq!(click(&mut view, x, y), None);
        }
        assert_eq!(view.selected, target, "wheel={wheel}");
    }
}

#[test]
fn horizontal_wheel_activates_the_list_and_task_panels() {
    for kind in [MouseEventKind::ScrollLeft, MouseEventKind::ScrollRight] {
        let (_directory, mut view) = fixture();
        view.prepare_prompt();
        let list = view.viewport.borrow().list;
        let event = MouseEvent {
            kind,
            column: list.x + 1,
            row: list.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(view.handle_mouse(event), None);
        assert_eq!(view.active_panel, Panel::TaskList);
        let events = view.viewport.borrow().event_panel;
        assert!(!events.is_empty());
        assert_eq!(
            view.handle_mouse(MouseEvent {
                column: events.x + 1,
                row: events.y + 1,
                ..event
            }),
            None
        );
        assert_eq!(view.active_panel, Panel::EventList);
    }
}

fn image_mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn image_fixture() -> (tempfile::TempDir, View) {
    let (directory, mut view) = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    view.event_session = view.selected_id().cloned();
    view.set_event(crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap());
    view.select_event(Some(0));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_event_detail(Rect::new(20, 8, 56, 25), &mut buffer, Utc::now());
    assert!(view.viewport.borrow().event_detail_layout.is_some());
    (directory, view)
}

fn drawn_image(view: &View) -> crate::event::ImageBlock {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-read-image.jsonl");
    let payload = std::fs::read_to_string(&path).unwrap();
    let image = crate::event::ImageBlock {
        media_type: "image/png".into(),
        record: crate::event::SourceRecord {
            path,
            offset: (payload.lines().next().unwrap().len() + 1) as u64,
        },
        pointer: "/message/content/0/content/0/source/data".into(),
    };
    let mut columns = super::super::event_detail::event_detail_columns(
        Rect::new(20, 8, 56, 25),
        Rect::new(0, 0, 80, 10),
    );
    columns.text.width = 40;
    view.viewport.borrow_mut().event_detail_columns = Some(columns);
    view.viewport.borrow_mut().event_detail_images = vec![super::super::event_detail::DrawnImage {
        index: 2,
        image: image.clone(),
        area: Rect::new(20, 10, 40, 20),
        first_row: 5,
    }];
    image
}

fn enter_image_zoom(view: &mut View) {
    view.handle_mouse(image_mouse(MouseEventKind::Down(MouseButton::Left), 30, 12));
    view.handle_mouse(image_mouse(MouseEventKind::Up(MouseButton::Left), 30, 12));
}

#[test]
fn image_zoom_click_enters_then_resets_and_invalidates_layout() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    assert_eq!(view.active_panel, Panel::EventDetail);
    let zoom = super::super::inline_images::Zoom {
        level: 3,
        left: 0.2,
    };
    view.event_detail.scroll.borrow_mut().zooms.insert(2, zoom);
    enter_image_zoom(&mut view);
    assert_eq!(
        view.event_detail
            .scroll
            .borrow()
            .zooms
            .get(&2)
            .copied()
            .unwrap_or_default(),
        super::super::inline_images::Zoom::default()
    );
    assert!(view.viewport.borrow().event_detail_layout.is_none());
    assert!(view.pending_click.is_none());
    assert!(view.last_copy_click.is_none());
    assert!(view.mouse_press.is_none());
}

#[test]
fn image_zoom_escape_consumes_key_and_resets() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    let selected = view.event_list.cursor;
    view.event_detail.scroll.borrow_mut().zooms.insert(
        2,
        super::super::inline_images::Zoom {
            level: 3,
            left: 0.2,
        },
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.active_panel, Panel::EventDetail);
    assert_eq!(view.event_list.cursor, selected);
    assert!(
        view.event_detail
            .scroll
            .borrow()
            .zooms
            .values()
            .all(|zoom| *zoom == super::super::inline_images::Zoom::default())
    );
    view.handle_mouse(image_mouse(MouseEventKind::ScrollUp, 30, 12));
    assert!(
        view.event_detail
            .scroll
            .borrow()
            .zooms
            .values()
            .all(|zoom| *zoom == super::super::inline_images::Zoom::default())
    );
}

#[test]
fn image_zoom_leaving_focus_resets() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    view.event_detail.scroll.borrow_mut().zooms.insert(
        2,
        super::super::inline_images::Zoom {
            level: 3,
            left: 0.2,
        },
    );
    view.set_active_panel(Panel::EventList);
    assert!(
        view.event_detail
            .scroll
            .borrow()
            .zooms
            .values()
            .all(|zoom| *zoom == super::super::inline_images::Zoom::default())
    );
    assert!(view.viewport.borrow().event_detail_layout.is_none());
}

#[test]
fn image_zoom_switching_event_resets_before_render() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    view.event_detail.scroll.borrow_mut().zooms.insert(
        2,
        super::super::inline_images::Zoom {
            level: 3,
            left: 0.2,
        },
    );
    view.select_event(Some(1));
    assert!(
        view.event_detail
            .scroll
            .borrow()
            .zooms
            .values()
            .all(|zoom| *zoom == super::super::inline_images::Zoom::default())
    );
}

#[test]
fn image_zoom_wheel_without_decodable_size_does_nothing() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    view.inline_images
        .replace(Some(super::super::inline_images::InlineImages::new(None)));
    view.viewport.borrow_mut().event_detail_images[0]
        .image
        .pointer = "/missing".into();
    let offset = view.event_detail.scroll.borrow().offset;
    for kind in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown] {
        view.handle_mouse(image_mouse(kind, 30, 12));
        assert!(view.event_detail.scroll.borrow().zooms.is_empty());
        assert_eq!(view.event_detail.scroll.borrow().offset, offset);
    }
}

#[test]
fn image_zoom_wheel_anchors_pointer() {
    let (_directory, mut view) = image_fixture();
    let image = drawn_image(&view);
    enter_image_zoom(&mut view);
    view.inline_images
        .replace(Some(super::super::inline_images::InlineImages::new(Some(
            (10, 20),
        ))));
    let default = super::super::inline_images::Zoom::default();
    let (width, height) = view
        .inline_images
        .borrow_mut()
        .as_mut()
        .unwrap()
        .size(&image, 40, default)
        .unwrap();
    view.event_detail.scroll.borrow_mut().offset = 20;
    view.handle_mouse(image_mouse(MouseEventKind::ScrollUp, 50, 17));
    let zoom = view.event_detail.scroll.borrow().zooms[&2];
    assert_eq!(zoom.level, 1);
    let (next_width, next_height) = view
        .inline_images
        .borrow_mut()
        .as_mut()
        .unwrap()
        .size(&image, 40, zoom)
        .unwrap();
    let expected_left = (30.0 / f32::from(width) - 30.0 / f32::from(next_width)).clamp(
        0.0,
        f32::from(next_width.saturating_sub(40)) / f32::from(next_width),
    );
    assert!((zoom.left - expected_left).abs() < 0.0001);
    let next_row = (12.0 * f32::from(next_height) / f32::from(height)).round() as usize;
    assert_eq!(view.event_detail.scroll.borrow().offset, 20 + next_row - 12);
    assert!(view.viewport.borrow().event_detail_layout.is_none());
    assert!(view.mouse_press.is_none());
}

#[test]
fn ordinary_image_wheel_keeps_panel_dispatch() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    let area = view.viewport.borrow().event_panel;
    view.handle_mouse(MouseEvent {
        modifiers: KeyModifiers::NONE,
        ..image_mouse(MouseEventKind::ScrollDown, area.x + 1, area.y + 1)
    });
    assert_eq!(view.active_panel, Panel::EventList);
    assert!(view.event_detail.scroll.borrow().zooms.is_empty());
}

#[test]
fn image_zoom_wheel_merges_reports_until_next_frame() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    view.inline_images
        .replace(Some(super::super::inline_images::InlineImages::new(Some(
            (10, 20),
        ))));
    view.begin_image_frame();
    for kind in [
        MouseEventKind::ScrollUp,
        MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown,
    ] {
        view.handle_mouse(image_mouse(kind, 30, 12));
        assert_eq!(view.event_detail.scroll.borrow().zooms[&2].level, 1);
    }
    view.begin_image_frame();
    view.handle_mouse(image_mouse(MouseEventKind::ScrollUp, 30, 12));
    assert_eq!(view.event_detail.scroll.borrow().zooms[&2].level, 2);
    for level in [1, 0, 0] {
        view.begin_image_frame();
        view.handle_mouse(image_mouse(MouseEventKind::ScrollDown, 30, 12));
        assert_eq!(view.event_detail.scroll.borrow().zooms[&2].level, level);
    }
    assert_eq!(
        view.event_detail.scroll.borrow().zooms[&2],
        super::super::inline_images::Zoom::default()
    );
}

#[test]
fn image_zoom_click_does_not_join_normal_click_state() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    let original = view.last_copy_click.clone();
    let selection = view.text_selection.borrow().is_some();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        view.handle_mouse(image_mouse(kind, 30, 12));
    }
    assert_eq!(view.last_copy_click, original);
    assert_eq!(view.text_selection.borrow().is_some(), selection);
    assert!(view.pending_click.is_none());
    assert!(view.mouse_press.is_none());
    assert!(view.mouse_panel.is_none());
}

#[test]
fn image_zoom_control_wheel_does_not_zoom() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    view.inline_images
        .replace(Some(super::super::inline_images::InlineImages::new(Some(
            (10, 20),
        ))));
    enter_image_zoom(&mut view);
    view.handle_mouse(MouseEvent {
        modifiers: KeyModifiers::CONTROL,
        ..image_mouse(MouseEventKind::ScrollUp, 30, 12)
    });
    assert!(view.event_detail.scroll.borrow().zooms.is_empty());
}

#[test]
fn image_zoom_wheel_over_other_image_scrolls_details() {
    let (_directory, mut view) = image_fixture();
    let image = drawn_image(&view);
    enter_image_zoom(&mut view);
    view.event_detail.scroll.borrow_mut().offset = 15;
    view.viewport
        .borrow_mut()
        .event_detail_images
        .push(super::super::event_detail::DrawnImage {
            index: 3,
            image,
            area: Rect::new(20, 30, 40, 2),
            first_row: 0,
        });
    view.handle_mouse(image_mouse(MouseEventKind::ScrollUp, 30, 30));
    assert!(view.event_detail.scroll.borrow().zooms.is_empty());
    assert!(view.event_detail.scroll.borrow().offset < 15);
}

#[test]
fn image_zoom_keyboard_scroll_preserves_mode() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    for key in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        view.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
        assert_eq!(view.event_detail.scroll.borrow().zoom_image, Some(2));
    }
}

#[test]
fn image_zoom_clicking_another_image_resets_previous() {
    let (_directory, mut view) = image_fixture();
    let image = drawn_image(&view);
    enter_image_zoom(&mut view);
    view.event_detail.scroll.borrow_mut().zooms.insert(
        2,
        super::super::inline_images::Zoom {
            level: 3,
            left: 0.2,
        },
    );
    view.viewport
        .borrow_mut()
        .event_detail_images
        .push(super::super::event_detail::DrawnImage {
            index: 3,
            image,
            area: Rect::new(20, 30, 40, 2),
            first_row: 0,
        });
    view.handle_mouse(image_mouse(MouseEventKind::Down(MouseButton::Left), 30, 30));
    assert_eq!(view.event_detail.scroll.borrow().zoom_image, Some(3));
    assert!(view.event_detail.scroll.borrow().zooms.is_empty());
}

#[test]
fn image_zoom_control_click_does_not_reset() {
    let (_directory, mut view) = image_fixture();
    drawn_image(&view);
    enter_image_zoom(&mut view);
    let zoom = super::super::inline_images::Zoom {
        level: 3,
        left: 0.2,
    };
    view.event_detail.scroll.borrow_mut().zooms.insert(2, zoom);
    assert!(!view.image_zoom_mouse(MouseEvent {
        modifiers: KeyModifiers::CONTROL,
        ..image_mouse(MouseEventKind::Down(MouseButton::Left), 30, 12)
    }));
    assert_eq!(view.event_detail.scroll.borrow().zooms.get(&2), Some(&zoom));
}

#[test]
fn image_zoom_switching_tasks_does_not_park_zoom_mode() {
    let (_directory, mut view) = image_fixture();
    let original = view.selected_row().unwrap().clone();
    let mut other = original.clone();
    other.id.id = "image-other".into();
    let other_id = other.id.clone();
    view.set_sessions(vec![original.clone(), other], false);
    drawn_image(&view);
    enter_image_zoom(&mut view);
    view.event_detail.scroll.borrow_mut().zooms.insert(
        2,
        super::super::inline_images::Zoom {
            level: 3,
            left: 0.2,
        },
    );
    let other = view.rows.iter().position(|row| row.id == other_id).unwrap();
    view.set_selected(other);
    let original = view
        .rows
        .iter()
        .position(|row| row.id == original.id)
        .unwrap();
    view.set_selected(original);
    assert_eq!(view.event_detail.scroll.borrow().zoom_image, None);
    assert!(view.event_detail.scroll.borrow().zooms.is_empty());
}
