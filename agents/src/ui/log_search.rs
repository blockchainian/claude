use super::*;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

// Twenty points per query character rejects scattered letters while keeping abbreviations.
const MIN_SCORE_PER_CHAR: u32 = 20;

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
        Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart)
    }

    fn cutoff(&self, pattern: &Pattern) -> u32 {
        // Count parsed needles so fzf operators and whitespace do not raise the cutoff.
        MIN_SCORE_PER_CHAR
            * pattern
                .atoms
                .iter()
                .filter(|atom| !atom.negative)
                .map(|atom| atom.needle_text().len() as u32)
                .sum::<u32>()
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
        let cutoff = self.log_search.cutoff(&pattern);
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
                    .filter(|score| *score >= cutoff)
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
