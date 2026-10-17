use super::*;

#[test]
fn log_content_centers_short_rows_and_keeps_truncated_rows_and_clock() {
    for width in [20, 30, 31, 100, 160] {
        let (mut view, now) = log_fixture();
        let area = Rect::new(3, 2, width, 1);
        let mut clock = None;
        let template = view.log[7].clone();
        for text in [
            "run tests",
            "界面",
            "a long tool message that fills the entire content column and must be truncated to fit",
        ] {
            let mut event = template.clone();
            event.body = text.into();
            view.set_log(vec![event]);
            let mut buf = Buffer::empty(Rect::new(0, 0, width + 6, 5));
            view.render_log(area, &mut buf, now);
            let content_width = width - 10;
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
            assert_eq!(buf[(area.right() - 10, area.y)].symbol(), " ");
            assert_eq!(buf[(area.right() - 9, area.y)].symbol(), " ");
            let cells = (area.right() - 8..area.right())
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
fn log_content_centered_links_click_and_underline_exactly_the_path() {
    for width in [100, 180] {
        let (mut view, now) = log_fixture();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("report.md");
        std::fs::write(&target, "report").unwrap();
        view.rows[view.selected].cwd = dir.path().to_owned();
        let mut event = view.log[7].clone();
        event.body = "Read report.md done".into();
        event.paths.clear();
        event.paths.push(5..14);
        view.set_log(vec![event]);
        let area = Rect::new(0, 0, width, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let log = view.viewport.borrow().log;
        let start = log.x + (log.width / 2 - 19 / 2).min(log.width - 10 - 19);
        let expected = Rect::new(start + 5, log.y, 9, 1);
        let links = view.viewport.borrow().log_links.clone();
        assert!(links.contains(&(expected, target.clone())), "{links:?}");
        let before = buf.clone();
        super::super::links::underline(&mut buf, &[(expected, target.clone())]);
        for x in log.x..log.right() {
            assert_eq!(
                buf[(x, log.y)] != before[(x, log.y)],
                expected.contains((x, log.y).into())
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
            Some(Action::OpenPath(target))
        );
    }
}

#[test]
fn log_content_centered_selection_copies_drawn_text() {
    for width in [100, 180] {
        let (mut view, now) = log_fixture();
        let mut event = view.log[7].clone();
        event.body = "run tests".into();
        view.set_log(vec![event]);
        let _ = log_screen(&view, width, 40, now);
        let log = view.viewport.borrow().log;
        let x = log.x + (log.width / 2 - 9 / 2).min(log.width - 10 - 9);
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, log.y));
        view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x + 9, log.y));
        assert_eq!(
            view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 9, log.y)),
            Some(Action::Copy {
                text: "run tests".into(),
                label: "Copied selection".into()
            })
        );
    }
}

#[test]
fn log_content_centered_search_highlights_the_shifted_match() {
    for width in [100, 180] {
        let (mut view, now) = log_fixture();
        let event = view.log[7].clone();
        view.set_log(vec![event]);
        view.log_selected = None;
        view.log_search.query.set_text("tests");
        view.rematch_log();
        view.log_selected = None;
        let area = Rect::new(0, 0, width, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let log = view.viewport.borrow().log;
        let x = log.x + (log.width / 2 - 9 / 2).min(log.width - 10 - 9);
        for column in log.x..log.right() - 10 {
            assert_eq!(
                buf[(column, log.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::BOLD),
                (x + 4..x + 9).contains(&column)
            );
        }
    }
}
