use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;

use unicode_width::UnicodeWidthStr;
fn display_width(s: &str) -> usize {
    s.width()
}

pub(crate) fn line_width(line: &Line<'_>) -> usize {
    line.iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

pub(crate) fn truncate_line_to_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 {
        return Line::from(Vec::<Span<'static>>::new());
    }

    let Line {
        style,
        alignment,
        spans,
    } = line;
    let mut used = 0usize;
    let mut spans_out: Vec<Span<'static>> = Vec::with_capacity(spans.len());

    for span in spans {
        let span_width = display_width(span.content.as_ref());

        if span_width == 0 {
            spans_out.push(span);
            continue;
        }

        if used >= max_width {
            break;
        }

        if used + span_width <= max_width {
            used += span_width;
            spans_out.push(span);
            continue;
        }

        let style = span.style;
        let text = span.content.as_ref();
        let mut end_idx = 0usize;
        for (idx, grapheme) in text.grapheme_indices(/*is_extended*/ true) {
            let grapheme_width = display_width(grapheme);
            if used + grapheme_width > max_width {
                break;
            }
            end_idx = idx + grapheme.len();
            used += grapheme_width;
        }

        if end_idx > 0 {
            spans_out.push(Span::styled(text[..end_idx].to_string(), style));
        }

        break;
    }

    Line {
        style,
        alignment,
        spans: spans_out,
    }
}

/// Truncate a styled line to `max_width` and append an ellipsis on overflow.
///
/// Intended for short UI rows. This preserves a fast no-overflow path (width
/// pre-scan + return original line unchanged) and uses `truncate_line_to_width`
/// for the overflow case.
/// Performance should be reevaluated if using this method in loops/over larger content in the future.
pub(crate) fn truncate_line_with_ellipsis_if_overflow(
    line: Line<'static>,
    max_width: usize,
) -> Line<'static> {
    if max_width == 0 {
        return Line::from(Vec::<Span<'static>>::new());
    }

    if line_width(&line) <= max_width {
        return line;
    }

    let truncated = truncate_line_to_width(line, max_width.saturating_sub(1));
    let Line {
        style,
        alignment,
        mut spans,
    } = truncated;
    let ellipsis_style = spans.last().map(|span| span.style).unwrap_or_default();
    spans.push(Span::styled("…", ellipsis_style));
    Line {
        style,
        alignment,
        spans,
    }
}

pub(super) fn abbreviate_path(path: &str, max_width: usize) -> String {
    let path = super::formatting::format_directory_display(std::path::Path::new(path));
    if display_width(&path) <= max_width {
        return path;
    }

    let mut components: Vec<String> = path
        .split(std::path::MAIN_SEPARATOR)
        .map(str::to_owned)
        .collect();
    let Some(last) = components
        .iter()
        .rposition(|component| !component.is_empty())
    else {
        return path;
    };
    let separator = std::path::MAIN_SEPARATOR.to_string();
    for index in 0..last {
        let component = &components[index];
        if component.is_empty() || component == "~" {
            continue;
        }
        let mut graphemes = component.graphemes(true);
        let first = graphemes.next().expect("nonempty component");
        let head = if first == "." {
            format!("{first}{}", graphemes.next().unwrap_or(""))
        } else {
            first.to_owned()
        };
        let abbreviated = format!("{head}…");
        if display_width(&abbreviated) < display_width(component) {
            components[index] = abbreviated;
            let candidate = components.join(&separator);
            if display_width(&candidate) <= max_width {
                return candidate;
            }
        }
    }

    let tail = components[last].clone();
    let boundaries: Vec<usize> = tail
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .collect();
    for &end in boundaries.iter().skip(1).rev() {
        let abbreviated = format!("{}…", &tail[..end]);
        components[last] = abbreviated;
        let candidate = components.join(&separator);
        if display_width(&candidate) <= max_width {
            return candidate;
        }
    }
    components.join(&separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_abbreviation_stops_after_each_left_component() {
        let path = "/Users/visitor/Code/.worktrees/agents-panel";
        for expected in [
            "/U…/visitor/Code/.worktrees/agents-panel",
            "/U…/v…/Code/.worktrees/agents-panel",
            "/U…/v…/C…/.worktrees/agents-panel",
            "/U…/v…/C…/.w…/agents-panel",
        ] {
            assert_eq!(abbreviate_path(path, display_width(expected)), expected);
        }
    }

    #[test]
    fn path_abbreviation_keeps_dot_and_skips_short_components() {
        assert_eq!(
            abbreviate_path("~/.worktrees/a/.x/ab/project", 22),
            "~/.w…/a/.x/ab/project"
        );
        assert_eq!(abbreviate_path("~/a/.x/ab/project", 1), "~/a/.x/ab/p…");
    }

    #[test]
    fn path_abbreviation_cuts_only_the_last_component_tail() {
        for (width, expected) in [
            (13, "/U…/agents-p…"),
            (10, "/U…/agent…"),
            (6, "/U…/a…"),
            (0, "/U…/a…"),
        ] {
            assert_eq!(abbreviate_path("/Users/agents-panel", width), expected);
        }
    }

    #[test]
    fn path_abbreviation_short_circuits_after_home_replacement() {
        let home = std::env::var_os("HOME").expect("HOME");
        let path = std::path::Path::new(&home).join("Code/agents-panel");
        assert_eq!(
            abbreviate_path(&path.to_string_lossy(), 19),
            "~/Code/agents-panel"
        );
        assert_eq!(abbreviate_path(&home.to_string_lossy(), 1), "~");
    }

    #[test]
    fn path_abbreviation_leaves_fitting_paths_unchanged() {
        for path in ["/tmp/project", "~/Code/project", "a/.x/ab", "/", ""] {
            assert_eq!(abbreviate_path(path, display_width(path)), path);
        }
    }

    #[test]
    fn path_abbreviation_uses_display_width_and_preserves_graphemes() {
        assert_eq!(abbreviate_path("/界面/工具", 9), "/界…/工具");
        assert_eq!(
            abbreviate_path("e\u{301}cole/project", 10),
            "e\u{301}…/project"
        );
    }
}
