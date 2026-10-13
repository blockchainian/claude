use super::*;
use unicode_segmentation::UnicodeSegmentation;

impl View {
    fn set_filter(&mut self, idx: usize) {
        self.status_filter = idx;
        self.viewport.borrow_mut().scroll = 0;
        self.reconcile_selection();
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        if self.delete_dialog.is_some()
            || self.help
            || self.rename_target.is_some()
            || self.searching
        {
            return None;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let tab = self
                .viewport
                .borrow()
                .tabs
                .iter()
                .find(|(_, rect)| rect.contains((mouse.column, mouse.row).into()))
                .map(|(idx, _)| *idx);
            if let Some(idx) = tab {
                self.set_filter(idx);
            }
        }
        None
    }

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
        if self.selected == usize::MAX
            && self.has_more
            && TASK_FILTERS[self.status_filter].1 == [SessionStatus::Inactive]
        {
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
        if let Some(dialog) = &mut self.delete_dialog {
            if plain {
                match key.code {
                    KeyCode::Esc => self.delete_dialog = None,
                    KeyCode::Enter => {
                        let dialog = self.delete_dialog.take().expect("delete dialog is open");
                        if dialog.confirm {
                            return Some(Action::Delete(dialog.id));
                        }
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                        dialog.confirm = !dialog.confirm;
                    }
                    // Codex's number shortcuts: 1 accepts Cancel at once; the delete item
                    // requires explicit confirmation, so 2 only selects it.
                    KeyCode::Char('1') => self.delete_dialog = None,
                    KeyCode::Home | KeyCode::PageUp => dialog.confirm = false,
                    KeyCode::End | KeyCode::PageDown | KeyCode::Char('2') => dialog.confirm = true,
                    _ => {}
                }
            } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('p' | 'k' | 'n' | 'j') => dialog.confirm = !dialog.confirm,
                    KeyCode::Char('b') => dialog.confirm = false,
                    KeyCode::Char('f') => dialog.confirm = true,
                    _ => {}
                }
            }
            return None;
        }
        if self.help {
            if key.code == KeyCode::Esc
                || (plain && key.code == KeyCode::Char('?') && key.kind == KeyEventKind::Press)
            {
                self.help = false;
            }
            return None;
        }
        if self.rename_target.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('c')
                    if key.code == KeyCode::Esc
                        || key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.rename_target = None;
                    self.rename.clear();
                    self.reconcile_selection();
                }
                KeyCode::Enter if !self.rename.trim().is_empty() => {
                    let id = self.rename_target.take().expect("rename target is set");
                    let name = self.rename.trim().to_owned();
                    self.rename.clear();
                    self.reconcile_selection();
                    return Some(Action::Rename { id, name });
                }
                KeyCode::Char(c) if plain => self.rename.push(c),
                KeyCode::Backspace => {
                    if let Some((offset, _)) = self.rename.grapheme_indices(true).next_back() {
                        self.rename.truncate(offset);
                    }
                }
                _ => {}
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
            let idx = (self.status_filter + if backwards { TASK_FILTERS.len() - 1 } else { 1 })
                % TASK_FILTERS.len();
            self.set_filter(idx);
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
            KeyCode::Esc => return Some(Action::Back),
            KeyCode::Char('g') => {
                self.grouping = self.grouping.next();
                self.viewport.borrow_mut().scroll = 0;
                return Some(Action::PersistGrouping(self.grouping));
            }
            KeyCode::Char('n') => {
                return Some(Action::New(self.selected_row().map(|s| s.cwd.clone())));
            }
            KeyCode::Char('w') => return Some(Action::Unsupported("New worktree")),
            KeyCode::Char('r') => {
                if let Some(session) = self.selected_row() {
                    let id = session.id.clone();
                    self.rename = session.display_title().to_owned();
                    self.rename_target = Some(id);
                    self.search.clear();
                    self.searching = false;
                }
            }
            KeyCode::Char('a') => return Some(Action::Unsupported("Archive")),
            KeyCode::Backspace => {
                if let Some(session) = self.selected_row() {
                    self.delete_dialog = Some(DeleteDialog {
                        id: session.id.clone(),
                        title: session.display_title().to_owned(),
                        confirm: false,
                    });
                }
            }
            KeyCode::Char('o') => return self.selected_id().map(|id| Action::Resume(id.into())),
            KeyCode::Char('f') => return self.selected_id().map(|id| Action::Fork(id.into())),
            KeyCode::Char('x') => return Some(Action::Unsupported("Stop")),
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
