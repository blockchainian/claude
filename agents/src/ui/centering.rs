//! Places a run of cells on its panel's midline without reaching the time column.

use ratatui::layout::Rect;

/// The start column of `width` cells centered on `panel`'s midline inside `span`, the cells left
/// of the time column's gap. Text that would cross the gap moves left; text wider than `span`
/// starts at its left edge and is truncated by the caller.
pub(super) fn centered_x(panel: Rect, span: Rect, width: u16) -> u16 {
    let midline = panel.x + panel.width / 2;
    midline
        .saturating_sub(width / 2)
        .min(span.right().saturating_sub(width))
        .max(span.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: Rect = Rect::new(10, 0, 100, 1);
    const SPAN: Rect = Rect::new(10, 0, 90, 1);

    #[test]
    fn short_text_sits_on_the_panel_midline() {
        assert_eq!(centered_x(PANEL, SPAN, 20), 50);
    }

    #[test]
    fn text_reaching_the_time_column_moves_left_against_the_gap() {
        assert_eq!(centered_x(PANEL, SPAN, 80), 20);
        assert_eq!(centered_x(PANEL, SPAN, 80) + 80, SPAN.right());
    }

    #[test]
    fn text_wider_than_the_span_starts_at_its_left_edge() {
        assert_eq!(centered_x(PANEL, SPAN, 90), 10);
        assert_eq!(centered_x(PANEL, SPAN, 120), 10);
    }

    #[test]
    fn empty_span_keeps_its_edge() {
        assert_eq!(centered_x(PANEL, Rect::new(10, 0, 0, 1), 5), 10);
    }
}
