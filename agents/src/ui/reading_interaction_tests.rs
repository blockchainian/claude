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

fn start_detail_header(width: u16) -> (String, String, usize, usize) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/claude-manager-start.jsonl"),
    )
    .unwrap();
    let mut event = crate::providers::claude::log::tail(&path, usize::MAX)
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
    event.calls = vec![call];
    event.ended_at = Some(event.started_at + chrono::Duration::seconds(41));
    event.previous_end = Some(event.started_at);
    let (mut view, now) = log_fixture();
    view.set_log(vec![event.clone()]);
    view.select_fixture_event(Some(event));
    let area = Rect::new(0, 0, width, 50);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let rendered = buf
        .content
        .chunks(usize::from(width))
        .map(|row| {
            let mut text = String::new();
            let mut column = 0;
            while column < row.len() {
                let symbol = row[column].symbol();
                text.push_str(symbol);
                column += symbol.width().max(1);
            }
            text
        })
        .collect::<Vec<_>>()
        .join("\n");
    let detail = view.viewport.borrow().log_detail;
    let cap = view
        .center_layout(Rect::new(0, 0, width, 50))
        .log
        .width
        .saturating_sub(10)
        .min(detail.width);
    let left = usize::from(
        detail.x
            + if width >= 150 {
                (detail.width - cap) / 2
            } else {
                0
            },
    );
    let header = rendered
        .lines()
        .nth(usize::from(detail.y + 1))
        .unwrap()
        .to_owned();
    let body = rendered
        .lines()
        .skip(usize::from(detail.y + 3))
        .collect::<Vec<_>>()
        .join("\n");
    (header, body, left, usize::from(cap))
}

#[test]
fn start_summary_occupies_left_header_slot_without_body_duplicate() {
    for width in [200, 120, 100] {
        let (header, body, left, cap) = start_detail_header(width);
        assert_eq!(
            header[..header.find("Claude 图片链接").expect("left summary")].width(),
            left
        );
        assert_eq!(
            header[..header.find("codex-manager start").unwrap()].width(),
            left + (cap - "codex-manager start".width()) / 2
        );
        assert_eq!(
            header[..header.find("41s").unwrap()].width(),
            left + cap - 3
        );
        assert!(!body.contains("Claude 图片链接"), "{body}");
        assert!(body.contains("cwd: /workspace/agents"));
    }
    let (header, body, _, _) = start_detail_header(200);
    let snapshot = format!(
        "{}\n{}\n",
        header.trim_end(),
        body.lines()
            .take(4)
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/start_description_header.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn start_summary_truncates_before_center_title_and_time() {
    let (header, body, left, cap) = start_detail_header(55);
    assert!(header.contains('…'), "{header}");
    assert_eq!(
        header[..header.find("codex-manager start").unwrap()].width(),
        left + (cap - "codex-manager start".width()) / 2
    );
    assert_eq!(
        header[..header.find("41s").unwrap()].width(),
        left + cap - 3
    );
    assert!(!body.contains("Claude 图片链接"));
}

#[test]
fn preformatted_detail_rows_use_the_viewport_width() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    for width in [200, 150, 149, 120] {
        let (mut view, now) = log_fixture();
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, width, 50);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let limit = viewport.log_detail.width;
        let wide = width >= super::super::render::WIDE_LAYOUT_COLUMNS;
        let left = viewport.log_detail.x;
        for row in &viewport.log_text.rows {
            for (cells, _) in &row.cells {
                assert!(
                    row.x + cells.start >= left && row.x + cells.end <= left + limit,
                    "detail must fit log-list message width {limit} at screen width {width}"
                );
            }
        }
        let first = viewport
            .log_text
            .rows
            .iter()
            .find(|row| !row.cells.is_empty())
            .unwrap();
        if !wide {
            assert_eq!(first.x + first.cells[0].0.start, viewport.log_detail.x);
        }
    }
}

#[test]
fn detail_without_description_centers_tool_in_both_modes() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-manager-detach.jsonl");
    let event = crate::providers::claude::log::tail(&path, 200)
        .unwrap()
        .remove(0);
    for width in [200, 120] {
        let (mut view, now) = log_fixture();
        view.select_fixture_event(Some(event.clone()));
        let screen = log_screen(&view, width, 50, now);
        let detail = view.viewport.borrow().log_detail;
        let cap = view
            .center_layout(Rect::new(0, 0, width, 50))
            .log
            .width
            .saturating_sub(10)
            .min(detail.width);
        let left = detail.x
            + if width >= 150 {
                (detail.width - cap) / 2
            } else {
                0
            };
        let header = screen.lines().nth(usize::from(detail.y + 1)).unwrap();
        let title = "codex-manager detach";
        assert_eq!(
            header[..header.find(title).unwrap()].width(),
            usize::from(left) + (usize::from(cap) - title.width()) / 2
        );
        assert!(
            screen
                .lines()
                .skip(usize::from(detail.y + 3))
                .any(|line| line.contains("threadId: 01a1489e"))
        );
    }
}
