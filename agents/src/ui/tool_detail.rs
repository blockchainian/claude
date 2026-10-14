//! The calls of an open tool row as the parts the log panel draws and copies.

use super::highlight;
use crate::log::{Event, Kind, Section};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One part of the open view: the text it is drawn from, which the selection copies, and
/// how that text turns into lines.
pub(super) struct Part {
    pub source: String,
    body: Body,
}

enum Body {
    /// A call name or section label, in the given style.
    Label(Style),
    /// The row's blank line between two calls.
    Blank,
    /// The dim note that a call has no output yet.
    Running,
    /// `key: value` lines.
    Fields,
    /// Code highlighted as the given language.
    Code(String),
    Diff,
    Markdown,
    /// Prose wrapped at the panel width.
    Text,
}

impl Part {
    fn new(source: impl Into<String>, body: Body) -> Self {
        Part {
            source: source.into(),
            body,
        }
    }

    /// Whether the selection maps the drawn lines back to the source as Markdown.
    pub fn markdown(&self) -> bool {
        matches!(self.body, Body::Markdown)
    }

    /// The part drawn `width` columns wide; every line fits so each cell maps to its source.
    pub fn lines(&self, width: usize, body_style: Style) -> Vec<Line<'static>> {
        let source = self.source.as_str();
        let options = super::line_truncation::prose_wrap_options(width);
        let lines = match &self.body {
            Body::Label(style) => vec![Line::from(Span::styled(source.to_owned(), *style))],
            Body::Blank => vec![Line::default()],
            Body::Running => vec![Line::from(Span::styled(
                source.to_owned(),
                Style::new().dim(),
            ))],
            Body::Fields => split_wide(highlight::plain_lines(source), width),
            Body::Code(lang) => split_wide(highlight::code_lines(source, lang), width),
            Body::Diff => split_wide(highlight::diff_lines(source), width),
            Body::Markdown => super::markdown::render_with_options(source, options),
            Body::Text => textwrap::wrap(source, options)
                .into_iter()
                .map(|line| Line::from(line.into_owned()))
                .collect(),
        };
        lines
            .into_iter()
            .map(|mut line| {
                for span in &mut line.spans {
                    span.style = body_style.patch(span.style);
                }
                line
            })
            .collect()
    }
}

/// The parts of an event's calls, in drawing order: per call its name, then each input and
/// output section as a label and a body, with a blank line between calls.
pub(super) fn parts(event: &Event) -> Vec<Part> {
    let label_style = super::style::log_styles(&event.kind).0;
    let error_style = super::style::log_styles(&Kind::Error).0;
    let mut parts = Vec::new();
    for (index, call) in event.calls.iter().enumerate() {
        if index > 0 {
            parts.push(Part::new("", Body::Blank));
        }
        parts.push(Part::new(call.name.clone(), Body::Label(label_style)));
        for section in &call.input {
            push_section(&mut parts, section, label_style);
        }
        match &call.output {
            Some(output) => {
                let style = if call.is_error {
                    error_style
                } else {
                    label_style
                };
                for section in output {
                    push_section(&mut parts, section, style);
                }
            }
            None => parts.push(Part::new("running…", Body::Running)),
        }
    }
    parts
}

fn push_section(parts: &mut Vec<Part>, section: &Section, label_style: Style) {
    let (label, body) = match section {
        Section::Fields(fields) => {
            let text = fields
                .iter()
                .map(|(key, value)| format!("{key}: {value}"))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(Part::new(cells(&text), Body::Fields));
            return;
        }
        Section::Code { label, lang, text } => {
            (label, Part::new(cells(text), Body::Code(lang.clone())))
        }
        Section::Diff { label, text } => (label, Part::new(cells(text), Body::Diff)),
        Section::Markdown { label, text } => (label, Part::new(cells(text), Body::Markdown)),
        Section::Text { label, text } => (label, Part::new(cells(text), Body::Text)),
    };
    if !label.is_empty() {
        parts.push(Part::new(label.clone(), Body::Label(label_style)));
    }
    parts.push(body);
}

/// `text` as it occupies cells: tabs as four spaces, so the drawn cells map back to the
/// copied text one grapheme each, and without trailing line breaks.
fn cells(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}

/// `lines` with every line wider than `width` split at grapheme boundaries.
fn split_wide(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return lines;
    }
    let mut split = Vec::new();
    for line in lines {
        if line.width() <= width {
            split.push(line);
            continue;
        }
        let mut current = Line::default();
        let mut used = 0;
        for span in line.spans {
            let mut text = String::new();
            for grapheme in span.content.graphemes(true) {
                let grapheme_width = grapheme.width();
                if used + grapheme_width > width && used > 0 {
                    if !text.is_empty() {
                        current
                            .spans
                            .push(Span::styled(std::mem::take(&mut text), span.style));
                    }
                    split.push(std::mem::take(&mut current));
                    used = 0;
                }
                text.push_str(grapheme);
                used += grapheme_width;
            }
            if !text.is_empty() {
                current.spans.push(Span::styled(text, span.style));
            }
        }
        split.push(current);
    }
    split
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::ToolCall;

    fn event(calls: Vec<ToolCall>) -> Event {
        Event {
            kind: Kind::Tool,
            body: String::new(),
            paths: Vec::new(),
            started_at: chrono::Utc::now(),
            ended_at: None,
            duration: None,
            previous_end: None,
            images: Vec::new(),
            calls,
        }
    }

    fn texts(parts: &[Part], width: usize) -> Vec<String> {
        parts
            .iter()
            .flat_map(|part| part.lines(width, Style::default()))
            .map(|line| line.to_string())
            .collect()
    }

    #[test]
    fn calls_become_name_label_body_parts_with_a_running_note() {
        let parts = parts(&event(vec![
            ToolCall {
                name: "Bash".into(),
                input: vec![Section::Code {
                    label: "command".into(),
                    lang: "bash".into(),
                    text: "cargo test\n".into(),
                }],
                output: Some(vec![Section::Text {
                    label: "stdout".into(),
                    text: "running 3 tests\n...".into(),
                }]),
                is_error: false,
            },
            ToolCall {
                name: "Read".into(),
                input: vec![Section::Fields(vec![
                    ("path".into(), "agents/src/log.rs".into()),
                    ("range".into(), "lines 1–40".into()),
                ])],
                output: None,
                is_error: false,
            },
        ]));
        assert_eq!(
            parts
                .iter()
                .map(|part| part.source.as_str())
                .collect::<Vec<_>>(),
            [
                "Bash",
                "command",
                "cargo test",
                "stdout",
                "running 3 tests\n...",
                "",
                "Read",
                "path: agents/src/log.rs\nrange: lines 1–40",
                "running…",
            ]
        );
        assert_eq!(
            texts(&parts, 80),
            [
                "Bash",
                "command",
                "cargo test",
                "stdout",
                "running 3 tests",
                "...",
                "",
                "Read",
                "path: agents/src/log.rs",
                "range: lines 1–40",
                "running…",
            ]
        );
    }

    #[test]
    fn wide_code_and_diff_lines_split_at_grapheme_boundaries() {
        let parts = parts(&event(vec![ToolCall {
            name: "Edit".into(),
            input: vec![Section::Diff {
                label: "a.rs".into(),
                text: "+中文字符串很长\n-short".into(),
            }],
            output: Some(vec![Section::Code {
                label: String::new(),
                lang: "rs".into(),
                text: "let value = \"abcdefghij\";".into(),
            }]),
            is_error: false,
        }]));
        assert_eq!(
            texts(&parts, 9),
            [
                "Edit",
                "a.rs",
                "+中文字符",
                "串很长",
                "-short",
                "let value",
                " = \"abcde",
                "fghij\";",
            ]
        );
        let lines = parts[3].lines(9, Style::default());
        assert!(lines.iter().all(|line| line.width() <= 9));
        assert!(lines[0].spans.len() > 1);
    }

    #[test]
    fn errors_label_output_in_the_error_style_and_tabs_become_spaces() {
        let parts = parts(&event(vec![ToolCall {
            name: "Bash".into(),
            input: vec![],
            output: Some(vec![Section::Text {
                label: "stderr".into(),
                text: "\tboom".into(),
            }]),
            is_error: true,
        }]));
        let lines = parts[1].lines(80, Style::default());
        assert_eq!(
            lines[0].spans[0].style,
            super::super::style::log_styles(&Kind::Error).0
        );
        assert_eq!(parts[2].source, "    boom");
    }
}
