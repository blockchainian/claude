use super::{Panel, View};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl View {
    /// Mouse interactions and Ctrl-W navigation share this activation entry point.
    pub(super) fn set_active_panel(&mut self, panel: Panel) {
        if panel != self.active_panel
            && (self.searching || self.log_search.editing || self.log_detail_search.editing)
        {
            self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        self.active_panel = panel;
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
