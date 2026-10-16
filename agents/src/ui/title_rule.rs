//! A details title starting after a dim rule cell and a space.

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

pub(super) fn title_rule(title: &str, width: usize, elapsed: Option<&str>) -> Line<'static> {
    let elapsed_width = elapsed.map_or(0, |text| text.width() + " · ".width());
    let mut title = super::line_truncation::truncate_line_with_ellipsis_if_overflow(
        Line::from(Span::styled(title.to_owned(), Style::new().not_dim())),
        width.saturating_sub(4).saturating_sub(elapsed_width),
    );
    for span in &mut title.spans {
        span.style = Style::new().not_dim();
    }
    if let Some(elapsed) = elapsed {
        title.spans.extend([" · ".dim(), elapsed.to_owned().dim()]);
    }
    title.spans.insert(0, "─ ".dim());
    title.spans.push(" ".not_dim());
    let title = super::line_truncation::truncate_line_to_width(title, width);
    let remaining = width.saturating_sub(title.width());
    let mut spans = title.spans;
    spans.push(Span::styled("─".repeat(remaining), Style::new().dim()));
    Line::from(spans)
}
