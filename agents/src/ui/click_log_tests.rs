use super::*;

fn real_log_view() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = log_fixture();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_log(crate::providers::claude::log::tail_session(&path, 200, true).unwrap());
    assert!(!view.log.is_empty());
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
    use crate::log::Section;
    let (mut view, now) = real_log_view();
    let mut event = view
        .log
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
    let Section::Code { text, .. } = &mut call.input[0] else {
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

fn detail_buffer(view: &View, width: u16, now: chrono::DateTime<Utc>) -> Buffer {
    let area = Rect::new(2, 1, width, 40);
    view.viewport.borrow_mut().log_detail = area;
    let mut buf = Buffer::empty(Rect::new(0, 0, width + 4, 42));
    view.render_log_detail(area, &mut buf, now);
    buf
}

#[test]
fn detail_centers_multiline_sections_as_blocks_and_arrow_on_panel() {
    for width in [80, 81, 160] {
        let (view, now, input, output) = multiline_tool_view();
        let buf = detail_buffer(&view, width, now);
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
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
            let offset = if buf.area.width >= 150 {
                (region.area.width - block_width) / 2
            } else {
                0
            };
            for (row, line) in rows.iter().zip(text.lines()) {
                let drawn: String = (0..block_width)
                    .map(|x| buf[(region.area.x + offset + x, row.y)].symbol())
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
                region.area.x
                    + if buf.area.width >= 150 {
                        (width - 1) / 2
                    } else {
                        0
                    },
                region.area.y + 2
            )]
                .symbol(),
            "↓"
        );
    }
}

#[test]
fn detail_centers_all_output_fragments_with_one_offset() {
    for width in [80, 81, 160] {
        let (mut view, now, _, _) = multiline_tool_view();
        let mut event = view.visible_log_event().cloned().unwrap();
        let json = include_str!("../../tests/fixtures/transcript.jsonl")
            .lines()
            .nth(2)
            .unwrap();
        let output = format!("before\n{json}\nafter");
        event.calls[0].output = Some(vec![crate::log::Section::Text {
            label: "stderr".into(),
            text: output,
        }]);
        let parts = super::super::tool_detail::parts(&event);
        let output_start = parts
            .iter()
            .position(|part| part.source == "stderr")
            .unwrap();
        let output_parts = &parts[output_start..];
        assert!(output_parts.len() >= 4);
        let content_width = view
            .center_layout(Rect::new(0, 0, width + 4, 42))
            .log
            .width
            .saturating_sub(10);
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
        let offset = if width + 4 >= 150 {
            (width - widest) / 2
        } else {
            0
        };
        view.select_fixture_event(Some(event));
        let buf = detail_buffer(&view, width, now);
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        assert_eq!(
            region
                .rows
                .iter()
                .filter(|row| row.segment >= output_start)
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
        for row in region.rows.iter().filter(|row| row.segment >= output_start) {
            let part = &parts[row.segment];
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
        let mut event = view.visible_log_event().cloned().unwrap();
        event.calls[0].output = Some(vec![crate::log::Section::Text {
            label: String::new(),
            text: include_str!("../../tests/fixtures/codex-manager-await.jsonl").into(),
        }]);
        let parts = super::super::tool_detail::parts(&event);
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
        let content_width = view
            .center_layout(Rect::new(0, 0, width + 4, 42))
            .log
            .width
            .saturating_sub(10);
        let lines = part.lines(
            content_width.into(),
            Style::default(),
            chrono::DateTime::UNIX_EPOCH,
        );
        let offset = if width + 4 >= 150 {
            (width as usize - lines.iter().map(Line::width).max().unwrap()) / 2
        } else {
            0
        };
        let copy = super::super::log::open_text(&event);
        assert!(copy.ends_with(&part.source));
        view.select_fixture_event(Some(event));
        let buf = detail_buffer(&view, width, now);
        let (x, y, expected) = {
            let viewport = view.viewport.borrow();
            let region = &viewport.log_text;
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
                let mut x = region.area.x + offset as u16;
                for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(
                    line.to_string().as_str(),
                    true,
                ) {
                    assert_eq!(buf[(x, row.y)].symbol(), grapheme);
                    x += grapheme.width() as u16;
                }
                assert_eq!(row.cells.first().unwrap().0.start, offset as u16);
            }
            (
                region.area.x + offset as u16,
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
    let mut event = view.visible_log_event().cloned().unwrap();
    let recorded = view
        .log
        .iter()
        .flat_map(|event| &event.calls)
        .find(|call| call.id == "toolu_01XU5dCe5eBxX8ccqC2DwN2G")
        .unwrap();
    event.calls[0].output = recorded.output.clone();
    let crate::log::Section::Text { text, .. } = &mut event.calls[0].output.as_mut().unwrap()[0]
    else {
        panic!("Bash output")
    };
    *text = text.replacen("decks.txt", "output/decks.txt", 1);
    view.rows[view.selected].cwd = directory.path().into();
    view.select_fixture_event(Some(event));
    view.log_detail_search.query.set_text("decks.txt");
    let mut buf = detail_buffer(&view, 200, now);
    let (rect, target) = view
        .viewport
        .borrow()
        .log_links
        .iter()
        .find(|(_, target)| target == &path)
        .unwrap()
        .clone();
    assert_eq!(target, path);
    assert!(rect.x > view.viewport.borrow().log_text.area.x);
    assert_eq!(buf[(rect.x, rect.y)].symbol(), "o");
    assert!(
        buf[(rect.x + 7, rect.y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    view.underline_links(&mut buf);
    assert!(buf[(rect.x, rect.y)].symbol().starts_with("\x1b[4:4m"));
    assert!(!buf[(rect.x - 1, rect.y)].symbol().contains("\x1b["));
    release_click(&mut view, MouseButton::Left, rect.x + 2, rect.y);
    assert_eq!(
        view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1)),
        Some(Action::OpenPath(path))
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
    let buf = detail_buffer(&view, 200, now);
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
            label: "Copied error message".into()
        })
    );
}

fn with_background() -> (View, chrono::DateTime<Utc>, tempfile::NamedTempFile) {
    use std::io::Write;
    let (mut view, now) = real_log_view();
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
            output: output.path().into(),
            started_at: now,
        });
    (view, now, output)
}

fn open_background(view: &mut View, now: chrono::DateTime<Utc>) {
    let screen = log_screen(view, 160, 40, now);
    let (x, y) = rendered_cell(&screen, "Real background");
    assert_eq!(release_click(view, MouseButton::Left, x, y), None);
    assert_eq!(release_click(view, MouseButton::Left, x, y), None);
    assert!(
        view.log_selected.is_some(),
        "Background double click must open its full log"
    );
    assert!(
        super::super::log::open_text(view.visible_log_event().unwrap()).contains("globalState")
    );
    assert_eq!(view.pending_click_in(Instant::now()), None);
}

#[test]
fn background_double_click_returns_to_previous_list() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn background_double_click_restores_previous_detail_and_scroll() {
    let (mut view, now, _output) = with_background();
    let previous = view
        .log
        .iter()
        .max_by_key(|event| log::open_text(event).len())
        .unwrap()
        .clone();
    view.select_fixture_event(Some(previous.clone()));
    log_screen(&view, 160, 40, now);
    view.viewport.borrow_mut().log_detail_scroll = 2;
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_log_event(), Some(&previous));
    assert_eq!(view.viewport.borrow().log_detail_scroll, 2);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn selected_detail_refreshes_and_log_wheel_changes_it() {
    let (mut view, now) = real_log_view();
    let previous = view.log.first().unwrap().clone();
    view.select_fixture_event(Some(previous.clone()));
    log_screen(&view, 100, 30, now);
    let mut refreshed = view.log.clone();
    refreshed.extend(view.log.clone());
    view.set_log(refreshed);
    assert_eq!(view.visible_log_event(), Some(&previous));
    let area = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x + 22, area.y + 3));
    assert_eq!(view.visible_log_event(), view.log.get(1));
    release_click(&mut view, MouseButton::Right, area.x + 22, area.y + 3);
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn detail_slash_search_jumps_and_highlights_without_closing() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    let expected = view.log_selected;
    log_screen(&view, 100, 25, now);
    assert_eq!(
        {
            view.mouse_focus = Panel::LogDetail;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        },
        None
    );
    assert_eq!(
        view.log_selected, expected,
        "Search must keep the open event"
    );
    for ch in "globalState".chars() {
        assert_eq!(key(&mut view, KeyCode::Char(ch)), None);
    }
    assert!(view.log_detail_search.editing);
    assert!(
        !view.log_search.active(),
        "detail search must not filter the list"
    );
    let screen = log_screen(&view, 100, 25, now);
    let (x, y) = rendered_cell(&screen, "globalState");
    assert!(
        view.viewport.borrow().log_detail_scroll > 0,
        "search must reveal the matching line"
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buffer.area, &mut buffer, now);
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "matched text must be highlighted"
    );
    assert_eq!(view.log_selected, expected);
    key(&mut view, KeyCode::Enter);
    assert!(!view.log_detail_search.editing);
    assert_eq!(view.log_selected, expected);
}

#[test]
fn right_click_cancels_innermost_modal_before_open_detail() {
    let (mut view, _now) = real_log_view();
    let event = view.log[0].clone();
    view.select_fixture_event(Some(event.clone()));
    view.rename_target = view.selected_id().cloned();
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.rename_target.is_none());
    assert_eq!(view.visible_log_event(), Some(&event));
    view.help = true;
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.help);
    assert_eq!(view.visible_log_event(), Some(&event));
    view.searching = true;
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.searching);
    assert_eq!(view.visible_log_event(), Some(&event));
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn background_detail_survives_refresh_with_matching_tool_timestamp() {
    let (mut view, now, _output) = with_background();
    let tool_time = view
        .log
        .iter()
        .find(|event| event.kind == crate::log::Kind::Tool)
        .unwrap()
        .started_at;
    view.rows[view.selected].background[0].started_at = tool_time;
    open_background(&mut view, now);
    let expected = view.log_selected;
    assert_eq!(view.visible_log_event().unwrap().started_at, tool_time);
    view.set_log(view.log.clone());
    assert_eq!(
        view.log_selected, expected,
        "transcript Tool events must not replace a Background detail sharing their timestamp"
    );
}

#[test]
fn background_return_restores_detail_search() {
    let (mut view, now, _output) = with_background();
    let previous = view.log[0].clone();
    view.select_fixture_event(Some(previous.clone()));
    let text = log::open_text(&previous);
    let query = text.split_whitespace().next().unwrap();
    view.log_detail_search.query.set_text(query);
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.visible_log_event(), Some(&previous));
    assert_eq!(view.log_detail_search.query.text(), query);
}

#[test]
fn cold_raw_detail_highlights_only_the_visible_source_prefix() {
    let (mut view, now) = real_log_view();
    let event = view
        .log
        .iter()
        .filter(|event| {
            let text = event.raw_text().unwrap();
            text.lines().count() > 40 && text.lines().all(|line| line.len() <= 4096)
        })
        .max_by_key(|event| event.raw_text().unwrap().len())
        .unwrap()
        .clone();
    let source = event.raw_text().unwrap();
    assert!(source.lines().count() > 40);
    view.select_fixture_event(Some(event));
    view.log_detail_raw = true;
    super::highlight::JSON_HIGHLIGHTED_LINES.with(|count| count.set(0));
    let _ = detail_buffer(&view, 80, now);
    let highlighted = super::highlight::JSON_HIGHLIGHTED_LINES.with(|count| count.get());
    assert!(highlighted > 0);
    assert!(
        highlighted <= 40,
        "highlighted {highlighted} source lines for a 40-row panel"
    );
    let _ = detail_buffer(&view, 80, now);
    assert_eq!(
        super::highlight::JSON_HIGHLIGHTED_LINES.with(|count| count.get()),
        highlighted
    );
}

#[test]
fn raw_detail_scrolling_preserves_eager_colors_and_source_maps() {
    use ratatui::widgets::{Paragraph, Widget};
    let (mut view, now) = real_log_view();
    let event = view
        .log
        .iter()
        .filter(|event| {
            let source = event.raw_text().unwrap();
            source.lines().count() > 40 && source.lines().all(|line| line.len() <= 4096)
        })
        .max_by_key(|event| event.raw_text().unwrap().len())
        .unwrap()
        .clone();
    let source = event.raw_text().unwrap();
    let body_style = super::style::log_styles(&event.kind).1;
    view.select_fixture_event(Some(event));
    view.log_detail_raw = true;
    for width in [23, 80, 120] {
        let content_width = view
            .center_layout(Rect::new(0, 0, width + 4, 42))
            .log
            .width
            .saturating_sub(10);
        let mut expected = super::tool_detail::split_wide(
            super::highlight::code_lines(&source, "json"),
            content_width as usize,
        );
        let plain = super::tool_detail::Part::raw_json(source.clone());
        assert_eq!(
            plain.maps(&expected),
            plain.maps(&plain.lines(content_width as usize, body_style, now))
        );
        let widest = expected.iter().map(Line::width).max().unwrap();
        let padding = if width + 4 >= 150 {
            (width as usize).saturating_sub(widest) / 2
        } else {
            0
        };
        for line in &mut expected {
            for span in &mut line.spans {
                span.style = body_style.patch(span.style);
            }
            if padding > 0 {
                line.spans.insert(0, Span::raw(" ".repeat(padding)));
            }
        }
        for scroll in [0, 7, expected.len(), 0] {
            view.viewport.borrow_mut().log_detail_scroll = scroll;
            let actual = detail_buffer(&view, width, now);
            let scroll = view.viewport.borrow().log_detail_scroll;
            let body = Rect::new(2, 4, width, 37);
            let mut reference = Buffer::empty(actual.area);
            Paragraph::new(
                expected
                    .iter()
                    .skip(scroll)
                    .take(37)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .render(body, &mut reference);
            for y in body.y..body.bottom() {
                for x in body.x..body.right() {
                    assert_eq!(
                        actual[(x, y)],
                        reference[(x, y)],
                        "width={width} scroll={scroll} x={x} y={y}"
                    );
                }
            }
        }
    }
}

#[test]
fn manager_detach_detail_keeps_tool_title_and_centered_input() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-manager-detach.jsonl");
    let event = crate::providers::claude::log::tail(&path, 200)
        .unwrap()
        .remove(0);
    assert_eq!(event.calls[0].title, "codex-manager detach");
    let (mut view, now) = log_fixture();
    view.select_fixture_event(Some(event));
    let buf = detail_buffer(&view, 160, now);
    let rows: Vec<String> = buf
        .content
        .chunks(164)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let title = rows
        .iter()
        .find(|row| row.contains("codex-manager detach"))
        .expect("detail must retain the log-list tool title");
    let left = title.find("codex-manager detach").unwrap();
    assert!(left.abs_diff((164 - "codex-manager detach".len()) / 2) <= 1);
    let input = rows
        .iter()
        .find(|row| row.contains("threadId: 01a1489e"))
        .expect("thread input remains visible");
    assert!(input.len() - input.trim_start().len() > 20);
    let snapshot = rows
        .iter()
        .map(|row| row.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/manager_detach_detail.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
