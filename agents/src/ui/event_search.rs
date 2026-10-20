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
    matcher: RefCell<Matcher>,
}

impl Default for EventSearch {
    fn default() -> Self {
        Self {
            query: line_edit::LineEdit::default(),
            editing: false,
            matches: Vec::new(),
            bodies: Vec::new(),
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
            .map(|event| {
                format!(
                    "{} {} {}",
                    super::event_list::kind_label(&event.kind),
                    super::event_list::body_line(event, usize::MAX, &cwd),
                    event
                        .calls
                        .iter()
                        .map(|call| call.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .into()
            })
            .collect();
        self.rematch_event();
    }

    pub(crate) fn rematch_event(&mut self) {
        let full = self
            .event_list.cursor
            .and_then(|index| self.shown_events().nth(index).map(|(full, _)| full));
        self.rematch_event_at(full);
    }

    pub(super) fn rematch_event_at(&mut self, selected_full: Option<usize>) {
        self.cancel_event_detail_selection();
        let pattern = self.event_list.search.pattern();
        let mut matcher = self.event_list.search.matcher.borrow_mut();
        self.event_list.search.matches = self
            .event_list.search
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(index, body)| pattern.score(body.slice(..), &mut matcher).map(|_| index))
            .collect();
        drop(matcher);
        self.event_list.cursor = selected_full
            .and_then(|full| {
                self.shown_position(full).or_else(|| {
                    (self.event_list.sort == EventSort::Time)
                        .then(|| self.shown_events().position(|(index, _)| index >= full))
                        .flatten()
                })
            })
            .or_else(|| self.matching_event_count().checked_sub(1));
        let selected = self
            .event_list.cursor
            .and_then(|index| self.shown_events().nth(index).map(|(full, _)| full));
        if selected != selected_full {
            let mut viewport = self.viewport.borrow_mut();
            viewport.event_detail_layout = None;
        }
    }

    pub(crate) fn matching_event_count(&self) -> usize {
        self.shown_events().count()
    }

    pub(super) fn event_matches(&self, index: usize) -> bool {
        !self.event_list.search.active() || self.event_list.search.matches.binary_search(&index).is_ok()
    }
}

#[cfg(test)]
#[path = "event_search_tests.rs"]
mod tests;
