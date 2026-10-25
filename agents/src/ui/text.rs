//! Message layout and one-line summaries shared by the panels.

use super::tool_detail::Part;
use crate::event::Kind;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::ops::Range;
use std::path::Path;

pub(super) fn lines(text: &str, cwd: &Path, width: usize, markdown: bool) -> Vec<Line<'static>> {
    layout(text, cwd, width, markdown).0
}

pub(super) fn layout(
    text: &str,
    cwd: &Path,
    width: usize,
    markdown: bool,
) -> (Vec<Line<'static>>, Vec<Option<bool>>) {
    let mut lines = Vec::new();
    let mut breaks = Vec::new();
    for part in Part::prose_parts(text.to_owned(), markdown) {
        let (drawn, joins) =
            part.layout_in(width, Style::default(), chrono::DateTime::UNIX_EPOCH, cwd);
        lines.extend(drawn);
        breaks.extend(joins);
    }
    (lines, breaks)
}

/// Whether a summary uses the message body's Markdown interpretation.
pub(super) fn is_prose(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::User
            | Kind::Compaction
            | Kind::Assistant
            | Kind::Agent
            | Kind::QueuedUser
            | Kind::QueuedSystem
    )
}

/// A message or activity on one row, with optional provider-marked paths.
pub(crate) fn summary(
    text: &str,
    cwd: &Path,
    width: usize,
    kind: &Kind,
    paths: Option<&[Range<usize>]>,
) -> Line<'static> {
    let style = super::style::event_styles(kind).1;
    if !is_prose(kind) {
        let normalized;
        let detected;
        let (text, paths) = match paths {
            Some(paths) => (text, paths),
            None => {
                normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                detected = super::paths::text_paths(&normalized, cwd);
                (normalized.as_str(), detected.as_slice())
            }
        };
        let text = super::line_truncation::fit_paths_in_row(text, paths, width, cwd);
        let text = crate::providers::text::printable(&text);
        return Line::styled(text.split_whitespace().collect::<Vec<_>>().join(" "), style);
    }
    let text = crate::providers::text::printable(text);
    let mut spans = Vec::new();
    for line in lines(&text, cwd, usize::MAX, true) {
        if line
            .to_string()
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '─' | '┼'))
        {
            continue;
        }
        let mut pending_space = !spans.is_empty();
        for span in line.spans {
            for fragment in span.content.split_inclusive(char::is_whitespace) {
                let word = fragment.trim_end_matches(char::is_whitespace);
                if !word.is_empty() {
                    if pending_space {
                        spans.push(Span::styled(" ", style));
                    }
                    spans.push(Span::styled(word.to_owned(), style.patch(span.style)));
                    pending_space = false;
                }
                pending_space |= word.len() != fragment.len();
            }
        }
    }
    super::line_truncation::fit_paths_in_spans(&mut spans, width, cwd);
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_report_summary_keeps_all_paragraphs_on_one_line() {
        let source = include_str!("../../tests/fixtures/claude-peer-message.txt");
        let line = summary(source, Path::new(""), usize::MAX, &Kind::Assistant, None).to_string();
        assert!(line.starts_with("FYI: VS Code traps"));
        assert!(line.contains("State as I understand it:"));
        assert!(line.contains("Traps:"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn recorded_question_summary_keeps_inline_code_next_to_chinese() {
        let record: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/claude-questions-pending.jsonl"
        ))
        .unwrap();
        let text = record["message"]["content"][0]["input"]["questions"][0]["question"]
            .as_str()
            .unwrap();
        let line = summary(text, Path::new(""), 200, &Kind::Assistant, None).to_string();
        assert!(line.contains("是子命令？比如 rg -n 'background' src 里的 background"));
        assert!(line.contains("cat README.md 的是文件"));
        assert!(!line.contains('`'));
    }

    /// A prose summary as drawn in a row of `width` columns.
    fn prose_row(text: &str, width: usize) -> String {
        let line = summary(text, Path::new("/nonexistent"), width, &Kind::User, None);
        super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(line, width)
            .to_string()
    }

    #[test]
    fn prose_summary_fits_paths_after_hidden_markdown_markers() {
        let text = "**读** **读** **读** **读** **读** **读** **读** **读**  /private/tmp/design.md \
                    先读设计文档，保留现有行为，检查所有调用方，再运行相关测试。";
        assert_eq!(
            prose_row(text, 50),
            "读 读 读 读 读 读 读 读 /private/tmp/design.md 先…"
        );
    }

    #[test]
    fn prose_summary_fits_paths_after_collapsed_whitespace() {
        let text = format!(
            "目标{}/private/tmp/design.md 先读设计文档，保留现有行为，检查所有调用方，再运行相关测试。",
            " ".repeat(60)
        );
        assert_eq!(
            prose_row(&text, 60),
            "目标 /private/tmp/design.md 先读设计文档，保留现有行为，检…"
        );
    }

    #[test]
    fn prose_summary_fits_paths_that_markdown_splits() {
        let home = "读 ~/notes/design.md 后面还有很长很长的一段文字以及其他信息，检查所有调用方。";
        assert_eq!(prose_row(home, 16), "读 design.md 后…");
        assert_eq!(prose_row(home, 20), "读 ~/…/design.md 后…");
        let bold = "读 /private/**src**/file.rs 后面还有很长很长的说明文字";
        assert_eq!(prose_row(bold, 20), "读 file.rs 后面还有…");
    }

    #[test]
    fn prose_summary_ends_a_row_at_a_cut_path_with_one_ellipsis() {
        let text = "**读** `/private/目录/工具.rs`，完成。";
        assert_eq!(prose_row(text, 7), "读 工…");
    }

    #[test]
    fn colored_document_keeps_structured_indentation() {
        let raw = include_str!("../../tests/fixtures/claude-attach/attach.raw");
        let start = raw.find("\x1b[2mAttaching…").unwrap();
        let styled = &raw[start..];
        let reset = styled.find("\x1b[0m").unwrap();
        let xml = include_str!("../../tests/fixtures/structured_text/real-theme.plist");
        let source = format!("{}{xml}{}", &styled[..4], &styled[reset..reset + 4]);
        let rendered = lines(&source, Path::new(""), 80, true);
        assert_eq!(rendered[0].to_string(), "<plist version=\"1.0\">");
        assert_eq!(rendered[1].to_string(), "  <dict>");
        assert!(
            rendered
                .iter()
                .all(|line| !line.to_string().contains('\x1b'))
        );
    }
}
