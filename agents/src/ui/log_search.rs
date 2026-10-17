use super::*;
use nucleo_matcher::{
    Config, Matcher, Utf32Str, Utf32String,
    pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern},
};

pub(crate) struct LogSearch {
    pub(super) query: line_edit::LineEdit,
    pub editing: bool,
    matches: Vec<usize>,
    bodies: Vec<Utf32String>,
    matcher: RefCell<Matcher>,
}

impl Default for LogSearch {
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

impl LogSearch {
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
    pub(crate) fn prepare_log_search(&mut self) {
        let cwd = self
            .selected_row()
            .map(|session| session.cwd.clone())
            .unwrap_or_default();
        self.log_search.bodies = self
            .log
            .iter()
            .map(|event| {
                format!(
                    "{} {}",
                    super::log::kind_label(&event.kind),
                    super::log::body_line(event, usize::MAX, &cwd)
                )
                .into()
            })
            .collect();
        self.rematch_log();
    }

    pub(crate) fn rematch_log(&mut self) {
        let full = self
            .log_selected
            .and_then(|index| self.log_search.matches.get(index).copied());
        self.rematch_log_at(full);
    }

    pub(super) fn rematch_log_at(&mut self, selected_full: Option<usize>) {
        self.cancel_detail_selection();
        let pattern = self.log_search.pattern();
        let mut matcher = self.log_search.matcher.borrow_mut();
        self.log_search.matches = self
            .log_search
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(index, body)| pattern.score(body.slice(..), &mut matcher).map(|_| index))
            .collect();
        drop(matcher);
        self.log_selected = selected_full
            .and_then(|full| {
                self.log_search
                    .matches
                    .iter()
                    .position(|index| *index >= full)
                    .or_else(|| self.log_search.matches.len().checked_sub(1))
            })
            .or_else(|| self.log_search.matches.len().checked_sub(1));
        let selected = self
            .log_selected
            .and_then(|index| self.log_search.matches.get(index).copied());
        if selected != selected_full {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_detail_layout = None;
            viewport.log_detail_other_layout = None;
        }
    }

    pub(crate) fn matching_log_count(&self) -> usize {
        self.log
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                !self.log_search.active() || self.log_search.matches.binary_search(index).is_ok()
            })
            .count()
    }

    pub(super) fn log_matches(&self, index: usize) -> bool {
        !self.log_search.active() || self.log_search.matches.binary_search(&index).is_ok()
    }
}

#[cfg(test)]
#[path = "log_search_tests.rs"]
mod tests;
