//! The selected task's token usage, drawn in braille on the task log's top rule: output rises
//! above the rule, input hangs below it, and each cell's color tells the cell's total.

use super::View;
use crate::providers::SessionKey;
use crate::usage::{BIN_SECONDS, Bin, bin_of};
use chrono::{DateTime, Local, TimeZone, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use std::collections::BTreeMap;

/// Dot rows each side of the rule spans: two in the rule's row and two in its neighbor.
const LEVELS: u64 = 4;

/// Bins one cell holds, one per braille dot column.
const CELL_BINS: i64 = 2;

/// Braille dot bits of the left and right dot columns, top row first.
const DOTS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// Rule cells left plain at each end of the chart.
const RULE_ENDS: u16 = 2;

/// Cells one wheel report moves the chart.
const WHEEL_CELLS: i64 = 3;

impl View {
    /// Hands the view `id`'s usage bins; a different task starts at the newest bins.
    pub(crate) fn set_usage(&mut self, id: &SessionKey, bins: BTreeMap<i64, Bin>) {
        if self.usage.as_ref().is_none_or(|(shown, _)| shown != id) {
            self.usage_anchor = None;
        }
        self.usage = Some((id.clone(), bins));
    }

    pub(crate) fn usage_session(&self) -> Option<&SessionKey> {
        self.usage.as_ref().map(|(id, _)| id)
    }

    /// The three rows centered on `rule` the chart may draw on, inside the rule's plain ends.
    pub(super) fn usage_chart_area(rule: Rect) -> Rect {
        Rect::new(
            rule.x + RULE_ENDS,
            rule.y.saturating_sub(1),
            rule.width.saturating_sub(2 * RULE_ENDS),
            3,
        )
    }

    /// Draws the selected task's usage over `area`, whose middle row is the rule.
    pub(super) fn render_usage_chart(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        let mut viewport = self.viewport.borrow_mut();
        viewport.usage_chart = Rect::default();
        let Some(session) = self.selected_row() else {
            return;
        };
        let Some((id, bins)) = &self.usage else {
            return;
        };
        if *id != session.id || area.width == 0 || area.height < 3 {
            return;
        }
        let live = session.status != crate::model::SessionStatus::Inactive;
        let live_end = if live {
            bin_of(now)
        } else {
            bins.keys()
                .next_back()
                .copied()
                .unwrap_or_else(|| bin_of(now))
        };
        let span = i64::from(area.width) * CELL_BINS;
        let min_end = bins
            .keys()
            .next()
            .map_or(live_end, |first| (first + span - 1).min(live_end));
        let end = self
            .usage_anchor
            .map_or(live_end, |anchor| anchor.clamp(min_end, live_end));
        viewport.usage_chart = area;
        viewport.usage_live_end = live_end;
        viewport.usage_min_end = min_end;
        viewport.usage_end = end;
        drop(viewport);

        let side_max = bins
            .values()
            .map(|bin| bin.input.max(bin.output))
            .max()
            .unwrap_or(0);
        let cell_totals = |right: i64| -> u64 {
            (right - CELL_BINS + 1..=right)
                .filter_map(|bin| bins.get(&bin))
                .map(|bin| bin.input + bin.output)
                .sum()
        };
        let total_max = bins
            .keys()
            .map(|bin| cell_totals(cell_right(*bin, end)))
            .max()
            .unwrap_or(0);
        let shades = crate::theme::accent_count();
        for column in 0..area.width {
            let right = end - i64::from(area.width - 1 - column) * CELL_BINS;
            // Dots of the row above the rule, the rule's row and the row below it.
            let mut rows = [0_u32; 3];
            for (side, bin) in (right - CELL_BINS + 1..=right).enumerate() {
                let Some(bin) = bins.get(&bin) else {
                    continue;
                };
                let dots = DOTS[side];
                for level in 0..level(bin.output, side_max) {
                    match level {
                        0 => rows[1] |= dots[1],
                        1 => rows[1] |= dots[0],
                        2 => rows[0] |= dots[3],
                        _ => rows[0] |= dots[2],
                    }
                }
                for level in 0..level(bin.input, side_max) {
                    match level {
                        0 => rows[1] |= dots[2],
                        1 => rows[1] |= dots[3],
                        2 => rows[2] |= dots[0],
                        _ => rows[2] |= dots[1],
                    }
                }
            }
            let style = if self.use_colors {
                let shade = shade(cell_totals(right), total_max, shades);
                // Slot 0 contrasts most with the background, so the busiest cells take it.
                Style::default().fg(crate::theme::accent(shades - shade))
            } else {
                Style::default()
            };
            for (row, dots) in rows.into_iter().enumerate() {
                if dots == 0 {
                    continue;
                }
                let symbol = char::from_u32(0x2800 + dots).unwrap_or(' ');
                buf[(area.x + column, area.y + row as u16)]
                    .set_char(symbol)
                    .set_style(style);
            }
        }
        if self.usage_anchor.is_some() && end < live_end {
            let label = format!(" {} ", clock((end + 1) * BIN_SECONDS));
            let width = (label.len() as u16).min(area.width);
            super::render::line(
                label.dim(),
                Rect::new(area.right() - width, area.y + 1, width, 1),
                buf,
            );
        }
    }

    /// Moves the chart one wheel report toward the past or, when `forward`, toward now.
    pub(super) fn scroll_usage_chart(&mut self, forward: bool) {
        let (end, live_end, min_end) = {
            let viewport = self.viewport.borrow();
            (
                viewport.usage_end,
                viewport.usage_live_end,
                viewport.usage_min_end,
            )
        };
        let step = WHEEL_CELLS * CELL_BINS;
        let end = if forward { end + step } else { end - step };
        self.usage_anchor = (end < live_end).then(|| end.max(min_end));
    }

    /// Time until the live chart moves to its next bin.
    pub(crate) fn usage_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        if self.viewport.borrow().usage_chart.is_empty() || self.usage_anchor.is_some() {
            return None;
        }
        let session = self.selected_row()?;
        if session.status == crate::model::SessionStatus::Inactive {
            return None;
        }
        let next = (bin_of(now) + 1) * BIN_SECONDS * 1000;
        u64::try_from(next - now.timestamp_millis())
            .ok()
            .map(std::time::Duration::from_millis)
    }
}

/// The right bin of the cell holding `bin` when the chart's right cell ends at `end`.
fn cell_right(bin: i64, end: i64) -> i64 {
    end - (end - bin).div_euclid(CELL_BINS) * CELL_BINS
}

/// Dot rows for `tokens` on a log scale up to `max`; any usage shows at least one.
fn level(tokens: u64, max: u64) -> u64 {
    scaled(tokens, max, LEVELS)
}

/// The shade of a cell totaling `tokens`, from 1 for the quietest to `shades` for the busiest.
fn shade(tokens: u64, max: u64, shades: usize) -> usize {
    (scaled(tokens, max, shades as u64) as usize).clamp(1, shades)
}

fn scaled(tokens: u64, max: u64, steps: u64) -> u64 {
    if tokens == 0 || max == 0 {
        return 0;
    }
    let ratio = (tokens as f64).ln_1p() / (max as f64).ln_1p();
    ((ratio * steps as f64).ceil() as u64).clamp(1, steps)
}

fn clock(seconds: i64) -> String {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map_or_else(String::new, |at| at.format("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_a_log_scale_and_never_hide_usage() {
        assert_eq!(level(0, 100_000), 0);
        assert_eq!(level(1, 100_000), 1);
        assert_eq!(level(100_000, 100_000), 4);
        // 300 tokens is half the doublings to 100k, so two of four rows.
        assert_eq!(level(300, 100_000), 2);
    }

    #[test]
    fn cells_pair_bins_ending_at_the_right_edge() {
        assert_eq!(cell_right(10, 10), 10);
        assert_eq!(cell_right(9, 10), 10);
        assert_eq!(cell_right(8, 10), 8);
    }
}
