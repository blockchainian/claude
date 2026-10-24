use super::*;

#[test]
fn event_content_centers_short_rows_and_keeps_truncated_rows_and_clock() {
    for width in [20, 30, 31, 100, 160] {
        let (mut view, now) = event_fixture();
        let area = Rect::new(3, 2, width, 1);
        let mut clock = None;
        let template = view.event_panel[7].clone();
        for text in [
            "run tests",
            "界面",
            "a long tool message that fills the entire content column and must be truncated to fit",
        ] {
            let mut event = template.clone();
            event.body = text.into();
            view.set_event(vec![event]);
            let mut buf = Buffer::empty(Rect::new(0, 0, width + 6, 5));
            view.render_event(area, &mut buf, now);
            let content_width = width - 11;
            let fitted = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::raw(text),
                usize::from(content_width),
            )
            .to_string();
            let padding = (usize::from(width / 2) - fitted.width() / 2)
                .min(usize::from(content_width) - fitted.width());
            let expected = format!(
                "{}{}{}",
                " ".repeat(padding),
                fitted,
                " ".repeat(usize::from(content_width) - padding - fitted.width())
            );
            let mut actual = String::new();
            let mut x = area.x;
            while x < area.x + content_width {
                let cell = &buf[(x, area.y)];
                actual.push_str(cell.symbol());
                x += cell.symbol().width().max(1) as u16;
            }
            assert_eq!(actual, expected, "width {width}, {text}");
            assert_eq!(buf[(area.right() - 11, area.y)].symbol(), " ");
            assert_eq!(buf[(area.right() - 10, area.y)].symbol(), " ");
            let cells = (area.right() - 9..area.right() - 1)
                .map(|x| buf[(x, area.y)].clone())
                .collect::<Vec<_>>();
            if let Some(expected) = &clock {
                assert_eq!(&cells, expected);
            } else {
                clock = Some(cells);
            }
        }
    }
}

#[test]
fn event_content_centered_links_click_and_underline_exactly_the_path() {
    for width in [100, 180] {
        let (mut view, now) = event_fixture();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("report.md");
        std::fs::write(&target, "report").unwrap();
        view.rows[view.selected].cwd = dir.path().to_owned();
        let mut event = view.event_panel[7].clone();
        event.body = "Read report.md done".into();
        event.paths.clear();
        event.paths.push(5..14);
        view.set_event(vec![event]);
        let area = Rect::new(0, 0, width, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let event_panel = view.viewport.borrow().event_panel;
        let start =
            event_panel.x + (event_panel.width / 2 - 19 / 2).min(event_panel.width - 11 - 19);
        let expected = Rect::new(start + 5, event_panel.y, 9, 1);
        let links = view.viewport.borrow().event_links.clone();
        assert!(
            links.contains(&(expected, crate::ui::links::LinkTarget::Path(target.clone()))),
            "{links:?}"
        );
        let before = buf.clone();
        super::super::links::underline(
            &mut buf,
            &[(expected, crate::ui::links::LinkTarget::Path(target.clone()))],
        );
        for x in event_panel.x..event_panel.right() {
            assert_eq!(
                buf[(x, event_panel.y)] != before[(x, event_panel.y)],
                expected.contains((x, event_panel.y).into())
            );
        }
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    expected.x,
                    expected.y
                )
            )
            .or_else(|| view.flush_pending_click(Instant::now() + view.double_click_interval)),
            Some(Action::Open(crate::ui::links::LinkTarget::Path(target)))
        );
    }
}

#[test]
fn event_content_centered_selection_copies_drawn_text() {
    for width in [100, 180] {
        let (mut view, now) = event_fixture();
        let mut event = view.event_panel[7].clone();
        event.body = "run tests".into();
        view.set_event(vec![event]);
        let _ = event_screen(&view, width, 40, now);
        let event_panel = view.viewport.borrow().event_panel;
        let x = event_panel.x + (event_panel.width / 2 - 9 / 2).min(event_panel.width - 11 - 9);
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            x,
            event_panel.y,
        ));
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            x + 9,
            event_panel.y,
        ));
        assert_eq!(
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Left),
                x + 9,
                event_panel.y
            )),
            Some(Action::Copy {
                text: "run tests".into(),
                label: "Copied selection".into()
            })
        );
    }
}

#[test]
fn event_content_centered_search_highlights_the_shifted_match() {
    for width in [100, 180] {
        let (mut view, now) = event_fixture();
        let event = view.event_panel[7].clone();
        view.set_event(vec![event]);
        view.event_list.cursor = None;
        view.event_list.search.input.set_text("tests");
        view.rematch_event();
        view.event_list.cursor = None;
        let area = Rect::new(0, 0, width, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let event_panel = view.viewport.borrow().event_panel;
        let x = event_panel.x + (event_panel.width / 2 - 9 / 2).min(event_panel.width - 11 - 9);
        for column in event_panel.x..event_panel.right() - 10 {
            assert_eq!(
                buf[(column, event_panel.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::BOLD),
                (x + 4..x + 9).contains(&column)
            );
        }
    }
}
