//! A details title starting after a dim rule cell and a space.

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

pub(super) const SEPARATOR: &str = " · ";

pub(super) fn title_rule(title: &str, width: usize, elapsed: Option<&str>) -> Line<'static> {
    let elapsed_width = elapsed.map_or(0, |text| text.width() + SEPARATOR.width());
    let mut title = super::line_truncation::truncate_line_with_ellipsis_if_overflow(
        Line::from(Span::styled(title.to_owned(), Style::new().not_dim())),
        width.saturating_sub(4).saturating_sub(elapsed_width),
    );
    for span in &mut title.spans {
        span.style = Style::new().not_dim();
    }
    if let Some(elapsed) = elapsed {
        title
            .spans
            .extend([SEPARATOR.dim(), elapsed.to_owned().dim()]);
    }
    title.spans.insert(0, "─ ".dim());
    title.spans.push(" ".not_dim());
    let title = super::line_truncation::truncate_line_to_width(title, width);
    let remaining = width.saturating_sub(title.width());
    let mut spans = title.spans;
    spans.push(Span::styled("─".repeat(remaining), Style::new().dim()));
    Line::from(spans)
}

/// A dim title rule with the title centered between equal rules, like the details titles,
/// and an optional time followed by a half line through the panel padding.
pub(super) fn centered_title_rule(
    title: &str,
    width: usize,
    time: Option<&str>,
    right_padding: usize,
) -> Line<'static> {
    let time = time.map(|time| format!(" {time}")).unwrap_or_default();
    let mut line = title_rule(title, width.saturating_sub(time.width() * 2), None);
    for span in &mut line.spans {
        span.style = Style::new().dim();
    }
    if line.spans.len() < 4 {
        // Too narrow for the time; the rule fills the width without it.
        return if time.is_empty() {
            line
        } else {
            centered_title_rule(title, width, None, right_padding)
        };
    }
    let last = line.spans.len() - 1;
    // The title and its trailing space, after the leading rule's space.
    let middle = line.width() - line.spans[0].width() - line.spans[last].width() + 1;
    let left = (width - middle) / 2;
    let available = width - middle - left - time.width();
    if !time.is_empty() && available < right_padding {
        return centered_title_rule(title, width, None, right_padding);
    }
    let right = available.saturating_sub(if time.is_empty() { 0 } else { right_padding });
    let suffix = if time.is_empty() {
        String::new()
    } else {
        format!("╶{}", "─".repeat(right_padding))
    };
    line.spans[0] = Span::styled(format!("{} ", "─".repeat(left)), Style::new().dim());
    line.spans[last] = Span::styled(
        format!("{}{time}{suffix}", "─".repeat(right)),
        Style::new().dim(),
    );
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_rule_keeps_its_title_centered_with_a_time_at_the_end() {
        for width in 0..=40 {
            let line = centered_title_rule("Bash", width, Some("35s"), 0);
            assert_eq!(
                line.width(),
                width + usize::from(line.to_string().ends_with("╶")),
                "{width}"
            );
            let text = line.to_string();
            if width >= 16 {
                assert!(text.ends_with("─ 35s╶"), "{text}");
                let left = text.chars().take_while(|ch| *ch == '─').count();
                assert_eq!(left, (width - 6) / 2, "{text}");
            }
        }
    }
}
