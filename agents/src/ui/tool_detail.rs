//! The calls of an open tool row as the parts the log panel draws and copies.

use super::highlight;
use crate::log::{Event, Kind, Section};
use ratatui::style::{Style, Stylize};
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
    /// A merged call's right-aligned title.
    Title(Style),
    /// A section label, in the given style.
    Label(Style),
    /// A full-width call separator, or a centred input/output arrow.
    Rule(bool),
    /// The dim note that a call has no output yet.
    Running,
    /// `key: value` lines.
    Fields(Vec<std::ops::Range<usize>>),
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

    /// Text fragments use the shared Markdown renderer; JSON fragments are copied as drawn.
    pub fn text_parts(source: String, markdown: bool) -> Vec<Self> {
        let mut parts = Vec::new();
        let mut cursor = 0;
        let mut text_start = 0;
        while cursor < source.len() {
            let byte = source.as_bytes()[cursor];
            if matches!(byte, b'{' | b'[')
                && let Some((end, value)) = embedded_json(&source, cursor)
                && (value.as_object().is_some_and(|object| !object.is_empty())
                    || value.as_array().is_some_and(|array| !array.is_empty()))
            {
                if text_start < cursor {
                    if let Some(text) =
                        text_boundary(&source[text_start..cursor], text_start > 0, true)
                    {
                        parts.push(Self::text_fragment(text.into(), markdown));
                    }
                }
                parts.push(Self::new(
                    serde_json::to_string_pretty(&value).expect("JSON value serializes"),
                    Body::Code("json".into()),
                ));
                cursor = end;
                text_start = end;
            } else {
                cursor += 1;
            }
        }
        if text_start < source.len() || parts.is_empty() {
            if let Some(text) = text_boundary(&source[text_start..], text_start > 0, false) {
                parts.push(Self::text_fragment(text.into(), markdown));
            }
        }
        parts
    }

    fn text_fragment(source: String, markdown: bool) -> Self {
        let markdown = markdown || super::markdown::has_markers(&source);
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    pub fn text(source: String, markdown: bool) -> Self {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&source)
            && (value.is_object() || value.is_array())
        {
            return Self::new(
                serde_json::to_string_pretty(&value).expect("JSON value serializes"),
                Body::Code("json".into()),
            );
        }
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    /// Whether the selection maps the drawn lines back to the source as Markdown.
    pub fn markdown(&self) -> bool {
        matches!(self.body, Body::Markdown)
    }

    /// Generated title padding has no source bytes to select.
    pub fn maps(&self, lines: &[Line<'_>]) -> Vec<Vec<super::text_selection_layout::CellMap>> {
        if !matches!(self.body, Body::Title(_)) {
            return super::text_selection_layout::map_lines(&self.source, lines, self.markdown());
        }
        let mut content = lines.to_vec();
        let padding: Vec<_> = content
            .iter_mut()
            .map(|line| line.spans.remove(0).width() as u16)
            .collect();
        let mut maps = super::text_selection_layout::map_lines(&self.source, &content, false);
        for (cells, padding) in maps.iter_mut().zip(padding) {
            for (range, _) in cells {
                range.start += padding;
                range.end += padding;
            }
        }
        maps
    }

    /// The part drawn `width` columns wide; every line fits so each cell maps to its source.
    pub fn lines(&self, width: usize, body_style: Style) -> Vec<Line<'static>> {
        let drawn = self.source.replace('\t', "    ");
        let source = drawn.as_str();
        let options = super::line_truncation::prose_wrap_options(width);
        let lines = match &self.body {
            Body::Title(style) | Body::Label(style) => split_wide(
                vec![Line::from(Span::styled(source.to_owned(), *style))],
                width,
            ),
            Body::Rule(arrow) => {
                if *arrow {
                    let mut row = " ".repeat(width);
                    if width > 0 {
                        row.replace_range(width / 2..width / 2 + 1, "↓");
                    }
                    vec![Line::styled(row, super::style::log_jump_style())]
                } else {
                    vec![Line::from("─".repeat(width)).dim()]
                }
            }
            Body::Running => vec![Line::from(Span::styled(
                source.to_owned(),
                Style::new().dim(),
            ))],
            Body::Fields(keys) => {
                let key_style = Style::default().fg(highlight::json_key_foreground());
                let mut offset = 0;
                let lines = self.source.split_inclusive('\n').map(|raw| {
                    let text = raw.lines().next().expect("split field line exists");
                    let end = offset + text.len();
                    let key = keys.iter().find(|key| key.start < end && key.end > offset);
                    let line = if let Some(key) = key {
                        let start = key.start.saturating_sub(offset);
                        let end = key.end.min(end) - offset;
                        let mut spans = Vec::new();
                        if start > 0 {
                            spans.push(Span::raw(text[..start].replace('\t', "    ")));
                        }
                        spans.push(Span::styled(
                            text[start..end].replace('\t', "    "),
                            key_style,
                        ));
                        spans.push(Span::raw(text[end..].replace('\t', "    ")));
                        Line::from(spans)
                    } else {
                        Line::from(text.replace('\t', "    "))
                    };
                    offset += raw.len();
                    line
                });
                split_wide(lines.collect(), width)
            }
            Body::Code(lang) => split_wide(highlight::code_lines(source, lang), width),
            Body::Diff => split_wide(highlight::diff_lines(source), width),
            Body::Markdown => super::markdown::render_with_options(source, options),
            Body::Text => textwrap::wrap(source, options)
                .into_iter()
                .map(|line| Line::from(line.into_owned()))
                .collect(),
        };
        if matches!(self.body, Body::Rule(_)) {
            return lines;
        }
        lines
            .into_iter()
            .map(|mut line| {
                if matches!(self.body, Body::Title(_)) {
                    let padding = width.saturating_sub(line.width());
                    line.spans.insert(0, Span::raw(" ".repeat(padding)));
                }
                for span in &mut line.spans {
                    span.style = body_style.patch(span.style);
                }
                line
            })
            .collect()
    }
}

/// Per-call inputs and outputs, with titles for merged calls and display-only rules.
pub(super) fn parts(event: &Event) -> Vec<Part> {
    let label_style = super::style::log_styles(&event.kind).0;
    let error_style = super::style::log_styles(&Kind::Error).0;
    let mut parts = Vec::new();
    for (index, call) in event.calls.iter().enumerate() {
        if index > 0 {
            parts.push(Part::new("", Body::Rule(false)));
        }
        if event.calls.len() > 1 {
            parts.push(Part::new(call.title.clone(), Body::Title(label_style)));
        }
        for section in &call.input {
            push_section(&mut parts, section, label_style, false);
        }
        match &call.output {
            Some(output) => {
                parts.push(Part::new("", Body::Rule(true)));
                if output.is_empty() {
                    parts.push(Part::new("(no output)", Body::Text));
                }
                let style = if call.is_error {
                    error_style
                } else {
                    label_style
                };
                for section in output {
                    push_section(&mut parts, section, style, true);
                }
            }
            None => parts.push(Part::new("running…", Body::Running)),
        }
    }
    parts
}

fn push_section(parts: &mut Vec<Part>, section: &Section, label_style: Style, text_output: bool) {
    let (label, body) = match section {
        Section::Fields(fields) => {
            let mut text = String::new();
            let mut keys = Vec::new();
            for (key, value) in fields {
                if !text.is_empty() {
                    text.push('\n');
                }
                keys.push(text.len()..text.len() + key.len());
                text.push_str(key);
                text.push_str(": ");
                text.push_str(value);
            }
            parts.push(Part::new(cells(&text), Body::Fields(keys)));
            return;
        }
        Section::Code { label, lang, text } => {
            (label, Part::new(cells(text), Body::Code(lang.clone())))
        }
        Section::Diff { label, text } => (label, Part::new(cells(text), Body::Diff)),
        Section::Markdown { label, text } => (label, Part::text(cells(text), true)),
        Section::Text { label, text } => {
            if label == "stderr" {
                parts.push(Part::new(label.clone(), Body::Label(label_style)));
            }
            if text_output {
                parts.extend(Part::text_parts(cells(text), false));
            } else {
                parts.push(Part::text(cells(text), false));
            }
            return;
        }
    };
    if matches!(section, Section::Diff { .. }) || label == "stderr" {
        parts.push(Part::new(label.clone(), Body::Label(label_style)));
    }
    parts.push(body);
}

/// A JSON block boundary already supplies the adjacent line break.
fn text_boundary(mut source: &str, after_json: bool, before_json: bool) -> Option<&str> {
    if after_json && before_json && matches!(source, "\n" | "\r\n") {
        return None;
    }
    if after_json {
        source = source
            .strip_prefix("\r\n")
            .or_else(|| source.strip_prefix('\n'))
            .unwrap_or(source);
    }
    if before_json {
        source = source
            .strip_suffix("\r\n")
            .or_else(|| source.strip_suffix('\n'))
            .unwrap_or(source);
    }
    Some(source)
}

/// Parse one complete balanced JSON value without scanning the trailing output.
fn embedded_json(source: &str, start: usize) -> Option<(usize, serde_json::Value)> {
    let mut values =
        serde_json::Deserializer::from_str(&source[start..]).into_iter::<serde_json::Value>();
    let value = values.next()?.ok()?;
    Some((start + values.byte_offset(), value))
}

/// Copy sources preserve tabs and omit trailing line breaks.
fn cells(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).to_owned()
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
    fn embedded_json_keeps_surroundings_and_pretty_copy_sources() {
        let parts = Part::text_parts(
            "before {\"text\":\"} \\\" [\",\"items\":[1,2]} after [3,4] end".into(),
            false,
        );
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].source, "before ");
        assert!(matches!(parts[1].body, Body::Code(ref lang) if lang == "json"));
        assert!(parts[1].source.contains("\n  \"items\": ["));
        assert_eq!(parts[2].source, " after ");
        assert_eq!(parts[3].source, "[\n  3,\n  4\n]");
        assert_eq!(parts[4].source, " end");
        for part in &parts {
            let lines = part.lines(80, Style::default());
            let maps = super::super::text_selection_layout::map_lines(
                &part.source,
                &lines,
                part.markdown(),
            );
            for (line, map) in lines.iter().zip(maps) {
                if matches!(part.body, Body::Code(_)) {
                    assert_eq!(
                        map.iter()
                            .map(|(cells, _)| (cells.end - cells.start) as usize)
                            .sum::<usize>(),
                        line.width()
                    );
                }
                for (cell, bytes) in map {
                    assert_eq!(
                        &line.to_string()[cell.start as usize..cell.end as usize],
                        &part.source[bytes]
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent_json_blocks_do_not_duplicate_boundary_newlines() {
        for newline in ["\n", "\r\n"] {
            let source = format!("before{newline}{{\"ok\":true}}{newline}after");
            let parts = Part::text_parts(source, false);
            assert_eq!(
                parts
                    .iter()
                    .map(|part| part.source.as_str())
                    .collect::<Vec<_>>(),
                ["before", "{\n  \"ok\": true\n}", "after"]
            );
            assert_eq!(
                texts(&parts, 80),
                ["before", "{", "  \"ok\": true", "}", "after"]
            );
        }
    }

    #[test]
    fn json_boundaries_preserve_surrounding_blank_lines() {
        for newline in ["\n", "\r\n"] {
            for count in 1..=3 {
                let gap = newline.repeat(count);
                let source = format!("{gap}{{\"a\":1}}{gap}{{\"b\":2}}{gap}");
                let parts = Part::text_parts(source, false);
                let copy = parts
                    .iter()
                    .map(|part| part.source.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(
                    copy.replace("\r\n", "\n"),
                    format!("{gap}{{\n  \"a\": 1\n}}{gap}{{\n  \"b\": 2\n}}{gap}")
                        .replace("\r\n", "\n")
                );
            }
        }
    }

    #[test]
    fn invalid_outer_json_does_not_hide_valid_inner_or_trivial_text() {
        let parts = Part::text_parts("{broken: {\"ok\":true}} {} [] [bad]".into(), false);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].source, "{broken: ");
        assert_eq!(parts[1].source, "{\n  \"ok\": true\n}");
        assert_eq!(parts[2].source, "} {} [] [bad]");
        assert_eq!(Part::text_parts("{} []".into(), false)[0].source, "{} []");
    }

    #[test]
    fn text_outputs_detect_markdown_but_code_keeps_source_and_language() {
        for source in [
            "# title",
            "```rust\ncode\n```",
            "- item",
            "**bold**",
            "`code`",
            "[link](https://example.com)",
            "| a | b |\n| --- | --- |\n| 1 | 2 |",
        ] {
            assert!(
                Part::text_parts(source.into(), false)[0].markdown(),
                "{source}"
            );
        }
        let plain = Part::text_parts("one\ntwo\n\nthree".into(), false);
        assert!(!plain[0].markdown());
        assert_eq!(texts(&plain, 80), ["one", "two", "", "three"]);
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Code {
                label: String::new(),
                lang: "rust".into(),
                text: "{\"ok\":true}".into(),
            },
            Style::default(),
            true,
        );
        assert_eq!(parts[0].source, "{\"ok\":true}");
        assert!(matches!(parts[0].body, Body::Code(ref lang) if lang == "rust"));
    }

    #[test]
    fn fields_keys_match_json_key_foregrounds() {
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Fields(vec![("path".into(), "first\nvalue: unchanged".into())]),
            Style::default(),
            false,
        );
        let json = highlight::code_lines(r#"{"path": "first"}"#, "json");
        let key = json[0]
            .spans
            .iter()
            .find(|span| span.content.contains("path"))
            .unwrap();
        assert!(key.style.fg.is_some());
        let body_style = Style::default().fg(ratatui::style::Color::Gray);
        let lines = parts[0].lines(80, body_style);
        assert_eq!(lines[0].spans[0].content, "path");
        assert_eq!(lines[0].spans[0].style.fg, key.style.fg);
        assert_eq!(lines[0].spans[1].content, ": first");
        assert_eq!(lines[0].spans[1].style, body_style);
        assert_eq!(lines[1].to_string(), "value: unchanged");
        assert!(lines[1].spans.iter().all(|span| span.style == body_style));
        assert_eq!(parts[0].source, "path: first\nvalue: unchanged");
        for (line, cells) in lines.iter().zip(parts[0].maps(&lines)) {
            let copied: String = cells
                .iter()
                .map(|(_, bytes)| &parts[0].source[bytes.clone()])
                .collect();
            assert_eq!(copied, line.to_string());
        }
    }

    #[test]
    fn review_wide_label_fits_drawn_cells() {
        let part = Part::new(
            "/workspace/very/long/path/to/source.rs",
            Body::Label(Style::default()),
        );
        assert!(
            part.lines(8, Style::default())
                .iter()
                .all(|line| line.width() <= 8)
        );
    }

    #[test]
    fn review_tab_copy_source_keeps_tabs() {
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Text {
                label: String::new(),
                text: "a\tb".into(),
            },
            Style::default(),
            true,
        );
        assert_eq!(parts[0].source, "a\tb");
        assert_eq!(texts(&parts, 80), ["a    b"]);
    }

    #[test]
    fn merged_calls_have_titles_and_rules_with_a_running_note() {
        let parts = parts(&event(vec![
            ToolCall {
                id: "Bash-fixture".into(),
                title: "Bash".into(),
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
                id: "Read-fixture".into(),
                title: "Read".into(),
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
                "cargo test",
                "",
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
                &format!("{}Bash", " ".repeat(76)),
                "cargo test",
                &format!("{}↓{}", " ".repeat(40), " ".repeat(39)),
                "running 3 tests",
                "...",
                &"─".repeat(80),
                &format!("{}Read", " ".repeat(76)),
                "path: agents/src/log.rs",
                "range: lines 1–40",
                "running…",
            ]
        );
    }

    #[test]
    fn wide_code_and_diff_lines_split_at_grapheme_boundaries() {
        let parts = parts(&event(vec![ToolCall {
            id: "Edit-fixture".into(),
            title: "Edit".into(),
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
                "a.rs",
                "+中文字符",
                "串很长",
                "-short",
                "    ↓    ",
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
            id: "Bash-fixture".into(),
            title: "Bash".into(),
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
        assert_eq!(parts[2].source, "\tboom");
    }
}
