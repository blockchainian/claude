use super::*;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern},
};

pub(crate) struct LogSearch {
    pub query: String,
    pub editing: bool,
    matches: Vec<usize>,
    bodies: Vec<String>,
    matcher: RefCell<Matcher>,
}

impl Default for LogSearch {
    fn default() -> Self {
        Self {
            query: String::new(),
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
        self.bodies.clear();
    }

    fn pattern(&self) -> Pattern {
        let mut pattern = Pattern::default();
        pattern.atoms = self
            .query
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
        self.log_search.bodies = self
            .log
            .iter()
            .map(|event| super::log::body_line(event, usize::MAX).to_string())
            .collect();
        self.rematch_log();
    }

    pub(crate) fn rematch_log(&mut self) {
        let pattern = self.log_search.pattern();
        let mut matcher = self.log_search.matcher.borrow_mut();
        let mut buffer = Vec::new();
        self.log_search.matches = self
            .log_search
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(index, body)| {
                pattern
                    .score(Utf32Str::new(body, &mut buffer), &mut matcher)
                    .map(|_| index)
            })
            .collect();
    }

    pub(crate) fn matching_log_count(&self) -> usize {
        self.log
            .iter()
            .enumerate()
            .filter(|(index, event)| {
                self.log_filter.matches(&event.kind)
                    && (!self.log_search.active()
                        || self.log_search.matches.binary_search(index).is_ok())
            })
            .count()
    }

    pub(super) fn log_matches(&self, index: usize) -> bool {
        !self.log_search.active() || self.log_search.matches.binary_search(&index).is_ok()
    }
}
