use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, App) {
    let (dir, mut app) = super::tests::fixture();
    let path = dir.path().join("rollout.jsonl");
    // Captured item_completed records at ordinals 70, 80 and 95.
    std::fs::write(
        &path,
        include_bytes!("../tests/fixtures/codex-cached-history.jsonl"),
    )
    .unwrap();
    app.sessions[0].id.provider = ProviderId::Codex;
    app.sessions[0].transcript = Some(path);
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event(2);
    super::event_search_tests::finish_search(&mut app);
    assert_eq!(app.view.event_panel.len(), 2);
    assert!(
        app.view
            .event_panel
            .iter()
            .all(|event| !event.body.contains("model-service"))
    );
    (dir, app)
}

#[test]
fn codex_navigation_reveals_older_events_already_decoded() {
    let (_dir, mut app) = fixture();
    let parsed = app.transcript_parsed_bytes;
    super::tests::click_event(&mut app);
    for code in [KeyCode::Home, KeyCode::Up] {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    assert!(app.view.event_page_pending());
    app.refresh_event(2);
    super::event_search_tests::finish_search(&mut app);
    assert!(
        app.view.event_panel.len() > 2,
        "cached rows must remain pageable"
    );
    assert_eq!(app.transcript_parsed_bytes, parsed, "reuse decoded records");
    assert!(
        app.view
            .event_panel
            .iter()
            .any(|event| event.body.contains("model-service"))
    );
}

#[test]
fn codex_search_includes_older_events_already_decoded() {
    let (_dir, mut app) = fixture();
    let parsed = app.transcript_parsed_bytes;
    super::tests::click_event(&mut app);
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    )));
    app.refresh_event(2);
    super::event_search_tests::finish_search(&mut app);
    for c in "model-service".chars() {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    assert!(app.view.transcript_fully_loaded());
    assert_eq!(
        app.view.matching_event_count(),
        1,
        "search must include the early FileChange"
    );
    assert_eq!(app.transcript_parsed_bytes, parsed, "reuse decoded records");
}

#[test]
fn codex_reader_distinguishes_decoded_history_from_shown_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        include_bytes!("../tests/fixtures/codex-cached-history.jsonl"),
    )
    .unwrap();
    let session = {
        let (_fixture_dir, app) = fixture();
        let mut session = app.sessions[0].clone();
        session.transcript = Some(path);
        session
    };
    let mut reader = crate::providers::event_reader(&session).unwrap();
    let tail = reader.refresh(2).unwrap();
    assert_eq!(tail.oldest_offset, 0, "all source records were decoded");
    assert!(
        tail.has_older,
        "one decoded event is outside the shown window"
    );
    let idle = reader.refresh(2).unwrap();
    assert!(idle.has_older, "an unchanged read must preserve pagination");
    assert_eq!(idle.parsed_bytes, 0);
    let full = reader.refresh(usize::MAX).unwrap();
    assert_eq!(full.events.len(), 3);
    assert!(!full.has_older);
    assert_eq!(full.parsed_bytes, 0);
}

#[test]
fn c2_pins_do_not_block_loading_older_timeline_events() {
    let (_dir, mut app) = fixture();
    super::tests::click_event(&mut app);
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::NONE,
    )));
    assert!(app.view.has_pinned_events(&app.sessions[0].id));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));

    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert!(
        app.view.event_page_pending(),
        "timeline navigation must request unloaded history"
    );
    assert_eq!(app.view.event_list.cursor, Some(1));
    app.refresh_event(2);
    super::event_search_tests::finish_search(&mut app);
    assert!(!app.transcript_has_older);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert!(!app.view.event_page_pending());
    assert_eq!(app.view.event_list.cursor, Some(1));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('`'),
        KeyModifiers::NONE,
    )));
    assert_eq!(app.view.event_list.cursor, Some(0));
}
