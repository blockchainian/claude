use super::*;

#[test]
fn details_footer_pins_project_and_model_below_spare_rows() {
    for width in [25, 60] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.prompt = "Short prompt".into();
        selected.last_message = None;
        selected.activity = None;
        let area = Rect::new(7, 4, width, 30);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        let text = |y| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(text(area.bottom() - 1).starts_with("Model"));
        assert!(text(area.bottom() - 2).trim().is_empty());
        assert_eq!(text(area.bottom() - 3).trim(), "/tmp/project");
        assert!(text(area.bottom() - 4).trim() == "Project");
        for y in area.y + 2..area.bottom() - 4 {
            assert!(text(y).trim().is_empty(), "unexpected row {y}: {}", text(y));
        }
        let viewport = view.viewport.borrow();
        assert_eq!(viewport.details_copy[0].0, area.y + 1..area.y + 2);
        assert_eq!(viewport.details_text.rows[0].y, area.y + 1);
        assert!(
            viewport
                .field_targets
                .iter()
                .any(|(rect, field)| { *field == Field::Model && rect.y == area.bottom() - 1 })
        );
        assert_eq!(viewport.details_scroll.hidden, vec![0]);
    }
}

#[test]
fn details_footer_shifted_links_keep_click_targets_in_both_layouts() {
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
        let project_y = area.bottom() - 4;
        let links = view.viewport.borrow().details_links.clone();
        for (target, y) in [
            (PathBuf::from("https://example.com/pull/833"), project_y + 1),
            (PathBuf::from("/tmp/project"), project_y + 1),
        ] {
            let rect = links.iter().find(|(_, path)| *path == target).unwrap().0;
            assert_eq!(rect.y, y);
            assert!(buf[(rect.x, y)].symbol().contains("\x1b[4:4m"));
            assert_eq!(
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), rect.x, y)
                ),
                Some(Action::OpenPath(target))
            );
        }
    }
}

#[test]
fn details_footer_keeps_filled_and_overflowing_rows_unchanged() {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.prompt = "Short prompt".into();
    selected.last_message = None;
    selected.activity = None;
    for (height, scroll, expected) in [
        (
            8,
            0,
            vec![
                "Prompt",
                "Short prompt",
                "",
                "",
                "Project",
                "/tmp/project",
                "",
                "Model",
            ],
        ),
        (
            7,
            0,
            vec![
                "Prompt",
                "Short prompt",
                "",
                "Project",
                "/tmp/project",
                "",
                "Model",
            ],
        ),
        (
            6,
            1,
            vec![
                "Short prompt",
                "",
                "Project",
                "/tmp/project",
                "",
                "Model",
            ],
        ),
    ] {
        let area = Rect::new(7, 4, 40, height);
        view.viewport.borrow_mut().details_scroll.offset = scroll;
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        for (y, expected) in (area.y..area.bottom()).zip(expected) {
            let text = (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>();
            if expected == "Model" {
                assert!(text.starts_with(expected), "{text}");
            } else {
                assert_eq!(text.trim(), expected);
            }
        }
    }
}

#[test]
fn hidden_model_row_has_no_field_targets() {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.prompt = "Short prompt".into();
    selected.last_message = None;
    selected.activity = None;
    let area = Rect::new(7, 4, 40, 6);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    assert!(view.viewport.borrow().field_targets.is_empty());
}
