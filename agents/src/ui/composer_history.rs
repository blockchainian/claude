use super::*;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) type History = Rc<RefCell<Vec<String>>>;

#[derive(Default)]
pub(super) struct Recall {
    position: Option<usize>,
    draft: Option<LineEdit>,
}

pub(super) struct Search {
    original: LineEdit,
    pub(super) query: LineEdit,
    position: Option<usize>,
}

impl PromptBox {
    pub(super) fn record_submission(&mut self) {
        self.history
            .borrow_mut()
            .push(self.editor.text().to_owned());
        self.recall = Recall::default();
        self.search = None;
    }

    pub(super) fn recall_history(&mut self, down: bool) {
        let history = self.history.borrow();
        if history.is_empty() {
            return;
        }
        let position = match (self.recall.position, down) {
            (None, true) | (Some(0), false) => return,
            (None, false) => {
                self.recall.draft = Some(self.editor.clone());
                history.len() - 1
            }
            (Some(position), true) => position + 1,
            (Some(position), false) => position - 1,
        };
        if position == history.len() {
            self.editor = self.recall.draft.take().expect("history draft");
            self.recall.position = None;
        } else {
            self.editor.set_text(&history[position]);
            self.recall.position = Some(position);
        }
    }

    fn search_history(&mut self, down: bool, restart: bool) {
        let search = self.search.as_mut().expect("active history search");
        let history = self.history.borrow();
        let query = search.query.text().to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let matches: Vec<usize> = history
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, text)| text.to_lowercase().contains(&query) && seen.insert(text.as_str()))
            .map(|(index, _)| index)
            .collect();
        let position = match (restart, search.position, down) {
            (true, _, _) | (false, None, false) => matches.first().copied(),
            (false, None, true) => None,
            (false, Some(current), true) => {
                matches.iter().rev().copied().find(|&index| index > current)
            }
            (false, Some(current), false) => matches.iter().copied().find(|&index| index < current),
        };
        if let Some(position) = position {
            self.editor.set_text(&history[position]);
            search.position = Some(position);
        } else if restart {
            self.editor = search.original.clone();
            search.position = None;
        }
    }

    pub(super) fn search_paste(&mut self, text: &str) {
        self.search
            .as_mut()
            .expect("history search")
            .query
            .insert(text);
        self.search_history(false, true);
    }

    pub(super) fn history_key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers == KeyModifiers::CONTROL;
        if self.search.is_none() {
            if control && key.code == KeyCode::Char('r') {
                self.search = Some(Search {
                    original: self.editor.clone(),
                    query: LineEdit::default(),
                    position: None,
                });
                return true;
            }
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.editor = self.search.take().expect("history search").original;
            }
            KeyCode::Char('c') if control => {
                self.editor = self.search.take().expect("history search").original;
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                self.search = None;
                self.recall = Recall::default();
            }
            KeyCode::Up | KeyCode::Char('r') if key.code == KeyCode::Up || control => {
                self.search_history(false, false)
            }
            KeyCode::Down | KeyCode::Char('s') if key.code == KeyCode::Down || control => {
                self.search_history(true, false)
            }
            _ => {
                let search = self.search.as_mut().expect("history search");
                if search.query.handle(key) {
                    self.search_history(false, true);
                }
            }
        }
        true
    }
}
