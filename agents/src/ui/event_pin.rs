//! Pinning logs to the top of a task's event list.

use super::task_ui::PinnedRow;
use super::*;
use crate::event::{Event, Kind, SourceRecord};
use crate::state::PinnedEvent;

/// Whether two record lists share a transcript line.
fn overlaps(left: &[SourceRecord], right: &[SourceRecord]) -> bool {
    left.iter().any(|record| right.contains(record))
}

impl View {
    /// Sets the pinned events of a task, whether it is shown or parked.
    pub fn set_pinned_events(&mut self, key: &SessionKey, rows: Vec<PinnedRow>) {
        let list = self.task_event_list(key);
        if list.pinned_cursor.is_some_and(|index| index >= rows.len()) {
            list.pinned_cursor = None;
        }
        list.pinned = rows;
    }

    /// Removes every pinned event of a task, whether it is shown or parked.
    pub fn clear_pinned_events(&mut self, key: &SessionKey) {
        let was_selected = self.task_event_list(key).pinned_cursor.is_some();
        self.task_event_list(key).pinned.clear();
        if was_selected {
            self.task_event_list(key).pinned_cursor = None;
            if self.shows_task(key) {
                self.select_event(None);
            }
        }
    }

    pub fn has_pinned_events(&self, key: &SessionKey) -> bool {
        if self.shows_task(key) {
            return !self.event_list.pinned.is_empty();
        }
        self.task_uis
            .get(&draft_controls::TaskKey::Session(key.clone()))
            .is_some_and(|ui| !ui.event_list.pinned.is_empty())
    }

    fn shows_task(&self, key: &SessionKey) -> bool {
        self.panels_task.as_ref().and_then(|task| task.session()) == Some(key)
    }

    fn task_event_list(&mut self, key: &SessionKey) -> &mut task_ui::EventList {
        if self.shows_task(key) {
            return &mut self.event_list;
        }
        &mut self
            .task_uis
            .entry(draft_controls::TaskKey::Session(key.clone()))
            .or_default()
            .event_list
    }

    /// Pins the selected event, or unpins every pinned event sharing a line with it.
    pub(super) fn toggle_event_pin(&mut self) -> Option<Action> {
        let task = self.selected_id()?.clone();
        let unsupported = || Some(Action::Unsupported("Pin"));
        if self
            .selected_row()
            .is_none_or(|row| row.status == SessionStatus::Inactive)
            || self.event_list.background.is_some()
        {
            return unsupported();
        }
        let event = self.visible_event()?;
        if !pinnable(event) {
            return unsupported();
        }
        let records = event.source_records.clone();
        let id = task_ui::EventId::from_event(event);
        let mut snapshot = event.clone();
        snapshot.turn = None;
        snapshot.previous_end = None;
        let from_pinned = self.event_list.pinned_cursor;
        let pinned = &mut self.event_list.pinned;
        if pinned.iter().any(|row| overlaps(&row.pin.records, &records)) {
            pinned.retain(|row| !overlaps(&row.pin.records, &records));
            if let Some(index) = from_pinned {
                self.leave_unpinned_event(index, id);
            }
        } else {
            let source = snapshot.source.clone().expect("pinnable event has a source");
            pinned.insert(
                0,
                PinnedRow {
                    pin: PinnedEvent { source, records },
                    event: snapshot,
                },
            );
        }
        Some(Action::ToggleEventPin {
            task,
            pinned: self
                .event_list
                .pinned
                .iter()
                .map(|row| row.pin.clone())
                .collect(),
        })
    }

    /// Moves the cursor off a pinned row that was just unpinned: to the nearest pinned row, or to the
    /// same log in the timeline once none is left.
    fn leave_unpinned_event(&mut self, index: usize, id: Option<task_ui::EventId>) {
        let shown: Vec<_> = self.shown_pinned().map(|(index, _)| index).collect();
        let next = shown
            .iter()
            .copied()
            .find(|&shown| shown >= index)
            .or_else(|| shown.last().copied());
        if let Some(next) = next {
            self.event_list.pinned_cursor = None;
            self.select_pinned_event(next);
            return;
        }
        let same = id.and_then(|id| self.event_position(&id));
        self.select_event(same);
        self.reveal_event_selection();
    }
}

/// Queued rows are replaced by another row once delivered, and rows without an identity or a
/// transcript position cannot be found again.
fn pinnable(event: &Event) -> bool {
    !matches!(event.kind, Kind::QueuedUser | Kind::QueuedSystem)
        && event.source.as_deref().is_some_and(|source| !source.is_empty())
        && !event.source_records.is_empty()
}
