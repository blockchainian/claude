use super::*;

fn real_event_view() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = event_fixture();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_event(crate::providers::claude::event_reader::tail_session(&path, 200, true).unwrap());
    assert!(!view.event_panel.is_empty());
    (view, now)
}

fn release_click(view: &mut View, button: MouseButton, x: u16, y: u16) -> Option<Action> {
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(button), x, y))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Up(button), x, y))
}

// Short sections cut from one recorded Bash call, preserving its line lengths and indentation.
fn multiline_tool_view() -> (View, chrono::DateTime<Utc>, String, String) {
    use crate::event::Section;
    let (mut view, now) = real_event_view();
    let mut event = view
        .event_panel
        .iter()
        .find(|event| {
            event
                .calls
                .iter()
                .any(|call| call.id == "toolu_01GnWk7yiKqPJ1tzHjouKcCx")
        })
        .unwrap()
        .clone();
    event
        .calls
        .retain(|call| call.id == "toolu_01GnWk7yiKqPJ1tzHjouKcCx");
    let call = &mut event.calls[0];
    let Section::Shell { text, .. } = &mut call.input[0] else {
        panic!("Bash command")
    };
    *text = text.lines().skip(1).take(2).collect::<Vec<_>>().join("\n");
    let input = text.clone();
    let Section::Text { text, .. } = &mut call.output.as_mut().unwrap()[0] else {
        panic!("Bash output")
    };
    *text = text
        .lines()
        .skip_while(|line| !line.starts_with("Traceback"))
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    let output = text.clone();
    assert_eq!(input, "import json,sys\nlast=None");
    assert!(output.starts_with("Traceback"));
    view.select_fixture_event(Some(event));
    (view, now, input, output)
}

fn event_detail_buffer(view: &View, width: u16, now: chrono::DateTime<Utc>) -> Buffer {
    let area = Rect::new(2, 1, width, 40);
    view.viewport.borrow_mut().event_detail = area;
    let mut buf = Buffer::empty(Rect::new(0, 0, width + 4, 42));
    view.render_event_detail(area, &mut buf, now);
    buf
}

#[test]
fn event_detail_centers_multiline_sections_and_arrow_on_panel_midline() {
    for width in [80, 81, 160] {
        let (view, now, input, output) = multiline_tool_view();
        let buf = event_detail_buffer(&view, width, now);
        let viewport = view.viewport.borrow();
        let region = &viewport.event_text;
        let columns = super::super::event_detail::event_detail_columns(
            viewport.event_detail,
            viewport.event_panel,
        );
        for text in [&input, &output] {
            let source = region
                .sources
                .iter()
                .find(|source| &*source.text == text)
                .unwrap();
            let rows: Vec<_> = region
                .rows
                .iter()
                .filter(|row| row.segment == source.id)
                .collect();
            let block_width = text.lines().map(UnicodeWidthStr::width).max().unwrap() as u16;
            let offset =
                super::super::centering::centered_x(columns.panel, columns.text, block_width)
                    - columns.text.x;
            for (row, line) in rows.iter().zip(text.lines()) {
                let drawn: String = (0..block_width)
                    .map(|x| cell_text(buf[(region.area.x + offset + x, row.y)].symbol()))
                    .collect();
                assert_eq!(
                    drawn.trim_end(),
                    line,
                    "each line shares the block's offset"
                );
                assert_eq!(row.cells.first().unwrap().0.start, offset);
            }
            assert_eq!(rows.len(), 2);
        }
        assert_eq!(
            buf[(
                super::super::centering::centered_x(columns.panel, columns.text, 1),
                region.area.y + 4
            )]
                .symbol(),
            "↓"
        );
    }
}

#[test]
fn event_detail_centers_all_output_fragments_with_one_offset() {
    for width in [80, 81, 160] {
        let (mut view, now, _, _) = multiline_tool_view();
        let mut event = view.visible_event().cloned().unwrap();
        let json = include_str!("../../tests/fixtures/transcript.jsonl")
            .lines()
            .nth(2)
            .unwrap();
        let output = format!("before\n{json}\nafter");
        event.calls[0].output = Some(vec![
            crate::event::Section::Label("stderr".into()),
            crate::event::Section::Text {
                label: String::new(),
                text: output,
            },
        ]);
        let parts = event_detail_parts(&event);
        let output_start = parts
            .iter()
            .position(|part| part.source == "stderr")
            .unwrap();
        let output_parts = &parts[output_start..];
        assert!(output_parts.len() >= 4);
        let columns = super::super::event_detail::event_detail_columns(
            Rect::new(2, 1, width, 40),
            view.center_layout(Rect::new(0, 0, width + 4, 42))
                .event_panel,
        );
        let content_width = columns.text.width;
        let widest = output_parts
            .iter()
            .flat_map(|part| {
                part.lines(
                    content_width.into(),
                    Style::default(),
                    chrono::DateTime::UNIX_EPOCH,
                )
            })
            .map(|line| line.width())
            .max()
            .unwrap() as u16;
        let offset = super::super::centering::centered_x(
            columns.panel,
            columns.text,
            widest.min(content_width),
        ) - columns.text.x;
        view.select_fixture_event(Some(event));
        let buf = event_detail_buffer(&view, width, now);
        let viewport = view.viewport.borrow();
        let region = &viewport.event_text;
        assert_eq!(
            region
                .rows
                .iter()
                .filter(|row| row.segment > output_start)
                .count(),
            output_parts
                .iter()
                .map(|part| part
                    .lines(
                        content_width.into(),
                        Style::default(),
                        chrono::DateTime::UNIX_EPOCH
                    )
                    .len())
                .sum::<usize>(),
        );
        for row in region.rows.iter().filter(|row| row.segment > output_start) {
            let part = &parts[row.segment - 1];
            let rows: Vec<_> = region
                .rows
                .iter()
                .filter(|other| other.segment == row.segment)
                .collect();
            let index = rows.iter().position(|other| other.y == row.y).unwrap();
            let line = part.lines(
                content_width.into(),
                Style::default(),
                chrono::DateTime::UNIX_EPOCH,
            )[index]
                .to_string();
            let line = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::from(line),
                content_width.into(),
            )
            .to_string();
            let mut column = region.area.x + offset;
            for grapheme in
                unicode_segmentation::UnicodeSegmentation::graphemes(line.as_str(), true)
            {
                assert_eq!(
                    buf[(column, row.y)].symbol(),
                    grapheme,
                    "all output rows share the section offset"
                );
                column += grapheme.width() as u16;
            }
            for (cells, bytes) in &row.cells {
                if buf[(region.area.x + cells.start, row.y)].symbol() == "…" {
                    continue;
                }
                assert_eq!(
                    buf[(region.area.x + cells.start, row.y)].symbol(),
                    &part.source[bytes.clone()]
                );
            }
        }
    }
}

#[test]
fn multiline_json_block_keeps_section_offset_and_copies_displayed_text() {
    for width in [120, 121, 160] {
        let (mut view, now, _, _) = multiline_tool_view();
        let mut event = view.visible_event().cloned().unwrap();
        event.calls[0].output = Some(vec![crate::event::Section::Text {
            label: String::new(),
            text: include_str!("../../tests/fixtures/codex-manager-await.jsonl").into(),
        }]);
        let parts = event_detail_parts(&event);
        let part = parts
            .iter()
            .find(|part| part.source.contains("lastMessage"))
            .unwrap();
        assert_eq!(
            part.source
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n"),
            include_str!("../../tests/ui_snapshots/manager_await_json.txt").trim_end_matches('\n')
        );
        let columns = super::super::event_detail::event_detail_columns(
            Rect::new(2, 1, width, 40),
            view.center_layout(Rect::new(0, 0, width + 4, 42))
                .event_panel,
        );
        let content_width = columns.text.width;
        let lines = part.lines(
            content_width.into(),
            Style::default(),
            chrono::DateTime::UNIX_EPOCH,
        );
        let widest = lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap()
            .min(content_width as usize);
        let offset =
            super::super::centering::centered_x(columns.panel, columns.text, widest as u16)
                - columns.text.x;
        let copy = super::super::event_list::open_text(&event);
        assert!(copy.ends_with(&part.source));
        view.select_fixture_event(Some(event));
        let buf = event_detail_buffer(&view, width, now);
        let (x, y, expected) = {
            let viewport = view.viewport.borrow();
            let region = &viewport.event_text;
            let source = region
                .sources
                .iter()
                .find(|source| source.text.contains("lastMessage"))
                .unwrap();
            let rows: Vec<_> = region
                .rows
                .iter()
                .filter(|row| row.segment == source.id)
                .collect();
            assert_eq!(rows.len(), lines.len());
            for (row, line) in rows.iter().zip(&lines) {
                let mut x = region.area.x + offset;
                for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(
                    super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                        line.clone(),
                        content_width.into(),
                    )
                    .to_string()
                    .as_str(),
                    true,
                ) {
                    assert_eq!(buf[(x, row.y)].symbol(), grapheme);
                    x += grapheme.width() as u16;
                }
                assert_eq!(row.cells.first().unwrap().0.start, offset);
            }
            (
                region.area.x + offset,
                rows[0].y,
                Action::Copy {
                    text: copy,
                    label: source.label.clone(),
                },
            )
        };
        release_click(&mut view, MouseButton::Left, x, y);
        assert_eq!(
            release_click(&mut view, MouseButton::Left, x, y),
            Some(expected)
        );
    }
}

#[test]
fn shifted_detail_path_click_search_underline_and_copy_use_source_bytes() {
    let (mut view, now, _, _) = multiline_tool_view();
    let directory = tempfile::tempdir().unwrap();
    // This filename is an actual output line from the recorded Bash call.
    let path = directory.path().join("output/decks.txt");
    std::fs::create_dir(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    let mut event = view.visible_event().cloned().unwrap();
    let recorded = view
        .event_panel
        .iter()
        .flat_map(|event| &event.calls)
        .find(|call| call.id == "toolu_01XU5dCe5eBxX8ccqC2DwN2G")
        .unwrap();
    event.calls[0].output = recorded.output.clone();
    let crate::event::Section::Text { text, .. } = &mut event.calls[0].output.as_mut().unwrap()[0]
    else {
        panic!("Bash output")
    };
    *text = text.replacen("decks.txt", "output/decks.txt", 1);
    view.rows[view.selected].cwd = directory.path().into();
    view.select_fixture_event(Some(event));
    view.event_detail.search.input.set_text("decks.txt");
    let buf = event_detail_buffer(&view, 200, now);
    let (rect, target) = view
        .viewport
        .borrow()
        .event_links
        .iter()
        .find(|(_, target)| target == &crate::ui::links::LinkTarget::Path(path.clone()))
        .unwrap()
        .clone();
    assert_eq!(target, crate::ui::links::LinkTarget::Path(path.clone()));
    assert!(rect.x > view.viewport.borrow().event_text.area.x);
    assert_eq!(cell_text(buf[(rect.x, rect.y)].symbol()), "o");
    assert!(
        buf[(rect.x + 7, rect.y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(buf[(rect.x, rect.y)].symbol().starts_with("\x1b[4:4m"));
    assert!(!buf[(rect.x - 1, rect.y)].symbol().contains("\x1b["));
    release_click(&mut view, MouseButton::Left, rect.x + 2, rect.y);
    assert_eq!(
        view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1)),
        Some(Action::Open(crate::ui::links::LinkTarget::Path(path)))
    );
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 8,
        rect.y,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        rect.x + 12,
        rect.y,
    ));
    let buf = event_detail_buffer(&view, 200, now);
    assert!(
        buf[(rect.x + 8, rect.y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            rect.x + 12,
            rect.y
        )),
        Some(Action::Copy {
            text: "ecks".into(),
            label: "Copied message".into()
        })
    );
}

fn with_background() -> (View, chrono::DateTime<Utc>, tempfile::NamedTempFile) {
    use std::io::Write;
    let (mut view, now) = real_event_view();
    let mut output = tempfile::NamedTempFile::new().unwrap();
    output
        .write_all(include_bytes!(
            "../../tests/fixtures/claude-peer-message.txt"
        ))
        .unwrap();
    view.rows[view.selected]
        .background
        .push(crate::model::BackgroundTask {
            description: "Real background output".into(),
            output: Some(output.path().into()),
            started_at: now,
        });
    (view, now, output)
}

fn open_background(view: &mut View, now: chrono::DateTime<Utc>) {
    let screen = event_screen(view, 160, 40, now);
    let (x, y) = rendered_cell(&screen, "Real background");
    assert_eq!(release_click(view, MouseButton::Left, x, y), None);
    assert!(
        view.event_list.cursor.is_some(),
        "Background click must open its full log"
    );
    assert!(
        super::super::event_list::open_text(view.visible_event().unwrap()).contains("globalState")
    );
    assert_eq!(view.pending_click_in(Instant::now()), None);
}

#[test]
fn background_click_returns_to_previous_list() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn background_click_restores_previous_detail_and_scroll() {
    let (mut view, now, _output) = with_background();
    let previous = view
        .event_panel
        .iter()
        .max_by_key(|event| event_list::open_text(event).len())
        .unwrap()
        .clone();
    view.select_fixture_event(Some(previous.clone()));
    event_screen(&view, 160, 40, now);
    view.event_detail.scroll.borrow_mut().offset = 2;
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_event(), Some(&previous));
    assert_eq!(view.event_detail.scroll.borrow().offset, 2);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn selected_detail_refreshes_and_event_wheel_changes_it() {
    let (mut view, now) = real_event_view();
    let previous = view.event_panel.first().unwrap().clone();
    view.select_fixture_event(Some(previous.clone()));
    event_screen(&view, 100, 30, now);
    let mut refreshed = view.event_panel.clone();
    refreshed.extend(view.event_panel.clone());
    view.set_event(refreshed);
    assert_eq!(view.visible_event(), Some(&previous));
    let area = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x + 22, area.y + 3));
    assert_eq!(view.visible_event(), view.event_panel.get(1));
    release_click(&mut view, MouseButton::Right, area.x + 22, area.y + 3);
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn event_detail_slash_search_jumps_and_highlights_without_closing() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    let expected = view.event_list.cursor;
    event_screen(&view, 100, 25, now);
    assert_eq!(
        {
            view.active_panel = Panel::EventDetail;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        },
        None
    );
    assert_eq!(
        view.event_list.cursor, expected,
        "Search must keep the open event"
    );
    for ch in "globalState".chars() {
        assert_eq!(key(&mut view, KeyCode::Char(ch)), None);
    }
    assert!(view.event_detail.search.input.focused());
    assert!(
        !view.event_list.search.input.applied(),
        "detail search must not filter the list"
    );
    // Wide enough that the matching line is not truncated before the match.
    let screen = event_screen(&view, 300, 25, now);
    let body = screen
        .lines()
        .take_while(|line| !line.contains("Search ❯"))
        .collect::<Vec<_>>()
        .join("\n");
    let (x, y) = rendered_cell(&body, "globalState");
    assert!(
        view.event_detail.scroll.borrow().offset > 0,
        "search must reveal the matching line"
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 300, 25));
    view.render_at(buffer.area, &mut buffer, now);
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "matched text must be highlighted"
    );
    assert_eq!(view.event_list.cursor, expected);
    key(&mut view, KeyCode::Enter);
    assert!(!view.event_detail.search.input.focused());
    assert_eq!(view.event_list.cursor, expected);
}

#[test]
fn right_click_cancels_innermost_modal_before_open_event_detail() {
    let (mut view, _now) = real_event_view();
    let event = view.event_panel[0].clone();
    view.select_fixture_event(Some(event.clone()));
    view.rename_target = view.selected_id().cloned();
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.rename_target.is_none());
    assert_eq!(view.visible_event(), Some(&event));
    view.help = true;
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.help);
    assert_eq!(view.visible_event(), Some(&event));
    view.search.focus();
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.search.focused());
    assert_eq!(view.visible_event(), Some(&event));
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn background_detail_survives_refresh_with_matching_tool_timestamp() {
    let (mut view, now, _output) = with_background();
    let tool_time = view
        .event_panel
        .iter()
        .find(|event| event.kind == crate::event::Kind::Tool)
        .unwrap()
        .started_at;
    view.rows[view.selected].background[0].started_at = tool_time;
    open_background(&mut view, now);
    let expected = view.event_list.cursor;
    assert_eq!(view.visible_event().unwrap().started_at, tool_time);
    view.set_event(view.event_panel.clone());
    assert_eq!(
        view.event_list.cursor, expected,
        "transcript Tool events must not replace a Background detail sharing their timestamp"
    );
}

#[test]
fn background_return_restores_detail_search() {
    let (mut view, now, _output) = with_background();
    let previous = view.event_panel[0].clone();
    view.select_fixture_event(Some(previous.clone()));
    let text = event_list::open_text(&previous);
    let query = text.split_whitespace().next().unwrap();
    view.event_detail.search.input.set_text(query);
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_event(), Some(&previous));
    assert_eq!(view.event_detail.search.input.text(), query);
}

#[test]
fn manager_detach_detail_centers_thread_and_has_no_input() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-manager-detach.jsonl");
    let event = crate::providers::claude::event_reader::tail(&path, 200)
        .unwrap()
        .remove(0);
    let thread_id = "01a1489e-5020-7f40-90e8-c9413012630b";
    assert_eq!(
        event.calls[0].summary,
        format!("codex-manager detach {thread_id}")
    );
    assert!(event.calls[0].input.is_empty());
    let (mut view, now) = event_fixture();
    view.select_fixture_event(Some(event));
    let buf = event_detail_buffer(&view, 160, now);
    let rows: Vec<String> = buf
        .content
        .chunks(164)
        .map(|row| row.iter().map(|cell| cell_text(cell.symbol())).collect())
        .collect();
    let title = rows
        .iter()
        .find(|row| row.contains("codex-manager detach"))
        .expect("detail must retain the tool name beside its subject");
    let left = title.find(thread_id).unwrap();
    let viewport = view.viewport.borrow();
    let columns = super::super::event_detail::event_detail_columns(
        viewport.event_detail,
        view.center_layout(buf.area).event_panel,
    );
    assert_eq!(
        left,
        usize::from(super::super::centering::centered_x(
            columns.panel,
            columns.text,
            thread_id.len() as u16,
        ))
    );
    assert_eq!(
        title.find("codex-manager detach"),
        Some(usize::from(columns.text.x))
    );
    assert_eq!(
        rows.iter().filter(|row| row.trim() == thread_id).count(),
        1,
        "the output contains only the thread ID"
    );
    assert!(!rows.iter().any(|row| row.contains("\"threadId\"")));
    let snapshot = rows
        .iter()
        .map(|row| row.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/manager_detach_event_detail.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot.trim_end(),
        std::fs::read_to_string(path).unwrap().trim_end()
    );
}

#[test]
fn background_await_detail_uses_markdown_body() {
    let (mut view, now, output) = with_background();
    let fixture = include_str!("../../tests/fixtures/background/codex-await.output");
    std::fs::write(
        output.path(),
        fixture.split("[exited with code").next().unwrap(),
    )
    .unwrap();
    event_screen(&view, 200, 60, now);
    let (x, y) = {
        let viewport = view.viewport.borrow();
        (
            viewport.task_details.x + 2,
            viewport.task_details_background[0].0.start,
        )
    };
    release_click(&mut view, MouseButton::Left, x, y);
    release_click(&mut view, MouseButton::Left, x, y);
    let event = view.visible_event().unwrap();
    let detail =
        super::super::tool_detail::event_detail_content(event, String::new(), Style::default());
    assert_eq!(detail.sections[0].title.left, None);
    let body = event_detail_parts(event);
    let message = body
        .iter()
        .find(|part| part.source.contains("d0b23338"))
        .unwrap();
    assert!(message.markdown());
    assert!(!message.source.contains("lastMessage"));
    let rendered = event_detail_buffer(&view, 160, now);
    let text = rendered
        .content
        .iter()
        .map(|cell| cell_text(cell.symbol()))
        .collect::<String>();
    assert!(!text.contains("lastMessage"));
    assert!(!text.contains("[exited with code"));
    let snapshot = rendered
        .content
        .chunks(164)
        .map(|row| {
            let mut text = String::new();
            let mut x = 0;
            while x < row.len() {
                let symbol = row[x].symbol();
                text.push_str(symbol);
                x += symbol.width().max(1);
            }
            text.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/background_await_event_detail.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
