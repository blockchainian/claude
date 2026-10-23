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
                    let model_y = if ScreenLayout::for_width(width) == ScreenLayout::Large {
                        original.prompt_top_rule.y
                    } else {
                        original.prompt_bottom_rule.y
                    };
                    let filter_y = if ScreenLayout::for_width(width) == ScreenLayout::Large {
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
                        if ScreenLayout::for_width(width) == ScreenLayout::Large {
                            layout.prompt_top_rule.y
                        } else {
                            layout.prompt_bottom_rule.y
                        },
                        if ScreenLayout::for_width(width) == ScreenLayout::Large {
                            layout.prompt_top_rule.y
                        } else {
                            layout.event_detail_top_rule.y
                        },
                    ];
                    for (label, rule) in labels {
                        assert!(!label.is_empty());
                        let shifted = rule == 2
                            && label.width == 9
                            && ScreenLayout::for_width(width) != ScreenLayout::Large
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
        let filter_y = if width < 100 {
            view.panel_layout(area, false).prompt_bottom_rule.y
        } else {
            view.panel_layout(area, false).event_detail_top_rule.y
        };
        label(&before, "Assistant", filter_y);
        view.height_zoom = Some(Panel::TaskList);
        let after = draw(&view, area, now);
        let layout = view.center_layout(area);
        let y = layout.prompt_bottom_rule.y;
        assert_eq!(y, layout.event_detail_top_rule.y);
        for rect in &models {
            let y = if width < 100 { rect.y } else { y };
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
        if width < 100 {
            assert_eq!(moved_jump.right(), jump.right());
        } else {
            assert_eq!(moved_jump.right() + 1, model_left);
        }
        if moved_jump.right() < area.right() {
            assert_eq!(after[(moved_jump.right(), y)].symbol(), "─");
        }
        label(&after, "duration", y);
        if width == 110 {
            let filter = label(&after, "Assistant", y);
            assert_eq!(filter.right() + 2, moved_jump.x);
            assert_eq!(after[(filter.right() + 1, y)].symbol(), "─");
        } else {
            let row = (area.x..area.right())
                .map(|x| cell_text(after[(x, y)].symbol()))
                .collect::<String>();
            assert!(row.contains("Assistant"), "{row}");
        }
    }
}

#[test]
fn small_layout_stacks_full_width_panels_inside_medium_upper_third() {
    let (_dir, view, _) = fixture();
    for height in [45, 60, 61] {
        let small = view.panel_layout(Rect::new(0, 0, 70, height), false);
        let medium = view.panel_layout(Rect::new(0, 0, 100, height), false);
        assert_eq!(small.top.height, medium.top.height);
        assert_eq!(
            small.prompt,
            Rect {
                width: 70,
                ..medium.prompt
            }
        );
        let rows = small.top.height - 1;
        assert_eq!(small.list, Rect::new(1, 2, 68, rows.div_ceil(2)));
        assert_eq!(
            small.task_details,
            Rect::new(1, small.list.bottom() + 1, 68, rows / 2)
        );
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
        middle_text
            .chars()
            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
        "{middle_text}"
    );
    assert!(
        !middle_text.contains("─"),
        "timeline should span the entire edge: {middle_text}"
    );
    for cell in &buf.content {
        assert!(
            !matches!(cell.symbol(), "│" | "┬" | "┴" | "┼"),
            "unexpected column separator: {cell:?}"
        );
    }
}

#[test]
fn small_layout_event_controls_share_the_rule_above_the_event_list() {
    for sort in [EventSort::Time, EventSort::Duration] {
        let (_dir, mut view, now) = fixture();
        view.event_list.sort = sort;
        view.event_list.cursor = Some(0);
        view.event_list.unseen.set(2);
        let area = Rect::new(0, 0, 70, 60);
        let buf = draw(&view, area, now);
        let layout = view.center_layout(area);
        assert_eq!(label(&buf, "Assistant", layout.prompt_bottom_rule.y).x, 2);
        let jump = view.viewport.borrow().event_label;
        assert_eq!(jump.y, layout.prompt_bottom_rule.y);
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
