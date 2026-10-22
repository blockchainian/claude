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

/// Keeps whole components around one `…` gap, then fits the final name alone.
/// Home and relative paths keep their tail; root paths keep their head and name,
/// since a lone `/…/` before the name says nothing.
pub(super) fn abbreviate_path(
    path: &str,
    max_width: usize,
    cwd: Option<&std::path::Path>,
) -> String {
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
        let name = components.last().copied().unwrap_or_default();
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
    let name = components.last().copied().unwrap_or(&path);
    truncate_line_with_ellipsis_if_overflow(Line::raw(name.to_owned()), max_width).to_string()
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

/// Fits paths in prose while giving the leftmost paths priority.
pub(super) fn abbreviate_paths_in_text(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    fit_paths(text, ranges, max_width, cwd)
}

/// Fits paths in a row, shrinking the rightmost path fully before its left neighbor.
pub(super) fn abbreviate_paths(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    fit_paths(text, ranges, max_width, cwd)
}

fn fit_paths(
    text: &str,
    ranges: &[std::ops::Range<usize>],
    max_width: usize,
    cwd: &std::path::Path,
) -> String {
    let mut paths: Vec<String> = ranges
        .iter()
        .map(|range| display_path(&text[range.clone()], Some(cwd)))
        .collect();
    let source_width: usize = ranges
        .iter()
        .map(|range| display_width(&text[range.clone()]))
        .sum();
    let width = display_width(text).saturating_sub(source_width)
        + paths.iter().map(|path| display_width(path)).sum::<usize>();
    let mut overflow = width.saturating_sub(max_width);
    for path in paths.iter_mut().rev() {
        if overflow == 0 {
            break;
        }
        let width = display_width(path);
        // Keep an ellipsis when surrounding prose alone exceeds the row width.
        let shrunk = abbreviate_path(path, width.saturating_sub(overflow).max(1), None);
        overflow = overflow.saturating_sub(width.saturating_sub(display_width(&shrunk)));
        *path = shrunk;
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

/// Fits each path to the wrapping width, independently of surrounding text.
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
        fitted.push_str(&abbreviate_path(&text[range.clone()], width, Some(cwd)));
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
            snapshot
                .push_str(format!("{width}: {}", abbreviate_path(&path, width, None)).trim_end());
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
                    abbreviate_path(path, width, None),
                    expected,
                    "{path} at {width}"
                );
            }
            for width in 0..30 {
                let display = abbreviate_path(path, width, None);
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
            abbreviate_path("~/project/alpha/file", 100, Some(&cwd)),
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
                abbreviate_path(path.to_str().unwrap(), width, Some(&cwd)),
                expected
            );
        }
        assert_eq!(
            abbreviate_path("/a/bc/file", 100, Some(Path::new("/a/b"))),
            "/a/bc/file"
        );
        assert_eq!(abbreviate_path("/", 1, None), "/");
        assert_eq!(abbreviate_path("", 0, None), "");
    }

    #[test]
    fn suffix_paths_fit_unicode_without_splitting_graphemes() {
        assert_eq!(abbreviate_path("/目录/工具", 6, None), "工具");
        assert_eq!(abbreviate_path("/目录/工具", 3, None), "工…");
        assert_eq!(abbreviate_path("/dir/e\u{301}cole", 3, None), "e\u{301}c…");
    }

    #[test]
    fn suffix_paths_compress_rightmost_path_to_the_limit_first() {
        let text = "/alpha/left /beta/gamma/right";
        let cwd = Path::new("/nonexistent");
        let ranges = crate::ui::paths::text_paths(text, cwd);
        for fit in [abbreviate_paths, abbreviate_paths_in_text] {
            for (width, expected) in [
                (100, text),
                (25, "/alpha/left /beta/…/right"),
                (17, "/alpha/left right"),
                (13, "/alpha/left …"),
                (9, "left …"),
                (6, "left …"),
            ] {
                assert_eq!(fit(text, &ranges, width, cwd), expected, "width {width}");
            }
        }
    }

    #[test]
    fn suffix_paths_wrap_and_keep_punctuation() {
        let text = "报告：`/private/workspace/reports/result.md`，已发送。";
        let cwd = Path::new("/nonexistent");
        let ranges = crate::ui::paths::text_paths(text, cwd);
        assert_eq!(
            abbreviate_paths_for_wrap(text, &ranges, 22, cwd),
            "报告：`/private/…/result.md`，已发送。"
        );
        assert_eq!(
            abbreviate_paths_for_wrap(text, &ranges, 5, cwd),
            "报告：`resu…`，已发送。"
        );
    }
}
