use super::*;
use crate::providers::Provider;

fn fixture() -> (tempfile::TempDir, View, DateTime<Utc>) {
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
    view.set_activity(
        (0..400)
            .map(|age| crate::usage::bin_of(now) - age)
            .collect(),
    );
    (dir, view, now)
}

fn draw(view: &View, area: Rect, now: DateTime<Utc>) -> Buffer {
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    buf
}

fn label(buf: &Buffer, text: &str, y: u16) -> Rect {
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
fn frame_labels_keep_their_columns_through_every_zoom() {
    let mut failures = Vec::new();
    for width in [110, 200] {
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for sort in [EventSort::Time, EventSort::Duration] {
                for commands in ["_", "|", "_|"] {
                    let (_dir, mut view, now) = fixture();
                    view.event_list.sort = sort;
                    view.event_list.cursor = Some(0);
                    view.event_list.unseen.set(2);
                    let area = Rect::new(0, 0, width, 50);
                    let original = view.panel_layout(area, false);
                    let baseline = draw(&view, area, now);
                    let title = view.viewport.borrow().task_details_title;
                    let model_y = if width >= WIDE_LAYOUT_COLUMNS {
                        original.prompt_top_rule.y
                    } else {
                        original.prompt_bottom_rule.y
                    };
                    let filter_y = if width >= WIDE_LAYOUT_COLUMNS {
                        original.prompt_top_rule.y
                    } else {
                        original.event_detail_top_rule.y
                    };
                    let labels = [
                        (title, 0),
                        (
                            label(&baseline, "frame-project", original.prompt_top_rule.y),
                            0,
                        ),
                        (
                            label(&baseline, "frame-labels", original.prompt_top_rule.y),
                            0,
                        ),
                        (
                            label(
                                &baseline,
                                &format!(
                                    "{} · medium",
                                    view.model_text(ProviderId::Claude, "claude-opus-4-6")
                                ),
                                model_y,
                            ),
                            1,
                        ),
                        (label(&baseline, "Assistant", filter_y), 2),
                        if sort == EventSort::Duration {
                            (label(&baseline, "duration", filter_y), 2)
                        } else {
                            (label(&baseline, "↓ 2 New", model_y), 1)
                        },
                    ];
                    view.set_active_panel(panel);
                    for command in commands.chars() {
                        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                        view.handle_key(KeyEvent::new(KeyCode::Char(command), KeyModifiers::SHIFT));
                    }
                    let layout = view.center_layout(area);
                    let actual = draw(&view, area, now);
                    let ys = [
                        layout.prompt_top_rule.y,
                        if width >= WIDE_LAYOUT_COLUMNS {
                            layout.prompt_top_rule.y
                        } else {
                            layout.prompt_bottom_rule.y
                        },
                        if width >= WIDE_LAYOUT_COLUMNS {
                            layout.prompt_top_rule.y
                        } else {
                            layout.event_detail_top_rule.y
                        },
                    ];
                    for (label, rule) in labels {
                        assert!(!label.is_empty());
                        let shifted = rule == 2
                            && label.width == 9
                            && width < WIDE_LAYOUT_COLUMNS
                            && ys[1] == ys[2];
                        let target_x = if shifted {
                            labels[3].0.x - label.width - 3
                        } else {
                            label.x
                        };
                        for offset in 0..label.width {
                            let x = target_x + offset;
                            if actual[(x, ys[rule])].symbol()
                                != baseline[(label.x + offset, label.y)].symbol()
                            {
                                failures.push(format!(
                                    "{width} {panel:?} {commands}: label {label:?} column {x}"
                                ));
                                break;
                            }
                        }
                        if shifted {
                            assert_eq!(
                                actual[(target_x + label.width + 1, ys[rule])].symbol(),
                                "─"
                            );
                        }
                    }
                    let dots: Vec<_> = (original.list.x..original.list.right())
                        .filter(|x| {
                            baseline[(*x, original.prompt_top_rule.y)]
                                .symbol()
                                .chars()
                                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                        })
                        .collect();
                    assert!(!dots.is_empty());
                    for x in dots {
                        if actual[(x, layout.prompt_top_rule.y)].symbol()
                            != baseline[(x, original.prompt_top_rule.y)].symbol()
                        {
                            failures
                                .push(format!("{width} {panel:?} {commands}: activity column {x}"));
                            break;
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
fn merged_rule_moves_jump_and_filter_without_changing_the_model() {
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
        let jump = view.viewport.borrow().event_label;
        assert!(!jump.is_empty());
        let filter_y = view.panel_layout(area, false).event_detail_top_rule.y;
        label(&before, "Assistant", filter_y);
        view.height_zoom = Some(Panel::TaskList);
        let after = draw(&view, area, now);
        let layout = view.center_layout(area);
        let y = layout.prompt_bottom_rule.y;
        assert_eq!(y, layout.event_detail_top_rule.y);
        for rect in &models {
            for x in rect.x..rect.right() {
                assert_eq!(
                    after[(x, y)],
                    before[(x, rect.y)],
                    "model changed: {width} ({x}, {y})"
                );
            }
        }
        let model_left = models[0].x - 1;
        let moved_jump = view.viewport.borrow().event_label;
        assert_eq!(moved_jump.right() + 1, model_left);
        assert_eq!(after[(moved_jump.right(), y)].symbol(), "─");
        label(&after, "duration", y);
        if width == 110 {
            let filter = label(&after, "Assistant", y);
            assert_eq!(filter.right() + 2, moved_jump.x);
            assert_eq!(after[(filter.right() + 1, y)].symbol(), "─");
        } else {
            let row = (area.x..area.right())
                .map(|x| cell_text(after[(x, y)].symbol()))
                .collect::<String>();
            assert!(!row.contains("Assistant"), "{row}");
        }
    }
}
