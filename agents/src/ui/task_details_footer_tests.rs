use super::*;

#[test]
fn task_details_body_leaves_spare_rows_without_project_or_model() {
    for width in [25, 60] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.prompt = "Short prompt".into();
        selected.last_message = None;
        selected.activity = None;
        for height in [6, 7, 8, 30] {
            let area = Rect::new(7, 4, width, height);
            let mut buf = Buffer::empty(area);
            view.render_task_details(area, &mut buf, Utc::now());
            let text = |y| {
                (area.x..area.right())
                    .map(|x| cell_text(buf[(x, y)].symbol()))
                    .collect::<String>()
            };
            assert_eq!(text(area.y).trim(), "Prompt");
            assert_eq!(text(area.y + 1).trim(), "Short prompt");
            for y in area.y + 2..area.bottom() {
                assert!(text(y).trim().is_empty(), "unexpected row {y}: {}", text(y));
            }
            let viewport = view.viewport.borrow();
            assert_eq!(viewport.task_details_copy[0].0, area.y + 1..area.y + 2);
            assert_eq!(viewport.task_details_text.rows[0].y, area.y + 1);
            assert!(viewport.field_targets.is_empty());
            assert!(viewport.task_details_links.is_empty());
            assert_eq!(viewport.task_details_scroll.hidden, vec![0]);
        }
    }
}

#[test]
fn task_details_status_links_keep_underlines_and_click_targets_in_both_layouts() {
    for width in [100, 160] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.prompt = "Short prompt".into();
        selected.last_message = None;
        selected.activity = None;
        selected.pr = Some(crate::model::PullRequest {
            number: 833,
            url: "https://example.com/pull/833".into(),
        });
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
        view.render(buf.area, &mut buf);
        let area = view.viewport.borrow().task_details;
        let links = view.viewport.borrow().task_details_links.clone();
        for target in [
            PathBuf::from("https://example.com/pull/833"),
            crate::test_project(),
        ] {
            let rect = links.iter().find(|(_, path)| *path == target).unwrap().0;
            assert_eq!(rect.y, area.bottom());
            assert!(buf[(rect.x, rect.y)].symbol().contains("\x1b[4:4m"));
            assert_eq!(
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y)
                ),
                Some(Action::OpenPath(target))
            );
        }
    }
}

#[test]
fn event_border_places_model_label_and_effort_targets_in_both_layouts() {
    for width in [100, 149, 150, 220] {
        for label in [None, Some("Opus 5.5")] {
            let mut view = fixture();
            let selected = &mut view.rows[view.selected];
            selected.transcript = Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/claude-model-identity.jsonl"),
            );
            selected.model = Some("claude-opus-5-5".into());
            if let Some(label) = label {
                view.input_choices
                    .entry(ProviderId::Claude)
                    .or_default()
                    .display_names
                    .insert("claude-opus-5-5".into(), label.into());
            }
            selected.effort = Some("high".into());
            let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
            let layout = view.center_layout(buf.area);
            view.render(buf.area, &mut buf);
            let viewport = view.viewport.borrow();
            let model = viewport
                .field_targets
                .iter()
                .find(|(_, field)| *field == Field::Model)
                .unwrap()
                .0;
            let effort = viewport
                .field_targets
                .iter()
                .find(|(_, field)| *field == Field::Effort)
                .unwrap()
                .0;
            let y = layout.list.bottom();
            assert_eq!(model.y, y);
            assert_eq!(effort.y, y);
            assert_eq!(model.x, 2);
            assert_eq!(effort.right(), layout.gap.x);
            let text = |rect: Rect| {
                (rect.x..rect.right())
                    .map(|x| cell_text(buf[(x, rect.y)].symbol()))
                    .collect::<String>()
            };
            assert_eq!(text(model), label.unwrap_or("claude-opus-5-5"));
            assert_eq!(text(effort), "high");
            let mut reference = ratatui::buffer::Cell::default();
            reference.set_style(style::time_style());
            assert_eq!(buf[(model.x, y)].style(), reference.style());
            assert_eq!(buf[(effort.x, y)].style(), reference.style());
            assert!(model.right() < effort.x);
        }
    }
}

#[test]
fn model_effort_border_shows_only_known_values_and_right_aligns_new() {
    for width in [100, 149, 150, 220] {
        for (model, effort, _expected) in [
            (Some("Opus"), Some("high"), "Opus · high"),
            (Some("Opus"), None, "Opus "),
            (None, Some("high"), "high "),
            (None, None, ""),
        ] {
            for unseen in [0, 3] {
                let mut view = fixture();
                let selected = &mut view.rows[view.selected];
                selected.model = model.map(str::to_owned);
                selected.effort = effort.map(str::to_owned);
                view.event_panel = super::event_fixture().0.event_panel;
                view.select_event(Some(0));
                view.event_list.unseen.set(unseen);
                view.event_list.restoring = false;
                let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
                view.render(buf.area, &mut buf);
                let viewport = view.viewport.borrow();
                let layout = view.center_layout(buf.area);
                let model_y = layout.list.bottom();
                let line: String = (0..layout.gap.x + 2)
                    .map(|x| cell_text(buf[(x, model_y)].symbol()))
                    .collect();
                for (field, value) in [(Field::Model, model), (Field::Effort, effort)] {
                    let target = viewport.field_targets.iter().find(|(_, key)| *key == field);
                    assert_eq!(target.is_some(), value.is_some(), "{line}");
                    if let (Some((rect, _)), Some(value)) = (target, value) {
                        let text: String = (rect.x..rect.right())
                            .map(|x| cell_text(buf[(x, rect.y)].symbol()))
                            .collect();
                        assert_eq!(text, value);
                        assert_eq!(rect.y, model_y);
                        if field == Field::Model {
                            assert_eq!(rect.x, 2);
                        } else {
                            assert_eq!(rect.right(), layout.gap.x);
                        }
                    }
                }
                assert!(!line.contains('·'), "{line}");
                if unseen == 0 {
                    assert!(viewport.event_jump.is_empty());
                } else {
                    let jump = viewport.event_jump;
                    assert_eq!(jump.y, layout.event_panel.bottom());
                    assert_eq!(
                        jump.right(),
                        super::super::event_list::event_row_areas(viewport.event_panel)
                            .1
                            .right()
                    );
                    let text: String = (jump.x..jump.right())
                        .map(|x| cell_text(buf[(x, jump.y)].symbol()))
                        .collect();
                    assert_eq!(text, " ↓ 3 New");
                }
            }
        }
    }
}

#[test]
fn draft_title_rule_places_directory_left_and_branch_right() {
    for width in [100, 149, 150, 220] {
        let mut view = fixture();
        view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            model: None,
            effort: None,
            branch: "feature/border".into(),
        });
        view.set_active_panel(Panel::TaskDetails);
        view.handle_paste("Border task");
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        view.close_selector();
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
        view.render(buf.area, &mut buf);
        let viewport = view.viewport.borrow();
        let target = |field| {
            viewport
                .field_targets
                .iter()
                .find(|(_, value)| *value == field)
                .unwrap()
                .0
        };
        let directory = target(Field::Directory);
        let branch = target(Field::Branch);
        let title = viewport.task_details_title;
        assert_eq!(directory.y, viewport.task_details.bottom());
        assert_eq!(branch.y, directory.y);
        assert!(directory.right() < title.x);
        assert!(branch.x > title.right());
        let text = |rect: Rect| {
            (rect.x..rect.right())
                .map(|x| cell_text(buf[(x, rect.y)].symbol()))
                .collect::<String>()
        };
        assert_eq!(text(directory), "agents");
        assert_eq!(text(branch), "feature/border");
    }
}
