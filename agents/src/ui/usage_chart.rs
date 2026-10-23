//! Active tasks' stacked token usage, drawn in braille behind the task list: each cell combines
//! two bins, rising from the list's bottom as high as their tokens, and the list's rows cover it.
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

/// The percentile of cell totals that fills the chart.
const TOP_PERCENTILE: usize = 95;

/// The braille bits of each dot row, bottom to top.
const DOT_BITS: [u8; 4] = [0xc0, 0x24, 0x12, 0x09];

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
        let columns: Vec<_> = (area.left()..area.right())
            .map(|x| {
                let bins = cell_bins(area, x, now);
                let tokens = |usage: &BTreeMap<i64, Bin>| {
                    bins.iter()
                        .map(|bin| usage.get(bin).map_or(0, total))
                        .sum::<u64>()
                };
                (tokens(&self.usage), tokens(&self.other_usage))
            })
            .collect();
        let max = chart_top(
            columns
                .iter()
                .map(|&(tokens, _)| tokens)
                .filter(|&tokens| tokens > 0)
                .collect(),
        );
        let height = chart_rows(area, self.cell_aspect) * CELL_DOTS;
        let heights: Vec<_> = columns
            .into_iter()
            .map(|(tokens, others)| stack_heights(tokens, others, max, height))
            .collect();
        let content_bottom = (area.top()..area.bottom())
            .rev()
            .find(|&y| (area.left()..area.right()).any(|x| !is_blank(&buf[(x, y)])))
            .map_or(area.top(), |y| y + 1);
        for y in content_bottom..area.bottom() {
            let floor = (area.bottom() - 1 - y) * CELL_DOTS;
            for (x, &(all, others)) in (area.left()..area.right()).zip(&heights) {
                let filled = usize::from(all.saturating_sub(floor).min(CELL_DOTS));
                let bits = DOT_BITS[..filled].iter().fold(0, |bits, bit| bits | bit);
                if bits != 0 {
                    let symbol =
                        char::from_u32(0x2800 + u32::from(bits)).expect("usage chart braille cell");
                    let style = if self.use_colors && floor >= others {
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

/// Total dot rows and other tasks' dot rows, with the color boundary on a cell edge.
fn stack_heights(tokens: u64, others: u64, max: u64, height: u16) -> (u16, u16) {
    let selected = tokens
        .checked_sub(others)
        .expect("usage chart other tasks exceed total");
    let all = dot_height(tokens, max, height);
    if selected == 0 {
        return (all, all);
    }
    if max == 0 || height == 0 {
        return (all, 0);
    }
    let numerator = u128::from(others) * u128::from(height);
    let denominator = u128::from(max) * u128::from(CELL_DOTS);
    let cells = (numerator + denominator / 2) / denominator;
    let mut lower = (cells * u128::from(CELL_DOTS)).min(u128::from(all)) as u16;
    if selected > 0 {
        lower = lower.min(all.saturating_sub(1) / CELL_DOTS * CELL_DOTS);
    }
    if others > 0 && lower == 0 && all > CELL_DOTS {
        lower = CELL_DOTS;
    }
    (all, lower)
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

/// The tokens that fill the chart: the 95th percentile of the cells' totals, so a few outsized cells
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
    fn a_bin_without_selected_usage_is_all_other_tasks() {
        // 24 of 100 tokens fill 10 of 40 dots; rounding others to a cell edge would leave 2.
        assert_eq!(stack_heights(24, 24, 100, 40), (10, 10));
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
