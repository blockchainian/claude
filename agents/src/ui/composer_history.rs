use super::*;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) type History = Rc<RefCell<Vec<Composer>>>;

#[derive(Default)]
pub(super) struct Recall {
    position: Option<usize>,
    draft: Option<Composer>,
}

pub(super) struct Search {
    original: Composer,
    pub(super) query: LineEdit,
    position: Option<usize>,
}

impl PromptBox {
    pub(super) fn record_submission(&mut self) {
        let mut entry = self.editor.clone();
        entry.clear_selection();
        entry.move_caret(crate::line_edit::Motion::TextEnd);
        self.history.borrow_mut().push(entry);
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
            self.editor = history[position].clone();
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
            .filter(|(_, entry)| {
                entry.text().to_lowercase().contains(&query)
                    && seen.insert((entry.text(), entry.image_paths()))
            })
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
            self.editor = history[position].clone();
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

    pub(super) fn history_key(&mut self, key: KeyEvent, kills: &mut crate::line_edit::KillBuffer) -> bool {
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
                if search.query.handle_key(key, kills) {
                    self.search_history(false, true);
                }
            }
        }
        true
    }
}
