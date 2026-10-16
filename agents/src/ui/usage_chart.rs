//! The selected task's token usage, drawn in braille behind the task list: each dot column is one
//! bin, rising from the list's bottom as high as the bin's tokens, and the list's rows cover it.

use super::View;
use crate::providers::SessionKey;
use crate::usage::{Bin, bin_of};
use chrono::{DateTime, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use std::collections::BTreeMap;

/// Dot rows one cell holds.
const CELL_DOTS: u16 = 4;

/// Bins one cell holds, one per dot column.
const CELL_BINS: i64 = 2;

/// The braille bits of a cell's left and right dot columns, bottom to top.
const DOT_BITS: [[u8; 4]; 2] = [[0x40, 0x04, 0x02, 0x01], [0x80, 0x20, 0x10, 0x08]];

impl View {
    pub(crate) fn set_usage(&mut self, id: &SessionKey, bins: BTreeMap<i64, Bin>) {
        self.usage = Some((id.clone(), bins));
    }

    pub(crate) fn usage_session(&self) -> Option<&SessionKey> {
        self.usage.as_ref().map(|(id, _)| id)
    }

    /// The selected task's usage bins, when the view holds them.
    fn selected_usage(&self) -> Option<&BTreeMap<i64, Bin>> {
        let session = self.selected_row()?;
        let (id, bins) = self.usage.as_ref()?;
        (*id == session.id).then_some(bins)
    }

    /// Draws the selected task's usage into the blank rows below the list's content in `area`,
    /// ending at now for a live task and at its last usage otherwise.
    pub(super) fn render_usage_chart(&self, area: Rect, buf: &mut Buffer, now: DateTime<Utc>) {
        let (Some(session), Some(bins)) = (self.selected_row(), self.selected_usage()) else {
            return;
        };
        let Some(&last) = bins.keys().next_back() else {
            return;
        };
        let end = cell_right(if session.status == crate::model::SessionStatus::Inactive {
            last
        } else {
            bin_of(now)
        });
        let max = bins.values().map(total).max().unwrap_or(0);
        let height = area.height * CELL_DOTS;
        let dots = |bin: i64| {
            bins.get(&bin)
                .map_or(0, |usage| dot_height(total(usage), max, height))
        };
        let content_bottom = (area.top()..area.bottom())
            .rev()
            .find(|&y| (area.left()..area.right()).any(|x| !is_blank(&buf[(x, y)])))
            .map_or(area.top(), |y| y + 1);
        for y in content_bottom..area.bottom() {
            let floor = (area.bottom() - 1 - y) * CELL_DOTS;
            for x in area.left()..area.right() {
                let right = end - i64::from(area.right() - 1 - x) * CELL_BINS;
                let bits = [right - 1, right]
                    .iter()
                    .zip(DOT_BITS)
                    .map(|(&bin, column)| {
                        let filled = usize::from(dots(bin).saturating_sub(floor).min(CELL_DOTS));
                        column[..filled].iter().fold(0, |bits, bit| bits | bit)
                    })
                    .fold(0, |bits, column| bits | column);
                if bits != 0 {
                    let symbol = char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ');
                    buf[(x, y)]
                        .set_char(symbol)
                        .set_style(Style::default().dim());
                }
            }
        }
    }

    /// Time until the live chart moves to its next bin.
    pub(crate) fn usage_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        self.selected_usage()?;
        if self.selected_row()?.status == crate::model::SessionStatus::Inactive {
            return None;
        }
        let next = (bin_of(now) + 1) * crate::usage::BIN_SECONDS * 1000;
        u64::try_from(next - now.timestamp_millis())
            .ok()
            .map(std::time::Duration::from_millis)
    }
}

fn total(usage: &Bin) -> u64 {
    usage.input + usage.output
}

/// A cell nothing has drawn on since the frame was cleared.
fn is_blank(cell: &ratatui::buffer::Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset
}

/// The right bin of the cell holding `bin`; cells cover fixed bin pairs, so a cell keeps its
/// bins as time passes.
fn cell_right(bin: i64) -> i64 {
    bin - bin.rem_euclid(CELL_BINS) + CELL_BINS - 1
}

/// Dots `tokens` rises to in a chart `height` dots tall whose top is `max`; any usage shows one.
fn dot_height(tokens: u64, max: u64, height: u16) -> u16 {
    if tokens == 0 || max == 0 {
        return 0;
    }
    let dots = (u128::from(tokens) * u128::from(height)).div_ceil(u128::from(max));
    u16::try_from(dots).unwrap_or(height).min(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_scale_linearly_and_never_hide_usage() {
        assert_eq!(dot_height(0, 100, 40), 0);
        assert_eq!(dot_height(1, 100, 40), 1);
        assert_eq!(dot_height(50, 100, 40), 20);
        assert_eq!(dot_height(100, 100, 40), 40);
    }

    #[test]
    fn cells_pair_fixed_bins() {
        assert_eq!(cell_right(10), 11);
        assert_eq!(cell_right(11), 11);
        assert_eq!(cell_right(-1), -1);
    }
}
