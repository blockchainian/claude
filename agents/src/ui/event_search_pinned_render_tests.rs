use super::*;
use crate::providers::Provider;

#[test]
fn pinned_rows_render_before_timeline_and_keep_their_order() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    ));
    view.event_session = view.selected_id().cloned();
    let events =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    view.set_event(events.clone());
    for event in [events.last().unwrap(), &events[0]] {
        let mut event = event.clone();
        event.turn = None;
        event.previous_end = None;
        view.event_list
            .pinned
            .push(super::super::task_ui::PinnedRow {
                pin: crate::state::PinnedEvent {
                    source: event.source.clone().unwrap_or_default(),
                    records: event.source_records.clone(),
                },
                event,
            });
    }
    let pinned_bodies: Vec<_> = view
        .event_list
        .pinned
        .iter()
        .map(|row| row.event.body.clone())
        .collect();
    for sort in [EventSort::Time, EventSort::Duration] {
        view.event_list.sort = sort;
        view.prepare_event_search();
        assert_eq!(
            view.shown_event_rows()
                .take(2)
                .map(|(_, event)| event.body.clone())
                .collect::<Vec<_>>(),
            pinned_bodies
        );
    }
    view.event_list.sort = EventSort::Time;
    view.prepare_event_search();
    view.event_list.cursor = None;
    view.event_list.scroll.set(0);
    let area = Rect::new(0, 0, 60, 8);
    view.viewport.borrow_mut().event_panel = area;
    let mut buffer = Buffer::empty(area);
    view.render_event(area, &mut buffer, events[0].started_at);
    assert_eq!(
        view.viewport.borrow().event_rows[0],
        super::super::task_ui::EventRow::Pinned(0)
    );
    assert_eq!(
        view.viewport.borrow().event_rows[2],
        super::super::task_ui::EventRow::Timeline(0)
    );
    let snapshot = buffer
        .content
        .chunks(60)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let snapshot_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/task_event_pinned.txt");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&snapshot_path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(snapshot_path).unwrap());
}
