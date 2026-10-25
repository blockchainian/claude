use super::render::HORIZONTAL_PADDING;
use super::*;

/// Rows one wheel report scrolls the event list and details panels, matching CLAUDE_CODE_SCROLL_SPEED and the tmux wheel binding.
const WHEEL_ROWS: usize = 3;

#[derive(PartialEq, Eq)]
pub(super) enum ClickTarget {
    ListBorder {
        panel: Panel,
        row: u16,
        bottom: bool,
    },
    Event {
        index: EventRow,
    },
    TaskDetails {
        session: SessionKey,
        source: Option<String>,
        text: String,
        label: &'static str,
    },
    Background(crate::model::BackgroundTask),
    Link(LinkTarget),
    Heading(PathBuf),
    Title(SessionKey),
}

pub(super) struct PendingClick {
    target: ClickTarget,
    deadline: Instant,
    open_heading: bool,
}

#[derive(Clone, Copy)]
enum PasteTarget {
    Terminal,
    Prompt(Rect),
    Title(Rect),
    Branch(Rect),
    Search(Panel),
    Question(Rect),
}

impl View {
    pub(crate) fn set_event_sort(&mut self, sorting: EventSort) {
        self.change_event_list(false, |list| {
            list.settings_initialized = true;
            list.sort = sorting;
        });
    }

    fn change_event_list(
        &mut self,
        keep_selection: bool,
        change: impl FnOnce(&mut task_ui::EventList),
    ) {
        let following = self.following_event();
        let full = self
            .event_list
            .cursor
            .and_then(|index| self.shown_event_rows().nth(index).map(|(full, _)| full));
        let background_full = self
            .event_list
            .background_return
            .as_ref()
            .and_then(|saved| saved.event)
            .and_then(|index| self.shown_event_rows().nth(index).map(|(full, _)| full));
        self.event_list.pinned_following = false;
        change(&mut self.event_list);
        self.prepare_event_search();
        if keep_selection {
            self.rematch_event_at(full);
            self.cancel_list_selection(text_selection::Panel::EventList);
        } else {
            self.event_list.cursor = full.and_then(|full| self.shown_row_position(full));
        }
        let background_event = background_full.and_then(|full| self.shown_row_position(full));
        if let Some(saved) = &mut self.event_list.background_return {
            saved.event = background_event;
        }
        self.event_list.pending_older = 0;
        self.event_list.unseen.set(0);
        if matches!(full, Some(EventRow::Pinned(_))) {
            self.reveal_event_selection();
        } else if self.event_list.background_return.is_none() && (!keep_selection || following) {
            self.show_newest_event();
        } else if following {
            self.event_list.scroll.set(self.event_max_scroll());
        } else {
            self.reveal_event_selection();
        }
    }

    fn event_detail_copy_label(&self, event: &crate::event::Event) -> String {
        super::event_list::copied_label(event)
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
            ClickTarget::Event { .. } | ClickTarget::ListBorder { .. } => None,
            ClickTarget::Link(target) => Some(Action::Open(target)),
            ClickTarget::Heading(path) => click
                .open_heading
                .then_some(Action::Open(LinkTarget::Path(path))),
            ClickTarget::TaskDetails { .. }
            | ClickTarget::Background(_)
            | ClickTarget::Title(_) => None,
        }
    }

    /// Click on a task details body: select the event it came from, or, on a double click, copy
    /// a body no event backs, such as a queued prompt or the task list.
    fn activate_task_details(&mut self, target: ClickTarget) -> Option<Action> {
        let ClickTarget::TaskDetails {
            session,
            source,
            text,
            label,
        } = target
        else {
            return None;
        };
        let Some(source) = source else {
            return Some(Action::Copy {
                text,
                label: label.into(),
            });
        };
        if self.selected_id() == Some(&session)
            && let Some((full, _)) = self
                .event_panel
                .iter()
                .enumerate()
                .rev()
                .find(|(_, event)| {
                    event.source.as_ref() == Some(&source)
                        || event.calls.iter().any(|call| call.id == source)
                })
        {
            let index = self.shown_position(full);
            self.event_list.background_return = None;
            self.select_event(index);
        }
        None
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
        if self.selector.is_some() {
            self.cancel_selector();
            return;
        }
        if self.confirm_dialog.is_some() {
            self.cancel_confirmation();
            return;
        }
        if self.title_editor().is_some() {
            self.cancel_name();
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        if self.search_focused() {
            self.edit_search(self.active_panel, search_input::SearchInput::blur);
            return;
        }
        if self.selected_draft.is_some() {
            self.confirm_draft_delete();
            return;
        }
        if self.prompt_focused() {
            return;
        }
        if let Some(mut previous) = self.event_list.background_return.take() {
            self.select_event(previous.event);

            previous.search.input.blur();
            self.event_detail.search = previous.search;
            self.event_detail.scroll.borrow_mut().offset = previous.scroll;
        } else {
            self.show_newest_event();
        }
    }

    fn cycle_event_filter(&mut self) -> Option<Action> {
        self.change_event_list(true, |list| {
            list.filter = list.filter.next();
            list.settings_initialized = true;
        });
        self.selected_id()
            .cloned()
            .map(|id| Action::PersistFiltering {
                id,
                filter: self.event_list.filter,
            })
    }

    fn open_background(&mut self, task: crate::model::BackgroundTask) {
        let Some(output) = task.output.as_deref() else {
            return;
        };
        if self.event_list.background_return.is_none() {
            self.event_list.background_return = Some(BackgroundReturn {
                event: self.event_list.cursor,

                scroll: self.event_detail.scroll.borrow().offset,
                search: std::mem::take(&mut self.event_detail.search),
            });
        }
        self.event_list.background = Some(crate::event::Event {
            turn: None,
            kind: crate::event::Kind::Tool,
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
            calls: vec![crate::event::ToolCall {
                agent_message: None,
                started_at: None,
                hide_output: false,
                model_settings: None,
                agent_thread_id: None,
                agent_path: None,
                title: task.description.clone(),
                show_event_detail: false,
                id: String::new(),
                summary: task.description,
                name: "background".into(),
                input: Vec::new(),
                output: Some(vec![crate::event::Section::OutputFile {
                    provider: crate::providers::ProviderId::Claude,
                    path: output.into(),
                }]),
                exit_code: None,
                is_error: false,
                duration: None,
            }],
        });

        self.viewport.borrow_mut().event_detail_layout = None;
        self.event_detail.scroll.borrow_mut().offset = 0;
        self.refresh_transcript();
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
        if self.image_zoom_mouse(mouse) {
            return None;
        }
        if let Some(action) = self.hint_mouse(mouse) {
            return action;
        }
        let focus = self.task_focus();
        let action = self.handle_panel_mouse(mouse);
        if self.task_focus() != focus {
            self.set_active_panel(self.active_panel);
        }
        self.finish_panel_action(action)
    }

    fn image_zoom_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.event_detail.scroll.borrow().image_pressed {
            match mouse.kind {
                MouseEventKind::Up(MouseButton::Left) => {
                    self.event_detail.scroll.borrow_mut().image_pressed = false;
                    return true;
                }
                MouseEventKind::Drag(MouseButton::Left) => return true,
                _ => {}
            }
        }
        if !mouse.modifiers.is_empty() {
            return false;
        }
        let image = self
            .viewport
            .borrow()
            .event_detail_images
            .iter()
            .find(|image| image.area.contains((mouse.column, mouse.row).into()))
            .map(|image| image.index);
        let Some(index) = image else {
            return false;
        };
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            self.set_active_panel(Panel::EventDetail);
            let was_active = self.event_detail.scroll.borrow().zoom_image == Some(index);
            self.exit_image_zoom();
            let mut detail = self.event_detail.scroll.borrow_mut();
            if !was_active {
                detail.zoom_image = Some(index);
            }
            detail.image_pressed = true;
            self.mouse_press = None;
            self.mouse_panel = None;
            self.pending_click = None;
            self.last_copy_click = None;
            self.last_event_detail_click = None;
            self.text_selection.borrow_mut().take();
            return true;
        }
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && self.event_detail.scroll.borrow().zoom_image == Some(index)
        {
            self.zoom_event_image(mouse);
            return true;
        }
        false
    }

    fn zoom_event_image(&mut self, mouse: MouseEvent) {
        use super::inline_images::Zoom;

        let mut detail = self.event_detail.scroll.borrow_mut();
        if detail.zoomed_this_frame {
            return;
        }
        let mut viewport = self.viewport.borrow_mut();
        let Some(drawn) = viewport
            .event_detail_images
            .iter()
            .find(|image| image.area.contains((mouse.column, mouse.row).into()))
        else {
            return;
        };
        let index = drawn.index;
        let zoom = detail.zooms.get(&index).copied().unwrap_or_default();
        let mut next = Zoom {
            level: if mouse.kind == MouseEventKind::ScrollUp {
                zoom.level.saturating_add(1)
            } else {
                zoom.level.saturating_sub(1)
            },
            left: zoom.left,
        };
        if next.level == zoom.level {
            return;
        }
        let sizes = viewport.event_detail_columns.and_then(|columns| {
            let mut images = self.inline_images.borrow_mut();
            let images = images.as_mut()?;
            let before = images.size(&drawn.image, columns.text.width, zoom)?;
            let after = images.size(&drawn.image, columns.text.width, next)?;
            (before.0 > 0 && before.1 > 0 && after.0 > 0 && after.1 > 0).then_some((
                columns.text.width,
                before,
                after,
            ))
        });
        if let Some((width, before, after)) = sizes {
            let x = f32::from(mouse.column - drawn.area.x);
            let fraction = zoom.left + x / f32::from(before.0);
            let max_left = f32::from(after.0.saturating_sub(width)) / f32::from(after.0);
            next.left = (fraction - x / f32::from(after.0)).clamp(0.0, max_left);
            let row = usize::from(drawn.first_row) + usize::from(mouse.row - drawn.area.y);
            let next_row = (row as f64 * f64::from(after.1) / f64::from(before.1)).round() as usize;
            detail.offset = if next_row >= row {
                detail.offset.saturating_add(next_row - row)
            } else {
                detail.offset.saturating_sub(row - next_row)
            };
        } else {
            return;
        }
        detail.zooms.insert(index, next);
        detail.zoomed_this_frame = true;
        viewport.event_detail_layout = None;
    }

    /// Executes the action of a completed click on a rendered hint.
    fn hint_mouse(&mut self, mouse: MouseEvent) -> Option<Option<Action>> {
        let at = |x, y| {
            self.viewport
                .borrow()
                .hint_targets
                .iter()
                .find(|target| target.area.contains((x, y).into()))
                .copied()
        };
        let target = at(mouse.column, mouse.row);
        let pressed = self.mouse_press.and_then(|(button, x, y, dragged)| {
            (button == MouseButton::Left)
                .then(|| at(x, y).map(|target| (target, dragged)))
                .flatten()
        });
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if target.is_some() => {
                self.mouse_press = Some((MouseButton::Left, mouse.column, mouse.row, false));
                self.mouse_panel = None;
                Some(None)
            }
            MouseEventKind::Drag(MouseButton::Left) if pressed.is_some() => {
                if let Some((_, _, _, dragged)) = self.mouse_press.as_mut() {
                    *dragged = true;
                }
                Some(None)
            }
            MouseEventKind::Up(MouseButton::Left) if pressed.is_some() || target.is_some() => {
                self.mouse_press = None;
                if let (Some((start, false)), Some(end)) = (pressed, target)
                    && start.area == end.area
                    && start.action == end.action
                {
                    return Some(match end.action {
                        hints::HintAction::Help => {
                            self.help = true;
                            None
                        }
                        hints::HintAction::CloseHelp => {
                            self.help = false;
                            None
                        }
                        hints::HintAction::Question(action) => self.question_footer_action(action),
                    });
                }
                Some(None)
            }
            _ => None,
        }
    }

    fn handle_panel_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        if self.frozen_draft() {
            return None;
        }
        let position = (mouse.column, mouse.row).into();
        let panel = match mouse.kind {
            MouseEventKind::Moved => None,
            MouseEventKind::Down(button) => {
                let panel = self.mouse_panel_at(position);
                self.mouse_panel = Some((button, panel));
                panel
            }
            MouseEventKind::Drag(button) | MouseEventKind::Up(button) => {
                let panel = match self.mouse_panel {
                    Some((held, panel)) if held == button => panel,
                    _ => self.mouse_panel_at(position),
                };
                if matches!(mouse.kind, MouseEventKind::Up(_)) {
                    self.mouse_panel = None;
                }
                panel
            }
            _ => self.mouse_panel_at(position),
        };
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            for panel in [Panel::TaskList, Panel::EventList, Panel::EventDetail] {
                let rect = self.search_rect(panel);
                if rect.contains(position) {
                    self.set_active_panel(panel);
                    self.edit_search(panel, |search| search.click(rect, mouse.column));
                    return None;
                }
            }
            if self.search_focused() {
                self.edit_search(self.active_panel, search_input::SearchInput::blur);
            }
            let viewport = self.viewport.borrow();
            self.pressed_task = viewport
                .list_rows
                .contains(position)
                .then(|| viewport.first_entry + usize::from(position.y - viewport.list_rows.y))
                .and_then(|entry| {
                    match self
                        .center_rows(&self.selectable_indices(), self.grouping)
                        .get(entry)
                    {
                        Some(super::navigation::CenterRow::Task(index)) => {
                            Some(self.rows[*index].id.clone())
                        }
                        _ => None,
                    }
                });
        }
        if let Some(panel) = panel {
            let leaves_task = panel == Panel::TaskList && self.active_panel != Panel::TaskList;
            self.set_active_panel(panel);
            // The task the user left keeps its parked focus one Enter away.
            if leaves_task {
                self.task_entry_pending = self.has_parked_focus(self.panels_task.as_ref());
            }
        }
        if let Some(action) = self.overlay_mouse(mouse) {
            return action;
        }
        let middle = mouse.kind == MouseEventKind::Down(MouseButton::Middle);
        let paste_target = middle.then(|| self.paste_target(mouse)).flatten();
        if middle && paste_target.is_none() {
            return None;
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
        let next = if let Some(target) = paste_target {
            self.position_paste(target, mouse);
            Some(Action::PasteText)
        } else {
            self.handle_mouse_event(mouse)
        };
        match (previous, next) {
            (Some(first), Some(second)) => Some(Action::Batch(vec![first, second])),
            (action, None) | (None, action) => action,
        }
    }

    fn mouse_panel_at(&self, position: ratatui::layout::Position) -> Option<Panel> {
        let viewport = self.viewport.borrow();
        if viewport.list.contains(position)
            || viewport.tab_bar.contains(position)
            || viewport.search.contains(position)
        {
            return Some(Panel::TaskList);
        }
        if viewport.title_input.contains(position)
            || viewport.task_details_title.contains(position)
            || viewport.task_details_input.contains(position)
            || viewport
                .field_targets
                .iter()
                .any(|(rect, _)| rect.contains(position))
        {
            return Some(Panel::TaskDetails);
        }
        for (panel, content, borders) in [
            (
                Panel::EventDetail,
                viewport.event_detail,
                viewport.output_borders,
            ),
            (
                Panel::EventList,
                viewport.event_panel,
                viewport.event_borders,
            ),
        ] {
            let within_content = !content.is_empty()
                && position.x >= content.x.saturating_sub(HORIZONTAL_PADDING + 1)
                && position.x <= content.right()
                && position.y >= content.y
                && position.y < content.bottom();
            if within_content || borders.iter().any(|rect| rect.contains(position)) {
                return Some(panel);
            }
        }
        if viewport.terminal.contains(position) {
            return Some(Panel::EventDetail);
        }
        for (panel, content) in [
            (Panel::TaskList, viewport.list),
            (Panel::TaskDetails, viewport.task_details),
        ] {
            if !content.is_empty()
                && position.x >= content.x.saturating_sub(HORIZONTAL_PADDING + 1)
                && position.x <= content.right()
                && position.y >= content.y.saturating_sub(1)
                && position.y <= content.bottom()
            {
                return Some(panel);
            }
        }
        viewport.prompt.contains(position).then_some(Panel::Prompt)
    }

    fn paste_target(&self, mouse: MouseEvent) -> Option<PasteTarget> {
        if self.help || self.confirm_dialog.is_some() {
            return None;
        }
        let at = (mouse.column, mouse.row).into();
        let v = self.viewport.borrow();
        if v.terminal.contains(at) && self.shown_terminal().is_some() {
            return Some(PasteTarget::Terminal);
        }
        if self.title_editor().is_some() {
            return v
                .title_input
                .contains(at)
                .then_some(PasteTarget::Title(v.title_input));
        }
        if let Some(selector) = &self.selector
            && selector.branch_editor().is_some()
            && v.task_details_input.contains(at)
        {
            return Some(PasteTarget::Branch(v.task_details_input));
        }
        if v.prompt.contains(at) && self.input_disabled().is_none() {
            let area = v.prompt.inner(Margin::new(1, 0));
            if let Some(question) = self.active_question() {
                return question
                    .paste_at(area, mouse.column, mouse.row)
                    .then_some(PasteTarget::Question(area));
            }
            return Some(PasteTarget::Prompt(area));
        }
        let panel = self.active_panel;
        if self.search_focused() && self.search_rect(panel).contains(at) {
            return Some(PasteTarget::Search(panel));
        }
        None
    }

    fn position_paste(&mut self, target: PasteTarget, mouse: MouseEvent) {
        match target {
            PasteTarget::Terminal => unreachable!("terminal mouse handles paste"),
            PasteTarget::Prompt(area) => {
                self.prepare_prompt();
                self.input_box.paste_mouse(area, &self.input_label(), mouse);
            }
            PasteTarget::Title(area) => {
                render::click_input_row(&mut self.rename, area, "", mouse.column)
            }
            PasteTarget::Branch(area) => {
                let selector = self.selector.as_mut().expect("branch selector");
                selector.selected = selector.len() - 1;
                render::click_input_row(
                    selector.active_editor().expect("branch editor"),
                    area,
                    &format!("New branch {PROMPT_MARKER} "),
                    mouse.column,
                );
            }
            PasteTarget::Search(panel) => {
                let rect = self.search_rect(panel);
                self.edit_search(panel, |search| search.click(rect, mouse.column));
            }
            PasteTarget::Question(area) => {
                self.position_question_paste(area, mouse.column, mouse.row);
            }
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

    /// The task details body under `position` and the event its text came from.
    fn task_details_target(&self, position: ratatui::layout::Position) -> Option<ClickTarget> {
        let viewport = self.viewport.borrow();
        if !viewport.task_details.contains(position) {
            return None;
        }
        let (_, text, label) = viewport
            .task_details_copy
            .iter()
            .find(|(rows, _, _)| rows.contains(&position.y))?;
        let session = self.selected_row()?;
        let source = match *label {
            "Copied last activity" => session.task_details_sources[0].clone(),
            "Copied update" => session.task_details_sources[1].clone(),
            "Copied prompt" if *text == session.prompt => session.task_details_sources[2].clone(),
            "Copied prompt" | "Copied tasks" => None,
            _ => return None,
        };
        Some(ClickTarget::TaskDetails {
            session: session.id.clone(),
            source,
            text: text.clone(),
            label,
        })
    }

    fn click_target_at(&self, position: ratatui::layout::Position) -> Option<ClickTarget> {
        let viewport = self.viewport.borrow();
        if viewport.task_details_title.contains(position)
            && let Some(id) = self.selected_id()
        {
            return Some(ClickTarget::Title(id.clone()));
        }
        if let Some(cwd) = self.heading_at(position) {
            return Some(ClickTarget::Heading(cwd));
        }
        if viewport.task_details.contains(position)
            && let Some((_, task)) = viewport
                .task_details_background
                .iter()
                .find(|(rows, _)| rows.contains(&position.y))
        {
            return Some(ClickTarget::Background(task.clone()));
        }
        if let Some(path) = self.link_at(position) {
            return Some(ClickTarget::Link(path));
        }
        if let Some(target) = self.task_details_target(position) {
            return Some(target);
        }
        if let Some(border) = viewport
            .output_borders
            .iter()
            .position(|rect| rect.contains(position))
        {
            return Some(ClickTarget::ListBorder {
                panel: Panel::EventDetail,
                row: position.y,
                bottom: border == 1,
            });
        }
        if !viewport.event_jump.contains(position)
            && let Some(border) = viewport
                .event_borders
                .iter()
                .position(|rect| rect.contains(position))
        {
            return Some(ClickTarget::ListBorder {
                panel: Panel::EventList,
                row: position.y,
                bottom: border == 1,
            });
        }
        for (panel, content) in [
            (Panel::TaskList, viewport.list),
            (Panel::TaskDetails, viewport.task_details),
        ] {
            if !content.is_empty()
                && position.x >= content.x
                && position.x < content.right()
                && (position.y == content.y.saturating_sub(1) || position.y == content.bottom())
            {
                return Some(ClickTarget::ListBorder {
                    panel,
                    row: position.y,
                    bottom: position.y == content.bottom(),
                });
            }
        }
        if viewport.event_panel.contains(position)
            && let Some(index) =
                viewport.event_row_at(usize::from(position.y - viewport.event_panel.y))
        {
            return Some(ClickTarget::Event { index });
        }
        None
    }

    fn overlay_mouse(&mut self, mouse: MouseEvent) -> Option<Option<Action>> {
        let position = (mouse.column, mouse.row).into();
        let modal = self
            .viewport
            .borrow()
            .overlays
            .iter()
            .rev()
            .find(|overlay| {
                overlay.modal
                    || overlay.area.contains(position)
                    || self
                        .mouse_press
                        .is_some_and(|(_, x, y, _)| overlay.area.contains((x, y).into()))
            })
            .map(|overlay| overlay.modal)?;
        if !modal
            && mouse.kind == MouseEventKind::Down(MouseButton::Middle)
            && self.viewport.borrow().task_details_input.contains(position)
            && self
                .selector
                .as_ref()
                .is_some_and(|selector| selector.branch_editor().is_some())
        {
            return None;
        }
        // Selectors keep their wheel navigation.
        if !modal
            && matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        {
            return None;
        }
        self.pending_click = None;
        self.text_selection.borrow_mut().take();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Right) => {
                self.mouse_press = Some((MouseButton::Right, mouse.column, mouse.row, false));
            }
            MouseEventKind::Up(MouseButton::Right) => {
                if self
                    .mouse_press
                    .take()
                    .is_some_and(|(button, _, _, dragged)| button == MouseButton::Right && !dragged)
                    && (modal || self.selector.is_some())
                {
                    self.last_event_detail_click = None;
                    self.right_click();
                }
            }
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
                let Some((MouseButton::Left, x, y, false)) = press else {
                    return Some(None);
                };
                if modal {
                    let choice = self
                        .viewport
                        .borrow()
                        .confirm_options
                        .iter()
                        .find(|(rect, _)| rect.contains(position) && rect.contains((x, y).into()))
                        .map(|(_, confirm)| *confirm);
                    if let Some(confirm) = choice {
                        return Some(if confirm {
                            let dialog =
                                self.confirm_dialog.take().expect("confirm dialog is open");
                            self.accept_confirmation(dialog)
                        } else {
                            self.cancel_confirmation();
                            None
                        });
                    }
                } else {
                    let choice = self
                        .viewport
                        .borrow()
                        .picker_rows
                        .iter()
                        .find(|(rect, _)| rect.contains((x, y).into()))
                        .map(|(_, index)| *index);
                    if let Some(index) = choice
                        && let Some(selector) = self.selector.as_mut()
                    {
                        selector.selected = index;
                        if selector.branch_editor().is_some() && index + 1 == selector.len() {
                            return Some(None);
                        }
                        return Some(self.apply_choice(index));
                    }
                }
            }
            _ => self.mouse_press = None,
        }
        Some(None)
    }

    fn handle_mouse_event(&mut self, mut mouse: MouseEvent) -> Option<Action> {
        let position = (mouse.column, mouse.row).into();
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
                self.last_event_detail_click = None;
                if self.search_focused() || self.selected_draft.is_some() {
                    self.right_click();
                    return None;
                }
                if self.viewport.borrow().event_panel.contains(position) {
                    return self.cycle_event_filter();
                }
                if self.viewport.borrow().event_detail.contains(position) {
                    return if self.terminal.is_some() {
                        Some(Action::CloseTerminal)
                    } else {
                        self.selected_id().cloned().map(Action::OpenTerminal)
                    };
                }
                if self.selector.is_some() && self.viewport.borrow().task_details.contains(position)
                {
                    self.pending_click = None;
                    self.cancel_selector();
                } else {
                    self.right_click();
                }
            }
            return None;
        }
        if self.active_panel == Panel::TaskDetails && self.rename_target.is_some() {
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
                    && prompt.contains(position)))
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
                self.prepare_prompt();
            }
            return self.input_box.select_mouse(
                prompt.inner(Margin::new(1, 0)),
                &self.input_label(),
                mouse,
            );
        }
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                if self.viewport.borrow().prompt.contains(position) =>
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
                        self.last_event_detail_click = None;
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
        let event_detail_click = if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            self.last_event_detail_click.take()
        } else {
            None
        };
        let position = (mouse.column, mouse.row).into();
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            let field = self
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(rect, _)| rect.contains(position))
                .map(|(_, field)| *field);
            if let Some(field) = field {
                self.pending_click = None;
                if field == Field::WeeklyUsage {
                    self.usage_view = match self.usage_view {
                        crate::state::UsageView::Compact => crate::state::UsageView::Detailed,
                        crate::state::UsageView::Detailed => crate::state::UsageView::Compact,
                    };
                    return Some(Action::PersistUsageView(self.usage_view));
                }
                if self
                    .selector
                    .as_ref()
                    .is_some_and(|selector| selector.field() == field)
                {
                    self.close_selector();
                } else {
                    self.set_active_panel(Panel::TaskDetails);
                    self.open_field(field);
                }
                return None;
            }
            if self.viewport.borrow().task_details_title.contains(position)
                && let Some(session) = self.selected_row()
            {
                let id = session.id.clone();
                if !self.defer_click(ClickTarget::Title(id)) {
                    return None;
                }
                if let Some(draft) = self.selected_draft() {
                    self.open_name(draft.id);
                    return None;
                }
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
            self.prepare_prompt();
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && self.viewport.borrow().event_jump.contains(position)
        {
            self.show_newest_event();
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left)
            && let Some(target @ ClickTarget::ListBorder { panel, bottom, .. }) =
                self.click_target_at(position)
        {
            if self.defer_click(target) {
                if panel == Panel::EventDetail {
                    self.set_active_panel(Panel::EventDetail);
                    self.navigate_event_detail(
                        Panel::EventDetail,
                        if bottom { KeyCode::End } else { KeyCode::Home },
                    );
                } else if panel == Panel::TaskDetails {
                    let mut viewport = self.viewport.borrow_mut();
                    let state = &mut viewport.task_details_scroll;
                    let row = if bottom { state.content_top } else { u16::MAX };
                    while state.scroll_window(bottom, row) {}
                } else if panel == Panel::TaskList {
                    let indices = self.visible_indices();
                    let selected = if bottom {
                        indices.last()
                    } else {
                        indices.first()
                    };
                    if let Some(&selected) = selected {
                        self.select(selected);
                        self.viewport.borrow_mut().scroll = if bottom { usize::MAX } else { 0 };
                    }
                } else if bottom {
                    self.show_newest_event();
                } else {
                    self.show_first_event();
                }
            }
            return None;
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            let event = {
                let viewport = self.viewport.borrow();
                viewport
                    .event_panel
                    .contains(position)
                    .then(|| usize::from(mouse.row - viewport.event_panel.y))
                    .and_then(|row| viewport.event_row_at(row))
                    .and_then(|full| self.shown_row_position(full))
            };
            if let Some(event) = event {
                self.event_list.background_return = None;
                self.select_event(Some(event));
            }
            let background = {
                let viewport = self.viewport.borrow();
                viewport
                    .task_details
                    .contains(position)
                    .then(|| {
                        viewport
                            .task_details_background
                            .iter()
                            .find(|(rows, _)| rows.contains(&mouse.row))
                            .map(|(_, task)| task.clone())
                    })
                    .flatten()
            };
            if let Some(task) = background {
                self.pending_click = None;
                self.open_background(task);
                return None;
            }
            if let Some(target) = self.link_at(position) {
                let detail = self.viewport.borrow().event_detail;
                let has_double = {
                    let viewport = self.viewport.borrow();
                    viewport.event_panel.contains(position)
                        || (detail.contains(position)
                            && self.visible_event().is_some()
                            && mouse.row >= detail.y + super::event_list::EVENT_TOP)
                        || viewport.list.contains(position)
                        || (viewport.task_details.contains(position)
                            && viewport
                                .task_details_copy
                                .iter()
                                .any(|(rows, _, _)| rows.contains(&mouse.row)))
                };
                if has_double {
                    if self.defer_click(ClickTarget::Link(target)) {
                        if let Some(target) = self.task_details_target(position) {
                            return self.activate_task_details(target);
                        }
                        let viewport = self.viewport.borrow();
                        if viewport.event_panel.contains(position) {
                            drop(viewport);
                            return None;
                        }
                        if detail.contains(position)
                            && let Some(event) = self.visible_event()
                        {
                            return Some(Action::Copy {
                                text: self.event_detail_text(event),
                                label: self.event_detail_copy_label(event),
                            });
                        }
                    }
                    return None;
                }
                return Some(Action::Open(target));
            }
        }
        let fold_toggle = self
            .viewport
            .borrow()
            .fold_toggles
            .iter()
            .find(|(area, _)| area.contains(position))
            .map(|(_, id)| id.clone());
        let (tab, over_tabs, over_list, over_task_details, over_event, entry, event_row, copy_text) = {
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
            let event_row = viewport
                .event_panel
                .contains(position)
                .then(|| usize::from(mouse.row - viewport.event_panel.y))
                .and_then(|row| viewport.event_row_at(row));
            // The selected event's body starts below the top rows, its header and a blank row.
            let copy_text = if viewport.task_details.contains(position) {
                viewport
                    .task_details_copy
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
                viewport.task_details.contains(position),
                viewport.event_panel.contains(position) || viewport.event_detail.contains(position),
                entry,
                event_row,
                copy_text,
            )
        };
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(idx) = tab {
                    self.set_filter(idx);
                } else if let Some(entry) = entry {
                    let entries = self.center_rows(&self.selectable_indices(), self.grouping);
                    let task = if let Some(id) = self.pressed_task.take() {
                        self.rows.iter().position(|row| row.id == id)?
                    } else {
                        match entries.get(entry) {
                            Some(super::navigation::CenterRow::Task(index)) => *index,
                            Some(super::navigation::CenterRow::ShowMore) => {
                                self.select(usize::MAX);
                                return self.activate();
                            }
                            _ => return None,
                        }
                    };
                    let now = Instant::now();
                    let double = self.last_click.take().is_some_and(|(last, at)| {
                        last == task && now.duration_since(at) < self.double_click_interval
                    });
                    self.select(task);
                    if let Some(id) = fold_toggle.filter(|id| *id == self.rows[task].id) {
                        if !self.collapsed[self.status_filter].remove(&id) {
                            self.collapsed[self.status_filter].insert(id);
                        }
                        return None;
                    }
                    if double {
                        return None;
                    }
                    self.last_click = Some((task, now));
                } else if let Some(index) = event_row {
                    self.defer_click(ClickTarget::Event { index });
                } else if let Some(target @ ClickTarget::TaskDetails { .. }) =
                    self.click_target_at(position)
                {
                    if matches!(
                        target,
                        ClickTarget::TaskDetails {
                            source: Some(_),
                            ..
                        }
                    ) {
                        self.pending_click = None;
                        return self.activate_task_details(target);
                    }
                    if self.defer_click(target) {
                        return self
                            .task_details_target(position)
                            .and_then(|target| self.activate_task_details(target));
                    }
                } else if self.viewport.borrow().event_text.area.contains(position)
                    && let Some(session) = self.selected_id().cloned()
                    && self.visible_event().is_some()
                {
                    let now = Instant::now();
                    let double = event_detail_click.is_some_and(|(last, index, at)| {
                        last == session
                            && index == self.event_list.cursor
                            && now.duration_since(at) < self.double_click_interval
                    });
                    if double {
                        let event = self.visible_event().expect("detail event is visible");
                        return Some(Action::Copy {
                            text: self.event_detail_text(event),
                            label: self.event_detail_copy_label(event),
                        });
                    }
                    self.last_event_detail_click = Some((session, self.event_list.cursor, now));
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
            MouseEventKind::Up(MouseButton::Right) => self.show_newest_event(),
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
                } else if over_event && self.viewport.borrow().event_panel.contains(position) {
                    self.move_event_selection(forward);
                } else if over_task_details && let Some(selector) = self.selector.as_mut() {
                    let code = if forward { KeyCode::Down } else { KeyCode::Up };
                    selector.selected =
                        super::navigation::step(selector.selected, selector.len(), 1, code);
                } else if over_event || over_task_details {
                    self.scroll_content_rows(forward, over_event, mouse.row, WHEEL_ROWS, true);
                }
            }
            _ => {}
        }
        None
    }

    /// What the link drawn at `position` opens, if one is there.
    fn link_at(&self, position: ratatui::layout::Position) -> Option<LinkTarget> {
        let viewport = self.viewport.borrow();
        [
            &viewport.list_links,
            &viewport.task_details_links,
            &viewport.event_links,
        ]
        .into_iter()
        .flatten()
        .find(|(rect, _)| rect.contains(position))
        .map(|(_, target)| target.clone())
    }

    /// Scrolls the event list panel, or the task details panel when `over_event` is false, by one row.
    #[cfg(test)]
    pub(super) fn scroll_content_row(&mut self, forward: bool, over_event: bool, row: u16) -> bool {
        self.scroll_content_rows(forward, over_event, row, 1, false)
    }

    /// With `wheel`, an entry reached past the boundary starts at the end it was entered from.
    fn scroll_content_rows(
        &mut self,
        forward: bool,
        over_event: bool,
        row: u16,
        rows: usize,
        wheel: bool,
    ) -> bool {
        let over_event_detail = {
            let viewport = self.viewport.borrow();
            (viewport.event_detail.y..viewport.event_detail.bottom()).contains(&row)
        };
        if over_event && over_event_detail {
            self.prepare_event_detail_scroll();
        }
        let mut viewport = self.viewport.borrow_mut();
        if over_event && over_event_detail {
            let mut detail = self.event_detail.scroll.borrow_mut();
            if detail.state.scroll_rows(forward, row, rows) {
                detail.offset = detail.state.position();
                true
            } else {
                drop(viewport);
                drop(detail);
                let selected = self.event_list.cursor;
                self.move_event_selection(forward);
                if wheel && self.event_list.cursor != selected {
                    self.event_detail.scroll.borrow_mut().entry = Some(forward);
                }
                false
            }
        } else if over_event {
            self.event_list.scroll.set(if forward {
                self.event_list.scroll.get().saturating_add(1)
            } else {
                self.event_list.scroll.get().saturating_sub(1)
            });
            true
        } else {
            let mut consumed = false;
            for _ in 0..rows {
                if !viewport.task_details_scroll.scroll(forward, row) {
                    break;
                }
                consumed = true;
            }
            consumed
        }
    }

    fn navigate_active_panel(&mut self, code: KeyCode) {
        match self.active_panel {
            Panel::TaskList => match code {
                KeyCode::Up => self.move_selection(false),
                KeyCode::Down => self.move_selection(true),
                KeyCode::PageUp => self.page_selection(false),
                KeyCode::PageDown => self.page_selection(true),
                KeyCode::Home | KeyCode::End => {
                    let indices = self.selectable_indices();
                    let selected = if code == KeyCode::Home {
                        indices.first()
                    } else {
                        indices.last()
                    };
                    self.select(selected.copied().unwrap_or(usize::MAX));
                }
                _ => unreachable!("list navigation code"),
            },
            Panel::EventList => self.navigate_event(code),
            panel @ (Panel::TaskDetails | Panel::EventDetail) => {
                self.navigate_event_detail(panel, code)
            }
            Panel::Prompt => {}
        }
    }

    /// Up, down and page keys in the event detail change events at its boundaries like the wheel.
    fn navigate_event_detail(&mut self, panel: Panel, code: KeyCode) {
        if panel == Panel::EventDetail {
            if self.shown_terminal().is_some() {
                return;
            }
            self.prepare_event_detail_scroll();
        }
        let forward = matches!(code, KeyCode::Down | KeyCode::PageDown | KeyCode::End);
        let edge = matches!(code, KeyCode::Home | KeyCode::End);
        let (row, rows) = {
            let viewport = &mut *self.viewport.borrow_mut();
            let mut detail = self.event_detail.scroll.borrow_mut();
            let page = if panel == Panel::TaskDetails {
                viewport.task_details.height
            } else {
                viewport.event_text.area.height
            };
            let state = if panel == Panel::TaskDetails {
                &mut viewport.task_details_scroll
            } else {
                &mut detail.state
            };
            let row = state.content_top;
            let rows = if matches!(code, KeyCode::PageUp | KeyCode::PageDown) {
                usize::from(page.max(1))
            } else {
                1
            };
            if edge {
                while state.scroll_window(forward, row) {}
            } else if panel == Panel::TaskDetails {
                state.scroll_rows(forward, row, rows);
            }
            if panel == Panel::EventDetail {
                detail.offset = detail.state.position();
            }
            (row, rows)
        };
        if panel == Panel::EventDetail && !edge {
            self.scroll_content_rows(forward, true, row, rows, true);
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
        let next = if forward {
            current.saturating_add(1).min(visible.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        let selected = visible[next];
        self.select(selected);
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
        if self.task_entry_pending {
            self.enter_task();
        } else {
            self.prepare_prompt();
        }
        None
    }
    /// Stops the selected task, keeping a running turn's prompt to return to the input.
    fn stop_from_input(&mut self) -> Option<Action> {
        if !self.can_stop_selected() {
            return None;
        }
        let session = self.selected_row()?;
        let id = session.id.clone();
        self.stopping_prompt = (session.status == SessionStatus::Working
            && self.event_session.as_ref() == Some(&id))
        .then(|| {
            self.event_panel
                .iter()
                .rev()
                .find(|event| event.kind == crate::event::Kind::User)
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
        if self.selected_draft.is_some() {
            return Some(Action::Unsupported("Rename"));
        }
        let session = self.selected_row()?;
        if !self.selected_capabilities().rename {
            return Some(Action::Unsupported("Rename"));
        }
        let id = session.id.clone();
        let title = session.display_title().to_owned();
        self.rename.set_text(&title);
        self.rename_target = Some(id);
        self.set_active_panel(Panel::TaskDetails);
        self.name_target = None;
        self.edit_search(Panel::TaskList, search_input::SearchInput::close);
        None
    }

    fn selected_capabilities(&self) -> Capabilities {
        self.selected_id()
            .and_then(|key| self.capabilities.get(&key.provider))
            .copied()
            .unwrap_or_default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.kill_buffer.begin_stroke();
        let focus = self.task_focus();
        let action = self.handle_key_event(key);
        if self.task_focus() != focus {
            self.set_active_panel(self.active_panel);
        }
        self.finish_panel_action(action)
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Option<Action> {
        let key = line_edit::keymap::normalize_control_code(key);
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Esc && self.exit_image_zoom() {
            return None;
        }
        if let Some(prefix) = self.window_prefix.take() {
            if key.modifiers.is_empty()
                && let KeyCode::Char(direction @ ('h' | 'j' | 'k' | 'l')) = key.code
            {
                self.move_panel(direction);
                return None;
            }
            if (key.modifiers - KeyModifiers::SHIFT).is_empty()
                && let KeyCode::Char(command @ ('_' | '|' | '=')) = key.code
            {
                match command {
                    '=' => self.clear_panel_zoom(),
                    '|' if self.active_panel != Panel::Prompt => {
                        self.height_zoom = Some(self.active_panel);
                    }
                    '_' if self.active_panel != Panel::Prompt
                        && !(super::render::ScreenLayout::for_width(
                            self.viewport.borrow().area.width,
                        ) == super::render::ScreenLayout::Small
                            && matches!(
                                self.active_panel,
                                Panel::TaskList | Panel::TaskDetails
                            )) =>
                    {
                        self.width_zoom = Some(self.active_panel);
                    }
                    _ => {}
                }
                return None;
            }
            let first = self.handle_panel_key(prefix);
            let second = self.handle_panel_key(key);
            return match (first, second) {
                (Some(first), Some(second)) => Some(Action::Batch(vec![first, second])),
                (action, None) | (None, action) => action,
            };
        }
        if key.code == KeyCode::Char('w')
            && key.modifiers == KeyModifiers::CONTROL
            && !self.focused_editor_has_text()
        {
            self.window_prefix = Some(key);
            return None;
        }
        self.handle_panel_key(key)
    }

    fn focused_editor_has_text(&self) -> bool {
        if self.help || self.confirm_dialog.is_some() || self.frozen_draft() {
            return false;
        }
        if self.active_panel == Panel::TaskDetails
            && let Some(editor) = self.selector.as_ref().and_then(Selector::branch_editor)
        {
            return !editor.is_empty();
        }
        if self.active_panel == Panel::TaskDetails
            && let Some(editor) = self.title_editor()
        {
            return !editor.is_empty();
        }
        if self.search_focused() {
            return self
                .panel_search(self.active_panel)
                .is_some_and(|search| !search.text().is_empty());
        }
        if self.active_panel == Panel::Prompt
            && let Some(question) = self.active_question()
            && let Some(editor) = question.focused_editor()
        {
            return !editor.is_empty();
        }
        self.prompt_focused()
            && self.input_disabled().is_none()
            && !self.input_box.editor.is_empty()
    }

    fn handle_panel_key(&mut self, key: KeyEvent) -> Option<Action> {
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
        if self.active_panel == Panel::TaskList
            && key.code == KeyCode::Char('l')
            && key.modifiers == KeyModifiers::CONTROL
        {
            return None;
        }
        if self.frozen_draft() {
            return if key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                Some(Action::Quit)
            } else {
                None
            };
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
        if self.selected_draft.is_some()
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return Some(Action::Quit);
        }
        if self.active_panel == Panel::TaskDetails
            && self.title_editing()
            && self.selector.is_none()
        {
            match key.code {
                KeyCode::Esc | KeyCode::Char('c')
                    if key.code == KeyCode::Esc
                        || key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.cancel_name();
                }
                KeyCode::Enter if !self.rename.text().trim().is_empty() => {
                    let first_name = self.name_target.is_some()
                        && self
                            .selected_draft()
                            .is_some_and(|draft| draft.name.is_none());
                    let action = self.save_title();
                    return if first_name {
                        self.advance_draft(false)
                    } else {
                        action
                    };
                }
                _ => {
                    self.rename.handle_key(key, &mut self.kill_buffer);
                }
            }
            return None;
        }
        if key.code == KeyCode::Char('/')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
            && !matches!(
                self.active_panel,
                Panel::TaskList | Panel::EventList | Panel::EventDetail
            )
            && !self.prompt_focused()
            && self.title_editor().is_none()
            && !self.search_focused()
        {
            return None;
        }
        if self.prompt_focused() && self.input_disabled().is_none() && !self.search_focused() {
            if key.code == KeyCode::Esc && self.selected_draft.is_some() {
                self.confirm_draft_delete();
                return None;
            }
            // A draft Enter opens its first missing field or submits the prompt.
            if key.code == KeyCode::Enter
                && !key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                && let Some(draft) = self.selected_draft()
            {
                if !draft.working && draft.job.is_none() && !self.input_box.editor.is_empty() {
                    return self.start_branch_step();
                }
                return None;
            }
            self.input_box.history = self.prompt_history.clone();
            return match self.input_box.handle_key(key, &mut self.kill_buffer) {
                Some(prompt_box::PromptAction::Quit) => Some(Action::Quit),
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
        if self.active_panel == Panel::TaskList
            && self.confirm_dialog.is_none()
            && !self.help
            && !self.search_focused()
            && self.selected_row().is_some_and(|session| {
                !session.can_resume()
                    && !(session.id.provider == ProviderId::Codex && key.code == KeyCode::Char('a'))
            })
            && plain
            && matches!(
                key.code,
                KeyCode::Backspace | KeyCode::Char('f' | 'r' | 'x' | 'a')
            )
        {
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
                        _ => unreachable!("subagent session key is gated above"),
                    })
                });
        }
        self.feedback = None;
        if let Some(dialog) = &mut self.confirm_dialog {
            if plain {
                match key.code {
                    KeyCode::Esc => {
                        self.cancel_confirmation();
                    }
                    KeyCode::Enter => {
                        if dialog.confirm {
                            let dialog =
                                self.confirm_dialog.take().expect("confirm dialog is open");
                            return self.accept_confirmation(dialog);
                        }
                        self.cancel_confirmation();
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                        dialog.confirm = !dialog.confirm;
                    }
                    // Each item's number accepts it at once.
                    KeyCode::Char('1') => {
                        self.cancel_confirmation();
                    }
                    KeyCode::Char('2') => {
                        let dialog = self.confirm_dialog.take().expect("confirm dialog is open");
                        return self.accept_confirmation(dialog);
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
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        let active = self.active_panel;
        if self.search_focused() {
            let mut kills = std::mem::take(&mut self.kill_buffer);
            let handled = self.edit_search(active, |search| search.handle_key(key, &mut kills));
            self.kill_buffer = kills;
            if handled {
                return None;
            }
        }
        if self.panel_search(active).is_some() {
            if plain && key.code == KeyCode::Char('/') {
                self.edit_search(active, search_input::SearchInput::focus);
                return None;
            }
            if key.code == KeyCode::Esc
                && self
                    .panel_search(active)
                    .is_some_and(search_input::SearchInput::applied)
            {
                self.edit_search(active, search_input::SearchInput::close);
                return None;
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
        if plain
            && self.active_panel == Panel::EventDetail
            && !self.terminal_requested()
            && key.code == KeyCode::Char(' ')
        {
            self.event_detail.mode = match self.event_detail.mode {
                super::task_ui::EventDetailMode::Event => {
                    super::task_ui::EventDetailMode::Transcript
                }
                super::task_ui::EventDetailMode::Transcript => {
                    super::task_ui::EventDetailMode::Event
                }
                super::task_ui::EventDetailMode::Terminal => {
                    unreachable!("terminal input handled above")
                }
            };
            self.cancel_event_detail_selection();
            self.edit_search(Panel::EventDetail, search_input::SearchInput::close);
            self.event_detail.scroll.borrow_mut().offset = 0;
            self.viewport.borrow_mut().event_detail_layout = None;
            self.refresh_transcript();
            return None;
        }
        if plain && key.code == KeyCode::Enter && self.active_panel == Panel::EventList {
            self.jump_event_agent();
            return None;
        }
        if plain
            && key.code == KeyCode::Char('p')
            && (self.active_panel == Panel::EventList
                || self.active_panel == Panel::EventDetail && !self.terminal_requested())
        {
            return self.toggle_event_pin();
        }
        if let Some(code) = super::navigation::list_navigation(key) {
            self.navigate_active_panel(code);
            return None;
        }
        if plain && key.code == KeyCode::Char('t') && self.active_panel == Panel::EventList {
            self.set_event_sort(match self.event_list.sort {
                EventSort::Time => EventSort::Duration,
                EventSort::Duration => EventSort::Time,
            });
            return self
                .selected_id()
                .cloned()
                .map(|id| Action::PersistSorting {
                    id,
                    sorting: self.event_list.sort,
                });
        }
        if plain
            && key.code == KeyCode::Esc
            && self.active_panel == Panel::TaskDetails
            && self.selected_draft.is_some()
        {
            self.confirm_draft_delete();
            return None;
        }
        // A panel that did not handle a key never borrows the task list's bindings.
        if self.active_panel != Panel::TaskList {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        if !plain {
            return None;
        }
        if self.selected_draft.is_some() && matches!(key.code, KeyCode::Char('f' | 'a' | 'x' | 'r'))
        {
            return Some(Action::Unsupported(match key.code {
                KeyCode::Char('f') => "Fork",
                KeyCode::Char('a') => "Archive",
                KeyCode::Char('x') => "Stop",
                KeyCode::Char('r') => "Rename",
                _ => unreachable!("draft session key is gated above"),
            }));
        }
        if key.code == KeyCode::Esc && self.selected_draft.is_some() {
            self.confirm_draft_delete();
            return None;
        }
        match key.code {
            KeyCode::Enter => return self.activate(),
            KeyCode::Esc => self.select_previous_task(),
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
                    self.confirm_draft_delete();
                    return None;
                }
                if let Some(session) = self.selected_row() {
                    if !self.selected_capabilities().delete {
                        return Some(Action::Unsupported("Delete"));
                    }
                    self.confirm_dialog = Some(ConfirmDialog {
                        action: ConfirmAction::Delete(session.id.clone()),
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
            KeyCode::Char('p') => {
                let id = self.selected_id()?.clone();
                if self.status_filter == 0 && !self.can_pin(self.selected) {
                    return Some(Action::Unsupported("Pin"));
                }
                let pinned = !self.is_pinned(self.selected);
                return Some(Action::TogglePin {
                    id,
                    pinned,
                    tab: self.status_filter,
                });
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
pub(super) use super::section_scroll::curtain_target;

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;
