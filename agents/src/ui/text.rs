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
    let normalized;
    let text = if !is_prose(kind) && paths.is_none() {
        normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        normalized.as_str()
    } else {
        text
    };
    let detected;
    let paths = match paths {
        Some(paths) => paths,
        None => {
            detected = super::paths::text_paths(text, cwd);
            &detected
        }
    };
    let text = if is_prose(kind) {
        super::line_truncation::abbreviate_paths_in_text(text, paths, width, cwd)
    } else {
        super::line_truncation::abbreviate_paths(text, paths, width, cwd)
    };
    let text = crate::providers::text::printable(&text);
    if !is_prose(kind) {
        return Line::styled(text.split_whitespace().collect::<Vec<_>>().join(" "), style);
    }
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
