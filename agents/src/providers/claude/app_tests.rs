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
    app.refresh_log(8);
    (dir, app)
}

pub(super) fn key(app: &mut App, code: KeyCode) -> bool {
    let quit = app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    app.refresh_log(8);
    quit
}

fn assert_resume(app: &App) {
    let command = app.launch.as_ref().expect("session should resume");
    assert_eq!(command.get_program(), "claude");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["--resume", app.sessions[0].id.id.as_str()]
    );
    assert_eq!(
        command.get_current_dir(),
        Some(app.sessions[0].cwd.as_path())
    );
}

#[test]
fn terminal_theme_events_update_background_and_accents() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (_dir, mut app) = fixture();
    crate::ui::set_light_background(false);
    crate::theme::use_default_accents(false);
    let dark = crate::theme::color_for_id("theme-test");
    assert!(!app.handle_event(Event::ThemeChanged(Theme::Light)));
    assert!(crate::ui::light_background());
    let light = crate::theme::color_for_id("theme-test");
    assert_ne!(light, dark);
    assert!(!app.handle_event(Event::ThemeChanged(Theme::Dark)));
    assert!(!crate::ui::light_background());
    assert_eq!(crate::theme::color_for_id("theme-test"), dark);

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
        crate::theme::color_for_id("theme-test"),
        ratatui::style::Color::Rgb(0x12, 0x34, 0x56)
    );
    for theme in [Theme::Light, Theme::Dark] {
        assert!(!app.handle_event(Event::ThemeChanged(theme)));
        assert_eq!(crate::ui::light_background(), theme == Theme::Light);
        assert_eq!(
            crate::theme::color_for_id("theme-test"),
            ratatui::style::Color::Rgb(0x12, 0x34, 0x56)
        );
    }
    crate::theme::use_default_accents(false);
}

#[test]
fn task_log_cache_rereads_when_event_count_changes() {
    let (_dir, mut app) = fixture();
    key(&mut app, KeyCode::BackTab);
    app.refresh();
    app.refresh_log(8);
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, payload.repeat(20)).unwrap();
    for (height, count) in [(24, 8), (40, 12), (60, 20), (19, 0), (24, 8)] {
        app.refresh_log(
            app.view
                .log_event_count(ratatui::layout::Rect::new(0, 0, 120, height)),
        );
        assert_eq!(app.view.log.len(), count);
    }
}

pub(super) fn scroll_log(app: &mut App, kind: crossterm::event::MouseEventKind) {
    let area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    app.handle_event(Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: 2,
        row: 14,
        modifiers: KeyModifiers::NONE,
    }));
}

#[test]
fn task_log_scroll_reloads_requested_events_and_keeps_cache() {
    use crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, payload.repeat(20)).unwrap();
    app.refresh_log(8);
    scroll_log(&mut app, ScrollUp);
    app.refresh_log(8);
    assert_eq!(app.log_stamp.as_ref().unwrap().count, 9);
    assert_eq!(app.view.log.len(), 9);
    let cached = app.view.log.clone();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, payload.replace("Codex", "CODEX").repeat(20)).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    app.refresh_log(8);
    assert_eq!(app.view.log, cached);
    scroll_log(&mut app, ScrollDown);
    app.refresh_log(8);
    assert_eq!(app.log_stamp.as_ref().unwrap().count, 8);
    assert_eq!(app.view.log.len(), 8);
    assert_ne!(app.view.log, cached[1..]);
}

#[test]
fn task_log_growth_keeps_distance_from_end() {
    use crossterm::event::MouseEventKind::ScrollUp;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, payload.repeat(20)).unwrap();
    app.refresh_log(8);
    scroll_log(&mut app, ScrollUp);
    app.refresh_log(8);
    let before = app.view.log.clone();
    std::fs::write(
        &path,
        payload.repeat(20) + &payload.replace("Codex", "CODEX"),
    )
    .unwrap();
    app.refresh_log(8);
    assert_eq!(app.view.log_scroll(), 1);
    assert_eq!(app.log_stamp.as_ref().unwrap().count, 9);
    assert_ne!(app.view.log, before);
}

#[test]
fn task_log_search_loads_all_caches_matches_and_refreshes_growth() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, payload.repeat(20)).unwrap();
    app.refresh_log(1);
    app.view.handle_key(crossterm::event::KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    ));
    app.refresh_log(1);
    for c in "Codex".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    app.refresh_log(1);
    assert_eq!(app.view.matching_log_count(), 20);
    for _ in 0..30 {
        scroll_log(&mut app, crossterm::event::MouseEventKind::ScrollUp);
    }
    app.refresh_log(1);
    assert_eq!(app.view.log_scroll(), 19);
    let cached = app.view.log.clone();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, payload.replace("Codex", "other").repeat(20)).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    key(&mut app, KeyCode::Backspace);
    app.refresh_log(1);
    assert_eq!(
        app.view.log, cached,
        "query changes reuse cached transcript"
    );
    key(&mut app, KeyCode::Char('x'));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({"type":"queue-operation", "operation":"enqueue", "timestamp":"2026-10-06T16:40:00Z", "content":"Codex timeout"})).unwrap();
    app.refresh_log(1);
    assert_eq!(app.view.matching_log_count(), 1);
    assert_eq!(app.view.log.last().unwrap().body, "Codex timeout");
    key(&mut app, KeyCode::Esc);
    app.refresh_log(1);
    assert_eq!(app.view.log.len(), 1);
}

#[test]
fn task_log_scroll_clamps_at_transcript_start() {
    use crossterm::event::MouseEventKind::{ScrollDown, ScrollUp};
    let (_dir, mut app) = fixture();
    assert_eq!(app.view.log.len(), 2);
    app.refresh_log(1);
    for _ in 0..5 {
        scroll_log(&mut app, ScrollUp);
    }
    app.refresh_log(1);
    assert_eq!(app.view.log.len(), 2);
    scroll_log(&mut app, ScrollDown);
    app.refresh_log(1);
    assert_eq!(app.log_stamp.as_ref().unwrap().count, 1);
    assert_eq!(app.view.log.len(), 1);
    // A transcript shorter than the visible window clamps all the way to zero.
    scroll_log(&mut app, ScrollUp);
    app.refresh_log(8);
    app.refresh_log(8);
    assert_eq!(app.log_stamp.as_ref().unwrap().count, 8);
}

#[test]
fn task_log_cache_uses_selection_length_and_mtime() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    key(&mut app, KeyCode::BackTab);
    app.refresh();
    app.refresh_log(8);
    assert_eq!(app.view.log.len(), 2);
    let path = app.sessions[0].transcript.clone().unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original.replace("Codex", "CODEX");
    assert_eq!(original.len(), changed.len());
    std::fs::write(&path, &changed).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(modified).unwrap();
    let cached = app.view.log.clone();
    app.refresh_log(8);
    assert_eq!(app.view.log, cached);
    file.set_modified(modified + Duration::from_secs(1))
        .unwrap();
    app.refresh_log(8);
    assert_ne!(app.view.log, cached);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({"type":"queue-operation", "operation":"enqueue", "timestamp":"2026-10-06T16:40:00Z", "content":"queued"})).unwrap();
    file.set_modified(modified + Duration::from_secs(1))
        .unwrap();
    app.refresh();
    app.refresh_log(8);
    assert_eq!(app.sessions.len(), 1);
    assert!(
        app.view.selected_id().is_some(),
        "selected disappeared after refresh"
    );
    assert_eq!(app.view.log.last().unwrap().body, "queued");
}

#[test]
fn task_log_reloads_on_keyboard_selection_and_clears_without_transcript() {
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
    app.refresh_log(8);
    key(&mut app, KeyCode::Down);
    assert_eq!(app.view.log_session.as_ref(), Some(&session_key("second")));
    assert_eq!(app.view.log[0].body, "second prompt");
    key(&mut app, KeyCode::Down);
    assert!(app.view.log.is_empty());
    assert!(app.view.log_session.is_none());
}

#[test]
fn subagent_selection_loads_own_transcript_and_restores_parent_log() {
    let (dir, mut app) = fixture();
    let parent = app.sessions[0].clone();
    let parent_log = app.view.log.clone();
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
    assert_eq!(app.view.log_session.as_ref(), Some(&child.id));
    assert_eq!(app.log_stamp.as_ref().unwrap().session, child.id);
    assert!(app.view.log[0].body.starts_with("SubagentHandback"));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    app.refresh_log(8);
    assert_eq!(app.full_log_stamp.as_ref().unwrap().session, child.id);
    assert!(app.view.log[0].body.starts_with("SubagentHandback"));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.view.log_session.as_ref(), Some(&parent.id));
    assert_eq!(app.log_stamp.as_ref().unwrap().session, parent.id);
    assert_eq!(app.view.log, parent_log);
}

#[test]
fn working_subagent_log_ticks_without_process_and_ready_log_stops() {
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
        app.view.log = crate::providers::claude::log::tail(&path, 8).unwrap();
        assert_eq!(app.view.log_redraw_in(now), expected);
    }
}

#[test]
fn last_prose_log_row_ticks_each_second_under_a_minute() {
    let (_dir, mut app) = fixture();
    let started_at = chrono::Utc::now();
    app.view.log_session = Some(app.sessions[0].id.clone());
    app.view.log = vec![crate::log::Event {
        kind: crate::log::Kind::Model,
        body: "done".into(),
        path: None,
        started_at,
        ended_at: None,
        previous_end: None,
    }];
    let at = |seconds| started_at + chrono::Duration::seconds(seconds);
    assert_eq!(app.view.log_redraw_in(at(12)), Some(Duration::from_secs(1)));
    assert_eq!(
        app.view.log_redraw_in(at(125)),
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

fn assert_subagent_action_ignored(make_action: impl FnOnce(&SessionKey) -> Action) {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (dir, mut app, child) = subagent_fixture();
    key(&mut app, KeyCode::Up);
    let selected = app.view.selected_id().unwrap().to_owned();
    assert_ne!(selected, child.id);
    let sessions = app.sessions.clone();
    let preferences = app.preferences.clone();
    let log = app.view.log.clone();
    let path = child.transcript.as_ref().unwrap();
    let transcript = std::fs::read(path).unwrap();
    let meta = path.with_extension("meta.json");
    std::fs::write(&meta, r#"{"description":"Check files","name":"worker"}"#).unwrap();
    let metadata = std::fs::read(&meta).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let mut before = ratatui::buffer::Buffer::empty(area);
    app.view.render_at(area, &mut before, child.updated_at);
    assert!(!app.handle_action(Some(make_action(&child.id))));
    let mut after = ratatui::buffer::Buffer::empty(area);
    app.view.render_at(area, &mut after, child.updated_at);
    assert_eq!(after, before);
    assert_eq!(app.sessions, sessions);
    assert_eq!(app.preferences, preferences);
    assert_eq!(app.view.log, log);
    assert_eq!(app.view.selected_id(), Some(&selected));
    assert!(app.launch.is_none());
    assert!(app.last_session.is_none());
    assert_eq!(std::fs::read(path).unwrap(), transcript);
    assert!(!path.with_extension("").exists());
    assert_eq!(std::fs::read(meta).unwrap(), metadata);
    assert!(!dir.path().join("agents.json").exists());
}

#[test]
fn subagent_open_is_ignored() {
    assert_subagent_action_ignored(|id| Action::Open(id.clone()));
}

#[test]
fn subagent_resume_is_ignored() {
    assert_subagent_action_ignored(|id| Action::Resume(id.clone()));
}

#[test]
fn subagent_fork_is_ignored() {
    assert_subagent_action_ignored(|id| Action::Fork(id.clone()));
}

#[test]
fn subagent_rename_is_ignored() {
    assert_subagent_action_ignored(|id| Action::Rename {
        id: id.clone(),
        name: "Rename".into(),
    });
}

#[test]
fn subagent_delete_is_ignored() {
    assert_subagent_action_ignored(|id| Action::Delete(id.clone()));
}

#[test]
fn subagent_pin_is_ignored() {
    for pinned in [false, true] {
        assert_subagent_action_ignored(|id| Action::TogglePin {
            id: id.clone(),
            pinned,
        });
    }
}

#[test]
fn subagent_launch_is_ignored_before_directory_validation() {
    let (_dir, mut app, mut child) = subagent_fixture();
    child.cwd = child.cwd.join("missing");
    assert!(!app.launch_session(Action::Resume(child.id.clone()), child.clone()));
    assert!(app.launch.is_none());
    assert!(app.last_session.is_none());
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
fn escape_without_launched_session_opens_new_session() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Esc));
    let command = app.launch.as_ref().expect("new session should launch");
    assert_eq!(command.get_program(), "claude");
    assert_eq!(command.get_args().len(), 0);
    assert_eq!(
        command.get_current_dir(),
        Some(std::env::current_dir().unwrap().as_path())
    );
    assert!(app.last_session.is_none());
}

#[test]
fn escape_resumes_last_opened_session() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Enter));
    app.launch.take().unwrap();
    // True leaves the dashboard for either quit or launch; the command distinguishes them.
    assert!(key(&mut app, KeyCode::Esc));
    assert_resume(&app);
}

#[test]
fn escape_after_session_removed_opens_new_session() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Enter));
    app.launch.take().unwrap();
    let last_session = app.last_session.clone();
    app.sessions.clear();
    app.view.set_sessions(Vec::new(), false);
    assert!(key(&mut app, KeyCode::Esc));
    let command = app.launch.as_ref().expect("new session should launch");
    assert_eq!(command.get_program(), "claude");
    assert_eq!(command.get_args().len(), 0);
    assert_eq!(
        command.get_current_dir(),
        Some(std::env::current_dir().unwrap().as_path())
    );
    assert_eq!(app.last_session, last_session);
}

#[test]
fn escape_remembers_resume_and_fork_but_not_new() {
    for code in [KeyCode::Char('o'), KeyCode::Char('f')] {
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, code));
        app.launch.take().unwrap();
        assert!(key(&mut app, KeyCode::Char('n')));
        app.launch.take().unwrap();
        assert!(key(&mut app, KeyCode::Esc));
        assert_resume(&app);
    }
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Char('n')));
    app.launch.take().unwrap();
    assert!(key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_some());
    assert!(app.last_session.is_none());
}

#[test]
fn escape_with_missing_directory_stays_in_dashboard() {
    let (dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Enter));
    app.launch.take().unwrap();
    app.sessions[0].cwd = dir.path().join("missing");
    assert!(!key(&mut app, KeyCode::Esc));
    assert!(app.launch.is_none());
}

#[test]
fn control_c_after_open_quits() {
    let (_dir, mut app) = fixture();
    assert!(key(&mut app, KeyCode::Enter));
    app.launch.take().unwrap();
    assert!(app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL
    ))));
    assert!(app.launch.is_none());
}

mod log_filters {
    use super::{fixture, key, scroll_log};
    use crate::log::Kind;
    use crossterm::event::KeyCode;
    use crossterm::event::MouseEventKind::ScrollUp;

    #[test]
    fn tool_filter_loads_matching_count_and_caches_by_filter() {
        let (_dir, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        let mut records: Vec<serde_json::Value> =
            include_str!("../../../tests/fixtures/transcript.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let mut tool = records[1].clone();
        tool["message"]["id"] = "tool-call".into();
        tool["message"]["content"] = serde_json::json!([{
            "type":"tool_use", "id":"read", "name":"Read", "input":{"file_path":"src/main.rs"}
        }]);
        records.insert(1, tool);
        let payload = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        std::fs::write(&path, payload.repeat(20)).unwrap();
        app.refresh_log(8);
        key(&mut app, KeyCode::Char('t'));
        assert_eq!(app.view.log.len(), 8);
        assert!(app.view.log.iter().all(|event| event.kind == Kind::Tool));
        scroll_log(&mut app, ScrollUp);
        app.refresh_log(8);
        assert_eq!(app.view.log.len(), 9);
        assert_eq!(app.log_stamp.as_ref().unwrap().count, 9);
        let cached = app.view.log.clone();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, payload.replace("main.rs", "test.rs").repeat(20)).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        app.refresh_log(8);
        assert_eq!(app.view.log, cached);
        for _ in 0..30 {
            scroll_log(&mut app, ScrollUp);
        }
        app.refresh_log(8);
        assert_eq!(app.view.log.len(), 20);
        assert_eq!(app.view.log_scroll(), 12);
        key(&mut app, KeyCode::Char('*'));
        assert_eq!(app.view.log_scroll(), 0);
        assert!(app.view.log.iter().any(|event| event.kind != Kind::Tool));
        key(&mut app, KeyCode::Char('t'));
        assert_eq!(app.view.log.len(), 8);
        assert!(
            app.view
                .log
                .iter()
                .all(|event| event.path.as_deref() == Some("src/test.rs"))
        );
    }
}
