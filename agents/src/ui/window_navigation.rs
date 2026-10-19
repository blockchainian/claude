use super::{Action, Panel, View};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl View {
    /// Panel interactions, editors and attention requests share this activation entry point.
    pub(super) fn set_active_panel(&mut self, panel: Panel) {
        if panel != self.active_panel
            && self.active_panel == Panel::Details
            && self.title_editor().is_some()
            && let Some(action) = self.save_title()
        {
            self.pending_actions.push(action);
        }
        if panel != self.active_panel
            && (self.searching || self.log_search.editing || self.log_detail_search.editing)
            && let Some(action) = self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        {
            self.pending_actions.push(action);
        }
        self.active_panel = panel;
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
                self.close_selector();
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
            (Panel::Output, v.terminal)
        } else {
            (Panel::Output, v.log_detail)
        };
        let panels = [
            (Panel::TaskList, v.list),
            (Panel::Details, v.details),
            (Panel::LogList, v.log),
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
