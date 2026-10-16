//! Whether any task worked, dotted on the task log's top rule below the task list: each dot column
//! of the shared timeline is one bin, dotted in Markdown's inline code color when a task worked
//! in it.

use super::View;
use super::timeline::cell_bins;
use chrono::{DateTime, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use std::collections::BTreeSet;

/// The braille bits of a cell's left and right dot columns: one dot each, the second from the top,
/// nearest the rule's height.
const DOT_BITS: [u8; 2] = [0x02, 0x10];

impl View {
    /// Takes the bins some task worked in.
    pub(crate) fn set_activity(&mut self, bins: BTreeSet<i64>) {
        self.activity = bins;
    }

    /// Dots the bins some task worked in on the rule row `area`, around the log's jump button.
    pub(super) fn render_activity_strip(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        let jump = self.viewport.borrow().log_jump;
        for x in area.left()..area.right() {
            if jump.contains(Position::new(x, area.y)) {
                continue;
            }
            let bits = cell_bins(area, x, now)
                .iter()
                .zip(DOT_BITS)
                .filter(|(bin, _)| self.activity.contains(bin))
                .fold(0, |bits, (_, column)| bits | column);
            if bits != 0 {
                // A fresh cell, so the rule's dim style does not carry into the dots.
                let cell = &mut buf[(x, area.y)];
                cell.reset();
                cell.set_char(char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '));
                if self.use_colors {
                    cell.set_fg(crate::theme::inline_code_color());
                }
            }
        }
    }
}
