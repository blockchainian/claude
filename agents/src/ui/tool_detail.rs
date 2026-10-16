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
    /// Parts sharing a section use the same horizontal offset.
    pub group: Option<usize>,
    body: Body,
}

enum Body {
    /// A merged call's title after a leading rule cell and space.
    TitleRule,
    /// A section label, in the given style.
    Label(Style),
    /// An input/output arrow without selectable source bytes.
    Arrow,
    /// The dim animation that a call has no output yet.
    Running,
    /// `key: value` lines.
    Fields(Vec<std::ops::Range<usize>>),
    /// Code highlighted as the given language.
    Code(String),
    /// Shell command ranges retain original bytes while separators are display-only.
    Shell(Vec<super::shell_commands::Command>),
    /// Parsed JSON with multiline string values.
    Json(serde_json::Value),
    Diff,
    Markdown,
    /// Prose wrapped at the panel width.
    Text,
}

impl Part {
    fn new(source: impl Into<String>, body: Body) -> Self {
        Part {
            source: source.into(),
            group: None,
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
            let (end, value) = if matches!(byte, b'{' | b'[') {
                embedded_json(&source, cursor)
            } else {
                (cursor + 1, None)
            };
            if let Some(value) = value.filter(|value| {
                value.as_object().is_some_and(|object| !object.is_empty())
                    || value.as_array().is_some_and(|array| !array.is_empty())
            }) {
                if text_start < cursor
                    && let Some(text) =
                        text_boundary(&source[text_start..cursor], text_start > 0, true)
                {
                    parts.push(Self::text_fragment(text.into(), markdown));
                }
                parts.push(Self::json(value));
                cursor = end;
                text_start = end;
            } else {
                cursor = end;
            }
        }
        if (text_start < source.len() || parts.is_empty())
            && let Some(text) = text_boundary(&source[text_start..], text_start > 0, false)
        {
            parts.push(Self::text_fragment(text.into(), markdown));
        }
        for part in &mut parts {
            part.group = Some(0);
        }
        parts
    }

    fn json(value: serde_json::Value) -> Self {
        let source = json_lines(&value)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        Self::new(source, Body::Json(value))
    }

    fn text_fragment(source: String, markdown: bool) -> Self {
        let markdown = markdown || super::markdown::has_markers(&source);
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    pub fn text(source: String, markdown: bool) -> Self {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&source)
            && (value.is_object() || value.is_array())
        {
            return Self::json(value);
        }
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    /// Whether the selection maps the drawn lines back to the source as Markdown.
    pub fn markdown(&self) -> bool {
        matches!(self.body, Body::Markdown)
    }

    pub fn running(&self) -> bool {
        matches!(self.body, Body::Running)
    }

    /// Generated rules and title padding have no source bytes to select.
    pub fn maps(&self, lines: &[Line<'_>]) -> Vec<Vec<super::text_selection_layout::CellMap>> {
        if self.running() {
            return vec![Vec::new(); lines.len()];
        }
        if let Body::Shell(commands) = &self.body {
            return shell_maps(&self.source, commands, lines);
        }
        if !matches!(self.body, Body::TitleRule) {
            return super::text_selection_layout::map_lines(&self.source, lines, self.markdown());
        }
        lines
            .iter()
            .map(|line| {
                if line.spans.len() < 4 {
                    return Vec::new();
                }
                let padding = line.spans[0].width() as u16;
                let mut content = Line::from(line.spans[1..line.spans.len() - 2].to_vec());
                let truncated = content.width() > 0
                    && self.source.replace('\t', "    ").width() > content.width();
                if truncated {
                    content.spans.pop(); // The ellipsis selects the remaining title bytes.
                }
                let mut maps = super::text_selection_layout::map_lines(
                    &self.source,
                    std::slice::from_ref(&content),
                    false,
                );
                let cells = &mut maps[0];
                if truncated {
                    let start = cells.last().map_or(0, |(_, bytes)| bytes.end);
                    let column = content.width() as u16;
                    cells.push((column..column + 1, start..self.source.len()));
                }
                for (range, _) in cells.iter_mut() {
                    range.start += padding;
                    range.end += padding;
                }
                maps.remove(0)
            })
            .collect()
    }

    /// The part drawn `width` columns wide; every line fits so each cell maps to its source.
    pub fn lines(
        &self,
        width: usize,
        body_style: Style,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Line<'static>> {
        let drawn = self.source.replace('\t', "    ");
        let source = drawn.as_str();
        let options = super::line_truncation::prose_wrap_options(width);
        let lines = match &self.body {
            Body::TitleRule => vec![super::title_rule::centered_title_rule(source, width)],
            Body::Label(style) => split_wide(
                vec![Line::from(Span::styled(source.to_owned(), *style))],
                width,
            ),
            Body::Arrow => {
                let row = if width > 0 { "↓" } else { "" };
                vec![Line::styled(row, super::style::log_jump_style())]
            }
            Body::Running => vec![Line::from(Span::styled(
                running_frame(now),
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
            Body::Shell(commands) => shell_lines(&self.source, commands, width),
            Body::Json(value) => split_wide(json_lines(value), width),
            Body::Diff => split_wide(highlight::diff_lines(source), width),
            Body::Markdown => super::markdown::render_with_options(source, options),
            Body::Text => textwrap::wrap(source, options)
                .into_iter()
                .map(|line| Line::from(line.into_owned()))
                .collect(),
        };
        if matches!(self.body, Body::TitleRule | Body::Arrow) {
            return lines;
        }
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

/// Use original source offsets for selection, including separators between selected rows.
fn shell_part(source: String, prefix: usize) -> Part {
    let Some(mut commands) = super::shell_commands::split(&source[prefix..]) else {
        return Part::new(source, Body::Code("bash".into()));
    };
    for command in &mut commands {
        command.range.start += prefix;
        command.range.end += prefix;
    }
    if prefix > 0
        && let Some(first) = commands.first_mut()
    {
        first.range.start = 0;
    }
    Part::new(source, Body::Shell(commands))
}

fn shell_lines(
    source: &str,
    commands: &[super::shell_commands::Command],
    width: usize,
) -> Vec<Line<'static>> {
    let connected = commands.iter().any(|command| command.and_next);
    let text_width = if connected && width > 2 {
        width - 2
    } else {
        width
    };
    let mut lines = Vec::new();
    let mut rows = Vec::new();
    for command in commands {
        let start = lines.len();
        let text = source[command.range.clone()].replace('\t', "    ");
        lines.extend(split_wide(highlight::code_lines(&text, "bash"), text_width));
        rows.push(start..lines.len());
    }
    let mut connectors = vec![std::collections::BTreeMap::new(); lines.len()];
    for (index, command) in commands.iter().enumerate() {
        if !command.and_next || index + 1 == commands.len() {
            continue;
        }
        let upper = rows[index].end - 1;
        let lower = rows[index + 1].start;
        let column = lines[upper].width().max(lines[lower].width()) + 1;
        if width > 0 && column >= width {
            continue;
        }
        for (row, glyph) in [(upper, '┐'), (lower, '┘')] {
            connectors[row]
                .entry(column)
                .and_modify(|ch| *ch = '┤')
                .or_insert(glyph);
        }
    }
    for (line, connectors) in lines.iter_mut().zip(connectors) {
        for (column, glyph) in connectors {
            // Decorations have their own dim spans and never receive a source map.
            let padding = column.saturating_sub(line.width());
            line.spans.push(Span::styled(
                format!("{}{glyph}", " ".repeat(padding)),
                Style::new().dim(),
            ));
        }
    }
    lines
}

fn shell_maps(
    source: &str,
    commands: &[super::shell_commands::Command],
    lines: &[Line<'_>],
) -> Vec<Vec<super::text_selection_layout::CellMap>> {
    let mut displayed = String::new();
    let mut ranges = Vec::new();
    for command in commands {
        if !displayed.is_empty() {
            displayed.push('\n');
        }
        let start = displayed.len();
        displayed.push_str(&source[command.range.clone()]);
        ranges.push((start..displayed.len(), command.range.start));
    }
    let content: Vec<_> = lines
        .iter()
        .map(|line| {
            let mut line = line.clone();
            while line.spans.last().is_some_and(|span| {
                span.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::DIM)
                    && span.content.contains(['┐', '┘', '┤'])
            }) {
                line.spans.pop();
            }
            line
        })
        .collect();
    let mut maps = super::text_selection_layout::map_lines(&displayed, &content, false);
    for cells in &mut maps {
        for (_, bytes) in cells {
            let (range, original) = ranges
                .iter()
                .find(|(range, _)| range.contains(&bytes.start))
                .expect("shell cell belongs to command");
            *bytes = original + bytes.start - range.start..original + bytes.end - range.start;
        }
    }
    maps
}

/// Pretty JSON with string line breaks aligned to the opening content column.
fn json_lines(value: &serde_json::Value) -> Vec<Line<'static>> {
    use highlight::JsonToken;

    struct Printer {
        lines: Vec<Line<'static>>,
        styles: [Style; 7],
    }

    impl Printer {
        fn push(&mut self, text: impl Into<String>, token: JsonToken) {
            self.lines
                .last_mut()
                .expect("JSON line exists")
                .spans
                .push(Span::styled(text.into(), self.styles[token as usize]));
        }

        fn newline(&mut self, indent: usize) {
            self.lines.push(Line::from(" ".repeat(indent)));
        }

        fn string(&mut self, text: &str, token: JsonToken, multiline: bool) {
            let quote = if multiline {
                JsonToken::StringQuote
            } else {
                token
            };
            self.push("\"", quote);
            let column = self.lines.last().expect("JSON line exists").width();
            // Serialize each logical line so all other JSON escapes retain their spelling.
            let normalized = if multiline {
                text.replace("\r\n", "\n")
            } else {
                text.into()
            };
            for (index, line) in normalized.split_inclusive('\n').enumerate() {
                if index > 0 && multiline {
                    self.newline(column);
                }
                let line = if multiline {
                    line.strip_suffix('\n').unwrap_or(line)
                } else {
                    line
                };
                let encoded = serde_json::to_string(line).expect("JSON string serializes");
                let content = &encoded[1..encoded.len() - 1];
                let mut start = 0;
                let mut cursor = 0;
                while cursor < content.len() {
                    if content.as_bytes()[cursor] != b'\\' {
                        cursor += 1;
                        continue;
                    }
                    self.push(&content[start..cursor], token);
                    let end = cursor
                        + if content.as_bytes()[cursor + 1] == b'u' {
                            6
                        } else {
                            2
                        };
                    self.push(&content[cursor..end], JsonToken::Escape);
                    cursor = end;
                    start = end;
                }
                self.push(&content[start..], token);
            }
            if multiline && normalized.ends_with('\n') {
                self.newline(column);
            }
            self.push("\"", quote);
        }

        fn value(&mut self, value: &serde_json::Value, indent: usize) {
            use serde_json::Value;
            match value {
                Value::Object(values) => {
                    self.push("{", JsonToken::Punctuation);
                    for (index, (key, value)) in values.iter().enumerate() {
                        self.newline(indent + 2);
                        self.string(key, JsonToken::Key, false);
                        self.push(": ", JsonToken::Punctuation);
                        self.value(value, indent + 2);
                        if index + 1 < values.len() {
                            self.push(",", JsonToken::Punctuation);
                        }
                    }
                    if !values.is_empty() {
                        self.newline(indent);
                    }
                    self.push("}", JsonToken::Punctuation);
                }
                Value::Array(values) => {
                    self.push("[", JsonToken::Punctuation);
                    for (index, value) in values.iter().enumerate() {
                        self.newline(indent + 2);
                        self.value(value, indent + 2);
                        if index + 1 < values.len() {
                            self.push(",", JsonToken::Punctuation);
                        }
                    }
                    if !values.is_empty() {
                        self.newline(indent);
                    }
                    self.push("]", JsonToken::Punctuation);
                }
                Value::String(text) => self.string(text, JsonToken::String, true),
                Value::Number(number) => self.push(number.to_string(), JsonToken::Number),
                Value::Bool(_) | Value::Null => self.push(value.to_string(), JsonToken::Literal),
            }
        }
    }

    let mut printer = Printer {
        lines: vec![Line::default()],
        styles: highlight::json_styles(),
    };
    printer.value(value, 0);
    printer.lines
}

/// Per-call inputs and outputs, with titles for merged calls and display-only rules.
pub(super) fn parts(event: &Event) -> Vec<Part> {
    let label_style = super::style::log_styles(&event.kind).0;
    let error_style = super::style::log_styles(&Kind::Error).0;
    let mut parts = Vec::new();
    for (index, call) in event.calls.iter().enumerate() {
        if index > 0 {
            // A blank row ends the previous call's output.
            parts.push(Part::new("", Body::Text));
        }
        if event.calls.len() > 1 {
            parts.push(Part::new(call.title.clone(), Body::TitleRule));
        }
        let input_start = parts.len();
        for section in &call.input {
            push_section(&mut parts, section, label_style, false);
        }
        for part in &mut parts[input_start..] {
            part.group = Some(input_start);
        }
        if let Some(code) = call.exit_code {
            parts.push(Part::new("", Body::Arrow));
            let status_start = parts.len();
            push_section(
                &mut parts,
                &Section::Fields(vec![("exit code".into(), code.to_string())]),
                label_style,
                false,
            );
            for part in &mut parts[status_start..] {
                part.group = Some(status_start);
            }
        }
        match &call.output {
            Some(output) => {
                parts.push(Part::new("", Body::Arrow));
                let output_start = parts.len();
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
                for part in &mut parts[output_start..] {
                    part.group = Some(output_start);
                }
            }
            None => parts.push(Part::new("", Body::Running)),
        }
    }
    parts
}

fn push_section(parts: &mut Vec<Part>, section: &Section, label_style: Style, text_output: bool) {
    let (label, body) = match section {
        Section::Fields(fields) => {
            if !text_output
                && fields
                    .iter()
                    .any(|(key, _)| matches!(key.as_str(), "command" | "cmd"))
            {
                for (key, value) in fields {
                    if matches!(key.as_str(), "command" | "cmd") {
                        parts.push(shell_part(format!("{key}: {value}"), key.len() + 2));
                    } else {
                        push_section(
                            parts,
                            &Section::Fields(vec![(key.clone(), value.clone())]),
                            label_style,
                            text_output,
                        );
                    }
                }
                return;
            }
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
            if !text_output && label == "command" && lang == "bash" {
                parts.push(shell_part(text.clone(), 0));
                return;
            }
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

/// Scan each candidate once, then parse only its balanced span. Failed spans stay prose.
fn embedded_json(source: &str, start: usize) -> (usize, Option<serde_json::Value>) {
    if source.as_bytes()[start] == b'['
        && source[..start]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | ']' | ')' | '"'))
    {
        return (start + 1, None);
    }
    let mut brackets = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for (offset, &byte) in source.as_bytes()[start..].iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => brackets.push(byte),
            b'}' | b']' => {
                let end = start + offset + 1;
                if brackets.pop() != Some(if byte == b'}' { b'{' } else { b'[' }) {
                    return (end, None);
                }
                if brackets.is_empty() {
                    return (end, serde_json::from_str(&source[start..end]).ok());
                }
            }
            _ => {}
        }
    }
    (source.len(), None)
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

pub(super) fn running_frame(now: chrono::DateTime<chrono::Utc>) -> &'static str {
    ["•··", "·•·", "··•"][now.timestamp_millis().div_euclid(300).rem_euclid(3) as usize]
}

pub(super) fn running_redraw_in(now: chrono::DateTime<chrono::Utc>) -> std::time::Duration {
    std::time::Duration::from_millis((300 - now.timestamp_millis().rem_euclid(300)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::ToolCall;

    fn event(calls: Vec<ToolCall>) -> Event {
        Event {
            source: None,
            sender: None,
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
            .flat_map(|part| part.lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH))
            .map(|line| line.to_string())
            .collect()
    }

    #[test]
    fn running_frames_follow_wall_time_and_cycle() {
        let start = chrono::DateTime::UNIX_EPOCH;
        for (millis, frame) in [
            (0, "•··"),
            (299, "•··"),
            (300, "·•·"),
            (600, "··•"),
            (900, "•··"),
        ] {
            assert_eq!(
                running_frame(start + chrono::Duration::milliseconds(millis)),
                frame
            );
            assert_eq!(frame.width(), 3);
        }
        assert_eq!("•".width(), 1);
        assert_eq!("·".width(), 1);
    }

    #[test]
    fn running_part_draws_the_supplied_time_in_dim_style() {
        let part = Part::new("", Body::Running);
        let now = chrono::DateTime::UNIX_EPOCH + chrono::Duration::milliseconds(600);
        let lines = part.lines(80, Style::default(), now);
        assert_eq!(lines[0].to_string(), "··•");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::DIM)
        );
    }

    #[test]
    fn running_part_has_no_copy_source_or_selectable_cells() {
        let parts = parts(&event(vec![ToolCall {
            id: "running-copy".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![],
            output: None,
            exit_code: None,
            is_error: false,
        }]));
        let part = parts.last().unwrap();
        let lines = part.lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert!(part.source.is_empty());
        assert!(part.maps(&lines)[0].is_empty());
    }

    #[test]
    fn manager_await_json_displays_multiline_strings_and_copies_drawn_text() {
        let source = include_str!("../../tests/fixtures/codex-manager-await.jsonl").trim_end();
        let parts = Part::text_parts(source.into(), false);
        assert_eq!(parts.len(), 1);
        let part = &parts[0];
        let lines = part.lines(200, Style::default(), chrono::DateTime::UNIX_EPOCH);
        let text: Vec<_> = lines.iter().map(ToString::to_string).collect();
        let first = text
            .iter()
            .position(|line| line.contains("已提交推送"))
            .unwrap();
        let column = text[first].find("已提交推送").unwrap();
        assert_eq!(text[first + 1], " ".repeat(column));
        assert_eq!(
            text[first + 2],
            format!("{}三个区块单击定位、双击复制已实现。", " ".repeat(column))
        );
        assert_eq!(text[first + 3], format!("{}\",", " ".repeat(column)));
        assert_eq!(part.source, text.join("\n"));
        let key = lines[first]
            .spans
            .iter()
            .find(|span| span.content.contains("lastMessage"))
            .unwrap();
        assert_eq!(key.style.fg, Some(highlight::json_key_foreground()));
        let reference = highlight::code_lines(r#"["已提交推送"]"#, "json");
        let string_style = reference[0]
            .spans
            .iter()
            .find(|span| span.content.contains("已提交推送"))
            .unwrap()
            .style;
        assert_eq!(
            lines[first + 2].spans.last().unwrap().style.fg,
            string_style.fg
        );
        for width in [20, 80, 200] {
            let lines = part.lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH);
            let maps = part.maps(&lines);
            for (line, map) in lines.iter().zip(maps) {
                let copied: String = map
                    .iter()
                    .map(|(_, bytes)| &part.source[bytes.clone()])
                    .collect();
                assert_eq!(copied, line.to_string());
            }
        }
    }

    #[test]
    fn json_preserves_escapes_order_and_empty_containers() {
        let value = serde_json::json!({
            "中": "first\r\nsecond\r\n",
            "escape": "quote\" tab\t slash\\n return\r 中文",
            "key\nline": [null, true, false, -1, 1.5, {}, [], ""],
        });
        let part = Part::text(serde_json::to_string(&value).unwrap(), false);
        let pretty = serde_json::to_string_pretty(&value).unwrap();
        let prefix = "  \"中\": \"";
        let expected = pretty.replace(
            "first\\r\\nsecond\\r\\n",
            &format!(
                "first\n{}second\n{}",
                " ".repeat(prefix.width()),
                " ".repeat(prefix.width())
            ),
        );
        assert_eq!(part.source, expected);
        assert_eq!(texts(&[part], 200).join("\n"), expected);
    }

    #[test]
    fn unterminated_json_detection_is_bounded() {
        // Each failed parse used to revisit the remainder of this output.
        for repetitions in [100_000, 4_000_000] {
            let source = "[{".repeat(repetitions);
            let started = std::time::Instant::now();
            let parts = Part::text_parts(source.clone(), false);
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].source, source);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "{:?}",
                started.elapsed()
            );
        }
    }

    #[test]
    fn embedded_json_preserves_source_indexing() {
        let source = include_str!("../../tests/fixtures/claude-indexing-output.txt");
        assert!(source.contains("record[\"message\"][\"content\"] = content.into();"));
        assert!(source.contains("record[\"toolUseResult\"][\"backgroundTaskId\"]"));
        let parts = Part::text_parts(source.into(), false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].source, source);
    }

    #[test]
    fn embedded_json_keeps_surroundings_and_pretty_copy_sources() {
        let parts = Part::text_parts(
            "before {\"text\":\"} \\\" [\",\"items\":[1,2]} after [3,4] end".into(),
            false,
        );
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].source, "before ");
        assert!(matches!(parts[1].body, Body::Json(_)));
        assert!(parts[1].source.contains("\n  \"items\": ["));
        assert_eq!(parts[2].source, " after ");
        assert_eq!(parts[3].source, "[\n  3,\n  4\n]");
        assert_eq!(parts[4].source, " end");
        for part in &parts {
            let lines = part.lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
            let maps = super::super::text_selection_layout::map_lines(
                &part.source,
                &lines,
                part.markdown(),
            );
            for (line, map) in lines.iter().zip(maps) {
                if matches!(part.body, Body::Json(_)) {
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
    fn invalid_json_spans_stay_prose_and_do_not_hide_following_json() {
        let source = "{broken: {\"ok\":true}} {} [] [bad]";
        assert_eq!(Part::text_parts(source.into(), false)[0].source, source);
        let parts = Part::text_parts(format!("{source} {{\"ok\":true}}"), false);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].source, format!("{source} "));
        assert_eq!(parts[1].source, "{\n  \"ok\": true\n}");
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

    const SHELL_SAMPLE: &str = r#"cd ~/Code/blockchainian/dot && F="darwin/Library/Application Support/Code/User/settings.json" && git diff --stat -- "$F"; sed -i '' 's/^  "workbench.panel.defaultLocation": "left",$/  "workbench.panel.defaultLocation": "left",\n  "workbench.panel.showLabels": false,/' "$F" && sed -n 6,9p "$F""#;

    fn command_part(command: &str) -> Part {
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Code {
                label: "command".into(),
                lang: "bash".into(),
                text: command.into(),
            },
            Style::default(),
            false,
        );
        parts.remove(0)
    }

    #[test]
    fn shell_sample_connectors_and_original_copy() {
        let part = command_part(SHELL_SAMPLE);
        let lines = part.lines(500, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(lines.len(), 5);
        let commands = [
            "cd ~/Code/blockchainian/dot",
            r#"F="darwin/Library/Application Support/Code/User/settings.json""#,
            r#"git diff --stat -- "$F""#,
            r#"sed -i '' 's/^  "workbench.panel.defaultLocation": "left",$/  "workbench.panel.defaultLocation": "left",\n  "workbench.panel.showLabels": false,/' "$F""#,
            r#"sed -n 6,9p "$F""#,
        ];
        let maps = part.maps(&lines);
        for (row, command) in commands.iter().enumerate() {
            assert!(lines[row].to_string().starts_with(command));
            assert_eq!(maps[row].last().unwrap().0.end as usize, command.width());
        }
        for (upper, lower) in [(0, 1), (1, 2), (3, 4)] {
            let column = commands[upper].width().max(commands[lower].width()) + 1;
            let upper_glyph = if upper == 1 { '┤' } else { '┐' };
            let lower_glyph = if lower == 1 { '┤' } else { '┘' };
            assert_eq!(
                lines[upper].to_string().chars().nth(column),
                Some(upper_glyph)
            );
            assert_eq!(
                lines[lower].to_string().chars().nth(column),
                Some(lower_glyph)
            );
        }
        assert_eq!(part.source, SHELL_SAMPLE);
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .filter(|span| span.content.contains(['┐', '┘', '┤']))
                .all(|span| span
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::DIM))
        );
        assert_eq!(
            command_part("a && bb && cccc")
                .lines(100, Style::default(), chrono::DateTime::UNIX_EPOCH)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["a  ┐", "bb ┘ ┐", "cccc ┘"]
        );
        assert_eq!(command_part("a && b\n").source, "a && b\n");
    }

    #[test]
    fn shell_wrapping_and_fields_keep_source_maps() {
        for command in [
            "echo 中文 && printf done; pwd",
            "echo 'a && b; c' && next",
            "echo $(a && b) || next && last",
            "cat <<'EOF'\na && b; c\nEOF\necho done && next",
            "  a && b  ",
        ] {
            let part = command_part(command);
            for width in [3, 12, 80] {
                let lines = part.lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH);
                let maps = part.maps(&lines);
                assert_eq!(maps.len(), lines.len());
                assert!(lines.iter().all(|line| line.width() <= width));
                for (line, map) in lines.iter().zip(maps) {
                    let displayed = line.to_string();
                    for (cells, bytes) in map {
                        let text: String = displayed
                            .graphemes(true)
                            .scan(0usize, |column, text| {
                                let start = *column;
                                *column += text.width();
                                Some((start, text))
                            })
                            .filter(|(column, _)| {
                                (*column as u16) >= cells.start && (*column as u16) < cells.end
                            })
                            .map(|(_, text)| text)
                            .collect();
                        assert_eq!(text, part.source[bytes]);
                    }
                }
            }
        }
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Fields(vec![
                ("command".into(), "a && bb".into()),
                ("timeout".into(), "1000".into()),
            ]),
            Style::default(),
            false,
        );
        assert_eq!(
            parts
                .iter()
                .map(|part| part.source.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            "command: a && bb\ntimeout: 1000"
        );
        assert_eq!(
            texts(&parts, 80),
            ["command: a ┐", "bb         ┘", "timeout: 1000"]
        );
        assert_eq!(
            command_part("echo 'broken && next").lines(
                80,
                Style::default(),
                chrono::DateTime::UNIX_EPOCH
            )[0]
            .to_string(),
            "echo 'broken && next"
        );
    }

    #[test]
    fn fields_keys_match_json_key_foregrounds() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
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
        let lines = parts[0].lines(80, body_style, chrono::DateTime::UNIX_EPOCH);
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
            part.lines(8, Style::default(), chrono::DateTime::UNIX_EPOCH)
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
    fn merged_calls_start_with_title_rules_and_end_with_content() {
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
                exit_code: None,
                is_error: false,
            },
            ToolCall {
                id: "Read-fixture".into(),
                title: "Read".into(),
                name: "Read".into(),
                input: vec![Section::Text {
                    label: "path".into(),
                    text: "agents/src/log.rs:1-40".into(),
                }],
                output: None,
                exit_code: None,
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
                "cargo test\n",
                "",
                "running 3 tests\n...",
                "",
                "Read",
                "agents/src/log.rs:1-40",
                "",
            ]
        );
        assert_eq!(
            texts(&parts, 80),
            [
                &format!("{} Bash {}", "─".repeat(37), "─".repeat(37)),
                "cargo test",
                "↓",
                "running 3 tests",
                "...",
                "",
                &format!("{} Read {}", "─".repeat(37), "─".repeat(37)),
                "agents/src/log.rs:1-40",
                "•··",
            ]
        );
    }

    #[test]
    fn title_rules_fit_one_row_with_centered_titles_and_fixed_arrows() {
        for title in [
            "Bash",
            "看探针结果",
            "a",
            "a界e\u{301}",
            "  Read 界面  Read 界面  Read 界面",
        ] {
            let calls = vec![
                ToolCall {
                    id: "title-fixture".into(),
                    title: title.into(),
                    name: "Read".into(),
                    input: vec![],
                    output: Some(vec![]),
                    exit_code: None,
                    is_error: false,
                };
                2
            ];
            let parts = parts(&event(calls));
            for width in 0..=40 {
                let lines = parts[0].lines(
                    width,
                    Style::default().fg(ratatui::style::Color::Red),
                    chrono::DateTime::UNIX_EPOCH,
                );
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].width(), width);
                if width < 5 {
                    continue;
                }
                let text = lines[0].to_string();
                let title_row = text.trim_matches('─');
                assert!(title_row.starts_with(' ') && title_row.ends_with(' '));
                let drawn = &title_row[1..title_row.len() - 1];
                let left = text.chars().take_while(|ch| *ch == '─').count() + 1;
                assert_eq!(left, (width - drawn.width() - 2) / 2 + 1);
                assert_eq!(
                    parts[1].lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH)[0]
                        .to_string()
                        .find('↓'),
                    Some(0)
                );
                for span in &lines[0].spans[1..lines[0].spans.len() - 2] {
                    assert_eq!(span.style, Style::new().not_dim());
                }
                assert_eq!(lines[0].spans[0].style, Style::new().dim());
                assert_eq!(lines[0].spans.last().unwrap().style, Style::new().dim());
                let maps = parts[0].maps(&lines);
                let copied: String = maps[0]
                    .iter()
                    .map(|(_, bytes)| &parts[0].source[bytes.clone()])
                    .collect();
                assert_eq!(copied, title);
                assert!(maps[0].iter().all(|(cells, _)| cells.start as usize >= left
                    && cells.end as usize <= left + drawn.width()));
                if title.width() > width - 4 {
                    assert!(drawn.ends_with('…'));
                } else {
                    assert_eq!(drawn, title);
                }
            }
        }
    }

    #[test]
    fn single_and_merged_arrows_keep_their_column_style_and_empty_copy_source() {
        for count in [1, 2] {
            let calls = vec![
                ToolCall {
                    id: "arrow-fixture".into(),
                    title: "Run checks".into(),
                    name: "Bash".into(),
                    input: vec![],
                    output: Some(vec![]),
                    exit_code: None,
                    is_error: false,
                };
                count
            ];
            let parts = parts(&event(calls));
            assert_eq!(
                parts
                    .iter()
                    .filter(|part| matches!(part.body, Body::TitleRule))
                    .count(),
                if count == 1 { 0 } else { 2 }
            );
            for part in parts.iter().filter(|part| matches!(part.body, Body::Arrow)) {
                for width in 0..=40 {
                    let lines =
                        part.lines(width, Style::default().dim(), chrono::DateTime::UNIX_EPOCH);
                    assert_eq!(lines[0].width(), usize::from(width > 0));
                    assert_eq!(lines[0].to_string().find('↓'), (width > 0).then_some(0));
                    assert_eq!(lines[0].style, super::super::style::log_jump_style());
                    assert!(part.maps(&lines)[0].is_empty());
                }
            }
        }
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
            exit_code: None,
            is_error: false,
        }]));
        assert_eq!(
            texts(&parts, 9),
            [
                "a.rs",
                "+中文字符",
                "串很长",
                "-short",
                "↓",
                "let value",
                " = \"abcde",
                "fghij\";",
            ]
        );
        let lines = parts[3].lines(9, Style::default(), chrono::DateTime::UNIX_EPOCH);
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
            exit_code: None,
            is_error: true,
        }]));
        let lines = parts[1].lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(
            lines[0].spans[0].style,
            super::super::style::log_styles(&Kind::Error).0
        );
        assert_eq!(parts[2].source, "\tboom");
    }
}
