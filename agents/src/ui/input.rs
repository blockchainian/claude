use super::*;

/// Rows one wheel report scrolls the log and details panels, matching CLAUDE_CODE_SCROLL_SPEED and the tmux wheel binding.
const WHEEL_ROWS: usize = 3;

#[derive(PartialEq, Eq)]
pub(super) enum ClickTarget {
    LogBorder {
        row: u16,
        latest: bool,
    },
    Log {
        index: usize,
    },
    Details {
        session: SessionKey,
        source: Option<String>,
        text: String,
        label: &'static str,
    },
    Background(crate::model::BackgroundTask),
    Link(PathBuf),
    Heading(PathBuf),
    /// A row of the details panel's selector.
    Choice(usize),
    Title(SessionKey),
}

pub(super) struct PendingClick {
    target: ClickTarget,
    deadline: Instant,
    open_heading: bool,
}

impl View {
    fn detail_copy_label(&self, event: &crate::log::Event) -> String {
        super::log::copied_label(event)
    }

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
            ClickTarget::Log { .. } | ClickTarget::LogBorder { .. } => None,
            ClickTarget::Details {
                session, source, ..
            } => {
                if self.selected_id() == Some(&session)
                    && let Some(source) = source
                    && let Some((full, _)) = self.log.iter().enumerate().rev().find(|(_, event)| {
                        event.source.as_ref() == Some(&source)
                            || event.calls.iter().any(|call| call.id == source)
                    })
                {
                    let index = self.shown_log_events().position(|(index, _)| index == full);
                    self.log_background_return = None;
                    self.select_log_event(index);
                }
                None
            }
            ClickTarget::Link(path) => Some(Action::OpenPath(path)),
            ClickTarget::Heading(path) => click.open_heading.then_some(Action::OpenPath(path)),
            ClickTarget::Choice(index) => self.single_choice(index),
            ClickTarget::Background(_) | ClickTarget::Title(_) => None,
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
            open_heading: false,
        });
        false
    }

    fn right_click(&mut self) {
        self.pending_click = None;
        if self.confirm_dialog.take().is_some() {
            return;
        }
        if self.title_editor().is_some() {
            let draft = self.name_target.take();
            self.rename_target = None;
            self.rename.clear();
            if draft.is_some() {
                self.discard_selected_draft();
            } else {
                self.reconcile_selection();
            }
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
            self.select_log_event(previous.event);
            self.log_detail_focused = previous.detail_focused;
            self.log_detail_search = previous.search;
            self.viewport.borrow_mut().log_detail_scroll = previous.scroll;
        } else {
            self.show_newest_log();
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
                event: self.log_selected,
                detail_focused: self.log_detail_focused,
                scroll: self.viewport.borrow().log_detail_scroll,
                search: std::mem::take(&mut self.log_detail_search),
            });
        }
        self.log_background_event = Some(crate::log::Event {
            kind: crate::log::Kind::Tool,
            source: None,
            completed_time: false,
            background: false,
            source_records: Vec::new(),
            sender: None,
            detail_body: None,
            body: task.description.clone(),
            paths: Vec::new(),
            started_at: task.started_at,
            ended_at: None,
            duration: None,
            previous_end: None,
            images: Vec::new(),
            calls: vec![crate::log::ToolCall {
                agent_message: None,
                started_at: None,
                hide_output: false,
                agent_thread_id: None,
                agent_path: None,
                detail_left: "background".into(),
                detail_title: task.description.clone(),
                show_details: false,
                id: String::new(),
                title: task.description,
                name: "background".into(),
                input: Vec::new(),
                output: Some(vec![crate::log::Section::Text {
                    label: String::new(),
                    text: body,
                }]),
                exit_code: None,
                is_error: false,
                duration: None,
            }],
        });
        self.log_detail_focused = true;
        self.viewport.borrow_mut().log_detail_layout = None;
        self.viewport.borrow_mut().log_detail_scroll = 0;
    }

    fn set_filter(&mut self, idx: usize) {
        self.remember_previous_task();
        self.leave_draft();
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

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        let position = (mouse.column, mouse.row).into();
        let previous_focus = self.mouse_focus;
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.mouse_focus = {
                let viewport = self.viewport.borrow();
                if viewport.list.contains(position) || viewport.tab_bar.contains(position) {
                    Panel::TaskList
                } else if viewport.log.contains(position)
                    || viewport
                        .log_borders
                        .iter()
                        .any(|rect| rect.contains(position))
                {
                    Panel::LogList
                } else if viewport.terminal.contains(position) && self.terminal.is_some() {
                    Panel::Terminal
                } else if viewport.log_detail.contains(position) {
                    Panel::LogDetail
                } else if viewport.details.contains(position) {
                    Panel::Details
                } else if viewport.prompt.contains(position) {
                    Panel::Prompt
                } else {
                    Panel::Other
                }
            };
        }
        if self.mouse_focus != previous_focus
            && (self.searching || self.log_search.editing || self.log_detail_search.editing)
        {
            self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        let previous = if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && let Some((MouseButton::Left, x, y, false)) = self.mouse_press
            && self.pending_click.as_ref().is_some_and(|pending| {
                Instant::now() >= pending.deadline
                    || self.click_target_at((x, y).into()).as_ref() != Some(&pending.target)
            }) {
            self.flush_pending_click(Instant::now() + self.double_click_interval)
        } else {
            None
        };
        if let Some(action) = self.terminal_mouse(mouse) {
            return action;
        }
        let next = self.handle_mouse_event(mouse);
        if matches!(mouse.kind, MouseEventKind::Down(_) | MouseEventKind::Up(_)) {
            self.input_box.focused = self.mouse_focus == Panel::Prompt
                && self.input_disabled().is_none()
                && self.title_editor().is_none();
            self.log_list_focused = self.mouse_focus == Panel::LogList;
            self.log_detail_focused = self.mouse_focus == Panel::LogDetail;
        }
        match (previous, next) {
            (Some(first), Some(second)) => Some(Action::Batch(vec![first, second])),
            (action, None) | (None, action) => action,
        }
    }

    fn heading_at(&self, position: ratatui::layout::Position) -> Option<PathBuf> {
        let viewport = self.viewport.borrow();
        if self.grouping != Grouping::Project || !viewport.list_rows.contains(position) {
            return None;
        }
        let entry = viewport.first_entry + usize::from(position.y - viewport.list_rows.y);
        let rect = Rect::new(
            viewport.list_rows.x,
            position.y,
            viewport.list_rows.width,
            1,
        );
        let (heading, target) = match self
            .center_rows(&self.selectable_indices(), self.grouping)
            .get(entry)?
        {
            super::navigation::CenterRow::Group(index) => (
                self.group_heading(*index, rect.width),
                self.project_groups[*index].heading.clone(),
            ),
            _ => return None,
        };
        super::rows::heading_rect(&heading, rect)
            .contains(position)
            .then_some(target)
    }

    fn click_target_at(&self, position: ratatui::layout::Position) -> Option<ClickTarget> {
        let viewport = self.viewport.borrow();
        if viewport.details_title.contains(position)
            && let Some(id) = self.selected_id()
        {
            return Some(ClickTarget::Title(id.clone()));
        }
        if let Some((_, index)) = viewport
            .picker_rows
            .iter()
            .find(|(rect, _)| rect.contains(position))
        {
            return Some(ClickTarget::Choice(*index));
        }
        if let Some(cwd) = self.heading_at(position) {
            return Some(ClickTarget::Heading(cwd));
        }
        if viewport.details.contains(position)
            && let Some((_, task)) = viewport
                .details_background
                .iter()
                .find(|(rows, _)| rows.contains(&position.y))
        {
            return Some(ClickTarget::Background(task.clone()));
        }
        if let Some(path) = self.link_at(position) {
            return Some(ClickTarget::Link(path));
        }
        if viewport.details.contains(position)
            && let Some((_, text, label)) = viewport
                .details_copy
                .iter()
                .find(|(rows, _, _)| rows.contains(&position.y))
            && let Some(session) = self.selected_row()
        {
            let source = match *label {
                "Copied latest activity" => session.detail_sources[0].clone(),
                "Copied update" => session.detail_sources[1].clone(),
                "Copied prompt" => session.detail_sources[2].clone(),
                "Copied tasks" => None,
                _ => return None,
            };
            return Some(ClickTarget::Details {
                session: session.id.clone(),
                source,
                text: text.clone(),
                label,
            });
        }
        if !viewport.log_jump.contains(position)
            && let Some(border) = viewport
                .log_borders
                .iter()
                .position(|rect| rect.contains(position))
        {
            return Some(ClickTarget::LogBorder {
                row: position.y,
                latest: border == 1,
            });
        }
        if viewport.log.contains(position)
            && let Some(&index) = viewport
                .log_rows
                .get(usize::from(position.y - viewport.log.y))
        {
            return Some(ClickTarget::Log { index });
        }
        None
    }

    fn handle_mouse_event(&mut self, mut mouse: MouseEvent) -> Option<Action> {
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
                self.last_detail_click = None;
                if self
                    .viewport
                    .borrow()
                    .log_detail
                    .contains((mouse.column, mouse.row).into())
                {
                    return if self.terminal.is_some() {
                        Some(Action::CloseTerminal)
                    } else {
                        self.selected_id().cloned().map(Action::OpenTerminal)
                    };
                }
                if self.selector.is_some()
                    && self
                        .viewport
                        .borrow()
                        .details
                        .contains((mouse.column, mouse.row).into())
                {
                    self.pending_click = None;
                    self.cancel_selector();
                } else {
                    self.right_click();
                }
            }
            return None;
        }
        if self.confirm_dialog.is_some() {
            self.pending_click = None;
            self.text_selection.borrow_mut().take();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.mouse_press = Some((MouseButton::Left, mouse.column, mouse.row, false));
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some((_, _, _, dragged)) = self.mouse_press.as_mut() {
                        *dragged = true;
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let press = self.mouse_press.take();
                    let viewport = self.viewport.borrow();
                    let choice = viewport
                        .confirm_options
                        .iter()
                        .find(|(rect, _)| {
                            rect.contains((mouse.column, mouse.row).into())
                                && press.is_some_and(|(button, x, y, dragged)| {
                                    button == MouseButton::Left
                                        && !dragged
                                        && rect.contains((x, y).into())
                                })
                        })
                        .map(|(_, confirm)| *confirm);
                    drop(viewport);
                    if let Some(confirm) = choice {
                        let dialog = self.confirm_dialog.take().expect("confirm dialog is open");
                        return confirm.then_some(Action::Delete(dialog.id));
                    }
                }
                _ => self.mouse_press = None,
            }
            return None;
        }
        if self.help || self.rename_target.is_some() || self.searching {
            self.mouse_press = None;
            self.text_selection.borrow_mut().take();
            return None;
        }
        if let Some(action) = self.question_mouse(mouse) {
            return action;
        }
        let prompt = self.viewport.borrow().prompt;
        if self.input_disabled().is_none()
            && (self.input_box.mouse_selecting()
                || (mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && prompt.contains((mouse.column, mouse.row).into())))
            && matches!(
                mouse.kind,
                MouseEventKind::Down(MouseButton::Left)
                    | MouseEventKind::Drag(MouseButton::Left)
                    | MouseEventKind::Up(MouseButton::Left)
            )
        {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.mouse_press = None;
                self.pending_click = None;
                self.text_selection.borrow_mut().take();
                self.focus_input();
            }
            return self.input_box.select_mouse(
                prompt.inner(Margin::new(1, 0)),
                &self.input_label(),
                mouse,
            );
        }
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if self
                    .viewport
                    .borrow()
                    .prompt
                    .contains((mouse.column, mouse.row).into()) =>
            {
                let area = self.viewport.borrow().prompt.inner(Margin::new(1, 0));
                self.input_box.scroll_mouse(
                    area,
                    &self.input_label(),
                    mouse.kind == MouseEventKind::ScrollDown,
                );
                return None;
            }
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
                        self.last_detail_click = None;
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
        let detail_click = if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            self.last_detail_click.take()
        } else {
            None
        };
        let position = (mouse.column, mouse.row).into();
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            let choice = self
                .viewport
                .borrow()
                .picker_rows
                .iter()
                .find(|(rect, _)| rect.contains(position))
                .map(|(_, index)| *index);
            if let Some(index) = choice
                && let Some(selector) = self.selector.as_mut()
            {
                selector.selected = index;
                self.input_box.focused = false;
                if selector.branch_editor().is_some() && index + 1 == selector.len() {
                    return None;
                }
                if matches!(
                    selector.kind,
                    draft_controls::SelectorKind::Repository { .. }
                        | draft_controls::SelectorKind::Branch { .. }
                ) {
                    self.pending_click = None;
                    return self.apply_choice(index);
                }
                if self.defer_click(ClickTarget::Choice(index)) {
                    return self.apply_choice(index);
                }
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
                self.pending_click = None;
                self.open_field(field);
                return None;
            }
            if self.viewport.borrow().details_title.contains(position)
                && let Some(session) = self.selected_row()
            {
                let id = session.id.clone();
                if !self.defer_click(ClickTarget::Title(id)) {
                    return None;
                }
                self.mouse_focus = Panel::Details;
                return self.begin_rename();
            }
            if let Some(cwd) = self.heading_at(position) {
                if self.defer_click(ClickTarget::Heading(cwd.clone())) {
                    return Some(Action::NewDraft { cwd });
                }
                let open_heading = self.link_at(position).is_some();
                self.pending_click
                    .as_mut()
                    .expect("heading click is pending")
                    .open_heading = open_heading;
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
            self.show_newest_log();
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && let Some(target @ ClickTarget::LogBorder { latest, .. }) =
                self.click_target_at(position)
        {
            if self.defer_click(target) {
                if latest {
                    self.show_newest_log();
                } else {
                    self.log_background_return = None;
                    self.log_pending_older = 0;
                    self.log_start_requested = true;
                    self.select_log_event(Some(0));
                }
            }
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            let event = {
                let viewport = self.viewport.borrow();
                viewport
                    .log
                    .contains(position)
                    .then(|| usize::from(mouse.row - viewport.log.y))
                    .and_then(|row| viewport.log_rows.get(row))
                    .and_then(|&full| self.shown_log_events().position(|(index, _)| index == full))
            };
            if let Some(event) = event {
                self.log_background_return = None;
                self.select_log_event(Some(event));
            }
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
                let detail = self.viewport.borrow().log_detail;
                let has_double = {
                    let viewport = self.viewport.borrow();
                    viewport.log.contains(position)
                        || (detail.contains(position)
                            && self.visible_log_event().is_some()
                            && mouse.row >= detail.y + super::log::LOG_TOP)
                        || viewport.list.contains(position)
                        || (viewport.details.contains(position)
                            && viewport
                                .details_copy
                                .iter()
                                .any(|(rows, _, _)| rows.contains(&mouse.row)))
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
                        if viewport.log.contains(position) {
                            drop(viewport);
                            self.focus_input();
                            return None;
                        }
                        if detail.contains(position)
                            && let Some(event) = self.visible_log_event()
                        {
                            return Some(Action::Copy {
                                text: self.detail_text(event),
                                label: self.detail_copy_label(event),
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
                .then(|| usize::from(mouse.row - viewport.log.y))
                .and_then(|row| viewport.log_rows.get(row).copied());
            // The selected event's body starts below the top rows, its header and a blank row.
            let copy_text = if viewport.details.contains(position) {
                viewport
                    .details_copy
                    .iter()
                    .find(|(rows, _, _)| rows.contains(&mouse.row))
                    .map(|(_, text, label)| (text.clone(), (*label).to_owned()))
            } else {
                None
            };
            (
                tab,
                viewport.tab_bar.contains(position),
                viewport.list.contains(position),
                viewport.details.contains(position),
                viewport.log.contains(position) || viewport.log_detail.contains(position),
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
                } else if let Some(index) = log_row {
                    if self.defer_click(ClickTarget::Log { index }) {
                        self.focus_input();
                    }
                } else if let Some(target @ ClickTarget::Details { .. }) =
                    self.click_target_at(position)
                {
                    let ClickTarget::Details { text, label, .. } = &target else {
                        unreachable!()
                    };
                    let action = Action::Copy {
                        text: text.clone(),
                        label: (*label).into(),
                    };
                    if self.defer_click(target) {
                        return Some(action);
                    }
                } else if self.viewport.borrow().log_text.area.contains(position)
                    && let Some(session) = self.selected_id().cloned()
                    && self.visible_log_event().is_some()
                {
                    let now = Instant::now();
                    let double = detail_click.is_some_and(|(last, index, at)| {
                        last == session
                            && index == self.log_selected
                            && now.duration_since(at) < self.double_click_interval
                    });
                    if double {
                        let event = self.visible_log_event().expect("detail event is visible");
                        return Some(Action::Copy {
                            text: self.detail_text(event),
                            label: self.detail_copy_label(event),
                        });
                    }
                    self.last_detail_click = Some((session, self.log_selected, now));
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
            MouseEventKind::Up(MouseButton::Right) => self.show_newest_log(),
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
                } else if over_log && self.viewport.borrow().log.contains(position) {
                    self.move_log_selection(forward);
                } else if over_details && let Some(selector) = self.selector.as_mut() {
                    let code = if forward { KeyCode::Down } else { KeyCode::Up };
                    selector.selected =
                        super::navigation::step(selector.selected, selector.len(), 1, code);
                } else if over_log || over_details {
                    self.scroll_content_rows(forward, over_log, mouse.row, WHEEL_ROWS);
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
    pub(super) fn scroll_content_row(&mut self, forward: bool, over_log: bool, row: u16) -> bool {
        self.scroll_content_rows(forward, over_log, row, 1)
    }

    fn scroll_content_rows(
        &mut self,
        forward: bool,
        over_log: bool,
        row: u16,
        rows: usize,
    ) -> bool {
        let over_log_detail = {
            let viewport = self.viewport.borrow();
            (viewport.log_detail.y..viewport.log_detail.bottom()).contains(&row)
        };
        if over_log && over_log_detail {
            self.prepare_log_detail_scroll();
        }
        let mut viewport = self.viewport.borrow_mut();
        if over_log && over_log_detail {
            if viewport.log_detail_state.scroll_rows(forward, row, rows) {
                viewport.log_detail_scroll = viewport.log_detail_state.position();
                true
            } else {
                drop(viewport);
                self.move_log_selection(forward);
                false
            }
        } else if over_log {
            viewport.log_scroll = if forward {
                viewport.log_scroll.saturating_add(1)
            } else {
                viewport.log_scroll.saturating_sub(1)
            };
            true
        } else {
            let mut consumed = false;
            for _ in 0..rows {
                if !viewport.details_scroll.scroll(forward, row) {
                    break;
                }
                consumed = true;
            }
            consumed
        }
    }

    pub(super) fn move_selection(&mut self, forward: bool) {
        let visible = self.selectable_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|i| *i == self.selected)
            .unwrap_or(0);
        let code = if forward { KeyCode::Down } else { KeyCode::Up };
        let selected = visible[super::navigation::step(current, visible.len(), 1, code)];
        self.select(selected);
    }
    fn activate(&mut self) -> Option<Action> {
        if self.selected_draft.is_some() {
            self.focus_input();
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
        if !self.selected_capabilities().resume
            || !self.selected_row().is_some_and(Session::can_resume)
        {
            return Some(Action::Unsupported("Terminal"));
        }
        if self.searching {
            self.search.clear();
            self.searching = false;
        }
        Some(Action::OpenTerminal(id))
    }
    /// Stops the selected task, keeping a running turn's prompt to return to the input.
    fn stop_from_input(&mut self) -> Option<Action> {
        if !self.can_stop_selected() {
            return None;
        }
        let session = self.selected_row()?;
        let id = session.id.clone();
        self.stopping_prompt = (session.status == SessionStatus::Working
            && self.log_session.as_ref() == Some(&id))
        .then(|| {
            self.log
                .iter()
                .rev()
                .find(|event| event.kind == crate::log::Kind::User)
                .map(|event| (id.clone(), event.body.clone()))
        })
        .flatten();
        Some(Action::Stop(id))
    }

    fn can_stop_selected(&self) -> bool {
        self.selected_capabilities().stop
            || self.selected_row().is_some_and(Session::is_live_background)
    }

    fn begin_rename(&mut self) -> Option<Action> {
        let session = self.selected_row()?;
        if !self.selected_capabilities().rename {
            return Some(Action::Unsupported("Rename"));
        }
        let id = session.id.clone();
        let title = session.display_title().to_owned();
        self.rename.set_text(&title);
        self.rename_target = Some(id);
        self.name_target = None;
        self.input_box.focused = false;
        self.search.clear();
        self.searching = false;
        None
    }

    fn selected_capabilities(&self) -> Capabilities {
        self.selected_id()
            .and_then(|key| self.capabilities.get(&key.provider))
            .copied()
            .unwrap_or_default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let key = line_edit::normalize_key(key);
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if let Some(action) = self.terminal_key(key) {
            return action;
        }
        if let Some(action) = self.question_key(key) {
            return action;
        }
        if let Some(action) = self.selector_key(key) {
            return action;
        }
        if self.title_editing() && self.selector.is_none() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('c')
                    if key.code == KeyCode::Esc
                        || key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    let draft = self.name_target.take();
                    self.rename_target = None;
                    self.rename.clear();
                    if draft.is_some() {
                        self.discard_selected_draft();
                    } else {
                        self.reconcile_selection();
                    }
                }
                KeyCode::Enter if !self.rename.text().trim().is_empty() => {
                    let name = self.rename.text().trim().to_owned();
                    self.rename.clear();
                    if let Some(id) = self.name_target.take() {
                        self.drafts
                            .iter_mut()
                            .find(|draft| draft.id == id)
                            .expect("title draft is selected")
                            .name = Some(name);
                        self.close_selector();
                    } else {
                        let id = self
                            .rename_target
                            .take()
                            .expect("title editor target is set");
                        self.reconcile_selection();
                        return Some(Action::Rename { id, name });
                    }
                }
                _ => {
                    self.rename.handle(key);
                }
            }
            return None;
        }
        if key.code == KeyCode::Char('/')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
            && !matches!(
                self.mouse_focus,
                Panel::TaskList | Panel::LogList | Panel::LogDetail
            )
            && !self.input_box.focused
            && self.title_editor().is_none()
            && !self.searching
            && !self.log_search.editing
            && !self.log_detail_search.editing
        {
            return None;
        }
        if self.input_box.focused
            && self.title_editor().is_none()
            && !self.searching
            && !self.log_search.editing
            && !self.log_detail_search.editing
        {
            if key.code == KeyCode::Esc && self.selected_draft.is_some() {
                self.discard_selected_draft();
                return None;
            }
            // A new task saves its first prompt before choosing its branch.
            if key.code == KeyCode::Enter
                && key.modifiers.is_empty()
                && let Some(draft) = self.selected_draft()
            {
                if !draft.working && draft.job.is_none() && !self.input_box.editor.is_empty() {
                    return self.start_branch_step();
                }
                return None;
            }
            self.input_box.history = self.prompt_history.clone();
            return match self.input_box.handle_key(key) {
                Some(prompt_box::PromptAction::Quit) => Some(Action::Quit),
                Some(prompt_box::PromptAction::PasteImage) => Some(Action::PasteImage),
                Some(prompt_box::PromptAction::Stop) => self.stop_from_input(),
                Some(prompt_box::PromptAction::Submit(text)) => {
                    let images = self.input_box.editor.image_paths();
                    if !images.is_empty() {
                        return self.selected_id().cloned().map(|id| Action::SendImages {
                            id,
                            text,
                            images,
                        });
                    }
                    self.selected_id()
                        .cloned()
                        .map(|id| Action::SendPrompt { id, text })
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
            if key.code == KeyCode::Char('l') {
                return self.activate();
            }
            return self
                .selected_row()
                .filter(|session| !session.can_resume())
                .map(|_| {
                    Action::Unsupported(match key.code {
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
                            return Some(Action::Delete(dialog.id));
                        }
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                        dialog.confirm = !dialog.confirm;
                    }
                    // Each item's number accepts it at once.
                    KeyCode::Char('1') => self.confirm_dialog = None,
                    KeyCode::Char('2') => {
                        let dialog = self.confirm_dialog.take().expect("confirm dialog is open");
                        return Some(Action::Delete(dialog.id));
                    }
                    KeyCode::Home | KeyCode::PageUp => dialog.confirm = false,
                    KeyCode::End | KeyCode::PageDown => dialog.confirm = true,
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
            let selected_full = self
                .log_selected
                .and_then(|index| self.shown_log_events().nth(index).map(|(full, _)| full));
            match key.code {
                KeyCode::Esc => {
                    self.log_search.clear();
                    self.log_list_focused = false;
                }
                KeyCode::Enter => self.log_search.editing = false,
                _ => {
                    let before = self.log_search.query.text().to_owned();
                    if self.log_search.query.handle(key) {
                        if self.log_search.query.text() != before {
                            self.rematch_log_at(selected_full);
                        }
                        return None;
                    }
                }
            }
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.rematch_log_at(selected_full);
                return None;
            }
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
            self.set_filter(1 - self.status_filter);
            return None;
        }
        if plain && key.code == KeyCode::Char('/') {
            match self.mouse_focus {
                Panel::TaskList => self.searching = true,
                Panel::LogList => {
                    self.log_search.editing = true;
                    self.log_list_focused = true;
                    self.prepare_log_search();
                }
                Panel::LogDetail => {
                    self.log_detail_search.editing = true;
                    self.jump_detail_match();
                }
                _ => {}
            }
            return None;
        }
        if plain && key.code == KeyCode::Enter && self.mouse_focus == Panel::LogList {
            self.jump_log_agent();
            return None;
        }
        let navigation = super::navigation::list_navigation(key);
        if let Some(code) = navigation
            && !self.searching
            && self.mouse_focus == Panel::LogDetail
        {
            match code {
                KeyCode::Home | KeyCode::End => self.navigate_log(code),
                _ => {
                    let viewport = self.viewport.borrow();
                    let row = viewport.log_detail.y + super::log::LOG_TOP;
                    let steps = if matches!(code, KeyCode::PageUp | KeyCode::PageDown) {
                        viewport.log_text.area.height.max(1)
                    } else {
                        1
                    };
                    drop(viewport);
                    let forward = matches!(code, KeyCode::Down | KeyCode::PageDown);
                    if steps == 1 {
                        self.scroll_content_row(forward, true, row);
                    } else {
                        self.scroll_content_rows(forward, true, row, usize::from(steps));
                    }
                }
            }
            return None;
        }
        if let Some(code) = navigation
            && !self.searching
            && self.log_list_focused
        {
            self.navigate_log(code);
            return None;
        }
        if navigation.is_none() {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return if key.code == KeyCode::Char('l') {
                    self.activate()
                } else {
                    None
                };
            }
            if !plain {
                return None;
            }
        }
        match navigation.unwrap_or(key.code) {
            KeyCode::Up => self.move_selection(false),
            KeyCode::Down => self.move_selection(true),
            KeyCode::PageUp => self.page_selection(false),
            KeyCode::PageDown => self.page_selection(true),
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
            KeyCode::Esc if self.mouse_focus == Panel::TaskList => self.select_previous_task(),
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
            KeyCode::Char('r') => return self.begin_rename(),
            KeyCode::Char('a') => {
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().archive {
                        return Some(Action::Unsupported("Archive"));
                    }
                    return Some(Action::Archive(session.id.clone()));
                }
            }
            KeyCode::Backspace => {
                if self.selected_draft.is_some() {
                    self.discard_selected_draft();
                    return None;
                }
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().delete {
                        return Some(Action::Unsupported("Delete"));
                    }
                    self.confirm_dialog = Some(ConfirmDialog {
                        id: session.id.clone(),
                        title: session.display_title().to_owned(),
                        confirm: false,
                    });
                }
            }
            KeyCode::Char('o') => return self.activate(),
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
                return Some(if self.can_stop_selected() {
                    Action::Stop(id)
                } else {
                    Action::Unsupported("Stop")
                });
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

#[cfg(test)]
pub(super) use super::details_scroll::curtain_target;

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;
