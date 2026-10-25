//! Task detail title layout.

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

pub(super) const SEPARATOR: &str = " · ";

/// A centered status and title with a time at the shared right column.
pub(super) fn centered_title(
    title: &str,
    indicator: Span<'static>,
    width: usize,
    time: Option<&str>,
    right_padding: usize,
) -> Line<'static> {
    let time_width = time.map_or(0, |time| time.width() + 1 + right_padding);
    let available = width.saturating_sub(time_width * 2);
    let title = super::line_truncation::truncate_line_with_ellipsis_if_overflow(
        Line::from(vec![indicator, Span::raw(" "), title.to_owned().dim()]),
        available,
    );
    let left = width.saturating_sub(title.width()) / 2;
    let mut spans = vec![Span::raw(" ".repeat(left))];
    let used = left + title.width();
    spans.extend(title.spans);
    if let Some(time) = time.filter(|time| used + 1 + time.width() + right_padding <= width) {
        spans.push(Span::raw(
            " ".repeat(width - used - time.width() - right_padding),
        ));
        spans.push(time.to_owned().dim());
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_title_keeps_its_title_centered_with_a_time_at_the_end() {
        for width in 0..=40 {
            let line = centered_title("Bash", "●".green(), width, Some("35s"), 0);
            assert!(line.width() <= width, "{width}");
            let text = line.to_string();
            assert!(!text.chars().any(|c| ('\u{2500}'..='\u{257f}').contains(&c)));
            if width >= 16 {
                assert!(text.ends_with(" 35s"), "{text}");
                assert_eq!(
                    text.chars().take_while(|ch| *ch == ' ').count(),
                    (width - 6) / 2
                );
            }
        }
    }
}
