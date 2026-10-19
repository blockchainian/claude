use super::*;
use crate::providers::Provider;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    key(&mut view, KeyCode::Tab);
    view.log_session = view.selected_id().cloned();
    view.set_log(crate::providers::claude::log::tail_session(&path, usize::MAX, true).unwrap());
    (dir, view)
}

fn key(view: &mut View, code: KeyCode) {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn draw_detail(view: &View) {
    let area = Rect::new(0, 0, 100, 30);
    view.render_log_detail(area, &mut Buffer::empty(area), Utc::now());
    assert!(view.viewport.borrow().log_detail_layout.is_some());
}

#[test]
fn list_search_finds_trailing_tool_name_after_non_ascii_text() {
    let (_dir, mut view) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    let mut session = view.selected_row().unwrap().clone();
    session.transcript = Some(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex-inner/command.jsonl"),
    );
    let event = provider
        .log(&session, usize::MAX, false)
        .unwrap()
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "exec_command"))
        .unwrap();
    for body in ["type", "检查日志", "type · 1", "❮ type"] {
        let mut event = event.clone();
        event.body = body.into();
        event.paths.clear();
        view.set_log(vec![event.clone()]);
        view.prepare_log_search();
        assert_eq!(
            view.log_search.bodies[0].to_string(),
            format!("Error {body} exec_command")
        );
        view.log_search.query.set_text("exec_command");
        view.rematch_log();
        assert_eq!(view.matching_log_count(), 1, "{body}");
        assert_eq!(view.visible_log_event(), Some(&event), "{body}");
    }
}

#[test]
fn list_search_keeps_layout_when_the_selected_event_survives() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    draw_detail(&view);
    let selected = view.visible_log_event().unwrap().clone();
    for code in "Stop".chars().map(KeyCode::Char).chain([
        KeyCode::Backspace,
        KeyCode::Backspace,
        KeyCode::Esc,
    ]) {
        key(&mut view, code);
        assert_eq!(view.visible_log_event(), Some(&selected));
        assert!(
            view.viewport.borrow().log_detail_layout.is_some(),
            "unchanged event lost its layout on {code:?}"
        );
    }
}

#[test]
fn list_search_discards_layout_when_the_selected_event_changes() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    draw_detail(&view);
    let selected = view.visible_log_event().unwrap().clone();
    view.log_search.query.set_text("run_in_background");
    view.rematch_log();
    assert_ne!(view.visible_log_event(), Some(&selected));
    assert!(view.viewport.borrow().log_detail_layout.is_none());
    draw_detail(&view);
}

#[test]
fn list_search_reuses_bodies_and_preserves_smart_matching() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    let bodies = view.log_search.bodies.as_ptr();
    for query in [
        "Stop",
        "Sto",
        "stop hook",
        "stop",
        "session",
        "错误",
        "",
        "Claude",
    ] {
        view.log_search.query.set_text(query);
        view.rematch_log();
        let pattern = view.log_search.pattern();
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buffer = Vec::new();
        let cwd = &view.selected_row().unwrap().cwd;
        let expected: Vec<_> = view
            .log
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let body = format!(
                    "{} {}",
                    super::super::log::kind_label(&event.kind),
                    super::super::log::body_line(event, usize::MAX, cwd)
                );
                pattern
                    .score(Utf32Str::new(&body, &mut buffer), &mut matcher)
                    .map(|_| index)
            })
            .collect();
        assert_eq!(view.log_search.matches, expected, "{query}");
        assert_eq!(view.log_search.bodies.as_ptr(), bodies);
    }
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.log_search.bodies.as_ptr(), bodies);
}

#[test]
fn list_search_keeps_partial_highlights_at_the_row_edge() {
    use ratatui::style::Modifier;
    use unicode_segmentation::UnicodeSegmentation;
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    view.log_search.query.set_text("hooks");
    view.rematch_log();
    assert_eq!(view.matching_log_count(), 1);
    let event = view.visible_log_event().unwrap().clone();
    view.log_selected = None;
    let cwd = &view.selected_row().unwrap().cwd;
    for width in 11..100 {
        let content_width = width - 10;
        let mut expected = super::super::log::body_line(&event, content_width as usize, cwd);
        let indices = view.log_search.indices(&expected.to_string());
        let mut offset = 0;
        expected.spans = expected
            .spans
            .into_iter()
            .flat_map(|span| {
                span.content
                    .graphemes(true)
                    .map(|glyph| {
                        let style = if indices.binary_search(&offset).is_ok() {
                            span.style.add_modifier(Modifier::BOLD)
                        } else {
                            span.style
                        };
                        offset += 1;
                        Span::styled(glyph.to_owned(), style)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let expected = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
            expected,
            content_width as usize,
        );
        let area = Rect::new(0, 0, width, 3);
        let mut actual = Buffer::empty(area);
        view.render_log(area, &mut actual, Utc::now());
        let mut reference = Buffer::empty(area);
        expected.render(Rect::new(0, 0, content_width, 1), &mut reference);
        for x in 0..content_width {
            assert_eq!(actual[(x, 0)].symbol(), reference[(x, 0)].symbol());
            assert_eq!(
                actual[(x, 0)].modifier.contains(Modifier::BOLD),
                reference[(x, 0)].modifier.contains(Modifier::BOLD),
                "width={width}, x={x}"
            );
        }
    }
}
