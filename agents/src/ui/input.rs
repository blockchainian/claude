use super::*;

impl View {
    fn set_filter(&mut self, idx: usize) {
        if self.status_filter != idx {
            self.viewport.borrow_mut().details_scroll = DetailsScroll::default();
        }
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
        let position = (mouse.column, mouse.row).into();
        let (tab, over_tabs, over_list, over_details, over_log, entry) = {
            let viewport = self.viewport.borrow();
            let tab = viewport
                .tabs
                .iter()
                .find(|(_, rect)| rect.contains(position))
                .map(|(idx, _)| *idx);
            let entry = viewport
                .list_rows
                .contains(position)
                .then(|| viewport.first_entry + usize::from(mouse.row - viewport.list_rows.y));
            (
                tab,
                viewport.tab_bar.contains(position),
                viewport.list.contains(position),
                viewport.details.contains(position),
                viewport.log.contains(position),
                entry,
            )
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(idx) = tab {
                    self.set_filter(idx);
                } else if let Some(entry) = entry {
                    let entries = self.center_rows(&self.selectable_indices(), self.grouping);
                    let selected = match entries.get(entry) {
                        Some(super::navigation::CenterRow::Task(index)) => *index,
                        Some(super::navigation::CenterRow::ShowMore) => usize::MAX,
                        _ => return None,
                    };
                    self.set_selected(selected);
                    return self.activate();
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let forward = mouse.kind == MouseEventKind::ScrollDown;
                if over_tabs {
                    let idx = if forward {
                        (self.status_filter + 1).min(TASK_FILTERS.len() - 1)
                    } else {
                        self.status_filter.saturating_sub(1)
                    };
                    self.set_filter(idx);
                } else if over_list {
                    self.move_selection(forward);
                } else if over_log {
                    let mut viewport = self.viewport.borrow_mut();
                    viewport.log_scroll = if forward {
                        viewport.log_scroll.saturating_sub(1)
                    } else {
                        viewport.log_scroll.saturating_add(1)
                    };
                } else if over_details {
                    let mut viewport = self.viewport.borrow_mut();
                    let state = &mut viewport.details_scroll;
                    if let Some(max) = state.fallback_max {
                        state.offset = if forward {
                            state.offset.saturating_add(1).min(max)
                        } else {
                            state.offset.saturating_sub(1).min(max)
                        };
                    } else {
                        let d = usize::from(mouse.row.saturating_sub(state.content_top))
                            + state.reveal.iter().sum::<usize>();
                        if let Some(target) =
                            curtain_target(&state.bodies, &state.reveal, &state.hidden, d, forward)
                        {
                            if forward {
                                state.reveal[target] += 1;
                                state.bodies[target].end += 1;
                                for body in &mut state.bodies[target + 1..] {
                                    body.start += 1;
                                    body.end += 1;
                                }
                            } else {
                                state.reveal[target] -= 1;
                                state.bodies[target].end -= 1;
                                for body in &mut state.bodies[target + 1..] {
                                    body.start -= 1;
                                    body.end -= 1;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
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
        let selected = visible[if forward {
            (current + 1) % visible.len()
        } else {
            current.checked_sub(1).unwrap_or(visible.len() - 1)
        }];
        self.set_selected(selected);
    }
    fn activate(&mut self) -> Option<Action> {
        if self.selected_is_subagent() {
            return None;
        }
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
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        if self.delete_dialog.is_none()
            && !self.help
            && self.rename_target.is_none()
            && !self.log_search.editing
            && !self.searching
            && self.selected_is_subagent()
            && ((plain
                && matches!(
                    key.code,
                    KeyCode::Enter
                        | KeyCode::Right
                        | KeyCode::Backspace
                        | KeyCode::Char('o' | 'r' | 'f' | 'x' | 'h' | 'p')
                ))
                || (key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('l')))
        {
            return None;
        }
        self.notice = None;
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
                KeyCode::Enter if !self.rename.text().trim().is_empty() => {
                    let id = self.rename_target.take().expect("rename target is set");
                    let name = self.rename.text().trim().to_owned();
                    self.rename.clear();
                    self.reconcile_selection();
                    return Some(Action::Rename { id, name });
                }
                _ => {
                    self.rename.handle(key);
                }
            }
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        if self.log_search.editing {
            match key.code {
                KeyCode::Esc => self.log_search.clear(),
                KeyCode::Enter => self.log_search.editing = false,
                _ => {
                    let before = self.log_search.query.text().to_owned();
                    if !self.log_search.query.handle(key) || self.log_search.query.text() == before
                    {
                        return None;
                    }
                }
            }
            self.rematch_log();
            self.viewport.borrow_mut().log_scroll = 0;
            return None;
        }
        if !self.searching
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('r')
        {
            self.log_search.editing = true;
            self.prepare_log_search();
            self.viewport.borrow_mut().log_scroll = 0;
            return None;
        }
        if self.searching {
            let before = self.search.text().to_owned();
            if self.search.handle(key) {
                if self.search.text() != before {
                    if (plain && matches!(key.code, KeyCode::Char(_)))
                        || key.code == KeyCode::Backspace
                    {
                        let selected = self
                            .selectable_indices()
                            .first()
                            .copied()
                            .unwrap_or(usize::MAX);
                        self.set_selected(selected);
                    } else {
                        self.reconcile_selection();
                    }
                    self.viewport.borrow_mut().scroll = 0;
                }
                return None;
            }
            match key.code {
                KeyCode::Esc => {
                    self.search.clear();
                    self.searching = false;
                    self.reconcile_selection();
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
            KeyCode::Char(hotkey @ ('*' | 'm' | 't' | 'u' | 'e')) => {
                use crate::sessions::log::Kind;
                let kinds: &[Kind] = match hotkey {
                    '*' => &[],
                    'm' => &[Kind::Model, Kind::Agent],
                    't' => &[Kind::Tool],
                    'u' => &[Kind::User, Kind::QueuedUser],
                    'e' => &[Kind::Error, Kind::Denied],
                    _ => unreachable!(),
                };
                let mut filter = crate::sessions::log::Filter::default();
                filter.toggle(kinds);
                if self.log_filter != filter {
                    self.log_filter = filter;
                    self.viewport.borrow_mut().log_scroll = 0;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true),
            KeyCode::PageUp => self.page_selection(false),
            KeyCode::PageDown => self.page_selection(true),
            KeyCode::Home => {
                let selected = self
                    .selectable_indices()
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX);
                self.set_selected(selected);
            }
            KeyCode::End => {
                let selected = self
                    .selectable_indices()
                    .last()
                    .copied()
                    .unwrap_or(usize::MAX);
                self.set_selected(selected);
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
                    let title = session.display_title().to_owned();
                    self.rename.set_text(&title);
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

/// Choose the body to open or close at a document row.
pub(super) fn curtain_target(
    bodies: &[std::ops::Range<usize>],
    reveal: &[usize],
    hidden: &[usize],
    d: usize,
    forward: bool,
) -> Option<usize> {
    let eligible = |i: usize| {
        if forward {
            reveal[i] < hidden[i]
        } else {
            reveal[i] > 0
        }
    };
    bodies
        .iter()
        .enumerate()
        .find(|(i, body)| body.contains(&d) && eligible(*i))
        .map(|(i, _)| i)
        .or_else(|| {
            bodies
                .iter()
                .enumerate()
                .find(|(i, body)| body.start > d && eligible(*i))
                .map(|(i, _)| i)
        })
        .or_else(|| {
            if forward {
                None
            } else {
                bodies
                    .iter()
                    .enumerate()
                    .rfind(|(i, body)| body.end <= d && eligible(*i))
                    .map(|(i, _)| i)
            }
        })
}
