use super::*;

fn fixture() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = log_fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    view.log = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    (view, now)
}

#[test]
fn wide_layout_separates_event_list_and_detail() {
    let (view, now) = fixture();
    log_screen(&view, 150, 40, now);
    let layout = view.center_layout(Rect::new(0, 0, 150, 40));
    assert_eq!(layout.log.y, layout.list.y);
    assert_eq!(layout.log.width, 150 / 3);
    assert!(layout.log.x > layout.details.x);
    assert!(layout.log.bottom() <= layout.prompt.y);
}

fn assert_snapshot(name: &str, actual: String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots")
        .join(name);
    if std::env::var_os("UPDATE_WIDE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(path).unwrap());
}

#[test]
fn wide_layout_newest_snapshot() {
    let (view, now) = fixture();
    assert_snapshot("wide_layout_newest.txt", log_screen(&view, 150, 40, now));
}

#[test]
fn wide_layout_open_snapshot() {
    let (mut view, now) = fixture();
    let event = view
        .log
        .iter()
        .find(|event| event.kind == crate::log::Kind::Assistant)
        .unwrap()
        .clone();
    view.set_log_open(Some(event));
    assert_snapshot("wide_layout_open.txt", log_screen(&view, 180, 45, now));
    assert!(!view.viewport.borrow().log_rows.is_empty());
}

#[test]
fn wide_layout_preview_follows_until_event_opens() {
    let (mut view, now) = fixture();
    let events = view.log.clone();
    let previous = events[events.len() - 2].clone();
    let newest = events.last().unwrap().clone();
    view.set_log(events[..events.len() - 1].to_vec());
    log_screen(&view, 150, 40, now);
    assert_eq!(view.visible_log_event(), Some(&previous));
    view.set_log(events.clone());
    log_screen(&view, 150, 40, now);
    assert_eq!(view.visible_log_event(), Some(&newest));
    view.set_log_open(Some(previous.clone()));
    view.set_log(events);
    log_screen(&view, 150, 40, now);
    assert_eq!(view.visible_log_event(), Some(&previous));
    view.set_log_open(None);
    log_screen(&view, 150, 40, now);
    assert_eq!(view.visible_log_event(), Some(&newest));
    let narrow = view.center_layout(Rect::new(0, 0, 149, 40));
    assert!(narrow.log.y > narrow.list.y);
    assert!(narrow.log_detail.is_empty());
}

#[test]
fn wide_layout_list_can_open_another_event() {
    let (mut view, now) = fixture();
    view.set_log_open(Some(view.log[0].clone()));
    log_screen(&view, 150, 40, now);
    let log = view.viewport.borrow().log;
    let index = view.viewport.borrow().log_rows[0].0;
    let expected = view.log[index].clone();
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            log.x + 30,
            log.y + super::super::log::LOG_TOP,
        ),
    );
    view.flush_pending_click(Instant::now() + view.double_click_interval);
    assert_eq!(view.log_open.as_ref(), Some(&expected));
}

#[test]
fn wide_preview_keeps_newest_when_list_search_changes() {
    let (mut view, now) = fixture();
    log_screen(&view, 150, 40, now);
    let newest = view.log.last().unwrap().clone();
    view.prepare_log_search();
    view.log_search
        .query
        .set_text("no-such-real-transcript-event");
    view.log_search.editing = true;
    view.rematch_log();
    log_screen(&view, 150, 40, now);
    assert_eq!(view.visible_log_event(), Some(&newest));
}

#[test]
fn wide_log_double_click_focuses_input_without_pinning_preview() {
    let (mut view, now) = fixture();
    view.rows[view.selected].process = None;
    log_screen(&view, 150, 40, now);
    let log = view.viewport.borrow().log;
    let click = mouse(
        MouseEventKind::Down(MouseButton::Left),
        log.x + 30,
        log.y + super::super::log::LOG_TOP,
    );
    assert!(click_mouse(&mut view, click).is_none());
    assert!(click_mouse(&mut view, click).is_none());
    assert!(view.input_box.focused);
    assert!(view.log_open.is_none());
}
