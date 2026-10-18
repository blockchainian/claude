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

pub(super) fn abbreviate_path(
    path: &str,
    max_width: usize,
    cwd: Option<&std::path::Path>,
) -> String {
    let path = cwd
        .and_then(|cwd| super::paths::relative_path(path, cwd))
        .unwrap_or_else(|| path.to_owned());
    let path = abbreviate_directories(&path, max_width);
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

/// Abbreviates the directories of the paths at `ranges` in prose, left to
/// right, until the text fits or every path's directories are shrunk.
pub(super) fn abbreviate_paths_in_text(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    fit_paths(text, ranges, max_width, false, cwd)
}

/// Shrinks the paths at `ranges` in `text`, left to right, until the text
/// fits: every path's directories first, then the last component of a lone
/// path, since cutting several file names leaves none readable.
pub(super) fn abbreviate_paths(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    fit_paths(text, ranges, max_width, ranges.len() == 1, cwd)
}

fn fit_paths(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cut_names: bool,
    cwd: &std::path::Path,
) -> String {
    let mut paths: Vec<String> = ranges
        .iter()
        .map(|range| {
            super::paths::relative_path(&text[range.clone()], cwd)
                .unwrap_or_else(|| text[range.clone()].to_owned())
        })
        .collect();
    let paths_width: usize = ranges
        .iter()
        .map(|range| display_width(&text[range.clone()]))
        .sum();
    let text_width = display_width(text).saturating_sub(paths_width)
        + paths.iter().map(|path| display_width(path)).sum::<usize>();
    let mut overflow = text_width.saturating_sub(max_width);
    let passes: &[fn(&str, usize) -> String] = if cut_names {
        &[abbreviate_directories, |path, width| {
            abbreviate_path(path, width, None)
        }]
    } else {
        &[abbreviate_directories]
    };
    for shrink in passes {
        for path in &mut paths {
            if overflow == 0 {
                break;
            }
            let width = display_width(path);
            let shrunk = shrink(path, width.saturating_sub(overflow));
            overflow = overflow.saturating_sub(width.saturating_sub(display_width(&shrunk)));
            *path = shrunk;
        }
    }
    let mut fitted = String::new();
    let mut end = 0;
    for (range, path) in ranges.iter().zip(&paths) {
        fitted.push_str(&text[end..range.start]);
        fitted.push_str(path);
        end = range.end;
    }
    fitted.push_str(&text[end..]);
    fitted
}

/// Fits each oversized path at `ranges` to the wrapping width, independently of surrounding text.
pub(super) fn abbreviate_paths_for_wrap(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    width: usize,
    cwd: &std::path::Path,
) -> String {
    let mut fitted = String::new();
    let mut end = 0;
    for range in ranges {
        fitted.push_str(&text[end..range.start]);
        let relative = super::paths::relative_path(&text[range.clone()], cwd);
        let path = relative.as_deref().unwrap_or(&text[range.clone()]);
        if display_width(path) <= width {
            fitted.push_str(path);
        } else {
            fitted.push_str(&abbreviate_path(path, width, None));
        }
        end = range.end;
    }
    fitted.push_str(&text[end..]);
    fitted
}

/// Keeps paths together while retaining Unicode line breaks in surrounding prose.
pub(super) fn prose_wrap_options(width: usize) -> textwrap::Options<'static> {
    textwrap::Options::new(width)
        .word_separator(textwrap::WordSeparator::Custom(prose_words))
        .word_splitter(textwrap::WordSplitter::Custom(|word| {
            if super::paths::word_path(word).is_some() {
                Vec::new()
            } else {
                textwrap::WordSplitter::HyphenSplitter.split_points(word)
            }
        }))
}

fn prose_words(text: &str) -> Box<dyn Iterator<Item = textwrap::core::Word<'_>> + '_> {
    let separator = textwrap::WordSeparator::new();
    Box::new(
        text.split_inclusive(char::is_whitespace)
            .flat_map(move |part| {
                let Some(range) =
                    super::paths::word_path(part.trim_end_matches(char::is_whitespace))
                else {
                    return separator.find_words(part).collect::<Vec<_>>();
                };
                let mut words: Vec<_> = separator.find_words(&part[..range.start]).collect();
                let suffix = &part[range.end..];
                let spaces = suffix.len() - suffix.trim_start_matches(' ').len();
                words.push(textwrap::core::Word::from(
                    &part[range.start..range.end + spaces],
                ));
                words.extend(separator.find_words(&part[range.end + spaces..]));
                words
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abbreviate_path(path: &str, width: usize) -> String {
        super::abbreviate_path(path, width, None)
    }

    /// A directory with no files, so only absolute and home paths are found.
    const EMPTY: &str = "/nonexistent";

    fn abbreviate_paths_for_wrap(text: &str, width: usize) -> String {
        let ranges = crate::ui::paths::text_paths(text, std::path::Path::new(EMPTY));
        super::abbreviate_paths_for_wrap(text, &ranges, width, std::path::Path::new(EMPTY))
    }

    fn abbreviate_paths_in_text(text: &str, max_width: usize) -> String {
        let ranges = crate::ui::paths::text_paths(text, std::path::Path::new(EMPTY));
        super::abbreviate_paths_in_text(text, &ranges, max_width, std::path::Path::new(EMPTY))
    }

    #[test]
    fn cwd_abbreviation_precedes_width_fitting_and_keeps_component_boundaries() {
        let cwd = std::path::Path::new("/a/b");
        for (path, expected) in [
            ("/a/b/src/lib.rs:272-274", "src/lib.rs:272-274"),
            ("/a/b", "."),
            ("/a/b/", "."),
            ("/a/bc/src/lib.rs", "/a/bc/src/lib.rs"),
            ("/a/b/../outside.rs", "/a/b/../outside.rs"),
            ("src/lib.rs", "src/lib.rs"),
        ] {
            assert_eq!(super::abbreviate_path(path, 100, Some(cwd)), expected);
        }
        let text = "See `/a/b/src/lib.rs`:272-274 and /a/bc/other.rs";
        let ranges = crate::ui::paths::text_paths(text, cwd);
        let expected = "See `src/lib.rs`:272-274 and /a/bc/other.rs";
        assert_eq!(
            super::abbreviate_paths_in_text(text, &ranges, 100, cwd),
            expected
        );
        assert_eq!(super::abbreviate_paths(text, &ranges, 100, cwd), expected);
        assert_eq!(
            super::abbreviate_paths_for_wrap(text, &ranges, 100, cwd),
            expected
        );
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(
            super::abbreviate_path("~/project/src/lib.rs", 100, Some(&home.join("project"))),
            "src/lib.rs"
        );
        assert_eq!(
            super::abbreviate_path(
                &home.join("other/file.rs").to_string_lossy(),
                100,
                Some(&home.join("project"))
            ),
            "~/other/file.rs"
        );
        assert_eq!(
            super::abbreviate_path("/a/b/long_directory/file.rs", 11, Some(cwd)),
            "l…/file.rs"
        );
    }

    #[test]
    fn wrapped_paths_use_each_paths_width_and_preserve_whitespace() {
        let text = "many words before /tmp/fits.md and /private/workspace/reports/result.md\n报告：/private/workspace/reports/result.md\tend";
        assert_eq!(
            abbreviate_paths_for_wrap(text, 30),
            "many words before /tmp/fits.md and /p…/w…/reports/result.md\n报告：/p…/w…/reports/result.md\tend"
        );
        assert_eq!(abbreviate_paths_for_wrap(text, 100), text);
    }

    #[test]
    fn wrapped_paths_preserve_cjk_suffix_and_markdown_punctuation() {
        assert_eq!(
            abbreviate_paths_for_wrap("报告：`/private/workspace/report.md`，已发送。", 20),
            "报告：`/p…/w…/report.md`，已发送。"
        );
        assert_eq!(
            abbreviate_paths_for_wrap("a/b/c https://example.com/a/b /report.md", 1),
            "a/b/c https://example.com/a/b /report.md"
        );
    }

    #[test]
    fn wrapped_paths_use_home_and_last_component_rules_only_on_overflow() {
        let home = std::env::var_os("HOME").expect("HOME");
        let path = std::path::Path::new(&home).join("workspace/report.md");
        let path = path.to_string_lossy();
        assert_eq!(abbreviate_paths_for_wrap(&path, path.width()), path);
        assert_eq!(abbreviate_paths_for_wrap(&path, 15), "~/w…/report.md");
        assert_eq!(
            abbreviate_paths_for_wrap("/workspace/longfilename.md", 12),
            "/w…/longfil…"
        );
    }

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
