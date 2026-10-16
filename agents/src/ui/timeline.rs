//! The time axis the usage chart and the activity strip share: each cell holds two bins, its left
//! and right braille dot columns, and the right edge of an area is the bin holding now.

use super::View;
use crate::usage::{BIN_SECONDS, bin_of};
use chrono::{DateTime, Utc};
use ratatui::layout::Rect;

/// Bins one cell holds, one per dot column.
pub(super) const CELL_BINS: i64 = 2;

/// The left and right bins of the cell at `x` in `area`, whose last cell holds `now`. Cells cover
/// fixed bin pairs, so a cell keeps its bins as time passes.
pub(super) fn cell_bins(area: Rect, x: u16, now: DateTime<Utc>) -> [i64; 2] {
    let now = bin_of(now);
    let last = now - now.rem_euclid(CELL_BINS) + CELL_BINS - 1;
    let right = last - i64::from(area.right() - 1 - x) * CELL_BINS;
    [right - 1, right]
}

/// Seconds an area `width` cells wide spans.
pub(crate) fn timeline_seconds(width: u16) -> i64 {
    i64::from(width) * CELL_BINS * BIN_SECONDS
}

impl View {
    /// Time until the timeline moves to its next bin, while it shows anything.
    pub(crate) fn timeline_redraw_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        if self.activity.is_empty() && !self.shows_usage() {
            return None;
        }
        let next = (bin_of(now) + 1) * BIN_SECONDS * 1000;
        u64::try_from(next - now.timestamp_millis())
            .ok()
            .map(std::time::Duration::from_millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_keep_fixed_bin_pairs_ending_at_now() {
        let area = Rect::new(2, 0, 3, 1);
        let now = DateTime::from_timestamp(10 * BIN_SECONDS, 0).unwrap();
        assert_eq!(cell_bins(area, 4, now), [10, 11]);
        assert_eq!(cell_bins(area, 2, now), [6, 7]);
        let later = DateTime::from_timestamp(11 * BIN_SECONDS, 0).unwrap();
        assert_eq!(cell_bins(area, 4, later), [10, 11]);
    }
}
