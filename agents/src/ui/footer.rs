//! The footer row: key hints on the left, the scrolled-off section title in the middle and
//! feedback, such as a copy confirmation, on the right. A narrow layout has no middle, so
//! the title takes the right whenever no feedback shows.

use super::event_detail::EventDetailColumns;
use super::line_truncation::truncate_line_with_ellipsis_if_overflow;
use super::*;

pub(super) struct Footer {
    pub hints: Line<'static>,
    pub title: Option<(Line<'static>, EventDetailColumns)>,
    pub feedback: Option<Line<'static>>,
}

impl Footer {
    pub(super) fn render(self, area: Rect, wide: bool, buf: &mut Buffer) {
        let start = area
            .x
            .saturating_add(self.hints.width() as u16)
            .saturating_add(2);
        render::line(self.hints, area, buf);
        // Each part after the hints keeps two cells clear of the one before it.
        let mut end = area.right();
        if let Some(feedback) = self.feedback {
            let width = usize::from(area.right().saturating_sub(start));
            let feedback = truncate_line_with_ellipsis_if_overflow(feedback, width);
            let width = feedback.width() as u16;
            feedback.render(Rect::new(area.right() - width, area.y, width, 1), buf);
            if !wide {
                return;
            }
            end = (area.right() - width).saturating_sub(2);
        }
        if let Some((title, columns)) = self.title {
            let span = Rect::new(start, area.y, end.saturating_sub(start), 1);
            render_title(title, columns, span, wide, buf);
        }
    }
}

/// Draws the title within `span`: on the event detail's midline when `centered` and that
/// fits, otherwise ending at the text column so the time column stays empty.
fn render_title(
    title: Line<'static>,
    columns: EventDetailColumns,
    span: Rect,
    centered: bool,
    buf: &mut Buffer,
) {
    let end = columns.text.right().min(span.right());
    if end <= span.x {
        return;
    }
    let title = truncate_line_with_ellipsis_if_overflow(
        Line::from(title.to_string()).dim(),
        usize::from(end - span.x),
    );
    let width = title.width() as u16;
    let midline = super::centering::centered_x(columns.panel, columns.text, width);
    let x = if centered && midline >= span.x && midline + width <= span.right() {
        midline
    } else {
        end - width
    };
    title.render(Rect::new(x, span.y, width, 1), buf);
}
