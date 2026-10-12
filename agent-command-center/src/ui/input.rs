use super::*;
use unicode_segmentation::UnicodeSegmentation;

impl View {
    fn move_selection(&mut self, forward: bool) {
        let visible = self.selectable_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|i| *i == self.selected)
            .unwrap_or(0);
        self.selected = visible[if forward {
            (current + 1) % visible.len()
        } else {
            current.checked_sub(1).unwrap_or(visible.len() - 1)
        }];
    }
    fn activate(&mut self) -> Option<Action> {
        if self.selected == usize::MAX && self.has_more {
            if self.loading {
                return None;
            }
            self.loading = true;
            return Some(Action::ShowMore);
        }
        let id = self.selected_id()?.to_owned();
        if self.searching {
            self.search.clear();
            self.searching = false;
        }
        Some(Action::Open(id))
    }
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.notice = None;
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        if self.help {
            if key.code == KeyCode::Esc
                || (plain && key.code == KeyCode::Char('?') && key.kind == KeyEventKind::Press)
            {
                self.help = false;
            }
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.search.clear();
                    self.searching = false;
                    self.reconcile_selection();
                    return None;
                }
                KeyCode::Char(c) if plain => {
                    self.search.push(c);
                    self.selected = self
                        .selectable_indices()
                        .first()
                        .copied()
                        .unwrap_or(usize::MAX);
                    self.viewport.borrow_mut().scroll = 0;
                    return None;
                }
                KeyCode::Backspace => {
                    if let Some((offset, _)) = self.search.grapheme_indices(true).next_back() {
                        self.search.truncate(offset);
                    }
                    self.selected = self
                        .selectable_indices()
                        .first()
                        .copied()
                        .unwrap_or(usize::MAX);
                    return None;
                }
                KeyCode::F(3) => {
                    self.searching = false;
                    return None;
                }
                _ => {}
            }
        }
        if plain && key.code == KeyCode::Char('?') && key.kind == KeyEventKind::Press {
            self.help = true;
            return None;
        }
        if plain && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            let backwards =
                key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
            self.status_filter = (self.status_filter
                + if backwards { TASK_FILTERS.len() - 1 } else { 1 })
                % TASK_FILTERS.len();
            self.viewport.borrow_mut().scroll = 0;
            self.reconcile_selection();
            return None;
        }
        if plain && matches!(key.code, KeyCode::Char('/') | KeyCode::F(3)) {
            self.searching = !self.searching;
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p' | 'k') => self.move_selection(false),
                KeyCode::Char('n' | 'j') => self.move_selection(true),
                KeyCode::Char('b') => self.page_selection(false),
                KeyCode::Char('f') => self.page_selection(true),
                KeyCode::Char('l') => return self.activate(),
                _ => {}
            }
            return None;
        }
        if !plain {
            return None;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true),
            KeyCode::PageUp => self.page_selection(false),
            KeyCode::PageDown => self.page_selection(true),
            KeyCode::Home => {
                self.selected = self
                    .selectable_indices()
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX)
            }
            KeyCode::End => {
                self.selected = self
                    .selectable_indices()
                    .last()
                    .copied()
                    .unwrap_or(usize::MAX)
            }
            KeyCode::Enter | KeyCode::Right => return self.activate(),
            KeyCode::Esc => return Some(Action::Quit),
            KeyCode::Char('g') => {
                self.grouping = self.grouping.next();
                self.viewport.borrow_mut().scroll = 0;
                return Some(Action::PersistGrouping(self.grouping));
            }
            KeyCode::Char('n') => {
                return Some(Action::New(self.selected_row().map(|s| s.cwd.clone())));
            }
            KeyCode::Char('w') => return Some(Action::Unsupported("New worktree")),
            KeyCode::Char('r') => return Some(Action::Unsupported("Rename")),
            KeyCode::Char('a') => return Some(Action::Unsupported("Archive")),
            KeyCode::Backspace => return Some(Action::Unsupported("Delete")),
            KeyCode::Char('o') => return self.selected_id().map(|id| Action::Resume(id.into())),
            KeyCode::Char('f') => return self.selected_id().map(|id| Action::Fork(id.into())),
            KeyCode::Char('x') => {
                return self
                    .selected_row()
                    .filter(|s| s.status == SessionStatus::Working)
                    .map(|s| Action::Stop(s.id.clone()));
            }
            KeyCode::Char('h') => {
                let id = self.selected_id()?.to_owned();
                self.hide(&id);
                return Some(Action::Hide(id));
            }
            KeyCode::Char('p') => {
                let id = self.selected_id()?.to_owned();
                let pinned = !self.pinned.contains(&id);
                return Some(Action::TogglePin { id, pinned });
            }
            _ => {}
        }
        None
    }
}
