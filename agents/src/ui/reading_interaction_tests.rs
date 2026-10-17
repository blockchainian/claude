use super::*;

#[test]
fn reading_column_maps_original_bytes_after_search_scroll_and_resize() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == crate::log::Kind::Assistant)
        .unwrap();
    for width in [200, 120, 150, 149] {
        let (mut view, now) = log_fixture();
        view.set_log(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, width, 50);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let wide = width >= super::super::render::WIDE_LAYOUT_COLUMNS;
        assert_eq!(view.viewport.borrow().log_detail_wide, wide);
        let assert_maps = |view: &View, buf: &Buffer| {
            let viewport = view.viewport.borrow();
            let region = &viewport.log_text;
            for row in &region.rows {
                for (cells, bytes) in &row.cells {
                    let original = &region.sources[row.segment].text[bytes.clone()];
                    // Source syntax and Markdown escapes can map to different glyphs.
                    if original.chars().count() == 1
                        && !original.chars().next().unwrap().is_whitespace()
                    {
                        assert_eq!(buf[(row.x + cells.start, row.y)].symbol(), original);
                    }
                }
            }
        };
        assert_maps(&view, &buf);
        // Search invalidates the cached layout before jumping to a match.
        view.log_detail_search.query.set_text("必须");
        view.viewport.borrow_mut().log_detail_layout = None;
        view.jump_detail_match();
        view.render_at(area, &mut buf, now);
        assert_maps(&view, &buf);
        {
            let viewport = view.viewport.borrow();
            let matched: Vec<_> = viewport
                .log_text
                .rows
                .iter()
                .flat_map(|row| {
                    row.cells
                        .iter()
                        .filter_map(|(cells, bytes)| {
                            let text = &viewport.log_text.sources[row.segment].text[bytes.clone()];
                            matches!(text, "必" | "须").then_some((row.x + cells.start, row.y))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            assert!(!matched.is_empty());
            assert!(
                matched
                    .iter()
                    .all(|&point| buf[point].modifier.contains(ratatui::style::Modifier::BOLD))
            );
        }
        let (x, y) = {
            let viewport = view.viewport.borrow();
            let row = viewport
                .log_text
                .rows
                .iter()
                .find(|r| !r.cells.is_empty())
                .unwrap();
            (row.x + row.cells[0].0.start, row.y)
        };
        assert!(x >= if wide { width / 4 } else { 1 });
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, x, y));
        view.render_at(area, &mut buf, now);
        assert_maps(&view, &buf);
        if view.viewport.borrow().log_detail_max > 0 {
            assert!(view.viewport.borrow().log_detail_scroll > 0);
        }
        let resized = Rect::new(0, 0, if wide { 120 } else { 200 }, 50);
        let mut buf = Buffer::empty(resized);
        view.render_at(resized, &mut buf, now);
        assert_eq!(view.viewport.borrow().log_detail_wide, !wide);
        assert_maps(&view, &buf);
    }
}

#[test]
fn generic_input_title_occupies_top_left_and_fields_share_prose_column() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|e| e.calls.iter().any(|c| c.name.ends_with("__start")))
        .unwrap();
    let call = event
        .calls
        .iter()
        .find(|c| c.name.ends_with("__start"))
        .unwrap()
        .clone();
    for width in [200, 120] {
        let (mut view, now) = log_fixture();
        let mut event = event.clone();
        event.calls = vec![call.clone()];
        view.set_log(vec![event.clone()]);
        view.select_fixture_event(Some(event));
        let rendered = log_screen(&view, width, 50, now);
        let title = super::super::tool_detail::input_title(&call).unwrap();
        let detail = view.viewport.borrow().log_detail;
        let header = rendered.lines().nth(usize::from(detail.y + 1)).unwrap();
        assert!(header.trim_start().starts_with(&title), "{header}");
        assert_eq!(
            header[..header.find(&title).unwrap()].width(),
            usize::from(detail.x)
        );
    }
}
