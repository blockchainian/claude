//! The selected task's token usage, drawn in block elements on the task log's top rule: output
//! rises above the rule, input hangs below it, and each column's color tells its total.

use super::View;
use crate::providers::SessionKey;
use crate::usage::{BIN_SECONDS, Bin, bin_of};
use chrono::{DateTime, Local, TimeZone, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style, Stylize};
use std::collections::BTreeMap;

/// Heights each side of the rule has: its half of the rule's row, then four eighths of the
/// neighboring row.
const LEVELS: u64 = 5;

/// Bins one column sums.
const CELL_BINS: i64 = 2;

/// Output above the rule, by height past the rule's half: bottom-anchored eighths.
const RISING: [char; 4] = ['▁', '▂', '▃', '▄'];

/// Input below the rule, by height past the rule's half: the unfilled bottom part, drawn
/// reversed so the filled top part takes the column's color.
const HANGING: [char; 4] = ['▇', '▆', '▅', '▄'];

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

        let mut columns: BTreeMap<i64, Bin> = BTreeMap::new();
        for (bin, usage) in bins {
            let column = columns.entry(cell_right(*bin, end)).or_default();
            column.input += usage.input;
            column.output += usage.output;
        }
        let side_max = columns
            .values()
            .map(|column| column.input.max(column.output))
            .max()
            .unwrap_or(0);
        let total_max = columns
            .values()
            .map(|column| column.input + column.output)
            .max()
            .unwrap_or(0);
        let shades = crate::theme::accent_count();
        for x in 0..area.width {
            let right = end - i64::from(area.width - 1 - x) * CELL_BINS;
            let Some(column) = columns.get(&right) else {
                continue;
            };
            let style = if self.use_colors {
                let shade = shade(column.input + column.output, total_max, shades);
                // Slot 0 contrasts most with the background, so the busiest columns take it.
                Style::default().fg(crate::theme::accent(shades - shade))
            } else {
                Style::default()
            };
            let output = level(column.output, side_max);
            let input = level(column.input, side_max);
            let mut draw = |row: u16, symbol: char, style: Style| {
                buf[(area.x + x, area.y + row)]
                    .set_char(symbol)
                    .set_style(style);
            };
            match (output > 0, input > 0) {
                (true, true) => draw(1, '█', style),
                (true, false) => draw(1, '▀', style),
                (false, true) => draw(1, '▄', style),
                (false, false) => {}
            }
            if output > 1 {
                draw(0, RISING[output as usize - 2], style);
            }
            if input > 1 {
                draw(
                    2,
                    HANGING[input as usize - 2],
                    style.add_modifier(Modifier::REVERSED),
                );
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

/// Height of `tokens` on a log scale up to `max`; any usage shows at least the rule's half.
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
        assert_eq!(level(100_000, 100_000), 5);
        // 300 tokens is half the doublings to 100k, so three of five heights.
        assert_eq!(level(300, 100_000), 3);
    }

    #[test]
    fn cells_pair_bins_ending_at_the_right_edge() {
        assert_eq!(cell_right(10, 10), 10);
        assert_eq!(cell_right(9, 10), 10);
        assert_eq!(cell_right(8, 10), 8);
    }
}
