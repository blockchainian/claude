use super::{Action, Panel, View, task_ui::Focus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl View {
    pub(super) fn selected_task_key(&self) -> Option<super::draft_controls::TaskKey> {
        self.selected_draft
            .map(super::draft_controls::TaskKey::Draft)
            .or_else(|| {
                self.selected_id()
                    .cloned()
                    .map(super::draft_controls::TaskKey::Session)
            })
    }

    /// Remembers the task destination even while the task list owns the keyboard.
    pub(super) fn task_focus(&self) -> Focus {
        match self.active_panel {
            Panel::TaskList => self
                .panels_task
                .as_ref()
                .and_then(|key| self.task_uis.get(key))
                .and_then(|ui| ui.focus)
                .unwrap_or_default(),
            Panel::Prompt if self.active_question().is_some() => Focus::Question,
            Panel::Prompt => Focus::Prompt,
            Panel::TaskDetails if self.selector.as_ref().is_some_and(|s| s.input_active()) => {
                Focus::Branch
            }
            Panel::TaskDetails if self.selector.is_some() => Focus::Selector,
            Panel::TaskDetails if self.title_editing() => Focus::Title,
            Panel::TaskDetails => Focus::TaskDetails,
            Panel::EventList if self.event_list.search.editing => Focus::EventSearch,
            Panel::EventList => Focus::EventList,
            Panel::EventDetail if self.event_detail.search.editing => Focus::EventDetailSearch,
            Panel::EventDetail => Focus::EventDetail,
        }
    }

    /// Enters a task selected by list browsing without replacing its parked focus.
    pub(super) fn enter_task(&mut self) -> bool {
        if !self.task_entry_pending || self.help || self.confirm_dialog.is_some() {
            return false;
        }
        self.restore_task_focus(self.task_focus())
    }

    pub(super) fn task_focus_available(&self, focus: Focus) -> bool {
        match focus {
            Focus::Prompt => self.input_disabled().is_none(),
            Focus::Question => self.active_question().is_some() || self.input_disabled().is_none(),
            _ => true,
        }
    }

    pub(super) fn input_focus(&self) -> Focus {
        if self.active_question().is_some() {
            Focus::Question
        } else {
            Focus::Prompt
        }
    }

    pub(super) fn restore_task_focus(&mut self, focus: Focus) -> bool {
        if !self.task_focus_available(focus) {
            return false;
        }
        self.task_entry_pending = false;
        let panel = match focus {
            Focus::Prompt | Focus::Question => Panel::Prompt,
            Focus::Title | Focus::Branch | Focus::Selector | Focus::TaskDetails => {
                Panel::TaskDetails
            }
            Focus::EventList | Focus::EventSearch => Panel::EventList,
            Focus::EventDetail | Focus::EventDetailSearch => Panel::EventDetail,
        };
        self.set_active_panel(panel);
        true
    }

    pub(super) fn task_search_editing(&self) -> bool {
        match self.active_panel {
            Panel::EventList => self.event_list.search.editing,
            Panel::EventDetail => self.event_detail.search.editing,
            _ => false,
        }
    }

    /// Panel interactions, editors and attention requests share this activation entry point.
    pub(super) fn set_active_panel(&mut self, panel: Panel) {
        if panel != self.active_panel
            && self.searching
            && let Some(action) = self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        {
            self.pending_actions.push(action);
        }
        // Cleanup may activate another panel; the requested destination takes precedence.
        self.active_panel = panel;
        if panel != Panel::TaskList {
            self.task_entry_pending = false;
            if let Some(key) = self.panels_task.clone() {
                let focus = self.task_focus();
                self.task_uis.entry(key).or_default().focus = Some(focus);
            }
        }
    }

    /// Saves the title and closes its editor; unchanged session names stay local.
    pub(super) fn save_title(&mut self) -> Option<Action> {
        let name = self.rename.text().trim().to_owned();
        if self.name_target.is_some() {
            if name.is_empty() {
                self.name_target = None;
                self.rename.clear();
            } else {
                self.accept_draft_name();
            }
            return None;
        }
        let id = self.rename_target.take()?;
        let unchanged = self
            .rows
            .iter()
            .find(|row| row.id == id)
            .expect("title editor session exists")
            .display_title()
            == name;
        self.rename.clear();
        self.reconcile_selection();
        (!name.is_empty() && !unchanged).then_some(Action::Rename { id, name })
    }

    pub(crate) fn take_pending_action(&mut self) -> Option<Action> {
        match self.pending_actions.len() {
            0 => None,
            1 => self.pending_actions.pop(),
            _ => Some(Action::Batch(std::mem::take(&mut self.pending_actions))),
        }
    }

    pub(super) fn finish_panel_action(&mut self, action: Option<Action>) -> Option<Action> {
        if let Some(action) = action {
            self.pending_actions.push(action);
        }
        self.take_pending_action()
    }

    pub(super) fn move_panel(&mut self, direction: char) {
        let v = self.viewport.borrow();
        let bottom = if self.shown_terminal().is_some() {
            (Panel::EventDetail, v.terminal)
        } else {
            (Panel::EventDetail, v.event_detail)
        };
        let panels = [
            (Panel::TaskList, v.list),
            (Panel::TaskDetails, v.task_details),
            (Panel::EventList, v.event_panel),
            (Panel::Prompt, v.prompt),
            bottom,
        ];
        let source = self.active_panel;
        let Some((_, from)) = panels
            .iter()
            .find(|(panel, area)| *panel == source && !area.is_empty())
        else {
            return;
        };
        // The full-width prompt sits under several panels; moving up always lands on the task list.
        if source == Panel::Prompt && direction == 'k' && !v.list.is_empty() {
            drop(v);
            self.set_active_panel(Panel::TaskList);
            return;
        }
        let horizontal = matches!(direction, 'h' | 'l');
        let axes = |r: ratatui::layout::Rect| {
            if horizontal {
                (r.x, r.right(), r.y, r.bottom())
            } else {
                (r.y, r.bottom(), r.x, r.right())
            }
        };
        let (start, end, cross_start, cross_end) = axes(*from);
        let target = panels
            .iter()
            .filter_map(|(panel, area)| {
                if *panel == source || area.is_empty() {
                    return None;
                }
                let (a, b, c, d) = axes(*area);
                // Only rectangles across a shared row or column are neighbors.
                if c >= cross_end || d <= cross_start {
                    return None;
                }
                let gap = match direction {
                    'h' | 'k' if b <= start => start - b,
                    'j' | 'l' if a >= end => a - end,
                    _ => return None,
                };
                let center_distance = (u32::from(c) + u32::from(d))
                    .abs_diff(u32::from(cross_start) + u32::from(cross_end));
                Some((*panel, (gap, center_distance)))
            })
            .min_by_key(|(_, distance)| *distance)
            .map(|(panel, _)| panel);
        drop(v);
        if let Some(panel) = target {
            self.set_active_panel(panel);
        }
    }
}
