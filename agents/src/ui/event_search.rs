use super::*;
use nucleo_matcher::{
    Config, Matcher, Utf32Str, Utf32String,
    pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern},
};

pub(crate) struct EventSearch {
    pub(super) query: line_edit::LineEdit,
    pub editing: bool,
    matches: Vec<usize>,
    bodies: Vec<Utf32String>,
    pinned_matches: Vec<usize>,
    matcher: RefCell<Matcher>,
}

impl Default for EventSearch {
    fn default() -> Self {
        Self {
            query: line_edit::LineEdit::default(),
            editing: false,
            matches: Vec::new(),
            bodies: Vec::new(),
            pinned_matches: Vec::new(),
            matcher: RefCell::new(Matcher::new(Config::DEFAULT)),
        }
    }
}

impl EventSearch {
    pub fn active(&self) -> bool {
        self.editing || !self.query.is_empty()
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.editing = false;
        self.matches.clear();
        self.pinned_matches.clear();
    }

    fn pattern(&self) -> Pattern {
        let mut pattern = Pattern::default();
        pattern.atoms = self
            .query
            .text()
            .split_whitespace()
            .map(|word| {
                Atom::new(
                    word,
                    CaseMatching::Smart,
                    Normalization::Smart,
                    AtomKind::Substring,
                    false,
                )
            })
            .collect();
        pattern
    }

    pub(super) fn indices(&self, text: &str) -> Vec<u32> {
        let mut indices = Vec::new();
        if !self.query.is_empty() {
            let mut buffer = Vec::new();
            self.pattern().indices(
                Utf32Str::new(text, &mut buffer),
                &mut self.matcher.borrow_mut(),
                &mut indices,
            );
            indices.sort_unstable();
            indices.dedup();
        }
        indices
    }
}

impl View {
    pub(crate) fn transcript_fully_loaded(&self) -> bool {
        self.event_list.search.active() || self.event_list.sort == EventSort::Duration
    }

    pub(crate) fn prepare_event_search(&mut self) {
        self.event_order = if self.event_list.sort == EventSort::Duration {
            let mut order: Vec<_> = (0..self.event_panel.len()).collect();
            if let Some(session) = self.selected_row() {
                order.sort_by_key(|&index| {
                    super::event_list::sort_duration(
                        &self.event_panel[index],
                        session,
                        self.last_update,
                        index + 1 == self.event_panel.len(),
                    )
                });
            }
            order
        } else {
            Vec::new()
        };
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.clone())
            .unwrap_or_default();
        self.event_list.search.bodies = self
            .event_panel
            .iter()
            .map(|event| search_body(event, &cwd))
            .collect();
        self.rematch_event();
    }

    pub(crate) fn rematch_event(&mut self) {
        let full = self
            .event_list
            .cursor
            .and_then(|index| self.shown_event_rows().nth(index).map(|(full, _)| full));
        self.rematch_event_at(full);
    }

    pub(super) fn rematch_event_at(&mut self, selected_full: Option<super::task_ui::EventRow>) {
        self.cancel_event_detail_selection();
        let pattern = self.event_list.search.pattern();
        let mut matcher = self.event_list.search.matcher.borrow_mut();
        self.event_list.search.matches = self
            .event_list
            .search
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(index, body)| pattern.score(body.slice(..), &mut matcher).map(|_| index))
            .collect();
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.as_path())
            .unwrap_or_else(|| Path::new(""));
        self.event_list.search.pinned_matches = self
            .event_list
            .pinned
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let body = search_body(&row.event, cwd);
                pattern.score(body.slice(..), &mut matcher).map(|_| index)
            })
            .collect();
        drop(matcher);
        self.event_list.cursor = selected_full
            .and_then(|full| {
                self.shown_row_position(full).or_else(|| {
                    let super::task_ui::EventRow::Timeline(full) = full else {
                        return None;
                    };
                    (self.event_list.sort == EventSort::Time)
                        .then(|| {
                            self.shown_event_rows().position(|(row, _)| {
                                matches!(row, super::task_ui::EventRow::Timeline(index) if index >= full)
                            })
                        })
                        .flatten()
                })
            })
            .or_else(|| self.matching_event_count().checked_sub(1));
        let selected = self
            .event_list
            .cursor
            .and_then(|index| self.shown_event_rows().nth(index).map(|(full, _)| full));
        if selected != selected_full {
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_detail_layout = None;
        }
    }

    pub(crate) fn matching_event_count(&self) -> usize {
        self.shown_event_rows().count()
    }

    pub(super) fn pinned_event_matches(&self, index: usize) -> bool {
        !self.event_list.search.active()
            || self
                .event_list
                .search
                .pinned_matches
                .binary_search(&index)
                .is_ok()
    }

    pub(super) fn event_matches(&self, index: usize) -> bool {
        !self.event_list.search.active()
            || self.event_list.search.matches.binary_search(&index).is_ok()
    }
}

fn search_body(event: &crate::event::Event, cwd: &Path) -> Utf32String {
    format!(
        "{} {} {}",
        super::event_list::kind_label(&event.kind),
        super::event_list::body_line(event, usize::MAX, cwd),
        event
            .calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    )
    .into()
}

#[cfg(test)]
#[path = "event_search_tests.rs"]
mod tests;

#[cfg(test)]
mod pinned_tests {
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
            .query
            .set_text("unmatchable-pinned-search-query");
        view.rematch_event();
        assert!(!view.pinned_event_matches(0));
        assert!(view.event_list.search.matches.is_empty());
        let query = super::super::event_list::kind_label(&view.event_list.pinned[0].event.kind);
        view.event_list.search.query.set_text(query);
        view.rematch_event();
        assert!(view.pinned_event_matches(0));
        assert!(view.event_list.search.matches.is_empty());
        view.event_list.search.clear();
        assert!(view.pinned_event_matches(0));
    }
}

#[cfg(test)]
mod pinned_render_tests {
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
}
