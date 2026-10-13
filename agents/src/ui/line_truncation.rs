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
    let path = abbreviate_directories(path, max_width);
    if display_width(&path) <= max_width {
        return path;
    }

    let separator = std::path::MAIN_SEPARATOR.to_string();
    let mut components: Vec<String> = path.split(&separator).map(str::to_owned).collect();
    let Some(last) = components
        .iter()
        .rposition(|component| !component.is_empty())
    else {
        return path;
    };
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

/// Shrinks a path's directory components from the left until it fits, keeping
/// the last component whole even when it still does not fit.
fn abbreviate_directories(path: &str, max_width: usize) -> String {
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
    components.join(&separator)
}

/// Abbreviates the directories of absolute and home paths in prose, left to
/// right, until the text fits or every path's directories are shrunk.
pub(super) fn abbreviate_paths_in_text(text: &str, max_width: usize) -> String {
    let mut overflow = display_width(text).saturating_sub(max_width);
    if overflow == 0 {
        return text.to_owned();
    }
    text.split(' ')
        .map(|word| {
            // A path starts at the first `/` or `~` after punctuation only, so
            // `a/b` and `https://…` stay prose while `报告：/tmp/…` is a path.
            let Some(start) = word.find(['/', '~']).filter(|&start| {
                !word[..start].contains(|c: char| c.is_ascii_alphanumeric() || c == ':')
            }) else {
                return word.to_owned();
            };
            let path = word[start..].trim_end_matches(['`', ')', '"', '\'', ',', '.', ';', ':']);
            let is_path =
                (path.starts_with('/') || path.starts_with("~/")) && path[1..].contains('/');
            if overflow == 0 || !is_path {
                return word.to_owned();
            }
            let width = display_width(path);
            let shrunk = abbreviate_directories(path, width.saturating_sub(overflow));
            overflow = overflow.saturating_sub(width.saturating_sub(display_width(&shrunk)));
            format!("{}{shrunk}{}", &word[..start], &word[start + path.len()..])
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn prose_paths_shrink_directories_only_as_far_as_the_overflow() {
        let text = "Report: /private/tmp/claude-501/report.md 1. done";
        assert_eq!(abbreviate_paths_in_text(text, display_width(text)), text);
        assert_eq!(
            abbreviate_paths_in_text(text, 44),
            "Report: /p…/tmp/claude-501/report.md 1. done"
        );
        assert_eq!(
            abbreviate_paths_in_text(text, 10),
            "Report: /p…/t…/c…/report.md 1. done"
        );
    }

    #[test]
    fn prose_paths_keep_surrounding_punctuation_and_skip_non_paths() {
        assert_eq!(
            abbreviate_paths_in_text("see a/b/c and `/tmp/project/x.md`.", 1),
            "see a/b/c and `/t…/p…/x.md`."
        );
        assert_eq!(
            abbreviate_paths_in_text("报告：/tmp/project/x.md", 1),
            "报告：/t…/p…/x.md"
        );
        assert_eq!(
            abbreviate_paths_in_text("/report.md and https://example.com/a/b", 1),
            "/report.md and https://example.com/a/b"
        );
    }
}
