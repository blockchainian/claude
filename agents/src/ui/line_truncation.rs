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
        for (idx, grapheme) in text.grapheme_indices(true) {
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

    let first_style = line
        .spans
        .first()
        .map(|span| span.style)
        .unwrap_or_default();
    let truncated = truncate_line_to_width(line, max_width.saturating_sub(1));
    let Line {
        style,
        alignment,
        mut spans,
    } = truncated;
    let ellipsis_style = spans.last().map(|span| span.style).unwrap_or(first_style);
    spans.push(Span::styled("…", ellipsis_style));
    Line {
        style,
        alignment,
        spans,
    }
}

/// Keeps whole components around one `…` gap, then fits the final name alone.
/// Home and relative paths keep their tail; root paths keep their head and name,
/// since a lone `/…/` before the name says nothing.
pub(super) fn shorten_path(path: &str, max_width: usize, cwd: Option<&std::path::Path>) -> String {
    let path = display_path(path, cwd);
    if display_width(&path) <= max_width {
        return path;
    }
    let anchor = if path.starts_with("~/") {
        "~/"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let components: Vec<_> = path[anchor.len()..]
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let candidates: Vec<String> = if anchor == "/" {
        let name = path_name(&path);
        (1..components.len().saturating_sub(1))
            .rev()
            .map(|end| format!("/{}/…/{name}", components[..end].join("/")))
            .collect()
    } else {
        (1..components.len())
            .map(|start| format!("{anchor}…/{}", components[start..].join("/")))
            .collect()
    };
    if let Some(candidate) = candidates
        .into_iter()
        .find(|candidate| display_width(candidate) <= max_width)
    {
        return candidate;
    }
    truncate_line_with_ellipsis_if_overflow(Line::raw(path_name(&path).to_owned()), max_width)
        .to_string()
}

/// A path's last component, the part shortening keeps longest.
fn path_name(path: &str) -> &str {
    path.split('/')
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
}

fn display_path(path: &str, cwd: Option<&std::path::Path>) -> String {
    if let Some(relative) = cwd.and_then(|cwd| super::paths::relative_path(path, cwd)) {
        return relative;
    }
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = std::path::Path::new(path).strip_prefix(home)
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    path.to_owned()
}

/// Fits paths in a row. When shortening paths to their names fits the whole row,
/// the rightmost path shrinks fully before its left neighbor; otherwise the row is
/// cut anyway, so paths fill it from the left and keep their full form while they fit.
pub(super) fn fit_paths_in_row(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    let (paths, row_end) = fit_paths(text, ranges, max_width, cwd);
    let mut fitted = String::new();
    let mut start = 0;
    for (range, path) in ranges.iter().zip(&paths) {
        if range.start >= row_end {
            break;
        }
        fitted.push_str(&text[start..range.start]);
        fitted.push_str(path);
        start = range.end;
    }
    fitted.push_str(&text[start..row_end.max(start)]);
    fitted
}

/// Fits the paths of a row of styled words, as `fit_paths_in_row` does for plain text.
/// Paths are found in the row's whole text, since Markdown can split one across words;
/// a fitted path takes the style of the word it starts in.
pub(super) fn fit_paths_in_spans(
    spans: &mut [Span<'static>],
    max_width: usize,
    cwd: &std::path::Path,
) {
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    let ranges = super::paths::text_paths(&text, cwd);
    if ranges.is_empty() {
        return;
    }
    let (paths, row_end) = fit_paths(&text, &ranges, max_width, cwd);
    let mut start = 0;
    for span in spans {
        let span_end = start + span.content.len();
        let mut fitted = String::new();
        let mut kept = start;
        for (range, path) in ranges.iter().zip(&paths) {
            if range.end <= start || range.start >= span_end || range.start >= row_end {
                continue;
            }
            fitted.push_str(&text[kept..range.start.max(start)]);
            if range.start >= start {
                fitted.push_str(path);
            }
            kept = range.end.min(span_end);
        }
        fitted.push_str(&text[kept..span_end.min(row_end).max(kept)]);
        span.content = fitted.into();
        start = span_end;
    }
}

/// Each path of `text` as it fits in a row of `max_width` columns, and the byte
/// offset where the row's text ends: right after a path whose name had to be cut.
fn fit_paths(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> (Vec<String>, usize) {
    let mut paths: Vec<String> = ranges
        .iter()
        .map(|range| display_path(&text[range.clone()], Some(cwd)))
        .collect();
    let source_width: usize = ranges
        .iter()
        .map(|range| display_width(&text[range.clone()]))
        .sum();
    let paths_width: usize = paths.iter().map(|path| display_width(path)).sum();
    let width = display_width(text).saturating_sub(source_width) + paths_width;
    if width <= max_width {
        return (paths, text.len());
    }
    let names_width: usize = paths
        .iter()
        .map(|path| display_width(path_name(path)).min(display_width(path)))
        .sum();
    let row_end = if width - paths_width + names_width <= max_width {
        shrink_rightmost_first(text, ranges, &mut paths, width - max_width, max_width);
        text.len()
    } else {
        fill_from_left(text, ranges, &mut paths, max_width)
    };
    (paths, row_end)
}

/// Shrinks paths from the right until the row loses its `overflow`.
fn shrink_rightmost_first(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    paths: &mut [String],
    mut overflow: usize,
    max_width: usize,
) {
    for (index, range) in ranges.iter().enumerate().rev() {
        if overflow == 0 {
            break;
        }
        let before = (display_width(&text[..range.start])
            + paths[..index]
                .iter()
                .map(|path| display_width(path))
                .sum::<usize>())
        .saturating_sub(
            ranges[..index]
                .iter()
                .map(|range| display_width(&text[range.clone()]))
                .sum(),
        );
        let path = &mut paths[index];
        let width = display_width(path);
        let name_width = display_width(path_name(path)).min(width);
        // Text after a path shrinks it only down to its name; text before it
        // can shrink it further, but always leaves at least an ellipsis.
        let target = max_width
            .saturating_sub(before)
            .min(width.saturating_sub(overflow).max(name_width))
            .max(1);
        let shrunk = shorten_path(path, target, None);
        let saved = width.saturating_sub(display_width(&shrunk));
        overflow = overflow.saturating_sub(saved);
        *path = shrunk;
    }
}

/// Gives each path, left to right, the room its row has left, and returns where
/// the row's text ends; paths past the row stay whole.
fn fill_from_left(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    paths: &mut [String],
    max_width: usize,
) -> usize {
    let mut before = 0;
    let mut end = 0;
    for (path, range) in paths.iter_mut().zip(ranges) {
        before += display_width(&text[end..range.start]);
        if before >= max_width {
            break;
        }
        let rest = display_width(&text[range.end..]);
        let (fitted, ends_row) = fit_path(std::mem::take(path), max_width - before, rest);
        *path = fitted;
        if ends_row {
            return range.end;
        }
        before += display_width(path);
        end = range.end;
    }
    text.len()
}

/// A path shortened to the `room` left in its row, with `rest` columns of text after
/// it, and whether the row ends with it. A row that overflows ends in an ellipsis, so
/// a path followed by text fits only with a column to spare; a path whose name must be
/// cut takes the full room and ends the row, its own ellipsis standing for the rest.
fn fit_path(path: String, room: usize, rest: usize) -> (String, bool) {
    let width = display_width(&path);
    if width + rest <= room || (rest > 0 && width < room) {
        return (path, false);
    }
    if rest > 0 {
        let shorter = shorten_path(&path, room - 1, None);
        if !shorter.is_empty() && !shorter.ends_with('…') {
            return (shorter, false);
        }
    }
    let shorter = shorten_path(&path, room, None);
    let ends_row = shorter.ends_with('…');
    (shorter, ends_row)
}

/// Fits each path to the wrapping width, independently of surrounding text.
pub(super) fn fit_paths_to_wrap_width(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    width: usize,
    cwd: &std::path::Path,
) -> String {
    let mut fitted = String::new();
    let mut end = 0;
    for range in ranges {
        fitted.push_str(&text[end..range.start]);
        fitted.push_str(&shorten_path(&text[range.clone()], width, Some(cwd)));
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
    use std::path::Path;

    #[test]
    fn recorded_path_widths_snapshot() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-start.jsonl")
                .lines()
                .nth(2)
                .unwrap(),
        )
        .unwrap();
        let body = record["message"]["content"].as_str().unwrap();
        let path = body
            .split('"')
            .find(|text| text.starts_with("/Users/"))
            .unwrap();
        let path = format!("~/{}", path.splitn(4, '/').nth(3).unwrap());
        let mut snapshot = String::new();
        for width in [100, 60, 40, 25, 15, 8, 1, 0] {
            snapshot.push_str(format!("{width}: {}", shorten_path(&path, width, None)).trim_end());
            snapshot.push('\n');
        }
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/path_widths.txt");
        if std::env::var_os("UPDATE_PATH_SNAPSHOT").is_some() {
            std::fs::write(&file, &snapshot).unwrap();
        }
        assert_eq!(snapshot, std::fs::read_to_string(file).unwrap());
    }

    #[test]
    fn suffix_paths_keep_whole_components_and_one_gap() {
        for (path, widths) in [
            (
                "~/alpha/beta/c/file",
                vec![
                    (24, "~/alpha/beta/c/file"),
                    (15, "~/…/beta/c/file"),
                    (10, "~/…/c/file"),
                    (8, "~/…/file"),
                    (7, "file"),
                    (3, "fi…"),
                    (1, "…"),
                    (0, ""),
                ],
            ),
            (
                "/alpha/beta/c/file",
                vec![
                    (24, "/alpha/beta/c/file"),
                    (17, "/alpha/…/file"),
                    (13, "/alpha/…/file"),
                    (12, "file"),
                    (3, "fi…"),
                ],
            ),
            (
                "alpha/beta/c/file",
                vec![
                    (24, "alpha/beta/c/file"),
                    (13, "…/beta/c/file"),
                    (8, "…/c/file"),
                    (6, "…/file"),
                    (5, "file"),
                    (3, "fi…"),
                ],
            ),
        ] {
            for (width, expected) in widths {
                assert_eq!(
                    shorten_path(path, width, None),
                    expected,
                    "{path} at {width}"
                );
            }
            for width in 0..30 {
                let display = shorten_path(path, width, None);
                assert!(display.width() <= width);
                assert!(!display.contains("…/…"));
            }
        }
    }

    #[test]
    fn suffix_paths_normalize_cwd_and_home_before_fitting() {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let cwd = home.join("project");
        assert_eq!(
            shorten_path("~/project/alpha/file", 100, Some(&cwd)),
            "alpha/file"
        );
        for (path, width, expected) in [
            (cwd.join("alpha/beta/file"), 100, "alpha/beta/file"),
            (cwd.join("alpha/beta/file"), 11, "…/beta/file"),
            (home.join("other/beta/file"), 100, "~/other/beta/file"),
            (home.join("other/beta/file"), 8, "~/…/file"),
            (home.clone(), 1, "~"),
            (cwd.clone(), 100, "project"),
        ] {
            assert_eq!(
                shorten_path(path.to_str().unwrap(), width, Some(&cwd)),
                expected
            );
        }
        assert_eq!(
            shorten_path("/a/bc/file", 100, Some(Path::new("/a/b"))),
            "/a/bc/file"
        );
        assert_eq!(shorten_path("/", 1, None), "/");
        assert_eq!(shorten_path("", 0, None), "");
    }

    #[test]
    fn suffix_paths_fit_unicode_without_splitting_graphemes() {
        assert_eq!(shorten_path("/目录/工具", 6, None), "工具");
        assert_eq!(shorten_path("/目录/工具", 3, None), "工…");
        assert_eq!(shorten_path("/dir/e\u{301}cole", 3, None), "e\u{301}c…");
    }

    /// The row as drawn: its paths fitted, then the row cut with an ellipsis.
    fn row(text: &str, width: usize) -> String {
        let cwd = Path::new("/nonexistent");
        let ranges = crate::ui::paths::text_paths(text, cwd);
        let fitted = fit_paths_in_row(text, &ranges, width, cwd);
        truncate_line_with_ellipsis_if_overflow(Line::raw(fitted), width).to_string()
    }

    #[test]
    fn row_paths_shrink_rightmost_first_when_names_fit_the_row() {
        let text = "/alpha/left /beta/gamma/right";
        for (width, expected) in [
            (100, text),
            (25, "/alpha/left /beta/…/right"),
            (17, "/alpha/left right"),
            (13, "/alpha/left …"),
            (9, "left rig…"),
            (6, "left …"),
        ] {
            assert_eq!(row(text, width), expected, "width {width}");
        }
    }

    #[test]
    fn row_paths_that_fit_keep_their_full_form() {
        let text = "Reopened /private/jobs/tmp/sed.md. Then a long tail of prose follows here";
        assert_eq!(row(text, 40), "Reopened /private/jobs/tmp/sed.md. Then…");
        assert_eq!(row(text, 12), "Reopened se…");
    }

    #[test]
    fn row_paths_ignore_text_beyond_the_row() {
        let text = "Read /private/docs/design.md then move it somewhere sensible \
                    over the coming days, /private/src/old.rs and more";
        assert_eq!(row(text, 40), "Read /private/docs/design.md then move …");
    }

    #[test]
    fn row_paths_after_another_path_keep_the_rest_of_the_row() {
        let text = "把 /private/src/ui/shell_commands.rs 挪到 /private/src/shell_commands.rs，\
                    让它成为独立模块。后面还有很长很长的一段文字";
        assert_eq!(
            row(text, 57),
            "把 /private/src/ui/shell_commands.rs 挪到 shell_commands…"
        );
    }

    #[test]
    fn row_paths_keep_the_name_of_a_directory_with_a_trailing_slash() {
        let text = "Add `/private/src/code_format/` support, with a long tail of prose";
        assert_eq!(row(text, 20), "Add `code_format` s…");
        assert_eq!(row(text, 40), "Add `/private/src/code_format/` support…");
    }

    #[test]
    fn suffix_paths_wrap_and_keep_punctuation() {
        let text = "报告：`/private/workspace/reports/result.md`，已发送。";
        let cwd = Path::new("/nonexistent");
        let ranges = crate::ui::paths::text_paths(text, cwd);
        assert_eq!(
            fit_paths_to_wrap_width(text, &ranges, 22, cwd),
            "报告：`/private/…/result.md`，已发送。"
        );
        assert_eq!(
            fit_paths_to_wrap_width(text, &ranges, 5, cwd),
            "报告：`resu…`，已发送。"
        );
    }
}
