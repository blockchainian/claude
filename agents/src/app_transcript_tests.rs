use super::*;

#[test]
fn first_transcript_draw_queues_loading_without_parsing_events() {
    let (_dir, mut app) = super::tests::fixture();
    app.transcript = None;
    app.transcript_stamp = None;
    app.full_transcript_stamp = None;
    app.transcript_parsed_bytes = 0;
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    assert_eq!(app.transcript_parsed_bytes, 0, "draw parsed the transcript");
    assert!(app.pending_transcript.is_some());
    super::event_search_tests::finish_search(&mut app);
    assert!(app.transcript_parsed_bytes > 0);
    assert!(!app.view.event_panel.is_empty());
}

#[test]
fn unchanged_activity_draw_does_not_stat_sessions_again() {
    let (_dir, mut app) = super::tests::fixture();
    let now = chrono::Utc::now();
    app.refresh_activity(120, now);
    assert!(app.activity.stats > 0);
    let stats = app.activity.stats;
    app.refresh_activity(120, now);
    assert_eq!(
        app.activity.stats, stats,
        "unchanged draw stats every session"
    );
}

#[test]
fn activity_window_growth_recovers_transcripts_outside_the_previous_window() {
    let (_dir, app) = super::tests::fixture();
    let mut sessions = app.sessions.clone();
    let records: Vec<serde_json::Value> =
        include_str!("../tests/fixtures/turn-groups/claude-boundaries.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|record| {
                record["sessionId"] == "d9cac3b7-7847-4953-b998-03aeb47c7d98"
                    && record["type"] == "user"
                    && record["isMeta"] != true
            })
            .take(2)
            .collect();
    assert_eq!(records.len(), 2);
    let at = chrono::DateTime::parse_from_rfc3339(records[0]["timestamp"].as_str().unwrap())
        .unwrap()
        .to_utc();
    let ended = chrono::DateTime::parse_from_rfc3339(records[1]["timestamp"].as_str().unwrap())
        .unwrap()
        .to_utc();
    let path = sessions[0].transcript.clone().unwrap();
    std::fs::write(
        &path,
        records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>(),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(ended.into())
        .unwrap();
    sessions[0].status = SessionStatus::Ready;
    let mut activity = ActivityCache::default();
    let now = ended + chrono::Duration::seconds(120);
    assert!(activity.update(&sessions, 1, now).unwrap().is_empty());
    let bins = activity.update(&sessions, 30, now).unwrap();
    assert!(bins.contains(&crate::usage::bin_of(at)));
}

#[test]
fn activity_clock_advances_without_stat_and_session_changes_invalidate() {
    let (_dir, app) = super::tests::fixture();
    let mut sessions = app.sessions.clone();
    let path = sessions[0].transcript.clone().unwrap();
    let record: serde_json::Value = serde_json::from_str(
        include_str!("../tests/fixtures/transcript.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap())
        .unwrap()
        .to_utc();
    std::fs::write(&path, format!("{record}\n")).unwrap();
    sessions[0].status = SessionStatus::Working;
    let mut activity = ActivityCache::default();
    let first = activity.update(&sessions, 120, at).unwrap();
    let stats = activity.stats;
    let later = at + chrono::Duration::seconds(10);
    let bins = activity.update(&sessions, 120, later).unwrap();
    assert!(bins.len() > first.len());
    assert!(bins.contains(&crate::usage::bin_of(later)));
    assert_eq!(activity.stats, stats);
    sessions[0].status = SessionStatus::Ready;
    assert!(activity.update(&sessions, 120, later).unwrap().is_empty());
    assert!(activity.stats > stats);
}

#[test]
fn usage_reads_only_when_the_selected_file_changes() {
    use std::io::Write;
    let (_dir, mut app) = super::tests::fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    app.refresh_usage();
    assert_eq!(app.usage_reads, 1);
    let bins = app.usage[&path].bins.clone();
    app.refresh_usage();
    assert_eq!(app.usage_reads, 1);
    assert_eq!(app.usage[&path].bins, bins);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        "{}",
        include_str!("../tests/fixtures/transcript.jsonl")
            .lines()
            .next()
            .unwrap()
    )
    .unwrap();
    app.refresh_usage();
    assert_eq!(app.usage_reads, 2);
    app.refresh_usage();
    assert_eq!(app.usage_reads, 2);
}

#[test]
fn batched_task_round_trip_restores_the_loaded_rows_without_reloading() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (_dir, mut app) = super::tests::fixture();
    let first = app.sessions[0].id.clone();
    let mut other = app.sessions[0].clone();
    other.id.id = "round-trip".into();
    other.updated_at -= chrono::Duration::seconds(1);
    app.sessions.push(other);
    app.view.set_sessions(app.sessions.clone(), false);
    super::tests::click_event(&mut app);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    let expected = app.view.event_panel.clone();
    let selected = app.view.event_panel[app.view.event_list.cursor.unwrap()]
        .source
        .clone();
    for _ in 0..2 {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )));
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        )));
    }
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    assert_ne!(app.view.selected_id(), Some(&first));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(app.view.selected_id(), Some(&first));
    let parsed = app.transcript_parsed_bytes;
    app.refresh_event(8);
    assert_eq!(app.view.event_panel, expected);
    assert_eq!(
        app.view.event_panel[app.view.event_list.cursor.unwrap()].source,
        selected
    );
    assert_eq!(app.transcript_parsed_bytes, parsed);
}
