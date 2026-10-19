use super::{Panel, View};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl View {
    pub(super) fn focus(&mut self, panel: Panel) {
        if panel != self.mouse_focus
            && (self.searching || self.log_search.editing || self.log_detail_search.editing)
        {
            self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        self.mouse_focus = panel;
        self.input_box.focused = panel == Panel::Prompt
            && self.input_disabled().is_none()
            && self.title_editor().is_none();
        self.log_list_focused = panel == Panel::LogList;
        self.log_detail_focused = panel == Panel::LogDetail;
    }

    pub(super) fn move_panel(&mut self, direction: char) {
        let v = self.viewport.borrow();
        let bottom = if self.shown_terminal().is_some() {
            (Panel::Terminal, v.terminal)
        } else {
            (Panel::LogDetail, v.log_detail)
        };
        let panels = [
            (Panel::TaskList, v.list),
            (Panel::Details, v.details),
            (Panel::LogList, v.log),
            (Panel::Prompt, v.prompt),
            bottom,
        ];
        let source = if self.input_box.focused {
            Panel::Prompt
        } else {
            self.mouse_focus
        };
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
            self.focus(panel);
        }
    }
}
