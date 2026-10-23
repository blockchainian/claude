//! Active tasks' stacked token usage, drawn in braille behind the task list: each dot column is one
//! bin, rising from the list's bottom as high as the bin's tokens, and the list's rows cover it.
//! The chart looks at most half as tall as it is wide, so a tall list keeps it to its bottom part.

use super::View;
use super::timeline::cell_bins;
use crate::providers::SessionKey;
use crate::usage::Bin;
use chrono::{DateTime, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::collections::BTreeMap;

/// Dot rows one cell holds.
const CELL_DOTS: u16 = 4;

/// The percentile of bin totals that fills the chart.
const TOP_PERCENTILE: usize = 95;

/// The braille bits of a cell's left and right dot columns, bottom to top.
const DOT_BITS: [[u8; 4]; 2] = [[0x40, 0x04, 0x02, 0x01], [0x80, 0x20, 0x10, 0x08]];

impl View {
    pub(crate) fn set_context_usage(&mut self, id: &SessionKey, percent: Option<u64>) {
        self.context_usage = percent.map(|percent| (id.clone(), percent));
    }

    pub(super) fn selected_context_percent(&self) -> Option<u64> {
        let session = self.selected_row()?;
        let (id, percent) = self.context_usage.as_ref()?;
        (*id == session.id).then_some(*percent)
    }

    #[cfg(test)]
    pub(crate) fn set_usage(&mut self, id: &SessionKey, bins: BTreeMap<i64, Bin>) {
        assert_eq!(self.selected_row().map(|session| &session.id), Some(id));
        self.set_usage_totals(bins, BTreeMap::new());
    }

    pub(crate) fn usage_width(&self, area: Rect) -> u16 {
        self.center_layout(area).usage_chart.width
    }

    pub(crate) fn set_usage_totals(
        &mut self,
        total: BTreeMap<i64, Bin>,
        others: BTreeMap<i64, Bin>,
    ) {
        self.usage = total;
        self.other_usage = others;
    }

    pub(super) fn shows_usage(&self) -> bool {
        self.usage.values().any(|bin| total(bin) != 0)
    }

    /// Draws active tasks' stacked token usage in the blank rows below the list's content.
    pub(super) fn render_usage_chart(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        if !self.shows_usage() {
            return;
        }
        let max = chart_top(self.usage.values().map(total).collect());
        let height = chart_rows(area, self.cell_aspect) * CELL_DOTS;
        let dots = |bin: i64| {
            let all = self.usage.get(&bin).map_or(0, total);
            let others = self.other_usage.get(&bin).map_or(0, total);
            (
                dot_height(all, max, height),
                dot_height(others, max, height),
            )
        };
        let content_bottom = (area.top()..area.bottom())
            .rev()
            .find(|&y| (area.left()..area.right()).any(|x| !is_blank(&buf[(x, y)])))
            .map_or(area.top(), |y| y + 1);
        for y in content_bottom..area.bottom() {
            let floor = (area.bottom() - 1 - y) * CELL_DOTS;
            for x in area.left()..area.right() {
                let mut bits = 0;
                let mut other_dots = 0;
                let mut selected_dots = 0;
                for (&bin, column) in cell_bins(area, x, now).iter().zip(DOT_BITS) {
                    let (all, others) = dots(bin);
                    let filled = usize::from(all.saturating_sub(floor).min(CELL_DOTS));
                    let lower = usize::from(others.saturating_sub(floor).min(CELL_DOTS));
                    bits |= column[..filled].iter().fold(0, |bits, bit| bits | bit);
                    other_dots += lower;
                    selected_dots += filled - lower;
                }
                if bits != 0 {
                    let symbol =
                        char::from_u32(0x2800 + u32::from(bits)).expect("usage chart braille cell");
                    let style = if self.use_colors && selected_dots >= other_dots {
                        super::style::assistant_style()
                    } else {
                        super::style::timeline_style(self.use_colors)
                    };
                    buf[(x, y)].set_char(symbol).set_style(style);
                }
            }
        }
    }
}

fn total(usage: &Bin) -> u64 {
    usage.input + usage.output
}

/// A cell nothing has drawn on since the frame was cleared.
fn is_blank(cell: &ratatui::buffer::Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset
}

/// Rows the chart fills in `area`, whose cells are `cell_aspect` times taller than wide: as many
/// as look no taller than half the area's width.
fn chart_rows(area: Rect, cell_aspect: f64) -> u16 {
    let half_width = (f64::from(area.width) / 2.0 / cell_aspect).round() as u16;
    area.height.min(half_width)
}

/// The tokens that fill the chart: the 95th percentile of the bins' totals, so a few outsized bins
/// fill it and cut off rather than flatten the rest.
fn chart_top(mut totals: Vec<u64>) -> u64 {
    if totals.is_empty() {
        return 0;
    }
    let rank = (totals.len() * TOP_PERCENTILE).div_ceil(100) - 1;
    *totals.select_nth_unstable(rank).1
}

/// Dots `tokens` rises to in a chart `height` dots tall whose top is `max`; any usage shows one.
fn dot_height(tokens: u64, max: u64, height: u16) -> u16 {
    if tokens == 0 || max == 0 {
        return 0;
    }
    if height == 0 {
        return 0;
    }
    let dots = (u128::from(tokens) * u128::from(height)).div_ceil(u128::from(max));
    u16::try_from(dots).unwrap_or(height).clamp(1, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_scale_linearly_cut_off_at_the_top_and_never_hide_usage() {
        assert_eq!(dot_height(0, 100, 40), 0);
        assert_eq!(dot_height(1, 100, 40), 1);
        assert_eq!(dot_height(50, 100, 40), 20);
        assert_eq!(dot_height(100, 100, 40), 40);
        assert_eq!(dot_height(1_000, 100, 40), 40);
    }

    #[test]
    fn the_top_leaves_out_the_busiest_five_percent() {
        let mut totals: Vec<u64> = (1..=100).collect();
        totals[99] = 1_000_000;
        assert_eq!(chart_top(totals), 95);
        assert_eq!(chart_top(vec![7]), 7);
        assert_eq!(chart_top(Vec::new()), 0);
    }

    #[test]
    fn the_chart_looks_no_taller_than_half_its_width() {
        // Half of 70 cells of 7 pixels is as tall as 12 rows of 21 pixels.
        assert_eq!(chart_rows(Rect::new(0, 0, 70, 59), 3.0), 12);
        assert_eq!(chart_rows(Rect::new(0, 0, 70, 60), 3.0), 12);
        // A list shorter than that gives the chart all its rows.
        assert_eq!(chart_rows(Rect::new(0, 0, 94, 10), 3.0), 10);
    }
}
