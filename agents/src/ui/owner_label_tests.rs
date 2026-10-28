use super::panel_frame_tests::{draw, fixture, label};
use super::*;

#[test]
fn expanded_details_keep_directory_and_pull_request_links_separate() {
    for width in [90, 120, 160] {
        let (_dir, mut view, now) = fixture();
        view.rows[0].pr = Some(crate::model::PullRequest {
            number: 12,
            url: "https://example.com/pull/12".into(),
        });
        view.rows[0].title = "Name".into();
        view.height_zoom = Some(Panel::TaskDetails);
        view.width_zoom = Some(Panel::TaskDetails);
        let area = Rect::new(0, 0, width, 60);
        let buf = draw(&view, area, now);
        let viewport = view.viewport.borrow();
        let directory = viewport
            .task_details_links
            .iter()
            .find(|(_, path)| path == &crate::ui::links::LinkTarget::Path(view.rows[0].cwd.clone()))
            .expect("directory link")
            .0;
        let pr = viewport
            .task_details_links
            .iter()
            .find(|(_, path)| {
                crate::ui::link_path(path)
                    .to_string_lossy()
                    .contains("/pull/12")
            })
            .expect("pull request link")
            .0;
        assert!(!directory.intersects(pr));
        let text = |rect: Rect| {
            (rect.x..rect.right())
                .map(|x| cell_text(buf[(x, rect.y)].symbol()))
                .collect::<String>()
        };
        assert_eq!(text(directory), "frame-project");
        assert_eq!(pr.width, 3);
        assert!(text(pr).contains("#12"));
    }
}

#[test]
fn task_labels_follow_visible_owner_and_borrow_only_visible_list_edges() {
    for width in [90, 120, 160] {
        let (_dir, mut view, now) = fixture();
        view.rows[0].title = "Name".into();
        view.rows[0].git_branch = Some("topic".into());
        view.rows[0].cwd = _dir.path().join("dir");
        std::fs::create_dir(&view.rows[0].cwd).unwrap();
        view.project_groups[view.selected].heading = "dir".into();
        view.rows[0].started_at = Some(now - chrono::Duration::seconds(65));
        view.set_context_usage(&view.rows[0].id.clone(), Some(12));
        let area = Rect::new(3, 5, width, 24);
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for commands in ["", "|", "_", "|_", "_|"] {
                view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                view.handle_key(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE));
                draw(&view, area, now);
                view.set_active_panel(panel);
                for command in commands.chars() {
                    view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                    view.handle_key(KeyEvent::new(KeyCode::Char(command), KeyModifiers::SHIFT));
                }
                let layout = view.center_layout(area);
                let buf = draw(&view, area, now);
                let viewport = view.viewport.borrow();
                let models: Vec<_> = viewport
                    .field_targets
                    .iter()
                    .filter(|(_, f)| matches!(f, Field::Model | Field::Effort))
                    .copied()
                    .collect();
                let context = format!("{width} {panel:?} {commands}");
                if layout.task_details.is_empty() {
                    assert!(
                        models.is_empty(),
                        "hidden model targets: {context}: {models:?}"
                    );
                    assert!(
                        viewport.task_details_title.is_empty(),
                        "hidden title: {context}"
                    );
                    for y in [
                        layout.header.bottom() - 1,
                        layout.prompt_top_rule.y,
                        layout.prompt_bottom_rule.y,
                    ] {
                        let text: String = (area.x..area.right())
                            .map(|x| buf[(x, y)].symbol())
                            .collect();
                        for forbidden in ["dir", "topic", "Name", "medium", "1m 5s", "12%", "Ready"]
                        {
                            assert!(
                                !text.contains(forbidden),
                                "hidden {forbidden}: {context}: {text}"
                            );
                        }
                    }
                    continue;
                }
                let top = layout.task_details.y - 1;
                let bottom = layout.task_details.bottom();
                let context_label = label(&buf, "12%", top);
                let state = label(&buf, SessionStatus::Ready.label(), top);
                let time = label(&buf, "1m 5s", top);
                let left = if layout.task_details.x == layout.top.x {
                    area.x
                } else {
                    layout.gap.x + HORIZONTAL_PADDING
                };
                let right = if layout.task_details.right() == layout.top.right() {
                    area.right()
                } else {
                    layout.event_panel.x - HORIZONTAL_PADDING
                };
                assert_eq!(context_label.x, left + 2, "{context}");
                assert_eq!(time.right(), right - 2, "{context}");
                assert!(
                    (i32::from(state.x * 2 + state.width) - i32::from(left + right)).abs() <= 1,
                    "{context}"
                );
                assert_eq!(models.len(), 2, "{context}");
                let model = models.iter().find(|(_, f)| *f == Field::Model).unwrap().0;
                let effort = models.iter().find(|(_, f)| *f == Field::Effort).unwrap().0;
                let folder = label(&buf, "dir", bottom);
                let branch = label(&buf, "topic", bottom);
                let name = viewport.task_details_title;
                assert_eq!(name.y, bottom, "{context}");
                if !layout.list.is_empty() {
                    let y = if width < 100 {
                        layout.list.y - 1
                    } else {
                        layout.list.bottom()
                    };
                    assert_eq!(model.y, y, "{context}");
                    assert_eq!(effort.y, y, "{context}");
                    assert_eq!(model.x, layout.list.x - HORIZONTAL_PADDING + 2, "{context}");
                    assert_eq!(
                        effort.right(),
                        if width >= 100 {
                            layout.gap.x + HORIZONTAL_PADDING - 1
                        } else {
                            area.right() - 2
                        },
                        "{context}"
                    );
                    assert_eq!(folder.x, left + 2, "{context}");
                } else {
                    assert_eq!(model.y, bottom, "{context}");
                    assert_eq!(effort.y, bottom, "{context}");
                    assert_eq!(model.x, left + 2, "{context}");
                    assert!(
                        model.right() < effort.x && effort.right() < name.x,
                        "{context}"
                    );
                    assert!(
                        name.right() < folder.x && folder.right() < branch.x,
                        "{context}"
                    );
                    let dot: String = (model.right()..effort.x)
                        .map(|x| buf[(x, bottom)].symbol())
                        .collect();
                    assert_eq!(dot, " · ", "{context}");
                }
                assert_eq!(branch.right(), right - 2, "{context}");
                assert!(
                    (i32::from(name.x * 2 + name.width) - i32::from(left + right)).abs() <= 1,
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn frame_joints_exist_only_at_visible_column_separators() {
    for width in [90, 120, 160] {
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for commands in ["", "|", "_", "|_"] {
                let (_dir, mut view, now) = fixture();
                let area = Rect::new(0, 0, width, 60);
                draw(&view, area, now);
                view.set_active_panel(panel);
                for c in commands.chars() {
                    view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
                    view.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT));
                }
                let layout = view.center_layout(area);
                let buf = draw(&view, area, now);
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        if matches!(buf[(x, y)].symbol(), "┬" | "┴" | "┼") {
                            assert!(
                                width >= 100,
                                "small joint {width} {panel:?} {commands} {x},{y}"
                            );
                            let separator = (!layout.gap.is_empty()
                                && x == layout.gap.x + HORIZONTAL_PADDING)
                                || (width >= 150
                                    && !layout.event_panel.is_empty()
                                    && x == layout.event_panel.x - HORIZONTAL_PADDING - 1);
                            assert!(
                                separator,
                                "extra joint {width} {panel:?} {commands} {x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }
}
