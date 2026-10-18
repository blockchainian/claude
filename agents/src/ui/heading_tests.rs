use super::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn headings_center_at_wide_and_narrow_widths() {
    for grouping in [Grouping::Project, Grouping::Model, Grouping::Status] {
        for width in [1, 8, 12, 13, 20, 39, 80, 120] {
            let mut view = test_view(grouping, false);
            view.set_sessions(
                vec![session("heading", "Task", SessionStatus::Ready, 0)],
                false,
            );
            let area = Rect::new(3, 2, width, 6);
            let mut buf = Buffer::empty(Rect::new(0, 0, width + 6, 10));
            view.render_center_rows(area, &mut buf, Utc::now());
            let label = match grouping {
                Grouping::Project => {
                    line_truncation::abbreviate_path("/tmp/project", usize::from(width), None)
                }
                Grouping::Model => "Unknown".to_owned(),
                Grouping::Status => "Ready".to_owned(),
            };
            let label = line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::from(label),
                usize::from(width),
            )
            .to_string();
            let padding = (usize::from(width) - label.width()) / 2;
            let drawn = (area.x..area.right())
                .map(|x| buf[(x, area.y)].symbol())
                .collect::<String>();
            assert_eq!(
                drawn.trim_end(),
                format!("{}{label}", " ".repeat(padding)),
                "{grouping:?}, {width}"
            );
        }
    }
}

#[test]
fn centered_project_heading_clicks_and_underline_cover_only_text() {
    for width in [8, 13, 20, 39, 80, 120] {
        let mut view = test_view(Grouping::Project, false);
        view.set_sessions(
            vec![session("heading", "Task", SessionStatus::Ready, 0)],
            false,
        );
        let area = Rect::new(3, 2, width, 6);
        let mut buf = Buffer::empty(Rect::new(0, 0, width + 6, 10));
        view.render_center_rows(area, &mut buf, Utc::now());
        let label =
            line_truncation::abbreviate_path("/tmp/project", usize::from(width).min(64), None);
        let start = area.x + (width - label.width() as u16) / 2;
        let end = start + label.width() as u16;
        let links = view.viewport.borrow().list_links.clone();
        assert_eq!(
            links,
            vec![(
                Rect::new(start, area.y, end - start, 1),
                PathBuf::from("/tmp/project")
            )]
        );
        links::underline(&mut buf, &links);
        for x in area.x..area.right() {
            let on_text = (start..end).contains(&x);
            assert_eq!(
                buf[(x, area.y)].symbol().contains("\x1b[4:4m"),
                on_text,
                "underline x={x}, width={width}"
            );
            assert_eq!(
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), x, area.y)
                ),
                None
            );
            let action =
                view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1));
            assert_eq!(
                action,
                on_text.then(|| Action::OpenPath("/tmp/project".into())),
                "click x={x}, width={width}"
            );
            if on_text {
                assert_eq!(
                    click_mouse(
                        &mut view,
                        mouse(MouseEventKind::Down(MouseButton::Left), x, area.y)
                    ),
                    None
                );
                assert_eq!(
                    click_mouse(
                        &mut view,
                        mouse(MouseEventKind::Down(MouseButton::Left), x, area.y)
                    ),
                    Some(Action::NewDraft {
                        cwd: "/tmp/project".into()
                    })
                );
            } else {
                assert_eq!(
                    click_mouse(
                        &mut view,
                        mouse(MouseEventKind::Down(MouseButton::Left), x, area.y)
                    ),
                    None
                );
                assert_eq!(
                    view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1)),
                    None
                );
            }
        }
    }
}
