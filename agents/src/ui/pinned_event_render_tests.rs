use super::*;
use crate::event::{Event, Kind};
use crate::providers::Provider;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::state::PinnedEvent;

fn fixture() -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    let events = crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true)
        .unwrap();
    let template = events
        .iter()
        .find(|event| event.kind == Kind::Assistant)
        .unwrap();
    view.set_event(
        (0..40)
            .map(|index| {
                let mut event = template.clone();
                event.source = Some(format!("entry-source-{index}"));
                event.source_records[0].offset = index as u64;
                event.body = format!("entry number {index}");
                event.paths.clear();
                event
            })
            .collect(),
    );
    (dir, view)
}

fn pinned_row(event: &Event) -> PinnedRow {
    PinnedRow {
        pin: PinnedEvent {
            source: event.source.clone().unwrap_or_default(),
            records: event.source_records.clone(),
        },
        event: event.clone(),
    }
}

fn pin(view: &mut View, indices: &[usize]) {
    let rows = indices
        .iter()
        .map(|&index| pinned_row(&view.event_panel[index]))
        .collect();
    view.event_list.pinned = rows;
}

fn draw(view: &View, height: u16) -> (Vec<String>, Buffer) {
    let area = Rect::new(0, 0, 100, height);
    let mut buffer = Buffer::empty(area);
    view.render_event(area, &mut buffer, Utc::now());
    let rows = (0..height)
        .map(|y| {
            (0..100)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    (rows, buffer)
}

fn first_words(event: &Event) -> String {
    event.body.clone()
}

#[test]
fn pinned_events_draw_above_the_timeline_in_pin_order() {
    let (_dir, mut view) = fixture();
    let (older, newer) = (1, view.event_panel.len() - 1);
    pin(&mut view, &[newer, older]);
    let (rows, _) = draw(&view, 30);
    assert!(rows[0].contains(&first_words(&view.event_panel[newer])), "{rows:#?}");
    assert!(rows[1].contains(&first_words(&view.event_panel[older])), "{rows:#?}");
    assert!(rows[2].starts_with("────"), "{rows:#?}");
    assert_eq!(view.pinned_height(), 3);
    let viewport = view.viewport.borrow();
    assert_eq!(viewport.event_pinned_rows, [0, 1]);
    assert_eq!(viewport.event_timeline_top, 3);
    assert_eq!(viewport.event_rows.len(), view.event_panel.len().min(27));
}

#[test]
fn a_pinned_event_still_shows_in_the_timeline() {
    let (_dir, mut view) = fixture();
    let newest = view.event_panel.len() - 1;
    pin(&mut view, &[newest]);
    let word = first_words(&view.event_panel[newest]);
    let (rows, _) = draw(&view, 30);
    let shown = rows.iter().filter(|row| row.contains(&word)).count();
    assert_eq!(shown, 2, "{rows:#?}");
    assert_eq!(view.viewport.borrow().event_rows.last(), Some(&newest));
}

#[test]
fn duration_sort_leaves_the_pinned_order_alone() {
    let (_dir, mut view) = fixture();
    pin(&mut view, &[3, 1]);
    view.event_list.sort = EventSort::Duration;
    view.prepare_event_search();
    let (rows, _) = draw(&view, 30);
    assert!(rows[0].contains(&first_words(&view.event_panel[3])), "{rows:#?}");
    assert!(rows[1].contains(&first_words(&view.event_panel[1])), "{rows:#?}");
}

#[test]
fn search_filters_the_pinned_events_on_their_own() {
    let (_dir, mut view) = fixture();
    pin(&mut view, &[6, 5]);
    view.prepare_event_search();
    view.event_list.search.query.set_text("number 5");
    view.rematch_event();
    let shown: Vec<_> = view.shown_pinned().map(|(index, _)| index).collect();
    assert_eq!(shown, [1]);
    // The timeline keeps its own matches: 5, 15, 25 and 35.
    assert_eq!(view.matching_event_count(), 4);
    let (rows, _) = draw(&view, 30);
    assert_eq!(view.pinned_height(), 2);
    assert!(rows[0].contains("entry number 5"), "{rows:#?}");
    assert!(!rows[0].contains("entry number 6"), "{rows:#?}");
    view.event_list.search.query.set_text("zzzzqqqq-no-match");
    view.rematch_event();
    assert_eq!(view.shown_pinned().count(), 0);
    let (rows, _) = draw(&view, 30);
    assert_eq!(view.pinned_height(), 0);
    assert!(!rows.iter().any(|row| row.starts_with("────")), "{rows:#?}");
}

#[test]
fn pinned_area_leaves_the_timeline_its_rows() {
    let (_dir, mut view) = fixture();
    let all: Vec<_> = (0..view.event_panel.len().min(10)).collect();
    pin(&mut view, &all);
    draw(&view, 9);
    // 9 rows: at most 4 pinned events and a rule, leaving 4 timeline rows.
    assert_eq!(view.pinned_height(), 5);
    assert_eq!(view.event_timeline_height(), 4);
    assert_eq!(view.viewport.borrow().event_pinned_rows, [0, 1, 2, 3]);
    assert_eq!(view.event_max_scroll(), view.matching_event_count().saturating_sub(4));
    draw(&view, 2);
    assert_eq!(view.pinned_height(), 0);
}

#[test]
fn the_cursor_row_in_the_pinned_area_stays_in_the_window() {
    let (_dir, mut view) = fixture();
    let all: Vec<_> = (0..view.event_panel.len().min(10)).collect();
    pin(&mut view, &all);
    view.event_list.cursor = None;
    view.event_list.pinned_cursor = Some(7);
    let (rows, buffer) = draw(&view, 9);
    assert_eq!(view.viewport.borrow().event_pinned_rows, [4, 5, 6, 7]);
    let cursor = super::style::cursor_style();
    assert_eq!(buffer[(0, 3)].style().bg, cursor.bg, "{rows:#?}");
    assert_ne!(buffer[(0, 2)].style().bg, cursor.bg);
}

#[test]
fn pinned_area_snapshot() {
    let (_dir, mut view) = fixture();
    let newest = view.event_panel.len() - 1;
    pin(&mut view, &[newest, 2]);
    let (rows, _) = draw(&view, 10);
    // The time column depends on the clock and the time zone, so it is cut off.
    let snapshot: String = rows
        .iter()
        .map(|row| format!("{}\n", row.chars().take(80).collect::<String>().trim_end()))
        .collect();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/pinned_events.txt");
    if std::env::var_os("UPDATE_PINNED_EVENT_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
