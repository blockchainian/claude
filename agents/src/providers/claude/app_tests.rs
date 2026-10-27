fn session_key(id: &str) -> crate::providers::SessionKey {
    crate::providers::SessionKey {
        provider: crate::providers::ProviderId::Claude,
        id: id.into(),
    }
}
use super::*;
use crate::model::SessionStatus;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn fixture() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("../../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(dir.path().to_owned()),
        )]),
        dir.path().to_owned(),
    );
    app.refresh();
    assert_eq!(app.sessions.len(), 1);
    app.sessions[0].cwd = dir.path().to_owned();
    app.sessions[0].status = SessionStatus::Ready;
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event_for_list_rows(8);
    super::event_search_tests::finish_search(&mut app);
    (dir, app)
}

fn replace_transcript(path: &std::path::Path, content: impl AsRef<[u8]>) {
    use std::io::Write;
    let mut replacement = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
    replacement.write_all(content.as_ref()).unwrap();
    replacement.persist(path).unwrap();
}

pub(super) fn key(app: &mut App, code: KeyCode) -> bool {
    let quit = app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    // A confirmed action has its own completion; do not consume it while settling a draw.
    if code == KeyCode::Enter {
        return quit;
    }
    app.refresh_event_for_list_rows(8);
    if app.pending_transcript.is_some()
        && !app.view.transcript_fully_loaded()
        && !app.view.restoring_event_position()
    {
        super::event_search_tests::finish_search(app);
    }
    quit
}

#[test]
fn terminal_theme_events_update_background_and_accents() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (_dir, mut app) = fixture();
    crate::ui::set_light_background(false);
    crate::theme::use_default_accents(false);
    let dark = crate::theme::accent(0);
    assert!(!app.handle_event(Event::ThemeChanged(Theme::Light)));
    assert!(crate::ui::light_background());
    let light = crate::theme::accent(0);
    assert_ne!(light, dark);
    assert!(!app.handle_event(Event::ThemeChanged(Theme::Dark)));
    assert!(!crate::ui::light_background());
    assert_eq!(crate::theme::accent(0), dark);

    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        r##"<?xml version="1.0"?>
<plist version="1.0"><dict><key>settings</key><array>
<dict><key>scope</key><string>keyword</string><key>settings</key>
<dict><key>foreground</key><string>#123456</string></dict></dict>
</array></dict></plist>"##,
    )
    .unwrap();
    app.preferences.theme = Some(file.path().to_owned());
    app.apply_background(true);
    assert!(crate::ui::light_background());
    assert_eq!(
        crate::theme::accent(0),
        ratatui::style::Color::Rgb(0x12, 0x34, 0x56)
    );
    for theme in [Theme::Light, Theme::Dark] {
        assert!(!app.handle_event(Event::ThemeChanged(theme)));
        assert_eq!(crate::ui::light_background(), theme == Theme::Light);
        assert_eq!(
            crate::theme::accent(0),
            ratatui::style::Color::Rgb(0x12, 0x34, 0x56)
        );
    }
    crate::theme::use_default_accents(false);
}

#[test]
fn task_event_cache_rereads_when_event_count_changes() {
    let (_dir, mut app) = fixture();
    key(&mut app, KeyCode::BackTab);
    app.refresh();
    app.refresh_event_for_list_rows(8);
    super::event_search_tests::finish_search(&mut app);
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 20, 0));
    for (height, count) in [(24, 6), (40, 11), (60, 18), (18, 18), (24, 18)] {
        app.refresh_event_for_list_rows(
            app.view
                .event_read_count(ratatui::layout::Rect::new(0, 0, 120, height)),
        );
        assert_eq!(app.view.event_panel.len(), count);
    }
}

/// Gives each copy of the captured transcript its own event identities and visible marker.
fn transcript_copies(payload: &str, copies: usize, first: usize) -> String {
    let records: Vec<serde_json::Value> = payload
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut text = String::new();
    for copy in first..first + copies {
        for record in &records {
            let mut record = record.clone();
            if let Some(timestamp) = record.get("timestamp").and_then(|value| value.as_str()) {
                let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp).unwrap()
                    + chrono::Duration::minutes(copy as i64);
                record["timestamp"] = timestamp.to_rfc3339().into();
            }
            for field in ["uuid", "parentUuid", "leafUuid"] {
                if let Some(id) = record.get(field).and_then(|value| value.as_str()) {
                    let (prefix, suffix) = id.split_at(id.len() - 8);
                    let suffix = u32::from_str_radix(suffix, 16).unwrap() ^ (copy as u32 + 1);
                    record[field] = format!("{prefix}{suffix:08x}").into();
                }
            }
            if let Some(content) = record.pointer_mut("/message/content") {
                match content {
                    serde_json::Value::String(body) => {
                        *body = format!("Transcript copy {copy}\n\n{body}");
                    }
                    serde_json::Value::Array(blocks) => {
                        let body = blocks
                            .iter_mut()
                            .find_map(|block| block.get_mut("text"))
                            .expect("captured assistant has text");
                        *body =
                            format!("Transcript copy {copy}\n\n{}", body.as_str().unwrap()).into();
                    }
                    _ => panic!("unexpected captured message content"),
                }
            }
            text.push_str(&serde_json::to_string(&record).unwrap());
            text.push('\n');
        }
    }
    text
}

fn event_detail(app: &App) -> String {
    event_detail_at(app, ratatui::layout::Rect::new(0, 0, 120, 23))
}

fn event_detail_at(app: &App, area: ratatui::layout::Rect) -> String {
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    let bottom = area.height - 2;
    let rule = (0..bottom)
        .rev()
        .find(|&y| {
            (0..area.width)
                .filter(|&x| crate::ui::cell_text(buffer[(x, y)].symbol()) == "─")
                .count()
                > usize::from(area.width / 2)
        })
        .expect("log detail has a top rule");
    (rule + 1..bottom)
        .map(|y| {
            (0..area.width)
                .map(|x| crate::ui::cell_text(buffer[(x, y)].symbol()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn scroll_event(app: &mut App, kind: crossterm::event::MouseEventKind) {
    let area = ratatui::layout::Rect::new(0, 0, 120, 23);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: 2,
        row: 13,
        modifiers: KeyModifiers::NONE,
    }));
}

pub(super) fn click_event(app: &mut App) {
    let area = ratatui::layout::Rect::new(0, 0, 120, 23);
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
            kind,
            column: 2,
            row: 13,
            modifiers: KeyModifiers::NONE,
        }));
    }
}

#[test]
fn queued_wheel_events_apply_before_the_next_draw() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 20, 0));
    app.refresh_event_for_list_rows(8);
    let area = ratatui::layout::Rect::new(0, 0, 120, 23);
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    let wheel = || {
        Input::Terminal(Ok(Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 13,
            modifiers: KeyModifiers::NONE,
        })))
    };
    for _ in 0..4 {
        app.sender.send(wheel()).unwrap();
    }
    let mut schedule = RefreshSchedule::new(std::time::Instant::now(), POLLING_INTERVAL);
    assert!(
        !app.handle_inputs(wheel(), &mut None, &mut schedule)
            .unwrap()
    );
    assert!(app.receiver.try_recv().is_err());
    assert_eq!(app.view.event_scroll(), 2);
    let detail = event_detail(&app);
    assert!(detail.contains("Transcript copy 17"), "{detail}");
    assert!(detail.contains("User"), "{detail}");
}

#[test]
fn task_event_scroll_reloads_requested_events_and_keeps_cache() {
    use crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 20, 0));
    app.refresh_event_for_list_rows(8);
    for _ in 0..7 {
        scroll_event(&mut app, ScrollUp);
    }
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_scroll(), 8);
    assert_eq!(app.transcript_stamp.as_ref().unwrap().count, 16);
    assert_eq!(app.view.event_panel.len(), 16);
    assert!(event_detail(&app).contains("Transcript copy 16"));
    let cached = app.view.event_panel.clone();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(
        &path,
        transcript_copies(&payload.replace("Codex", "CODEX"), 20, 0),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_panel, cached);
    for _ in 0..7 {
        scroll_event(&mut app, ScrollDown);
    }
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_scroll(), 11);
    assert_eq!(app.transcript_stamp.as_ref().unwrap().count, 16);
    assert_eq!(app.view.event_panel.len(), 16);
    assert_eq!(app.view.event_panel, cached);
    assert!(event_detail(&app).contains("Transcript copy 19"));
}

#[test]
fn task_event_wheel_reverses_across_reloaded_windows() {
    task_event_navigation_reverses_across_reloaded_windows(None);
}

#[test]
fn task_event_wheel_then_arrow_keys_reverse_across_reloaded_windows() {
    task_event_navigation_reverses_across_reloaded_windows(Some(KeyCode::Down));
}

#[test]
fn task_event_wheel_then_j_k_keys_reverse_across_reloaded_windows() {
    task_event_navigation_reverses_across_reloaded_windows(Some(KeyCode::Char('j')));
}

fn task_event_navigation_reverses_across_reloaded_windows(navigation: Option<KeyCode>) {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 150, 19);
    let visible = app.view.event_read_count(area);
    let copies = visible * 2 + 1;
    let transcript = format!(
        "{}{}{}",
        transcript_copies(&payload, 20, 0),
        include_str!("../../../tests/fixtures/claude-task-notifications.jsonl"),
        transcript_copies(&payload, copies, 20)
    );
    std::fs::write(&path, transcript).unwrap();
    let column = area.x + area.width * 2 / 3 + 1;
    let row = area.y + 2;
    let wheel = |app: &mut App, kind| {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
        app.refresh_event_for_list_rows(visible);
        app.view
            .render(area, &mut ratatui::buffer::Buffer::empty(area));
    };
    app.refresh_event_for_list_rows(visible);
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    // Each complete copy contributes two events; stop six events before copy 20.
    let loaded = app.view.event_panel.len();
    for _ in 0..(copies * 2 + 5) {
        wheel(&mut app, MouseEventKind::ScrollUp);
    }
    assert!(
        app.view.event_panel.len() > loaded,
        "older navigation must reload the window"
    );
    let before = event_detail_at(&app, area);
    if let Some(code) = navigation {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            app.view
                .render(area, &mut ratatui::buffer::Buffer::empty(area));
        }
        // Clicking the selected top row gives keyboard navigation to the event list.
        for _ in 0..6 {
            app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
            app.refresh_event_for_list_rows(visible);
            app.view
                .render(area, &mut ratatui::buffer::Buffer::empty(area));
        }
    } else {
        for _ in 0..6 {
            wheel(&mut app, MouseEventKind::ScrollDown);
        }
    }
    let after = event_detail_at(&app, area);
    assert!(
        after.contains("Transcript copy 20"),
        "newer navigation must pass the notification: {after}"
    );
    for _ in 0..6 {
        if let Some(code) = navigation {
            let code = if code == KeyCode::Down {
                KeyCode::Up
            } else {
                KeyCode::Char('k')
            };
            app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
            app.refresh_event_for_list_rows(visible);
            app.view
                .render(area, &mut ratatui::buffer::Buffer::empty(area));
        } else {
            wheel(&mut app, MouseEventKind::ScrollUp);
        }
    }
    assert_eq!(event_detail_at(&app, area), before);
}

#[test]
fn home_loads_first_event_of_whole_event() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 200, 0));
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_panel.len(), 8);
    click_event(&mut app);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    let area = ratatui::layout::Rect::new(0, 0, 120, 23);
    app.refresh_event_for_list_rows(app.view.event_read_count(area));
    assert!(app.pending_transcript.is_some());
    super::event_search_tests::finish_search(&mut app);
    let detail = event_detail_at(&app, area);
    assert!(detail.contains("Transcript copy 0"), "{detail}");
}

#[test]
fn top_border_loads_first_event_of_whole_event() {
    top_border_first_event_fixture(None);
}

fn top_border_first_event_fixture(last_copy: Option<usize>) -> (tempfile::TempDir, App) {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 200, 45);
    let visible = app.view.event_read_count(area);
    // More than three windows plus their next pages keeps copy 0 out of a page load.
    let copies = 3 * (visible * 2) + 1;
    let mut transcript = transcript_copies(&payload, copies, 0);
    if let Some(copy) = last_copy {
        transcript.push_str(&transcript_copies(&payload, 1, copy));
    }
    replace_transcript(&path, transcript);
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_panel.len(), 8);
    let scroll = |app: &mut App, kind| {
        app.view
            .render(area, &mut ratatui::buffer::Buffer::empty(area));
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column: 134,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }));
    };
    let click = |app: &mut App, row| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column: 134,
                row,
                modifiers: KeyModifiers::NONE,
            }));
        }
    };
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    click(&mut app, 1);
    click(&mut app, 1);
    app.refresh_event_for_list_rows(visible);
    super::event_search_tests::finish_search(&mut app);
    let detail = event_detail_at(&app, area);
    assert!(detail.contains("Transcript copy 0"), "{detail}");
    scroll(&mut app, MouseEventKind::ScrollDown);
    scroll(&mut app, MouseEventKind::ScrollDown);
    app.refresh_event_for_list_rows(visible);
    assert!(event_detail_at(&app, area).contains("Transcript copy 1"));
    scroll(&mut app, MouseEventKind::ScrollUp);
    scroll(&mut app, MouseEventKind::ScrollUp);
    app.refresh_event_for_list_rows(visible);
    assert!(event_detail_at(&app, area).contains("Transcript copy 0"));
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    (_dir, app)
}

#[test]
fn new_indicator_loads_latest_event_after_top_border() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_dir, mut app) = top_border_first_event_fixture(None);
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = include_str!("../../../tests/fixtures/transcript.jsonl");
    let area = ratatui::layout::Rect::new(0, 0, 200, 45);
    let visible = app.view.event_read_count(area);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(transcript_copies(payload, 1, 200).as_bytes())
        .unwrap();
    app.refresh_event_for_list_rows(visible);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    let row = (0..45)
        .find(|&y| {
            (130..200)
                .map(|x| crate::ui::cell_text(buffer[(x, y)].symbol()))
                .collect::<String>()
                .contains(" New╶")
        })
        .unwrap();
    let line = (0..200)
        .map(|x| crate::ui::cell_text(buffer[(x, row)].symbol()))
        .collect::<String>();
    let column = line.chars().position(|c| c == '↓').unwrap() as u16;
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    app.refresh_event_for_list_rows(visible);
    assert!(event_detail_at(&app, area).contains("Transcript copy 200"));
    assert_eq!(app.view.event_scroll(), app.view.event_max_scroll());
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
}

#[test]
fn bottom_border_loads_latest_event_after_top_border() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_dir, mut app) = top_border_first_event_fixture(Some(200));
    let area = ratatui::layout::Rect::new(0, 0, 200, 45);
    let visible = app.view.event_read_count(area);
    app.refresh_event_for_list_rows(visible);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    let bottom = (2..45)
        .find(|&y| (100..200).any(|x| buffer[(x, y)].symbol() == "┴"))
        .expect("event list bottom rule joins its visible column separator");
    for _ in 0..2 {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column: 134,
                row: bottom,
                modifiers: KeyModifiers::NONE,
            }));
        }
    }
    app.refresh_event_for_list_rows(visible);
    assert!(event_detail_at(&app, area).contains("Transcript copy 200"));
    assert_eq!(app.view.event_scroll(), app.view.event_max_scroll());
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
}

#[test]
fn task_event_growth_keeps_older_selection_and_viewport() {
    use crossterm::event::MouseEventKind::ScrollUp;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 20, 0));
    app.refresh_event_for_list_rows(
        app.view
            .event_read_count(ratatui::layout::Rect::new(0, 0, 120, 23)),
    );
    scroll_event(&mut app, ScrollUp);
    app.refresh_event_for_list_rows(
        app.view
            .event_read_count(ratatui::layout::Rect::new(0, 0, 120, 23)),
    );
    let before = app.view.event_panel.clone();
    let selected = event_detail(&app);
    let scroll = app.view.event_scroll();
    std::fs::write(
        &path,
        transcript_copies(&payload, 20, 0)
            + transcript_copies(&payload.replace("Codex", "CODEX"), 1, 20).as_str(),
    )
    .unwrap();
    app.refresh_event_for_list_rows(
        app.view
            .event_read_count(ratatui::layout::Rect::new(0, 0, 120, 23)),
    );
    assert_eq!(app.view.event_scroll(), scroll);
    assert_eq!(event_detail(&app), selected);
    assert_ne!(app.view.event_panel, before);
    let detail = event_detail(&app);
    assert!(detail.contains("Transcript copy 19"), "{detail}");
    assert!(detail.contains("User"), "{detail}");
    assert!(!detail.contains("Transcript copy 20"), "{detail}");
}

#[test]
fn task_event_search_loads_all_caches_matches_and_refreshes_growth() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    replace_transcript(&path, transcript_copies(&payload, 20, 0));
    app.refresh_event_for_list_rows(1);
    click_event(&mut app);
    key(&mut app, KeyCode::Char('/'));
    app.refresh_event_for_list_rows(1);
    for c in "Codex".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    super::event_search_tests::finish_search(&mut app);
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.view.matching_event_count(), 20);
    for _ in 0..30 {
        scroll_event(&mut app, crossterm::event::MouseEventKind::ScrollUp);
    }
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.view.event_scroll(), 0);
    let detail = event_detail(&app);
    assert!(detail.contains("Transcript copy 0"), "{detail}");
    assert!(detail.contains("User"), "{detail}");
    let cached = app.view.event_panel.clone();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(
        &path,
        transcript_copies(&payload.replace("Codex", "other"), 20, 0),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    key(&mut app, KeyCode::Backspace);
    app.refresh_event_for_list_rows(1);
    assert_eq!(
        app.view.event_panel, cached,
        "query changes reuse cached transcript"
    );
    key(&mut app, KeyCode::Char('x'));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({"type":"queue-operation", "operation":"enqueue", "timestamp":"2026-10-06T17:00:00Z", "content":"Codex timeout"})).unwrap();
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.view.matching_event_count(), 21);
    assert_eq!(app.view.event_panel.last().unwrap().body, "Codex timeout");
    key(&mut app, KeyCode::Esc);
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.view.event_panel.len(), 41);
}

#[test]
fn task_event_scroll_clamps_at_transcript_start() {
    use crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
    let (_dir, mut app) = fixture();
    assert_eq!(app.view.event_panel.len(), 2);
    app.refresh_event_for_list_rows(1);
    for _ in 0..5 {
        scroll_event(&mut app, ScrollUp);
    }
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.view.event_panel.len(), 2);
    scroll_event(&mut app, ScrollDown);
    app.refresh_event_for_list_rows(1);
    assert_eq!(app.transcript_stamp.as_ref().unwrap().count, 2);
    assert_eq!(app.view.event_panel.len(), 2);
    // A transcript shorter than the visible window clamps all the way to zero.
    scroll_event(&mut app, ScrollUp);
    app.refresh_event_for_list_rows(8);
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.transcript_stamp.as_ref().unwrap().count, 8);
}

#[test]
fn task_event_cache_uses_selection_length_and_mtime() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    key(&mut app, KeyCode::BackTab);
    app.refresh();
    app.refresh_event_for_list_rows(8);
    super::event_search_tests::finish_search(&mut app);
    assert_eq!(app.view.event_panel.len(), 2);
    let path = app.sessions[0].transcript.clone().unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original.replace("Codex", "CODEX");
    assert_eq!(original.len(), changed.len());
    std::fs::write(&path, &changed).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(modified).unwrap();
    let cached = app.view.event_panel.clone();
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.view.event_panel, cached);
    file.set_modified(modified + Duration::from_secs(1))
        .unwrap();
    app.refresh_event_for_list_rows(8);
    assert_ne!(app.view.event_panel, cached);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({"type":"queue-operation", "operation":"enqueue", "timestamp":"2026-10-06T16:40:00Z", "content":"queued"})).unwrap();
    file.set_modified(modified + Duration::from_secs(1))
        .unwrap();
    app.refresh();
    app.refresh_event_for_list_rows(8);
    assert_eq!(app.sessions.len(), 1);
    assert!(
        app.view.selected_id().is_some(),
        "selected disappeared after refresh"
    );
    assert_eq!(app.view.event_panel.last().unwrap().body, "queued");
}

#[test]
fn task_event_reloads_on_keyboard_selection_and_clears_without_transcript() {
    let (dir, mut app) = fixture();
    let mut second = app.sessions[0].clone();
    second.id = session_key("second");
    second.updated_at -= chrono::Duration::seconds(1);
    let path = dir.path().join("second.jsonl");
    std::fs::write(&path, serde_json::json!({"type":"user", "timestamp":"2026-10-06T16:40:00Z", "message":{"content":"second prompt"}}).to_string()).unwrap();
    second.transcript = Some(path);
    let mut third = second.clone();
    third.id = session_key("third");
    third.updated_at -= chrono::Duration::seconds(1);
    third.transcript = None;
    app.sessions.extend([second, third]);
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event_for_list_rows(8);
    key(&mut app, KeyCode::Down);
    assert_eq!(
        app.view.event_session.as_ref(),
        Some(&session_key("second"))
    );
    assert_eq!(app.view.event_panel[0].body, "second prompt");
    key(&mut app, KeyCode::Down);
    assert!(app.view.event_panel.is_empty());
    assert!(app.view.event_session.is_none());
}

#[test]
fn subagent_selection_loads_own_transcript_and_restores_parent_event() {
    let (dir, mut app) = fixture();
    let parent = app.sessions[0].clone();
    let parent_event = app.view.event_panel.clone();
    let mut child = parent.clone();
    child.id = session_key("agent-abf695d5995bd43ab");
    child.parent = Some(parent.id.clone());
    child.process = None;
    child.status = SessionStatus::Working;
    let path = dir.path().join("agent-abf695d5995bd43ab.jsonl");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/subagent.jsonl"),
    )
    .unwrap();
    child.transcript = Some(path);
    app.sessions.push(child.clone());
    app.view.set_sessions(app.sessions.clone(), false);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.view.selected_id(), Some(&child.id));
    assert_eq!(app.view.event_session.as_ref(), Some(&child.id));
    assert_eq!(app.transcript_stamp.as_ref().unwrap().session, child.id);
    assert!(
        app.view.event_panel[0]
            .body
            .starts_with("SyntaxWarning 修好了")
    );
    click_event(&mut app);
    key(&mut app, KeyCode::Char('/'));
    app.refresh_event_for_list_rows(8);
    assert_eq!(
        app.full_transcript_stamp.as_ref().unwrap().session,
        child.id
    );
    assert!(
        app.view.event_panel[0]
            .body
            .starts_with("SyntaxWarning 修好了")
    );
    key(&mut app, KeyCode::Esc);
    // A remembered task row enters its parked focus; use panel navigation for the list.
    for direction in ['k', 'k', 'k', 'k', 'h'] {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )));
        key(&mut app, KeyCode::Char(direction));
    }
    key(&mut app, KeyCode::Up);
    assert_eq!(app.view.event_session.as_ref(), Some(&parent.id));
    assert_eq!(app.transcript_stamp.as_ref().unwrap().session, parent.id);
    assert_eq!(app.view.event_panel, parent_event);
}

#[test]
fn working_subagent_event_ticks_without_process_and_ready_event_stops() {
    let (dir, mut app, _child) = subagent_fixture();
    let path = dir.path().join("running-subagent.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}{}\n",
            include_str!("../../../tests/fixtures/transcript.jsonl"),
            include_str!("../../../tests/fixtures/automode_blocked.jsonl")
                .lines()
                .next()
                .unwrap()
        ),
    )
    .unwrap();
    let now = chrono::Utc::now();
    for (status, expected) in [
        (SessionStatus::Working, Some(Duration::from_secs(1))),
        (SessionStatus::Ready, None),
    ] {
        app.sessions[1].status = status;
        app.view.set_sessions(app.sessions.clone(), false);
        app.view.event_panel = crate::providers::claude::event_reader::tail(&path, 8).unwrap();
        assert_eq!(app.view.event_redraw_in(now), expected);
    }
}

#[test]
fn last_prose_event_row_ticks_each_second_under_a_minute() {
    let (_dir, mut app) = fixture();
    let started_at = chrono::Utc::now();
    app.view.event_session = Some(app.sessions[0].id.clone());
    app.view.event_panel = vec![crate::event::Event {
        turn: None,
        completed_time: false,
        background: false,
        source: None,
        source_records: Vec::new(),
        sender: None,
        detail_body: None,
        kind: crate::event::Kind::Assistant,
        body: "done".into(),
        paths: Vec::new(),
        started_at,
        ended_at: None,
        duration: None,
        previous_end: None,
        images: Vec::new(),
        calls: Vec::new(),
    }];
    let at = |seconds| started_at + chrono::Duration::seconds(seconds);
    assert_eq!(
        app.view.event_redraw_in(at(12)),
        Some(Duration::from_secs(1))
    );
    assert_eq!(
        app.view.event_redraw_in(at(125)),
        Some(Duration::from_secs(55))
    );
}

fn subagent_fixture() -> (tempfile::TempDir, App, Session) {
    let (dir, mut app) = fixture();
    let mut child = app.sessions[0].clone();
    child.id = session_key("agent-abf695d5995bd43ab");
    child.parent = Some(app.sessions[0].id.clone());
    child.process = None;
    let path = dir.path().join(format!("{}.jsonl", child.id.id));
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/subagent.jsonl"),
    )
    .unwrap();
    child.transcript = Some(path);
    app.sessions.push(child.clone());
    app.view.set_sessions(app.sessions.clone(), false);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.view.selected_id(), Some(&child.id));
    (dir, app, child)
}

fn assert_subagent_action_blocked(make_action: impl FnOnce(&SessionKey) -> Action) {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (dir, mut app, child) = subagent_fixture();
    key(&mut app, KeyCode::Up);
    let selected = app.view.selected_id().unwrap().to_owned();
    assert_ne!(selected, child.id);
    let sessions = app.sessions.clone();
    let preferences = app.preferences.clone();
    let event_panel = app.view.event_panel.clone();
    let path = child.transcript.as_ref().unwrap();
    let transcript = std::fs::read(path).unwrap();
    let meta = path.with_extension("meta.json");
    std::fs::write(&meta, r#"{"description":"Check files","name":"worker"}"#).unwrap();
    let metadata = std::fs::read(&meta).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let mut before = ratatui::buffer::Buffer::empty(area);
    app.view.render_at(area, &mut before, child.updated_at);
    let action = make_action(&child.id);
    let operation = match action {
        Action::Resume(_) => Some("Resume"),
        Action::OpenTerminal(_) => Some("Terminal"),
        _ => None,
    };
    assert!(!app.handle_action(Some(action)));
    let mut after = ratatui::buffer::Buffer::empty(area);
    app.view.render_at(area, &mut after, child.updated_at);
    if let Some(operation) = operation {
        assert!(
            after
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains(&format!("{operation} is not supported"))
        );
        app.view
            .expire_messages(std::time::Instant::now() + std::time::Duration::from_secs(60));
        after = ratatui::buffer::Buffer::empty(area);
        app.view.render_at(area, &mut after, child.updated_at);
    }
    assert_eq!(after, before);
    assert_eq!(app.sessions, sessions);
    assert_eq!(app.preferences, preferences);
    assert_eq!(app.view.event_panel, event_panel);
    assert_eq!(app.view.selected_id(), Some(&selected));
    assert!(app.launch.is_none());
    assert_eq!(std::fs::read(path).unwrap(), transcript);
    assert!(!path.with_extension("").exists());
    assert_eq!(std::fs::read(meta).unwrap(), metadata);
    assert!(!dir.path().join("agents.json").exists());
}

#[test]
fn subagent_terminal_shows_unsupported_notice() {
    assert_subagent_action_blocked(|id| Action::OpenTerminal(id.clone()));
}

#[test]
fn subagent_resume_shows_unsupported_notice() {
    assert_subagent_action_blocked(|id| Action::Resume(id.clone()));
}

#[test]
fn subagent_fork_is_ignored() {
    assert_subagent_action_blocked(|id| Action::Fork(id.clone()));
}

#[test]
fn subagent_rename_is_ignored() {
    assert_subagent_action_blocked(|id| Action::Rename {
        id: id.clone(),
        name: "Rename".into(),
    });
}

#[test]
fn subagent_delete_is_ignored() {
    assert_subagent_action_blocked(|id| Action::Delete(id.clone()));
}

#[test]
fn subagent_pin_persists_and_unpins() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (dir, mut app, child) = subagent_fixture();
    for pinned in [true, false] {
        assert!(!app.handle_action(Some(Action::TogglePin {
            tab: 0,
            id: child.id.clone(),
            pinned,
        })));
        let expected = if pinned {
            vec![child.id.clone()]
        } else {
            vec![]
        };
        assert_eq!(app.preferences.pinned.active, expected);
        assert_eq!(Preferences::load(dir.path()).pinned.active, expected);
    }
}

#[test]
fn subagent_launch_is_ignored_before_directory_validation() {
    let (_dir, mut app, mut child) = subagent_fixture();
    child.cwd = child.cwd.join("missing");
    assert!(!app.launch_session(Action::Resume(child.id.clone()), child.clone()));
    assert!(app.launch.is_none());
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render_at(area, &mut buffer, child.updated_at);
    assert!(
        !buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("Directory no longer exists")
    );
}

#[test]
fn escape_without_selection_history_stays_in_dashboard() {
    let (_dir, mut app) = fixture();
    assert!(!key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_none());
}

#[test]
fn escape_after_fork_stays_in_dashboard() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Char('f')));
    app.launch.take().unwrap();
    assert!(!key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_none());
}

#[test]
fn escape_after_session_removed_stays_in_dashboard() {
    let (_dir, mut app) = fixture();
    app.sessions.clear();
    app.view.set_sessions(Vec::new(), false);
    assert!(!key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_none());
}

#[test]
fn escape_with_missing_directory_stays_in_dashboard() {
    let (dir, mut app) = fixture();
    app.sessions[0].cwd = dir.path().join("missing");
    assert!(!key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_none());
}

#[test]
fn control_c_after_fork_quits() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Char('f')));
    app.launch.take().unwrap();
    assert!(app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ))));
    assert!(app.launch.is_none());
}

#[test]
fn child_resume_actions_reach_the_provider_or_show_the_notice() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    use crate::providers::{Provider, ProviderId, SessionKey, codex::CodexProvider};
    use ratatui::{buffer::Buffer, layout::Rect};
    for manager in [false, true] {
        let (dir, mut app) = fixture();
        rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
            .unwrap()
            .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        let mut provider = CodexProvider::new(dir.path().into());
        let mut child = provider
            .refresh(2)
            .sessions
            .into_iter()
            .find(|session| session.id.id == "thread-13")
            .unwrap();
        if manager {
            child.parent = Some(app.sessions[0].id.clone());
        }
        child.cwd = dir.path().into();
        app.hub = Hub::new(vec![Box::new(provider)]);
        app.sessions.push(child.clone());
        let action = Action::Resume(child.id.clone());
        assert_eq!(app.handle_action(Some(action)), manager);
        if manager {
            let command = app.launch.take().unwrap();
            assert_eq!(command.get_program(), "codex");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["resume", "thread-13"]
            );
        } else {
            assert!(app.launch.is_none());
            let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
            app.view.render(buffer.area, &mut buffer);
            assert!(
                buffer
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .contains("Resume is not supported")
            );
        }
        if !manager {
            for action in [
                Action::Fork(child.id.clone()),
                Action::Delete(child.id.clone()),
            ] {
                assert!(!app.handle_action(Some(action)));
                assert!(app.launch.is_none());
                assert!(app.preferences.pinned.active.is_empty());
            }
        }
        let mut claude_child = app.sessions[0].clone();
        claude_child.id = SessionKey {
            provider: ProviderId::Claude,
            id: "agent-child".into(),
        };
        claude_child.parent = Some(app.sessions[0].id.clone());
        app.sessions.push(claude_child.clone());
        assert!(!app.handle_action(Some(Action::Resume(claude_child.id))));
        assert!(app.launch.is_none());
    }
}

#[test]
fn child_rename_only_reaches_provider_for_manager_threads() {
    use crate::providers::{Provider, ProviderId, codex::CodexProvider};
    use ratatui::{buffer::Buffer, layout::Rect};
    for (child_provider, parent_provider) in [
        (ProviderId::Codex, ProviderId::Claude),
        (ProviderId::Codex, ProviderId::Codex),
        (ProviderId::Claude, ProviderId::Claude),
    ] {
        let (dir, mut app) = fixture();
        rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
            .unwrap()
            .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        let mut provider = CodexProvider::new(dir.path().into());
        let mut child = provider
            .refresh(2)
            .sessions
            .into_iter()
            .find(|session| session.id.id == "thread-13")
            .unwrap();
        child.id.provider = child_provider;
        child.parent = Some(SessionKey {
            provider: parent_provider,
            id: app.sessions[0].id.id.clone(),
        });
        app.hub = Hub::new(vec![Box::new(provider)]);
        app.sessions.push(child.clone());
        assert!(!app.handle_action(Some(Action::Rename {
            id: child.id.clone(),
            name: "Rename probe".into()
        })));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
        app.view.render(buffer.area, &mut buffer);
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(
            rendered.contains("Cannot rename"),
            child_provider == ProviderId::Codex && parent_provider == ProviderId::Claude
        );
        assert!(app.launch.is_none());
        if !child.can_resume() {
            for action in [
                Action::Fork(child.id.clone()),
                Action::Delete(child.id.clone()),
                Action::Archive(child.id.clone()),
                Action::Stop(child.id.clone()),
            ] {
                assert!(!app.handle_action(Some(action)));
                assert!(app.launch.is_none());
                assert!(app.preferences.pinned.active.is_empty());
            }
        }
    }
}

#[test]
fn manager_thread_fork_reaches_app_launch() {
    use crate::providers::{Provider, codex::CodexProvider};
    let (dir, mut app) = fixture();
    rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut provider = CodexProvider::new(dir.path().into());
    let mut child = provider
        .refresh(2)
        .sessions
        .into_iter()
        .find(|session| session.id.id == "thread-13")
        .unwrap();
    child.parent = Some(app.sessions[0].id.clone());
    child.cwd = dir.path().into();
    app.hub = Hub::new(vec![Box::new(provider)]);
    app.sessions.push(child.clone());
    assert!(app.handle_action(Some(Action::Fork(child.id))));
    let command = app.launch.unwrap();
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["fork", "thread-13"]
    );
}

#[test]
fn manager_thread_mutations_reach_app_provider_paths() {
    use crate::providers::codex::CodexProvider;
    use ratatui::{buffer::Buffer, layout::Rect};
    for operation in ["archive", "delete"] {
        let (dir, mut app) = fixture();
        rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
            .unwrap()
            .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        let parent = app.sessions[0].id.id.clone();
        let manager = dir.path().join("manager");
        std::fs::create_dir_all(manager.join("threads")).unwrap();
        std::fs::create_dir_all(manager.join(&parent)).unwrap();
        let mut supervisor: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../tests/fixtures/codex-manager/attached-supervisor.json"
        ))
        .unwrap();
        supervisor["sessionId"] = parent.clone().into();
        std::fs::write(
            manager.join("threads/thread-12.json"),
            serde_json::to_vec(&supervisor).unwrap(),
        )
        .unwrap();
        let state = manager.join(&parent).join("state.json");
        let bytes = include_bytes!("../../../tests/fixtures/codex-manager/attached-state.json");
        std::fs::write(&state, bytes).unwrap();
        app.hub = Hub::new(vec![
            Box::new(CodexProvider::new(dir.path().into())),
            Box::new(
                crate::providers::claude::ClaudeProvider::new(dir.path().into())
                    .with_codex_manager(manager),
            ),
        ]);
        // The initial lookup reads two pages; action refreshes keep their one-page budget.
        let snapshot = app
            .hub
            .session_source()
            .run(2, crate::sessions::RefreshRequest::Full);
        let child = snapshot
            .sessions
            .iter()
            .find(|session| session.id.id == "thread-12")
            .cloned()
            .unwrap();
        assert!(child.parent.is_some() && child.can_resume());
        app.session_refresh = SessionRefresh::new(app.hub.session_source());
        app.supervisions = snapshot.supervisions;
        app.sessions.push(child.clone());
        let action = match operation {
            "archive" => Action::Archive(child.id),
            "delete" => Action::Delete(child.id),
            _ => unreachable!(),
        };
        assert!(!app.handle_action(Some(action)));
        if operation == "delete" {
            app.finish_refreshes();
        }
        if operation == "archive" {
            let event = app.receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(matches!(&event, Input::Archived { result: Err(_), .. }));
            let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
            app.handle_inputs(event, &mut None, &mut schedule).unwrap();
        }
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
        app.view.render(buffer.area, &mut buffer);
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        // Captured data has neither a daemon nor a real transcript.
        let notice = if operation == "delete" {
            "Nothing to delete".to_owned()
        } else {
            format!("Cannot {operation}")
        };
        assert!(rendered.contains(&notice), "{rendered}");
        assert_eq!(std::fs::read(&state).unwrap(), bytes);
    }
}

#[test]
fn new_key_adds_a_claude_draft_whatever_is_selected() {
    use crate::providers::{Provider, codex::CodexProvider};
    let (dir, mut app) = fixture();
    for args in [
        vec!["init", "-b", "workstream/branch-name-tests"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut codex = CodexProvider::new(dir.path().into());
    let mut thread = codex.refresh(1).sessions.remove(0);
    thread.cwd = dir.path().into();
    app.hub = Hub::new(vec![
        Box::new(crate::providers::claude::ClaudeProvider::new(
            dir.path().to_owned(),
        )),
        Box::new(codex),
    ]);
    let claude = app.sessions[0].clone();
    for rows in [vec![claude], vec![thread], vec![]] {
        app.view = crate::ui::View::default();
        app.view.startup_cwd = dir.path().to_owned();
        app.view.set_capabilities(app.hub.capabilities());
        app.view.set_input_choices(app.hub.input_choices());
        let inactive = rows
            .first()
            .is_some_and(|row| row.status == SessionStatus::Inactive);
        app.view.set_sessions(rows, false);
        if inactive {
            key(&mut app, KeyCode::Tab);
        }
        assert!(!key(&mut app, KeyCode::Char('n')));
        assert!(
            app.launch.is_none(),
            "a draft must not start an interactive process"
        );
        assert!(
            app.view.selected_id().is_none(),
            "a draft has no session key"
        );
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 120, 40));
        app.view.render(buffer.area, &mut buffer);
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("New task"), "{rendered}");
    }
}

#[test]
fn delete_confirmation_removes_real_transcript_through_provider() {
    let (_dir, mut app) = fixture();
    let target = app.sessions[0].clone();
    let path = target.transcript.as_ref().unwrap();
    let original = std::fs::read(path).unwrap();
    let keep_id = "00000000-0000-0000-0000-000000000002";
    let keep = path.parent().unwrap().join(format!("{keep_id}.jsonl"));
    std::fs::write(&keep, &original).unwrap();
    let children = path.with_extension("");
    std::fs::create_dir_all(children.join("subagents")).unwrap();
    std::fs::write(
        children.join("subagents/agent-child.jsonl"),
        include_bytes!("../../../tests/fixtures/subagent.jsonl"),
    )
    .unwrap();
    app.refresh();
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Home);
    for _ in 0..app.sessions.len() {
        if app.view.selected_id() == Some(&target.id) {
            break;
        }
        key(&mut app, KeyCode::Down);
    }
    assert_eq!(app.view.selected_id(), Some(&target.id));

    let before_delete = std::fs::read(path).unwrap();
    assert!(!key(&mut app, KeyCode::Backspace));
    assert!(!key(&mut app, KeyCode::Enter));
    assert_eq!(std::fs::read(path).unwrap(), before_delete);
    assert!(children.is_dir());
    assert!(app.session(&target.id).is_some());

    assert!(!key(&mut app, KeyCode::Backspace));
    assert!(!key(&mut app, KeyCode::Down));
    assert!(!key(&mut app, KeyCode::Enter));
    let event = app.next_input(Duration::from_secs(10));
    assert_ne!(app.view.selected_id(), Some(&target.id));
    assert!(matches!(&event, Input::Deleted { id, result: Ok(None) } if id == &target.id));
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    app.finish_refreshes();
    assert!(!path.exists());
    assert!(!children.exists());
    assert!(app.session(&target.id).is_none());
    assert_eq!(std::fs::read(&keep).unwrap(), original);
    assert!(app.session(&session_key(keep_id)).is_some());
    app.refresh();
    assert!(app.session(&target.id).is_none());
    assert_eq!(std::fs::read(keep).unwrap(), original);
}

#[test]
fn rename_without_socket_leaves_claude_files_untouched() {
    for foreground in [false, true] {
        let (_dir, mut app) = fixture();
        let mut target = app.sessions[0].clone();
        if foreground {
            target.process = Some(crate::model::Process {
                pid: std::process::id(),
                label: None,
                started_at: None,
            });
        }
        app.sessions[0] = target.clone();
        app.view.set_sessions(app.sessions.clone(), false);
        let path = target.transcript.as_ref().unwrap();
        let original = std::fs::read(path).unwrap();
        assert!(!app.handle_action(Some(Action::Rename {
            id: target.id.clone(),
            name: "Do not write this title".into(),
        })));
        let event = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let Input::Sent { input, result, .. } = event else {
            panic!("rename must use the provider input worker");
        };
        assert_eq!(
            input,
            SessionInput::Rename("Do not write this title".into())
        );
        let error = result.unwrap_err();
        if foreground {
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        } else {
            assert_eq!(
                error.to_string(),
                "This input requires a live Claude session"
            );
        }
        assert!(
            std::fs::read(path).unwrap() == original,
            "rename modified the transcript"
        );
        assert!(!path.with_extension("").join("custom-title.json").exists());
    }
}
