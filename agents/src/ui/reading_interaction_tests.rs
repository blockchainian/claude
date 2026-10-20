use super::*;

#[test]
fn reading_column_maps_original_bytes_after_search_scroll_and_resize() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == crate::event::Kind::Assistant)
        .unwrap();
    for width in [200, 120, 150, 149] {
        let (mut view, now) = event_fixture();
        view.set_event(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, width, 50);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let wide = width >= super::super::render::WIDE_LAYOUT_COLUMNS;
        assert_eq!(
            view.viewport.borrow().event_text.area.x,
            super::super::event_detail::event_detail_columns(
                view.viewport.borrow().event_detail,
                view.viewport.borrow().event_panel
            )
            .text
            .x
        );
        let assert_maps = |view: &View, buf: &Buffer| {
            let viewport = view.viewport.borrow();
            let region = &viewport.event_text;
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
        view.event_detail_search.query.set_text("必须");
        view.viewport.borrow_mut().event_detail_layout = None;
        view.jump_event_detail_match();
        view.render_at(area, &mut buf, now);
        assert_maps(&view, &buf);
        {
            let viewport = view.viewport.borrow();
            let matched: Vec<_> = viewport
                .event_text
                .rows
                .iter()
                .flat_map(|row| {
                    row.cells
                        .iter()
                        .filter_map(|(cells, bytes)| {
                            let text =
                                &viewport.event_text.sources[row.segment].text[bytes.clone()];
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
                .event_text
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
        if view.viewport.borrow().event_detail_max > 0 {
            assert!(view.viewport.borrow().event_detail_scroll > 0);
        }
        let resized = Rect::new(0, 0, if wide { 120 } else { 200 }, 50);
        let mut buf = Buffer::empty(resized);
        view.render_at(resized, &mut buf, now);
        assert_eq!(
            view.viewport.borrow().event_text.area.x,
            super::super::event_detail::event_detail_columns(
                view.viewport.borrow().event_detail,
                view.viewport.borrow().event_panel
            )
            .text
            .x
        );
        assert_maps(&view, &buf);
    }
}

fn start_event_detail_header(
    width: u16,
) -> (
    String,
    String,
    super::super::event_detail::EventDetailColumns,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/claude-manager-start.jsonl"),
    )
    .unwrap();
    let mut event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
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
    let (mut view, now) = event_fixture();
    view.set_event(vec![event.clone()]);
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
    let detail = view.viewport.borrow().event_detail;
    let columns = super::super::event_detail::event_detail_columns(
        detail,
        view.center_layout(area).event_panel,
    );
    let header = rendered
        .lines()
        .nth(usize::from(detail.y + 1))
        .unwrap()
        .to_owned();
    let body = rendered
        .lines()
        .skip(usize::from(detail.y + 2))
        .collect::<Vec<_>>()
        .join("\n");
    (header, body, columns)
}

#[test]
fn mcp_start_centers_subject_and_shows_only_prompt() {
    for width in [200, 120, 100] {
        let (header, body, columns) = start_event_detail_header(width);
        let title = "Claude 图片链接";
        assert_eq!(
            header[..header.rfind(title).expect("center subject")].width(),
            usize::from(super::super::centering::centered_x(
                columns.panel,
                columns.text,
                title.width() as u16,
            ))
        );
        assert_eq!(
            header[..header.find("codex-manager start").unwrap()].width(),
            usize::from(columns.text.x)
        );
        assert_eq!(
            header[..header.find("41s").unwrap()].width(),
            usize::from(columns.time.right()) - 3
        );
        assert!(body.contains("Implement workstream A"), "{body}");
        assert!(!body.contains("\"name\":"), "{body}");
        assert!(!body.contains("\"cwd\":"), "{body}");
        assert!(!body.contains("\"prompt\":"), "{body}");
        assert!(!body.contains("Claude 图片链接"), "{body}");
    }
    let (header, body, _) = start_event_detail_header(200);
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
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn mcp_start_tool_metadata_truncates_before_title_and_time() {
    let (header, body, columns) = start_event_detail_header(45);
    assert!(header.contains('…'), "{header}");
    assert!(header.contains("codex-manager"), "{header}");
    assert!(header.contains("Claude 图片链接"), "{header}");
    assert_eq!(
        header[..header.find("41s").unwrap()].width(),
        usize::from(columns.time.right()) - 3
    );
    assert!(body.contains("Implement workstream A"), "{body}");
    assert!(!body.contains("\"cwd\":"), "{body}");
}

#[test]
fn preformatted_detail_rows_stay_in_content_column() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    for width in [200, 150, 149, 120] {
        let (mut view, now) = event_fixture();
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, width, 50);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let columns = super::super::event_detail::event_detail_columns(
            viewport.event_detail,
            view.center_layout(area).event_panel,
        );
        let limit = columns.text.width;
        let left = columns.text.x;
        for row in &viewport.event_text.rows {
            for (cells, _) in &row.cells {
                assert!(
                    row.x + cells.start >= left && row.x + cells.end <= left + limit,
                    "detail must fit the text column {limit} at screen width {width}"
                );
            }
        }
        let first = viewport
            .event_text
            .rows
            .iter()
            .find(|row| !row.cells.is_empty())
            .unwrap();
        assert!(first.x + first.cells[0].0.start >= columns.text.x);
    }
}

#[test]
fn event_detail_without_subject_centers_tool_name_without_duplicate_metadata() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-agent-names.jsonl");
    let event = crate::providers::claude::event_reader::tail(&path, 200)
        .unwrap()
        .into_iter()
        .find(|event| {
            event
                .calls
                .iter()
                .any(|call| call.name == "SubagentHandback")
        })
        .unwrap();
    for width in [200, 120] {
        let (mut view, now) = event_fixture();
        view.select_fixture_event(Some(event.clone()));
        let screen = event_screen(&view, width, 50, now);
        let detail = view.viewport.borrow().event_detail;
        let columns = super::super::event_detail::event_detail_columns(
            detail,
            view.center_layout(Rect::new(0, 0, width, 50)).event_panel,
        );
        let header = screen.lines().nth(usize::from(detail.y + 1)).unwrap();
        let title = "SubagentHandback";
        assert_eq!(
            header[..header.rfind(title).unwrap()].width(),
            usize::from(columns.panel.x) + (usize::from(columns.panel.width) - title.width()) / 2
        );
        assert_eq!(header.matches(title).count(), 1);
        assert!(header[..header.find(title).unwrap()].trim().is_empty());
        assert!(
            screen
                .lines()
                .skip(usize::from(detail.y + 2))
                .any(|line| line.contains("\"message\":"))
        );
    }
}

#[test]
fn event_detail_event_and_tool_times_share_the_list_clock_column() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-manager-detach.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .remove(0);
    event.calls = vec![event.calls[0].clone(); 2];
    event.calls[0].duration = Some(chrono::Duration::seconds(12));
    event.calls[1].duration = Some(chrono::Duration::seconds(125));
    for width in [120, 200] {
        let (mut view, now) = event_fixture();
        view.set_event(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, width, 100);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let columns = super::super::event_detail::event_detail_columns(
            viewport.event_detail,
            viewport.event_panel,
        );
        let (_, list_time) = super::super::event_list::event_row_areas(viewport.event_panel);
        assert_eq!(columns.time.width, list_time.width);
        assert_eq!(
            columns.time.width,
            super::super::event_list::CLOCK_WIDTH as u16
        );
        assert_eq!(columns.time.right(), columns.panel.right());
        let clock = |y| {
            (columns.time.x..columns.time.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        let header_y = viewport.event_detail.y + super::super::event_list::EVENT_TOP;
        assert!(!clock(header_y).trim().is_empty());
        assert_ne!(buf[(columns.time.right() - 1, header_y)].symbol(), " ");
        let detail = tool_detail::event_detail_content(&event, String::new(), Style::new().dim());
        let mut id = 0;
        for section in &detail.sections {
            let title = &section.title;
            let source = &viewport.event_text.sources[id];
            assert_eq!(source.text.as_ref(), title.source());
            id += 1 + section
                .groups
                .iter()
                .map(|group| group.parts.len())
                .sum::<usize>();
            let row = viewport
                .event_text
                .rows
                .iter()
                .find(|row| row.segment == source.id && !row.cells.is_empty())
                .unwrap();
            let duration = title.time.as_ref().unwrap();
            assert_eq!(clock(row.y).trim(), duration);
            assert_ne!(buf[(columns.time.right() - 1, row.y)].symbol(), " ");
            assert!(!title.line(columns).0.to_string().contains(duration));
        }
    }
}

#[test]
fn clipped_command_and_output_keep_time_empty_and_double_click_copies_original() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    event.calls.retain(|call| call.name == "Bash");
    event.calls.truncate(1);
    let crate::event::Section::Shell { text, .. } = &event.calls[0].input[0] else {
        panic!("recorded Bash command")
    };
    let text = text.lines().max_by_key(|line| line.width()).unwrap();
    let command = text.to_owned();
    event.calls[0].input = vec![crate::event::Section::Shell {
        prefix: 0,
        text: command.clone(),
    }];
    event.calls[0].output = Some(vec![crate::event::Section::Text {
        label: "stdout".into(),
        text: command.clone(),
    }]);
    for width in [90, 200] {
        let (mut view, now) = event_fixture();
        view.set_event(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 60));
        view.render_at(buf.area, &mut buf, now);
        let (x, ys) = {
            let viewport = view.viewport.borrow();
            let columns = super::super::event_detail::event_detail_columns(
                viewport.event_detail,
                viewport.event_panel,
            );
            assert!(command.width() > columns.text.width.into());
            let rows: Vec<_> = viewport
                .event_text
                .rows
                .iter()
                .filter(|row| viewport.event_text.sources[row.segment].text.as_ref() == command)
                .collect();
            assert_eq!(rows.len(), 2);
            for row in &rows {
                assert_eq!(buf[(columns.text.right() - 1, row.y)].symbol(), "…");
                assert!(
                    (columns.text.right()..columns.time.right())
                        .all(|x| buf[(x, row.y)].symbol() == " ")
                );
                let (cells, bytes) = row.cells.last().unwrap();
                assert_eq!(cells.end, columns.text.width);
                assert_eq!(bytes.end, command.len());
                assert!(bytes.end - bytes.start > 1);
            }
            (
                columns.text.right() - 1,
                rows.iter().map(|row| row.y).collect::<Vec<_>>(),
            )
        };
        for y in ys {
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
            view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x, y));
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
            assert_eq!(
                view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x, y)),
                Some(Action::Copy {
                    text: event_list::open_text(&event),
                    label: event_list::copied_label(&event)
                })
            );
        }
    }
}
