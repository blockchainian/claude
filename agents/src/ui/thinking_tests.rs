use super::*;

fn thinking_fixture(body: &str) -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = event_fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-thinking.jsonl");
    let mut event = crate::providers::claude::event_reader::tail_session(&path, 10, true)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == crate::event::Kind::Thinking)
        .expect("real thinking event");
    event.kind = crate::event::Kind::Thinking;
    event.body = body.into();
    event.started_at = now - Duration::seconds(3);
    event.previous_end = Some(event.started_at - Duration::seconds(232));
    event.ended_at = Some(event.started_at);
    view.rows[0].activity = Some(event.activity());
    view.set_event(vec![event.clone()]);
    view.select_fixture_event(Some(event));
    (view, now)
}

fn snapshot(name: &str, rendered: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots")
        .join(name);
    if std::env::var_os("UPDATE_THINKING_SNAPSHOTS").is_some() {
        std::fs::write(&path, rendered).unwrap();
    }
    assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
}

#[test]
fn thinking_list_detail_and_last_activity_snapshot() {
    let (view, now) = thinking_fixture("(encrypted)");
    let rendered = event_screen(&view, 120, 36, now);
    assert!(rendered.contains("Thinking"), "{rendered}");
    assert_eq!(rendered.matches("(encrypted)").count(), 3, "{rendered}");
    assert!(rendered.contains("3m 52s"), "{rendered}");
    assert!(rendered.contains("Last activity"), "{rendered}");
    snapshot("thinking_encrypted.txt", &rendered);
}

#[test]
fn thinking_prose_renders_in_all_three_panels() {
    let (view, now) = thinking_fixture("Consider **both** options.");
    let rendered = event_screen(&view, 120, 36, now);
    assert_eq!(
        rendered.matches("Consider both options.").count(),
        3,
        "{rendered}"
    );
    assert!(!rendered.contains("**both**"), "{rendered}");
    assert_eq!(
        super::super::style::event_styles(&crate::event::Kind::Thinking),
        super::super::style::event_styles(&crate::event::Kind::Assistant)
    );
    snapshot("thinking_prose.txt", &rendered);
}

#[test]
fn thinking_duration_sort_keeps_completed_thinking() {
    let (mut view, now) = thinking_fixture("(encrypted)");
    assert_eq!(
        super::super::event_list::sort_duration(&view.event_panel[0], &view.rows[0], now, true),
        Some(Duration::seconds(232))
    );
    view.set_event_sort(super::super::EventSort::Duration);
    assert_eq!(view.shown_events().count(), 1);
    assert!(event_screen(&view, 120, 36, now).contains("3m 52s"));
}

#[test]
fn thinking_keeps_tool_row_duration_after_thinking() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-thinking.jsonl");
    let events = crate::providers::claude::event_reader::tail_session(&path, 10, true).unwrap();
    let thinking = events
        .iter()
        .find(|event| event.kind == crate::event::Kind::Thinking)
        .unwrap();
    let mut tool = events
        .iter()
        .find(|event| event.kind == crate::event::Kind::Tool)
        .unwrap()
        .clone();
    tool.ended_at = Some(tool.started_at + Duration::seconds(1));
    assert_eq!(tool.previous_end, Some(thinking.started_at));
    let (view, now) = event_fixture();
    let elapsed = super::super::event_list::sort_duration(&tool, &view.rows[0], now, true).unwrap();
    assert_eq!(elapsed, tool.ended_at.unwrap() - thinking.started_at);
    assert!(elapsed >= Duration::seconds(1));
    assert!(elapsed < Duration::seconds(2), "{elapsed}");
}
