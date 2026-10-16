use super::*;

/// Rows one wheel report scrolls the log and details panels, matching CLAUDE_CODE_SCROLL_SPEED and the tmux wheel binding.
const WHEEL_ROWS: usize = 3;

#[derive(PartialEq, Eq)]
pub(super) enum ClickTarget {
    Log {
        event: crate::log::Event,
        expand: bool,
    },
    Background(crate::model::BackgroundTask),
    Link(PathBuf),
    Heading(PathBuf),
    Field(Field),
}

pub(super) struct PendingClick {
    target: ClickTarget,
    deadline: Instant,
}

impl View {
    pub(crate) fn pending_click_in(&self, now: Instant) -> Option<Duration> {
        self.pending_click
            .as_ref()
            .map(|click| click.deadline.saturating_duration_since(now))
    }

    pub(crate) fn flush_pending_click(&mut self, now: Instant) -> Option<Action> {
        if self
            .pending_click
            .as_ref()
            .is_none_or(|click| now < click.deadline)
        {
            return None;
        }
        let click = self.pending_click.take().expect("click is due");
        match click.target {
            ClickTarget::Log { event, expand } => {
                if expand {
                    self.set_log_open(Some(event));
                    self.viewport.borrow_mut().log_open_scroll = 0;
                }
                None
            }
            ClickTarget::Link(path) | ClickTarget::Heading(path) => Some(Action::OpenPath(path)),
            ClickTarget::Field(_) => None,
            ClickTarget::Background(_) => None,
        }
    }

    fn defer_click(&mut self, target: ClickTarget) -> bool {
        let now = Instant::now();
        if self
            .pending_click
            .as_ref()
            .is_some_and(|click| click.target == target && now < click.deadline)
        {
            self.pending_click = None;
            return true;
        }
        self.pending_click = Some(PendingClick {
            target,
            deadline: now + self.double_click_interval,
        });
        false
    }

    fn right_click(&mut self) {
        self.pending_click = None;
        if self.picker.take().is_some() {
            return;
        }
        if self.branch_editor.take().is_some() {
            return;
        }
        if self.confirm_dialog.take().is_some() {
            return;
        }
        if self.rename_target.take().is_some() {
            self.rename.clear();
            self.reconcile_selection();
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        if self.searching {
            self.searching = false;
            self.search.clear();
            self.reconcile_selection();
            return;
        }
        if self.input_box.focused {
            self.input_box.focused = false;
            return;
        }
        if self.log_detail_search.editing {
            self.log_detail_search.editing = false;
            return;
        }
        if self.log_search.editing {
            self.log_search.editing = false;
            return;
        }
        if let Some(previous) = self.log_background_return.take() {
            self.set_log_open(previous.event);
            self.log_detail_search = previous.search;
            self.viewport.borrow_mut().log_open_scroll = previous.scroll;
        } else {
            self.set_log_open(None);
        }
    }

    fn open_background(&mut self, task: crate::model::BackgroundTask) {
        let body = match std::fs::read_to_string(&task.output) {
            Ok(body) => body,
            Err(error) => {
                self.set_notice(Some(format!("Cannot read background log: {error}")));
                return;
            }
        };
        if self.log_background_return.is_none() {
            self.log_background_return = Some(BackgroundReturn {
                event: self.log_open.take(),
                scroll: self.viewport.borrow().log_open_scroll,
                search: std::mem::take(&mut self.log_detail_search),
            });
        }
        self.set_log_open(Some(crate::log::Event {
            kind: crate::log::Kind::Tool,
            sender: None,
            body,
            paths: Vec::new(),
            started_at: task.started_at,
            ended_at: None,
            duration: None,
            previous_end: None,
            images: Vec::new(),
            calls: Vec::new(),
        }));
        self.viewport.borrow_mut().log_open_scroll = 0;
    }

    fn set_filter(&mut self, idx: usize) {
        // A chosen task may have changed status since; remember it in the tabs that list it now.
        if self.selected_id() == self.chosen.as_ref() {
            self.remember_selection();
        }
        self.save_draft_input();
        self.status_filter = idx;
        self.viewport.borrow_mut().scroll = 0;
        let visible = self.visible_indices();
        let remembered = self.tab_selections[idx].as_ref().and_then(|id| {
            visible
                .iter()
                .copied()
                .find(|&index| &self.rows[index].id == id)
        });
        self.set_selected(remembered.unwrap_or_else(|| self.fallback_selection()));
        self.reconcile_selection();
    }

    pub fn handle_mouse(&mut self, mut mouse: MouseEvent) -> Option<Action> {
        if mouse.kind == MouseEventKind::Down(MouseButton::Right) {
            self.mouse_press = Some((MouseButton::Right, mouse.column, mouse.row, false));
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Right) {
            if self
                .mouse_press
                .take()
                .is_some_and(|(button, _, _, dragged)| button == MouseButton::Right && !dragged)
            {
                self.right_click();
            }
            return None;
        }
        if self.confirm_dialog.is_some()
            || self.help
            || self.rename_target.is_some()
            || self.searching
        {
            self.mouse_press = None;
            self.text_selection.borrow_mut().take();
            return None;
        }
        match mouse.kind {
            MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) => {
                self.mouse_press = Some((button, mouse.column, mouse.row, false));
                if button == MouseButton::Left {
                    self.begin_text_selection(mouse.column, mouse.row);
                }
                return None;
            }
            MouseEventKind::Drag(button @ (MouseButton::Left | MouseButton::Right)) => {
                if let Some((held, _, _, dragged)) = self.mouse_press.as_mut()
                    && *held == button
                {
                    *dragged = true;
                    if button == MouseButton::Left {
                        self.extend_text_selection(mouse.column, mouse.row);
                        self.last_click = None;
                        self.pending_click = None;
                        self.last_copy_click = None;
                    }
                }
                return None;
            }
            MouseEventKind::Up(button @ (MouseButton::Left | MouseButton::Right)) => {
                let (held, x, y, dragged) = self.mouse_press?;
                if held != button {
                    return None;
                }
                self.mouse_press = None;
                let selection = if button == MouseButton::Left {
                    self.finish_text_selection(mouse.column, mouse.row)
                } else {
                    None
                };
                if dragged {
                    return selection;
                }
                mouse.column = x;
                mouse.row = y;
            }
            _ => {}
        }
        let position = (mouse.column, mouse.row).into();
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            if self.picker.is_some() {
                let chosen = self
                    .viewport
                    .borrow()
                    .picker_rows
                    .iter()
                    .find(|(rect, _)| rect.contains(position))
                    .map(|(_, index)| *index);
                return chosen.and_then(|index| self.apply_choice(index));
            }
            if self.branch_editor.is_some() {
                return None;
            }
            let field = self
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(rect, _)| rect.contains(position))
                .map(|(_, field)| *field);
            if let Some(field) = field {
                if self.defer_click(ClickTarget::Field(field)) {
                    self.open_field(field);
                }
                return None;
            }
            let heading = {
                let viewport = self.viewport.borrow();
                if viewport.list_rows.contains(position) {
                    let entry =
                        viewport.first_entry + usize::from(mouse.row - viewport.list_rows.y);
                    match self
                        .center_rows(&self.selectable_indices(), self.grouping)
                        .get(entry)
                    {
                        Some(super::navigation::CenterRow::Group(index)) => {
                            Some(self.project_groups[*index].heading.clone())
                        }
                        Some(super::navigation::CenterRow::DraftGroup(id)) => self
                            .drafts
                            .iter()
                            .find(|draft| draft.id == *id)
                            .map(|draft| draft.setup.cwd.clone()),
                        _ => None,
                    }
                } else {
                    None
                }
            };
            if let Some(cwd) = heading
                && self.grouping == Grouping::Project
            {
                if self.defer_click(ClickTarget::Heading(cwd.clone())) {
                    return Some(Action::NewDraft { cwd });
                }
                return None;
            }
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && self.viewport.borrow().prompt.contains(position)
        {
            self.focus_input();
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && self.viewport.borrow().log_jump.contains(position)
        {
            if self.log_open.is_some() {
                self.viewport.borrow_mut().log_open_scroll = 0;
            } else {
                self.show_newest_log();
            }
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            let background = {
                let viewport = self.viewport.borrow();
                viewport
                    .details
                    .contains(position)
                    .then(|| {
                        viewport
                            .details_background
                            .iter()
                            .find(|(rows, _)| rows.contains(&mouse.row))
                            .map(|(_, task)| task.clone())
                    })
                    .flatten()
            };
            if let Some(task) = background {
                if self.defer_click(ClickTarget::Background(task.clone())) {
                    self.open_background(task);
                }
                return None;
            }
            if let Some(target) = self.link_at(position) {
                let has_double = {
                    let viewport = self.viewport.borrow();
                    viewport.log.contains(position)
                        || viewport.list.contains(position)
                        || viewport
                            .details_copy
                            .iter()
                            .any(|(rows, _, _)| rows.contains(&mouse.row))
                };
                if has_double {
                    if self.defer_click(ClickTarget::Link(target)) {
                        let viewport = self.viewport.borrow();
                        if viewport.details.contains(position) {
                            return viewport
                                .details_copy
                                .iter()
                                .find(|(rows, _, _)| rows.contains(&mouse.row))
                                .map(|(_, text, label)| Action::Copy {
                                    text: text.clone(),
                                    label: (*label).into(),
                                });
                        }
                        if viewport.log.contains(position) && self.log_open.is_none() {
                            drop(viewport);
                            self.focus_input();
                            return None;
                        }
                        if viewport.log.contains(position)
                            && let Some(event) = &self.log_open
                        {
                            return Some(Action::Copy {
                                text: super::log::open_text(event),
                                label: super::log::copied_label(event),
                            });
                        }
                    }
                    return None;
                }
                return Some(Action::OpenPath(target));
            }
        }
        let (tab, over_tabs, over_list, over_details, over_log, entry, log_row, copy_text) = {
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
            let log_row = viewport
                .log
                .contains(position)
                .then(|| {
                    usize::from(mouse.row - viewport.log.y)
                        .checked_sub(usize::from(super::log::LOG_TOP))
                })
                .flatten()
                .and_then(|row| viewport.log_rows.get(row).copied());
            // The open log event's body starts below the top rows, its header and a blank row.
            let copy_text = if viewport.details.contains(position) {
                viewport
                    .details_copy
                    .iter()
                    .find(|(rows, _, _)| rows.contains(&mouse.row))
                    .map(|(_, text, label)| (text.clone(), (*label).to_owned()))
            } else if viewport.log.contains(position)
                && mouse.row >= viewport.log.y + super::log::LOG_TOP + 2
            {
                self.log_open.as_ref().map(|event| {
                    (
                        super::log::open_text(event),
                        super::log::copied_label(event),
                    )
                })
            } else {
                None
            };
            (
                tab,
                viewport.tab_bar.contains(position),
                viewport.list.contains(position),
                viewport.details.contains(position),
                viewport.log.contains(position),
                entry,
                log_row,
                copy_text,
            )
        };
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(idx) = tab {
                    self.set_filter(idx);
                } else if let Some(entry) = entry {
                    let entries = self.center_rows(&self.selectable_indices(), self.grouping);
                    let task = match entries.get(entry) {
                        Some(super::navigation::CenterRow::Draft(id)) => {
                            self.select_draft(*id);
                            return None;
                        }
                        Some(super::navigation::CenterRow::Task(index)) => *index,
                        Some(super::navigation::CenterRow::ShowMore) => {
                            self.select(usize::MAX);
                            return self.activate();
                        }
                        _ => return None,
                    };
                    let now = Instant::now();
                    let double = self.last_click.take().is_some_and(|(last, at)| {
                        last == task && now.duration_since(at) < self.double_click_interval
                    });
                    self.select(task);
                    if double {
                        self.focus_input();
                        return None;
                    }
                    self.last_click = Some((task, now));
                } else if self.log_open.is_none()
                    && let Some((index, cut)) = log_row
                    && let Some(event) = self.log.get(index)
                {
                    let expand = cut || super::log::shows_calls(event);
                    if self.defer_click(ClickTarget::Log {
                        event: event.clone(),
                        expand,
                    }) {
                        self.focus_input();
                    }
                } else if let Some((text, label)) = copy_text {
                    let now = Instant::now();
                    let double = self.last_copy_click.take().is_some_and(|(last, at)| {
                        last == text && now.duration_since(at) < self.double_click_interval
                    });
                    if double {
                        return Some(Action::Copy { text, label });
                    }
                    self.last_copy_click = Some((text, now));
                }
            }
            MouseEventKind::Up(MouseButton::Right) => self.set_log_open(None),
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
                } else if over_log || over_details {
                    for _ in 0..WHEEL_ROWS {
                        self.scroll_content_row(forward, over_log, mouse.row);
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// What the link drawn at `position` opens, if one is there.
    fn link_at(&self, position: ratatui::layout::Position) -> Option<PathBuf> {
        let viewport = self.viewport.borrow();
        [
            &viewport.list_links,
            &viewport.details_links,
            &viewport.log_links,
        ]
        .into_iter()
        .flatten()
        .find(|(rect, _)| rect.contains(position))
        .map(|(_, target)| target.clone())
    }

    /// Scrolls the log panel, or the details panel when `over_log` is false, by one row.
    pub(super) fn scroll_content_row(&mut self, forward: bool, over_log: bool, row: u16) {
        if over_log && self.log_open.is_some() {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_open_scroll = if forward {
                viewport.log_open_scroll.saturating_add(1)
            } else {
                viewport.log_open_scroll.saturating_sub(1)
            };
        } else if over_log {
            let mut viewport = self.viewport.borrow_mut();
            viewport.log_scroll = if forward {
                viewport.log_scroll.saturating_add(1)
            } else {
                viewport.log_scroll.saturating_sub(1)
            };
        } else {
            let mut viewport = self.viewport.borrow_mut();
            let state = &mut viewport.details_scroll;
            if let Some(max) = state.fallback_max {
                state.offset = if forward {
                    state.offset.saturating_add(1).min(max)
                } else {
                    state.offset.saturating_sub(1).min(max)
                };
            } else {
                let d = usize::from(row.saturating_sub(state.content_top))
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

    pub(super) fn move_selection(&mut self, forward: bool) {
        if self.status_filter == 0 && !self.drafts.is_empty() {
            let targets: Vec<_> = self
                .center_rows(&self.selectable_indices(), self.grouping)
                .into_iter()
                .filter_map(|row| match row {
                    super::navigation::CenterRow::Task(index) => Some((Some(index), None)),
                    super::navigation::CenterRow::Draft(id) => Some((None, Some(id))),
                    super::navigation::CenterRow::ShowMore => Some((Some(usize::MAX), None)),
                    _ => None,
                })
                .collect();
            if targets.is_empty() {
                return;
            }
            let current = targets
                .iter()
                .position(|(index, draft)| match draft {
                    Some(id) => self.selected_draft == Some(*id),
                    None => self.selected_draft.is_none() && *index == Some(self.selected),
                })
                .unwrap_or(0);
            let next = if forward {
                (current + 1) % targets.len()
            } else {
                (current + targets.len() - 1) % targets.len()
            };
            match targets[next] {
                (Some(index), _) => self.select(index),
                (_, Some(id)) => self.select_draft(id),
                _ => unreachable!(),
            };
            return;
        }
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
        self.select(selected);
    }
    fn activate(&mut self) -> Option<Action> {
        if self.selected_draft.is_some() {
            self.focus_input();
            return None;
        }
        if self
            .selected_row()
            .is_some_and(|session| !session.can_resume())
        {
            return Some(Action::Unsupported("Resume"));
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
        if !self.selected_capabilities().resume {
            return Some(Action::Unsupported("Resume"));
        }
        Some(Action::Open(id))
    }
    fn selected_capabilities(&self) -> Capabilities {
        self.selected_id()
            .and_then(|key| self.capabilities.get(&key.provider))
            .copied()
            .unwrap_or_default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if let Some(action) = self.control_key(key) {
            return action;
        }
        if self.input_box.focused {
            return match self.input_box.handle_key(key) {
                Some(prompt_box::PromptAction::Quit) => Some(Action::Quit),
                Some(prompt_box::PromptAction::Submit(text)) => {
                    if let Some(id) = self.selected_draft {
                        let draft = self.drafts.iter_mut().find(|draft| draft.id == id)?;
                        if draft.working || draft.job.is_some() {
                            return None;
                        }
                        draft.working = true;
                        Some(Action::CreateTask {
                            draft_id: id,
                            provider: draft.setup.provider,
                            request: crate::tasks::NewTask {
                                cwd: draft.setup.cwd.clone(),
                                model: draft.setup.model.clone(),
                                effort: draft.setup.effort.clone(),
                                prompt: text,
                            },
                        })
                    } else {
                        self.selected_id()
                            .cloned()
                            .map(|id| Action::SendPrompt { id, text })
                    }
                }
                None => None,
            };
        }
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        if self.confirm_dialog.is_none()
            && !self.help
            && self.rename_target.is_none()
            && !self.log_search.editing
            && !self.log_detail_search.editing
            && !self.searching
            && self
                .selected_row()
                .is_some_and(|session| !session.can_resume())
            && ((plain
                && matches!(
                    key.code,
                    KeyCode::Backspace | KeyCode::Char('f' | 'r' | 'x' | 'a' | 'h' | 'p')
                ))
                || (key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('l')))
        {
            return self
                .selected_row()
                .filter(|session| !session.can_resume())
                .map(|_| {
                    Action::Unsupported(match key.code {
                        KeyCode::Char('l') => "Resume",
                        KeyCode::Char('f') => "Fork",
                        KeyCode::Char('r') => "Rename",
                        KeyCode::Char('x') => "Stop",
                        KeyCode::Char('a') => "Archive",
                        KeyCode::Backspace => "Delete",
                        KeyCode::Char('h') => "Hide",
                        KeyCode::Char('p') => "Pin",
                        _ => unreachable!("subagent session key is gated above"),
                    })
                });
        }
        self.notice = None;
        if let Some(dialog) = &mut self.confirm_dialog {
            if plain {
                match key.code {
                    KeyCode::Esc => self.confirm_dialog = None,
                    KeyCode::Enter => {
                        let dialog = self.confirm_dialog.take().expect("confirm dialog is open");
                        if dialog.confirm {
                            return Some(dialog.action());
                        }
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                        dialog.confirm = !dialog.confirm;
                    }
                    // Codex's number shortcuts: 1 accepts Cancel at once; 2 accepts archive
                    // at once, while the delete item requires explicit confirmation, so 2
                    // only selects it.
                    KeyCode::Char('1') => self.confirm_dialog = None,
                    KeyCode::Char('2') if matches!(dialog.removal, Removal::Archive) => {
                        let dialog = self.confirm_dialog.take().expect("confirm dialog is open");
                        return Some(dialog.action());
                    }
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
        if self.log_detail_search.editing {
            match key.code {
                KeyCode::Esc => self.log_detail_search.clear(),
                KeyCode::Enter => self.log_detail_search.editing = false,
                _ => {
                    self.log_detail_search.query.handle(key);
                }
            }
            self.jump_detail_match();
            return None;
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
            self.show_newest_log();
            return None;
        }
        if !self.searching
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('r')
        {
            if self.log_open.is_some() {
                self.log_detail_search.editing = true;
                self.jump_detail_match();
            } else {
                self.log_search.editing = true;
                self.prepare_log_search();
                self.show_newest_log();
            }
            return None;
        }
        if self.searching {
            let before = self.search.text().to_owned();
            if self.search.handle(key) {
                if self.search.text() != before {
                    if (plain && matches!(key.code, KeyCode::Char(_)))
                        || key.code == KeyCode::Backspace
                    {
                        let selectable = self.selectable_indices();
                        let selected = selectable
                            .iter()
                            .copied()
                            .find(|&i| self.rows.get(i).is_some_and(|s| self.matches_search(s)))
                            .or_else(|| selectable.first().copied())
                            .unwrap_or(usize::MAX);
                        self.select(selected);
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
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true),
            KeyCode::PageUp => self.page_selection(false),
            KeyCode::PageDown => self.page_selection(true),
            KeyCode::Home if self.status_filter == 0 && !self.drafts.is_empty() => {
                let target = self
                    .center_rows(&self.selectable_indices(), self.grouping)
                    .into_iter()
                    .find(|row| {
                        matches!(
                            row,
                            super::navigation::CenterRow::Task(_)
                                | super::navigation::CenterRow::Draft(_)
                        )
                    });
                match target {
                    Some(super::navigation::CenterRow::Task(index)) => self.select(index),
                    Some(super::navigation::CenterRow::Draft(id)) => self.select_draft(id),
                    _ => {}
                }
            }
            KeyCode::End if self.status_filter == 0 && !self.drafts.is_empty() => {
                let target = self
                    .center_rows(&self.selectable_indices(), self.grouping)
                    .into_iter()
                    .rev()
                    .find(|row| {
                        matches!(
                            row,
                            super::navigation::CenterRow::Task(_)
                                | super::navigation::CenterRow::Draft(_)
                        )
                    });
                match target {
                    Some(super::navigation::CenterRow::Task(index)) => self.select(index),
                    Some(super::navigation::CenterRow::Draft(id)) => self.select_draft(id),
                    _ => {}
                }
            }
            KeyCode::Home => {
                let selected = self
                    .selectable_indices()
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX);
                self.select(selected);
            }
            KeyCode::End => {
                let selected = self
                    .selectable_indices()
                    .last()
                    .copied()
                    .unwrap_or(usize::MAX);
                self.select(selected);
            }
            KeyCode::Enter | KeyCode::Right => return self.activate(),
            KeyCode::Esc => return Some(Action::Back),
            KeyCode::Char('g') => {
                self.grouping = self.grouping.next();
                self.viewport.borrow_mut().scroll = 0;
                return Some(Action::PersistGrouping(self.grouping));
            }
            KeyCode::Char('n') => {
                let cwd = self
                    .selected_draft()
                    .map(|draft| draft.heading())
                    .or_else(|| {
                        self.selected_row().map(|_| {
                            self.project_groups
                                [self.group_index(self.selected).unwrap_or(self.selected)]
                            .heading
                            .clone()
                        })
                    })
                    .unwrap_or_else(|| self.startup_cwd.clone());
                return Some(Action::NewDraft { cwd });
            }
            KeyCode::Char('w') => return Some(Action::Unsupported("New worktree")),
            KeyCode::Char('r') => {
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().rename {
                        return Some(Action::Unsupported("Rename"));
                    }
                    let id = session.id.clone();
                    let title = session.display_title().to_owned();
                    self.rename.set_text(&title);
                    self.rename_target = Some(id);
                    self.search.clear();
                    self.searching = false;
                }
            }
            KeyCode::Char('a') => {
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().archive {
                        return Some(Action::Unsupported("Archive"));
                    }
                    self.confirm_dialog = Some(ConfirmDialog {
                        removal: Removal::Archive,
                        id: session.id.clone(),
                        title: session.display_title().to_owned(),
                        confirm: false,
                    });
                }
            }
            KeyCode::Backspace => {
                if let Some(id) = self.selected_draft {
                    if self
                        .selected_draft()
                        .is_some_and(|draft| draft.working || draft.job.is_some())
                    {
                        return None;
                    }
                    self.save_draft_input();
                    self.drafts.retain(|draft| draft.id != id);
                    self.reconcile_selection();
                    self.show_selected_panels();
                    return None;
                }
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().delete {
                        return Some(Action::Unsupported("Delete"));
                    }
                    self.confirm_dialog = Some(ConfirmDialog {
                        removal: Removal::Delete,
                        id: session.id.clone(),
                        title: session.display_title().to_owned(),
                        confirm: false,
                    });
                }
            }
            KeyCode::Char('o') => {
                let id = self.selected_id()?.clone();
                return Some(
                    if self.selected_capabilities().resume
                        && self.selected_row().is_some_and(Session::can_resume)
                    {
                        Action::Resume(id)
                    } else {
                        Action::Unsupported("Resume")
                    },
                );
            }
            KeyCode::Char('f') => {
                let id = self.selected_id()?.clone();
                return Some(if self.selected_capabilities().fork {
                    Action::Fork(id)
                } else {
                    Action::Unsupported("Fork")
                });
            }
            KeyCode::Char('x') => {
                let id = self.selected_id()?.clone();
                return Some(
                    if self.selected_capabilities().stop
                        || self.selected_row().is_some_and(Session::is_live_background)
                    {
                        Action::Stop(id)
                    } else {
                        Action::Unsupported("Stop")
                    },
                );
            }
            KeyCode::Char('h') => {
                let id = self.selected_id()?.to_owned();
                self.hide(&id);
                return Some(Action::Hide(id));
            }
            KeyCode::Char('p') => {
                let group = self.group_index(self.selected)?;
                let id = self.rows[group].id.clone();
                let pinned = !self.is_pinned(self.selected);
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
