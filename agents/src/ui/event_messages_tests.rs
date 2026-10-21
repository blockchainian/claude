use super::*;
use crate::{
    event::{EventFilter, Kind},
    providers::Provider,
};

fn fixture() -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().into());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    let mut events =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    let tools = dir.path().join("tools.jsonl");
    std::fs::write(
        &tools,
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    events.extend(
        crate::providers::claude::event_reader::tail_session(&tools, usize::MAX, true).unwrap(),
    );
    let queued = dir.path().join("queued.jsonl");
    std::fs::write(
        &queued,
        include_str!("../../tests/fixtures/claude-queued-dequeue.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    events.extend(
        crate::providers::claude::event_reader::tail_session(&queued, usize::MAX, true).unwrap(),
    );
    view.set_event(events);
    view.set_active_panel(Panel::EventList);
    (dir, view)
}

fn space(view: &mut View) {
    let id = view.selected_id().unwrap().clone();
    let action = view.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    if view.active_panel == Panel::EventList {
        assert_eq!(
            action,
            Some(Action::PersistFiltering {
                id,
                filter: view.event_list.filter,
            })
        );
    } else {
        assert_eq!(action, None);
    }
}

fn tool(kind: &Kind) -> bool {
    matches!(kind, Kind::Tool | Kind::Error | Kind::Denied)
}

fn message(kind: &Kind) -> bool {
    match kind {
        Kind::Assistant | Kind::Compaction | Kind::QueuedSystem | Kind::Agent | Kind::Session => {
            true
        }
        Kind::User | Kind::QueuedUser | Kind::Tool | Kind::Error | Kind::Denied => false,
    }
}

#[test]
fn space_cycles_all_messages_tools_and_keeps_pins() {
    let (_dir, mut view) = fixture();
    let template = view.event_panel[0].clone();
    view.set_event(
        [
            Kind::User,
            Kind::QueuedUser,
            Kind::Assistant,
            Kind::Compaction,
            Kind::QueuedSystem,
            Kind::Agent,
            Kind::Session,
            Kind::Tool,
            Kind::Error,
            Kind::Denied,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| {
            let mut event = template.clone();
            event.kind = kind;
            event.source = Some(format!("kind-{i}"));
            event
        })
        .collect(),
    );
    assert_eq!(view.event_list.filter, EventFilter::All);
    let all: Vec<_> = view.shown_events().map(|(i, _)| i).collect();
    assert_eq!(all.len(), view.event_panel.len());
    let event = view
        .event_panel
        .iter()
        .find(|e| e.kind == Kind::Tool)
        .unwrap()
        .clone();
    view.event_list.pinned.push(PinnedRow {
        pin: crate::state::PinnedEvent {
            source: event.source.clone().unwrap(),
            records: event.source_records.clone(),
        },
        event,
    });
    let event = view
        .event_panel
        .iter()
        .find(|e| e.kind == Kind::User)
        .unwrap()
        .clone();
    view.event_list.pinned.push(PinnedRow {
        pin: crate::state::PinnedEvent {
            source: event.source.clone().unwrap(),
            records: event.source_records.clone(),
        },
        event,
    });
    let expected: Vec<_> = view
        .event_panel
        .iter()
        .enumerate()
        .filter(|(_, e)| message(&e.kind))
        .map(|(i, _)| i)
        .collect();
    for kind in [
        Kind::Assistant,
        Kind::Compaction,
        Kind::QueuedSystem,
        Kind::Agent,
        Kind::Session,
    ] {
        assert!(expected.iter().any(|&i| view.event_panel[i].kind == kind));
    }
    assert_eq!(expected.len(), 5);
    space(&mut view);
    assert_eq!(view.event_list.filter, EventFilter::Message);
    assert_eq!(
        view.shown_events().map(|(i, _)| i).collect::<Vec<_>>(),
        expected
    );
    assert!(view.shown_row_position(EventRow::Pinned(0)).is_some());
    assert!(view.shown_row_position(EventRow::Pinned(1)).is_some());
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
    space(&mut view);
    assert_eq!(view.event_list.filter, EventFilter::Tool);
    let tools: Vec<_> = view
        .event_panel
        .iter()
        .enumerate()
        .filter(|(_, e)| tool(&e.kind))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        view.shown_events().map(|(i, _)| i).collect::<Vec<_>>(),
        tools
    );
    assert!(view.shown_row_position(EventRow::Pinned(0)).is_some());
    assert!(view.shown_row_position(EventRow::Pinned(1)).is_some());
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    space(&mut view);
    assert_eq!(view.event_list.filter, EventFilter::All);
    assert_eq!(view.shown_events().map(|(i, _)| i).collect::<Vec<_>>(), all);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
}

#[test]
fn space_preserves_message_selection_and_rematches_hidden_rows_in_both_sorts() {
    for sort in [EventSort::Time, EventSort::Duration] {
        let (_dir, mut view) = fixture();
        view.set_event_sort(sort);
        let full = view
            .event_panel
            .iter()
            .position(|e| message(&e.kind))
            .unwrap();
        view.select_event(view.shown_row_position(EventRow::Timeline(full)));
        space(&mut view);
        assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(full)));
        space(&mut view);
        assert!(view.shown_events().all(|(_, e)| tool(&e.kind)));
        space(&mut view);
        view.select_event(view.shown_row_position(EventRow::Timeline(full)));
        assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(full)));
        let hidden = view
            .event_panel
            .iter()
            .position(|e| !message(&e.kind))
            .unwrap();
        view.select_event(view.shown_row_position(EventRow::Timeline(hidden)));
        let expected = if sort == EventSort::Time {
            view.event_panel
                .iter()
                .enumerate()
                .skip(hidden)
                .find(|(_, e)| message(&e.kind))
                .map(|(i, _)| EventRow::Timeline(i))
                .unwrap()
        } else {
            view.shown_events()
                .filter(|(_, e)| message(&e.kind))
                .last()
                .map(|(i, _)| EventRow::Timeline(i))
                .unwrap()
        };
        space(&mut view);
        assert_eq!(view.selected_event_row(), Some(expected));
    }
}

#[test]
fn space_combines_with_search_without_changing_event_matches() {
    let (_dir, mut view) = fixture();
    view.prepare_event_search();
    view.event_list.search.query.set_text("Tool");
    view.rematch_event();
    let matches: Vec<_> = (0..view.event_panel.len())
        .map(|i| view.event_matches(i))
        .collect();
    assert!(view.shown_events().any(|(_, e)| tool(&e.kind)));
    space(&mut view);
    assert!(view.shown_events().all(|(_, e)| message(&e.kind)));
    assert_eq!(
        (0..view.event_panel.len())
            .map(|i| view.event_matches(i))
            .collect::<Vec<_>>(),
        matches
    );
    view.event_list.search.query.set_text("User");
    view.rematch_event();
    assert!(view.shown_events().next().is_none());
    assert!(
        view.shown_events()
            .all(|(i, e)| message(&e.kind) && view.event_matches(i))
    );
}

#[test]
fn space_keeps_following_at_the_bottom_and_clears_pending_counts() {
    let (_dir, mut view) = fixture();
    view.viewport.borrow_mut().event_panel = Rect::new(0, 0, 100, 4);
    view.show_newest_event();
    for _ in 0..3 {
        view.event_list.pending_older = 10;
        view.event_list.unseen.set(3);
        space(&mut view);
        assert!(view.following_event());
        assert_eq!(view.event_scroll(), view.event_max_scroll());
        assert_eq!(view.event_list.pending_older, 0);
        assert_eq!(view.event_list.unseen.get(), 0);
    }
}

#[test]
fn space_while_searching_counts_and_navigates_only_current_mode_matches() {
    let (_dir, mut view) = fixture();
    view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for editing in [true, false] {
        view.event_list.search.editing = editing;
        for query in ["", "Tool", "User", "e"] {
            view.event_list.search.query.set_text(query);
            view.rematch_event();
            let all: Vec<_> = view
                .shown_events()
                .map(|(i, _)| EventRow::Timeline(i))
                .collect();
            let messages: Vec<_> = view
                .shown_events()
                .filter(|(_, e)| message(&e.kind))
                .map(|(i, _)| EventRow::Timeline(i))
                .collect();
            let tools: Vec<_> = view
                .shown_events()
                .filter(|(_, e)| tool(&e.kind))
                .map(|(i, _)| EventRow::Timeline(i))
                .collect();
            for expected in [&messages, &tools, &all] {
                space(&mut view);
                assert_eq!(view.event_list.search.query.text(), query);
                assert_eq!(view.event_list.search.editing, editing);
                assert_eq!(view.matching_event_count(), expected.len(), "{query}");
                assert_eq!(
                    view.shown_event_rows()
                        .map(|(row, _)| row)
                        .collect::<Vec<_>>(),
                    *expected
                );
                view.navigate_event(KeyCode::Home);
                for row in expected {
                    assert_eq!(view.selected_event_row(), Some(*row), "{query}");
                    view.navigate_event(KeyCode::Down);
                }
                if expected.is_empty() {
                    assert!(view.selected_event_row().is_none());
                }
            }
        }
    }
}

#[test]
fn space_remaps_background_return_and_keeps_its_details() {
    for hidden in [false, true] {
        let (_dir, mut view) = fixture();
        let full = view
            .event_panel
            .iter()
            .position(|e| message(&e.kind) != hidden)
            .unwrap();
        view.select_event(view.shown_row_position(EventRow::Timeline(full)));
        view.event_list.background = Some(view.event_panel[full].clone());
        view.event_list.background_return = Some(BackgroundReturn {
            event: view.event_list.cursor,
            scroll: 7,
            search: Default::default(),
        });
        space(&mut view);
        let saved = view.event_list.background_return.as_ref().unwrap();
        assert_eq!(
            saved.event,
            view.shown_row_position(EventRow::Timeline(full))
        );
        assert_eq!(saved.scroll, 7);
        assert!(view.event_list.background.is_some());
    }
}

#[test]
fn space_keeps_event_detail_space_behavior() {
    let (_dir, mut view) = fixture();
    view.set_active_panel(Panel::EventDetail);
    space(&mut view);
    assert_eq!(view.event_list.filter, EventFilter::All);
    assert_eq!(view.event_detail.mode, task_ui::EventDetailMode::Transcript);
}

#[test]
fn filters_restore_when_reopening_a_task() {
    for filter in [EventFilter::Message, EventFilter::Tool] {
        let (dir, view) = fixture();
        let id = view.selected_id().unwrap().clone();
        let mut preferences = crate::state::Preferences::default();
        preferences
            .save_task_filter(dir.path(), &id, filter)
            .unwrap();
        preferences
            .save_task_sort(dir.path(), &id, EventSort::Duration)
            .unwrap();
        let loaded = crate::state::Preferences::load(dir.path());
        let (_reopened_dir, mut reopened) = fixture();
        reopened.initialize_event_list(loaded.task_sort(&id), loaded.task_filter(&id));
        assert_eq!(reopened.event_list.filter, filter);
        assert_eq!(reopened.event_list.sort, EventSort::Duration);
        assert!(
            reopened
                .shown_events()
                .all(|(_, e)| message(&e.kind) == (filter == EventFilter::Message))
        );
        space(&mut reopened);
        reopened.initialize_event_list(loaded.task_sort(&id), loaded.task_filter(&id));
        assert_eq!(
            reopened.event_list.filter,
            filter.next(),
            "refresh must not overwrite a toggle"
        );
        assert!(
            reopened
                .shown_events()
                .all(|(_, e)| filter.next().includes(&e.kind))
        );
    }
}

#[test]
fn filtered_event_border_labels_have_layout_coordinates() {
    for width in [100, 149, 150, 200] {
        let (_dir, mut view) = fixture();
        let area = Rect::new(0, 0, width, 40);
        let mut rows = String::new();
        for (filter, title) in [
            (EventFilter::All, ""),
            (EventFilter::Message, "Assistant"),
            (EventFilter::Tool, "Tool"),
        ] {
            view.event_list.filter = filter;
            let mut buf = Buffer::empty(area);
            view.render(area, &mut buf);
            let viewport = view.viewport.borrow();
            let border = viewport.event_borders[1];
            assert_eq!(border.y, viewport.event_panel.bottom());
            let y = border.y;
            let row: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
            let top: String = (border.x..border.right())
                .map(|x| buf[(x, viewport.event_borders[0].y)].symbol())
                .collect();
            assert!(!top.contains("Assistant") && !top.contains("Tool"), "{top}");
            if title.is_empty() {
                assert!(!row.contains("Assistant") && !row.contains("Tool"), "{row}");
            } else {
                let length = title.len() as u16;
                let label = Rect::new(border.x + (border.width - length - 2) / 2, y, length + 2, 1);
                let blocked = label.intersects(viewport.event_label)
                    || viewport
                        .field_targets
                        .iter()
                        .any(|(target, _)| label.intersects(*target));
                if blocked {
                    assert!(!row.contains(title), "width {width}: {row}");
                } else {
                    let actual: String = (label.x..label.right())
                        .map(|x| buf[(x, y)].symbol())
                        .collect();
                    assert_eq!(actual, format!(" {title} "), "width {width}: {row}");
                    let status_y = view.center_layout(area).header.y + 1;
                    let status_row: String =
                        (0..width).map(|x| buf[(x, status_y)].symbol()).collect();
                    let status = View::status(view.selected_row().unwrap()).0;
                    let status_x = status_row[..status_row.find(status).unwrap()].width() as u16;
                    for column in label.x + 1..label.right() - 1 {
                        assert_eq!(buf[(column, y)].style(), buf[(status_x, status_y)].style());
                    }
                    assert_eq!(buf[(label.x - 1, y)].symbol(), "─");
                    assert_eq!(buf[(label.right(), y)].symbol(), "─");
                }
            }
            rows.push_str(&row);
            rows.push('\n');
        }
        if [100, 200].contains(&width) {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "tests/ui_snapshots/event_filter_labels_{width}.txt"
            ));
            if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
                std::fs::write(&path, &rows).unwrap();
            }
            assert_eq!(rows, std::fs::read_to_string(path).unwrap());
        }
    }
}

#[test]
fn filtered_event_labels_preserve_duration_and_model_labels() {
    for width in [24, 44, 80, 100, 149, 150, 200] {
        let (_dir, mut view) = fixture();
        view.event_list.sort = EventSort::Duration;
        view.event_list.restoring = false;
        for filter in [EventFilter::Message, EventFilter::Tool] {
            view.event_list.filter = EventFilter::All;
            let area = Rect::new(0, 0, width, 40);
            let mut before = Buffer::empty(area);
            view.render(area, &mut before);
            view.event_list.filter = filter;
            let mut after = Buffer::empty(area);
            view.render(area, &mut after);
            let viewport = view.viewport.borrow();
            for target in std::iter::once(viewport.event_label)
                .chain(viewport.field_targets.iter().map(|(target, _)| *target))
            {
                for y in target.y..target.bottom() {
                    for x in target.x..target.right() {
                        assert_eq!(after[(x, y)], before[(x, y)], "width {width}: {target:?}");
                    }
                }
            }
            let border = viewport.event_borders[1];
            let title = if filter == EventFilter::Message {
                "Assistant"
            } else {
                "Tool"
            };
            let label = Rect::new(
                border.x + border.width.saturating_sub(title.len() as u16 + 2) / 2,
                border.y,
                title.len() as u16 + 2,
                1,
            );
            let row: String = (border.x..border.right())
                .map(|x| after[(x, border.y)].symbol())
                .collect();
            if label.intersects(viewport.event_label) {
                assert!(!row.contains(title), "{row}");
            }
        }
    }
}
