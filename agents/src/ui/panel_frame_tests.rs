use super::*;
use crate::providers::Provider;

pub(super) fn fixture() -> (tempfile::TempDir, View, DateTime<Utc>) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().into());
    let mut sessions = provider.refresh(1).sessions;
    let now = sessions[0].updated_at;
    sessions[0].status = SessionStatus::Ready;
    sessions[0].cwd = dir.path().join("frame-project");
    std::fs::create_dir(&sessions[0].cwd).unwrap();
    sessions[0].model = Some("claude-opus-4-6".into());
    sessions[0].effort = Some("medium".into());
    sessions[0].git_branch = Some("frame-labels".into());
    sessions[0].title = "Frame labels".into();
    let id = sessions[0].id.clone();
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(sessions, false);
    view.event_session = Some(id);
    view.apply_event_update(crate::event::Update {
        events: crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true)
            .unwrap(),
        reset: true,
        ..Default::default()
    });
    view.event_list.filter = crate::event::EventFilter::Message;
    view.event_list.sort = EventSort::Duration;
    (dir, view, now)
}

pub(super) fn draw(view: &View, area: Rect, now: DateTime<Utc>) -> Buffer {
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    buf
}

pub(super) fn label(buf: &Buffer, text: &str, y: u16) -> Rect {
    for x in buf.area.x..buf.area.right() {
        if (x..buf.area.right())
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect::<String>()
            .starts_with(text)
        {
            return Rect::new(x, y, text.width() as u16, 1);
        }
    }
    panic!(
        "missing {text:?} on row {y}: {}",
        (buf.area.x..buf.area.right())
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect::<String>()
    );
}

#[test]
fn frame_labels_restore_after_every_zoom() {
    for width in [90, 120, 160] {
        let (_dir, mut view, now) = fixture();
        let area = Rect::new(0, 0, width, 24);
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for commands in ["_", "|", "_|", "|_"] {
                view.set_active_panel(panel);
                let baseline = draw(&view, area, now);
                for command in commands.chars() {
                    view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                    view.handle_key(KeyEvent::new(KeyCode::Char(command), KeyModifiers::SHIFT));
                }
                let actual = draw(&view, area, now);
                let layout = view.center_layout(area);
                for y in [layout.prompt_top_rule.y, layout.prompt_bottom_rule.y] {
                    assert!((area.x..area.right()).all(|x| {
                        !actual[(x, y)]
                            .symbol()
                            .chars()
                            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                    }));
                }
                view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                view.handle_key(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE));
                assert_eq!(
                    draw(&view, area, now),
                    baseline,
                    "{width} {panel:?} {commands}"
                );
            }
        }
    }
}

#[test]
fn short_windows_keep_both_prompt_rules() {
    for width in [200, 110] {
        for commands in ["", "|", "_|"] {
            let (_dir, mut view, now) = fixture();
            view.width_zoom = commands.contains('|').then_some(Panel::TaskList);
            view.height_zoom = commands.contains('_').then_some(Panel::TaskList);
            let area = Rect::new(0, 0, width, 18);
            let buf = draw(&view, area, now);
            let layout = view.center_layout(area);
            assert!(layout.event_panel.is_empty());
            assert_eq!(layout.prompt_top_rule.bottom(), layout.prompt.y);
            assert_eq!(layout.prompt_bottom_rule.y, layout.prompt.bottom());
            assert_eq!(layout.prompt_bottom_rule, layout.footer_rule);
            for y in [layout.prompt.y - 1, layout.prompt.bottom()] {
                for x in [area.x, area.right() - 1] {
                    assert_eq!(buf[(x, y)].symbol(), "─", "{width} {commands} ({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn merged_rule_keeps_borrowed_settings_only_while_owner_is_visible() {
    for width in [40, 110] {
        let (_dir, mut view, now) = fixture();
        view.rows[0].model =
            Some("model-with-a-long-name-that-fills-the-rest-of-the-frame-rule".into());
        let area = Rect::new(0, 0, width, 50);
        let before = draw(&view, area, now);
        let models: Vec<_> = view
            .viewport
            .borrow()
            .field_targets
            .iter()
            .filter(|(_, field)| matches!(field, Field::Model | Field::Effort))
            .map(|(rect, _)| *rect)
            .collect();
        view.height_zoom = Some(Panel::TaskList);
        let after = draw(&view, area, now);
        let layout = view.center_layout(area);
        let targets = view.viewport.borrow().field_targets.clone();
        if width < 100 {
            assert!(
                targets
                    .iter()
                    .all(|(_, field)| !matches!(field, Field::Model | Field::Effort))
            );
        } else {
            for rect in models {
                let y = layout.list.bottom();
                for x in rect.x..rect.right() {
                    assert_eq!(after[(x, y)], before[(x, rect.y)], "{width} ({x},{y})");
                }
            }
        }
    }
}

#[test]
fn small_layout_stacks_full_width_panels_with_double_middle_heights() {
    let (_dir, view, _) = fixture();
    for height in [45, 60, 61] {
        let small = view.panel_layout(Rect::new(0, 0, 70, height), false);
        let medium = view.panel_layout(Rect::new(0, 0, 100, height), false);
        let rows = height - small.header.height - small.footer.height - small.prompt.height - 5;
        let list = rows / 6;
        let details = (rows - 2 * list).div_ceil(2);
        assert_eq!(small.list, Rect::new(1, 2, 68, list));
        assert_eq!(
            small.task_details,
            Rect::new(1, small.list.bottom() + 1, 68, details)
        );
        assert_eq!(small.event_panel.height, rows - 2 * list - details);
        assert_eq!(small.event_detail.height - 1, list);
        assert_eq!(small.task_details.bottom(), small.top.bottom());
        assert_eq!(small.gap, Rect::new(0, small.list.bottom(), 70, 1));
        assert_eq!(small.usage_chart, small.list);
        assert_eq!(medium.list.y, medium.task_details.y);
        assert_eq!(medium.list.height, medium.task_details.height);
        assert!(medium.list.right() < medium.task_details.x);
    }
}

#[test]
fn small_layout_labels_belong_to_the_three_task_edges() {
    let (_dir, mut view, now) = fixture();
    view.rows[0].started_at = Some(now - chrono::Duration::seconds(65));
    view.set_context_usage(&view.rows[0].id.clone(), Some(12));
    let area = Rect::new(0, 0, 70, 60);
    let layout = view.panel_layout(area, false);
    let buf = draw(&view, area, now);
    let top = layout.header.bottom() - 1;
    let middle = layout.list.bottom();
    let bottom = layout.prompt_top_rule.y;
    let model = label(
        &buf,
        &view.model_text(ProviderId::Claude, "claude-opus-4-6"),
        top,
    );
    let effort = label(&buf, "medium", top);
    assert_eq!(model.x, 2);
    assert_eq!(effort.right(), area.right() - 2);
    let state = label(&buf, SessionStatus::Ready.label(), middle);
    assert!((i32::from(state.x * 2 + state.width) - 70).abs() <= 1);
    let elapsed = label(&buf, "1m 5s", middle);
    assert_eq!(elapsed.right(), 68);
    let context = view.selected_context_percent().unwrap();
    assert_eq!(label(&buf, &format!("{context}%"), middle).x, 2);
    label(&buf, "frame-project", bottom);
    label(&buf, "Frame labels", bottom);
    label(&buf, "frame-labels", bottom);
    let middle_text = (0..70)
        .map(|x| cell_text(buf[(x, middle)].symbol()))
        .collect::<String>();
    assert!(
        !middle_text
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
        "{middle_text}"
    );
    assert!(
        middle_text.contains("─"),
        "gap keeps its rule: {middle_text}"
    );
    for cell in &buf.content {
        assert!(
            !matches!(cell.symbol(), "│" | "┬" | "┴" | "┼"),
            "unexpected column separator: {cell:?}"
        );
    }
}

#[test]
fn small_layout_event_controls_share_the_event_list_bottom_edge() {
    for sort in [EventSort::Time, EventSort::Duration] {
        let (_dir, mut view, now) = fixture();
        view.event_list.sort = sort;
        view.event_list.cursor = Some(0);
        view.event_list.unseen.set(2);
        let area = Rect::new(0, 0, 70, 60);
        let buf = draw(&view, area, now);
        let layout = view.center_layout(area);
        assert_eq!(
            label(&buf, "Assistant", layout.event_detail_top_rule.y).x,
            1
        );
        let jump = view.viewport.borrow().event_label;
        assert_eq!(jump.y, layout.event_detail_top_rule.y);
        assert!(!jump.is_empty());
    }
}

#[test]
fn layout_breakpoints_select_the_three_panel_arrangements() {
    for (width, expected) in [
        (0, ScreenLayout::Small),
        (70, ScreenLayout::Small),
        (80, ScreenLayout::Small),
        (99, ScreenLayout::Small),
        (100, ScreenLayout::Medium),
        (149, ScreenLayout::Medium),
        (150, ScreenLayout::Large),
        (200, ScreenLayout::Large),
    ] {
        assert_eq!(ScreenLayout::for_width(width), expected, "width {width}");
    }
}

#[test]
fn small_task_height_zoom_shortcut_shows_only_target() {
    for panel in [Panel::TaskList, Panel::TaskDetails] {
        let (_dir, mut view, now) = fixture();
        let area = Rect::new(0, 0, 90, 60);
        draw(&view, area, now);
        view.set_active_panel(panel);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('|'), KeyModifiers::SHIFT)),
            None
        );
        assert_eq!(view.height_zoom, Some(panel));
        let layout = view.center_layout(area);
        assert_eq!(
            (layout.list.height, layout.task_details.height),
            if panel == Panel::TaskList {
                (54, 0)
            } else {
                (0, 54)
            }
        );
        assert!(layout.gap.is_empty());
        assert!(layout.event_panel.is_empty());
        assert!(layout.event_detail.is_empty());
    }
}
