use super::*;

#[test]
fn pinned_search_uses_snapshot_body_without_timeline_indices() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let events =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    let event = events
        .into_iter()
        .find(|event| !event.body.is_empty())
        .unwrap();
    let mut view = View::new(Grouping::Project, false);
    view.event_list
        .pinned
        .push(super::super::task_ui::PinnedRow {
            pin: crate::state::PinnedEvent {
                source: event.source.clone().unwrap_or_default(),
                records: event.source_records.clone(),
            },
            event,
        });
    view.prepare_event_search();
    assert!(view.pinned_event_matches(0));
    view.event_list
        .search
        .input
        .set_text("unmatchable-pinned-search-query");
    view.rematch_event();
    assert!(!view.pinned_event_matches(0));
    assert!(view.event_list.search.matches.is_empty());
    let query = super::super::event_list::kind_label(&view.event_list.pinned[0].event.kind);
    view.event_list.search.input.set_text(&query);
    view.rematch_event();
    assert!(view.pinned_event_matches(0));
    assert!(view.event_list.search.matches.is_empty());
    view.event_list.search.input.close();
    assert!(view.pinned_event_matches(0));
}
