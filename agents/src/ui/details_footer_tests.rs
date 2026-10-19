use super::*;

#[test]
fn details_body_leaves_spare_rows_without_project_or_model() {
    for width in [25, 60] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.prompt = "Short prompt".into();
        selected.last_message = None;
        selected.activity = None;
        for height in [6, 7, 8, 30] {
            let area = Rect::new(7, 4, width, height);
            let mut buf = Buffer::empty(area);
            view.render_details(area, &mut buf, Utc::now());
            let text = |y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            };
            assert_eq!(text(area.y).trim(), "Prompt");
            assert_eq!(text(area.y + 1).trim(), "Short prompt");
            for y in area.y + 2..area.bottom() {
                assert!(text(y).trim().is_empty(), "unexpected row {y}: {}", text(y));
            }
            let viewport = view.viewport.borrow();
            assert_eq!(viewport.details_copy[0].0, area.y + 1..area.y + 2);
            assert_eq!(viewport.details_text.rows[0].y, area.y + 1);
            assert!(viewport.field_targets.is_empty());
            assert!(viewport.details_links.is_empty());
            assert_eq!(viewport.details_scroll.hidden, vec![0]);
        }
    }
}

#[test]
fn details_status_links_keep_underlines_and_click_targets_in_both_layouts() {
    for width in [80, 160] {
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
        view.underline_links(&mut buf);
        let area = view.viewport.borrow().details;
        let links = view.viewport.borrow().details_links.clone();
        for target in [
            PathBuf::from("https://example.com/pull/833"),
            PathBuf::from("/tmp/project"),
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
fn log_border_places_model_label_and_effort_targets_in_both_layouts() {
    for width in [80, 149, 150, 220] {
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
            let y = if width >= 150 {
                layout.log.bottom()
            } else {
                layout.log.y - 1
            };
            assert_eq!(model.y, y);
            assert_eq!(effort.y, y);
            assert_eq!(model.x, layout.log.x + u16::from(width < 150));
            assert_eq!(effort.x, model.right() + 3);
            let text = |rect: Rect| {
                (rect.x..rect.right())
                    .map(|x| buf[(x, rect.y)].symbol())
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
fn model_effort_border_groups_optional_values_and_right_aligns_new() {
    for width in [80, 149, 150, 220] {
        for (model, effort, expected) in [
            (Some("Opus"), Some("high"), "Opus · high"),
            (Some("Opus"), None, "Opus · Default"),
            (None, Some("high"), "Unknown · high"),
            (None, None, "Unknown · Default"),
        ] {
            for unseen in [0, 3] {
                let mut view = fixture();
                let selected = &mut view.rows[view.selected];
                selected.model = model.map(str::to_owned);
                selected.effort = effort.map(str::to_owned);
                view.log = super::log_fixture().0.log;
                view.select_log_event(Some(0));
                view.viewport.borrow_mut().log_unseen = unseen;
                let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
                view.render(buf.area, &mut buf);
                let viewport = view.viewport.borrow();
                let model_y = if width >= 150 {
                    viewport.log.bottom()
                } else {
                    viewport.log.y - 1
                };
                let line: String = (viewport.log.x + u16::from(width < 150)..buf.area.right())
                    .map(|x| buf[(x, model_y)].symbol())
                    .collect();
                if !expected.is_empty() {
                    assert!(line.starts_with(expected), "{line}");
                }
                assert_eq!(line.contains('·'), true, "{line}");
                if unseen == 0 {
                    assert!(viewport.log_jump.is_empty());
                } else {
                    let jump = viewport.log_jump;
                    assert_eq!(jump.y, model_y);
                    assert_eq!(
                        jump.right() - 1,
                        super::super::log::log_row_areas(viewport.log).1.right()
                    );
                    let text: String = (jump.x..jump.right())
                        .map(|x| buf[(x, jump.y)].symbol())
                        .collect();
                    assert_eq!(text, " ↓ 3 New ");
                }
            }
        }
    }
}
