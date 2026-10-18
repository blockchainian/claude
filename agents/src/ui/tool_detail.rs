//! The calls of an open tool row as the parts the log panel draws and copies.

use super::content::{Block, Kind as ContentKind};
use super::detail_model::{Detail, Group, Section as DetailSection, TitleRow};
use super::highlight;
use crate::log::{Event, Kind, Section};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

#[cfg(test)]
thread_local! {
    static JSON_SCAN_BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn record_json_scan(bytes: usize) {
    JSON_SCAN_BUDGET.with(|budget| {
        if let Some(remaining) = budget.get() {
            budget.set(Some(
                remaining
                    .checked_sub(bytes)
                    .expect("JSON detection exceeded its linear scan budget"),
            ));
        }
    });
}

/// One part of the open view: the text it is drawn from, which the selection copies, and
/// how that text turns into lines.
pub(super) struct Part {
    pub source: String,
    body: Body,
}

enum Body {
    /// A section label, in the given style.
    Label(Style),
    /// An input/output arrow without selectable source bytes.
    Arrow,
    /// The dim animation that a call has no output yet.
    Running,
    /// Code highlighted as the given language.
    Code(String),
    /// Shell command ranges retain original bytes while separators are display-only.
    Shell(Vec<super::shell_commands::Command>),
    /// Parsed JSON with multiline string values.
    Json(serde_json::Value),
    Diff,
    Markdown,
    /// A message or free-text input, with a shared reading column.
    Prose(bool),
    /// Literal output with its original logical lines.
    Text,
}

impl Body {
    fn content_kind(&self) -> ContentKind {
        match self {
            Self::Markdown | Self::Prose(_) => ContentKind::Prose,
            Self::Label(_)
            | Self::Arrow
            | Self::Running
            | Self::Code(_)
            | Self::Shell(_)
            | Self::Json(_)
            | Self::Diff
            | Self::Text => ContentKind::Preformatted,
        }
    }
}

impl Part {
    fn new(source: impl Into<String>, body: Body) -> Self {
        Part {
            source: source.into(),
            body,
        }
    }

    /// Split embedded JSON from text; each fragment retains its content semantics.
    pub fn text_parts(source: String, markdown: bool) -> Vec<Self> {
        let mut parts = Vec::new();
        let mut cursor = 0;
        let mut text_start = 0;
        while cursor < source.len() {
            #[cfg(test)]
            record_json_scan(1);
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

    pub fn prose(source: String, markdown: bool) -> Self {
        Self::text(source, markdown).into_prose(false)
    }

    pub fn prose_parts(source: String, markdown: bool) -> Vec<Self> {
        Self::text_parts(source, markdown)
            .into_iter()
            .map(|part| part.into_prose(true))
            .collect()
    }

    fn into_prose(mut self, detect_markdown: bool) -> Self {
        self.body = match self.body {
            Body::Json(value) => Body::Json(value),
            Body::Markdown => Body::Prose(true),
            Body::Text => {
                Body::Prose(detect_markdown && super::markdown::has_markers(&self.source))
            }
            _ => unreachable!("only text fragments become prose"),
        };
        self
    }

    /// Whether the selection maps the drawn lines back to the source as Markdown.
    pub fn markdown(&self) -> bool {
        matches!(self.body, Body::Markdown | Body::Prose(true))
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
        super::text_selection_layout::map_lines(&self.source, lines, self.markdown())
    }

    /// Lay out prose at `width`, preserving logical lines in preformatted blocks.
    pub fn lines(
        &self,
        width: usize,
        body_style: Style,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Line<'static>> {
        let drawn = self.source.replace('\t', "    ");
        let source = drawn.as_str();
        let options = super::line_truncation::prose_wrap_options(width);
        let kind = self.body.content_kind();
        let blocks = match &self.body {
            Body::Markdown | Body::Prose(true) => super::markdown::blocks(source),
            _ => vec![Block {
                lines: self.render_lines(width, now),
                indent: 0,
                kind,
            }],
        };
        let lines: Vec<_> = blocks
            .into_iter()
            .flat_map(|block| block.layout(options.clone()))
            .collect();
        if matches!(self.body, Body::Arrow) {
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

    fn render_lines(&self, width: usize, now: chrono::DateTime<chrono::Utc>) -> Vec<Line<'static>> {
        let drawn = self.source.replace('\t', "    ");
        let source = drawn.as_str();
        match &self.body {
            Body::Label(style) => vec![Line::from(Span::styled(source.to_owned(), *style))],
            Body::Arrow => {
                let row = if width > 0 { "↓" } else { "" };
                vec![Line::styled(row, super::style::log_jump_style())]
            }
            Body::Running => vec![Line::from(Span::styled(
                running_frame(now),
                Style::new().dim(),
            ))],
            Body::Code(lang) => highlight::code_lines(source, lang),
            Body::Shell(commands) => shell_lines(&self.source, commands),
            Body::Json(value) => json_lines(value),
            Body::Diff => highlight::diff_lines(source),
            Body::Markdown | Body::Prose(true) => unreachable!("Markdown renders semantic blocks"),
            Body::Text | Body::Prose(false) => source
                .split('\n')
                .map(|line| Line::from(line.to_owned()))
                .collect(),
        }
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

const SHELL_BRACKET: [&str; 3] = [
    ratatui::symbols::border::PLAIN.top_left,
    ratatui::symbols::border::PLAIN.vertical_left,
    ratatui::symbols::border::PLAIN.bottom_left,
];

/// Shell chains carry display-only brackets; unchained rows reserve the same two cells.
fn shell_lines(source: &str, commands: &[super::shell_commands::Command]) -> Vec<Line<'static>> {
    let prefixed = commands.iter().any(|command| command.and_next);
    let mut lines = Vec::new();
    let mut first = 0;
    for (index, command) in commands.iter().enumerate() {
        if command.and_next {
            continue;
        }
        let mut rows: Vec<_> = commands[first..=index]
            .iter()
            .flat_map(|command| {
                highlight::code_lines(&source[command.range.clone()].replace('\t', "    "), "bash")
            })
            .collect();
        let count = rows.len();
        if prefixed {
            for (row, line) in rows.iter_mut().enumerate() {
                let bracket = if first == index {
                    " "
                } else if row == 0 {
                    SHELL_BRACKET[0]
                } else if row + 1 == count {
                    SHELL_BRACKET[2]
                } else {
                    SHELL_BRACKET[1]
                };
                line.spans.insert(
                    0,
                    Span::styled(
                        format!("{bracket} "),
                        Style::new().fg(ratatui::style::Color::Reset).dim(),
                    ),
                );
            }
        }
        lines.extend(rows);
        first = index + 1;
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
    let prefixed = commands.iter().any(|command| command.and_next);
    let command_lines: Vec<_> = lines
        .iter()
        .map(|line| {
            if prefixed {
                Line::from(line.spans[1..].to_vec())
            } else {
                line.clone()
            }
        })
        .collect();
    let mut maps = super::text_selection_layout::map_lines(&displayed, &command_lines, false);
    for cells in &mut maps {
        for (columns, bytes) in cells {
            if prefixed {
                columns.start += 2;
                columns.end += 2;
            }
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

        fn string(&mut self, text: &str, token: JsonToken, multiline: bool, indent: usize) {
            let quote = if multiline {
                JsonToken::StringQuote
            } else {
                token
            };
            self.push("\"", quote);
            // Continuations use the value's structural indent, preserving its own whitespace.
            // Serialize each logical line so all other JSON escapes retain their spelling.
            let normalized = if multiline {
                text.replace("\r\n", "\n")
            } else {
                text.into()
            };
            for (index, line) in normalized.split_inclusive('\n').enumerate() {
                if index > 0 && multiline {
                    self.newline(indent);
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
                self.newline(indent);
            }
            self.push("\"", quote);
        }

        /// Render a multi-line Markdown value between its quotes, indented like its continuations.
        fn markdown(&mut self, text: &str, indent: usize) {
            self.push("\"", JsonToken::StringQuote);
            let base = self.styles[JsonToken::String as usize];
            for (index, line) in super::markdown::render(text, usize::MAX)
                .into_iter()
                .enumerate()
            {
                if index > 0 {
                    self.newline(indent);
                }
                let spans = &mut self.lines.last_mut().expect("JSON line exists").spans;
                spans.extend(line.spans.into_iter().map(|mut span| {
                    span.style = base.patch(span.style);
                    span
                }));
            }
            self.push("\"", JsonToken::StringQuote);
        }

        fn value(&mut self, value: &serde_json::Value, indent: usize) {
            use serde_json::Value;
            match value {
                Value::Object(values) => {
                    self.push("{", JsonToken::Punctuation);
                    for (index, (key, value)) in values.iter().enumerate() {
                        self.newline(indent + 2);
                        self.string(key, JsonToken::Key, false, indent + 2);
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
                Value::String(text)
                    if text.contains('\n') && super::markdown::has_markers(text) =>
                {
                    self.markdown(text, indent)
                }
                Value::String(text) => self.string(text, JsonToken::String, true, indent),
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

/// Build one titled section per message or tool call.
pub(super) fn detail(event: &Event, time: String, meta_style: Style) -> Detail {
    let sections = if super::log::shows_calls(event) {
        let mut sections = parts(event);
        if let ([_], Some(section)) = (event.calls.as_slice(), sections.first_mut()) {
            section.title.time = Some(time);
            section.title.meta_style = meta_style;
        }
        sections
    } else {
        let parts = if matches!(
            event.kind,
            Kind::User | Kind::Compaction | Kind::Assistant | Kind::Agent
        ) {
            vec![Part::prose(event.body.clone(), true)]
        } else if matches!(event.kind, Kind::QueuedUser | Kind::QueuedSystem) {
            Part::prose_parts(event.body.clone(), false)
        } else if event.kind == Kind::Tool {
            Vec::new()
        } else {
            Part::text_parts(event.body.clone(), false)
        };
        vec![DetailSection {
            title: TitleRow {
                left: event.sender.clone(),
                title: if event.kind == Kind::Tool {
                    event.body.clone()
                } else {
                    super::log::kind_label(&event.kind).into()
                },
                title_style: super::style::log_styles(&event.kind).0,
                time: Some(time),
                meta_style,
            },
            groups: vec![Group { parts }],
        }]
    };
    Detail { sections }
}

/// Choose the tool section's metadata and centered title together.
fn call_title(call: &crate::log::ToolCall) -> (Option<String>, String) {
    let mut left = call.detail_left.clone();
    if let Some(code) = call.exit_code.filter(|code| *code != 0) {
        if left.is_empty() {
            left = call.name.clone();
        }
        left.push_str(&format!(" · exit {code}"));
    }
    (
        (!left.is_empty()).then_some(left),
        call.detail_title.clone(),
    )
}

/// One section per call; arrows are independent alignment groups.
fn parts(event: &Event) -> Vec<DetailSection> {
    let label_style = super::style::log_styles(&event.kind).0;
    let error_style = super::style::log_styles(&Kind::Error).0;
    event
        .calls
        .iter()
        .flat_map(|call| {
            let (left, title) = call_title(call);
            let mut sections = vec![DetailSection {
                title: TitleRow {
                    left,
                    title,
                    title_style: label_style,
                    time: call.duration.map(super::formatting::format_log_duration),
                    meta_style: Style::new().dim(),
                },
                groups: Vec::new(),
            }];
            // An input label titles its own section, such as each question an ask poses.
            let mut input = Vec::new();
            for section in &call.input {
                match section {
                    Section::Label(label) if input.is_empty() && sections.len() == 1 => {
                        sections[0].title.title = label.clone();
                    }
                    Section::Label(label) => {
                        let parts = std::mem::take(&mut input);
                        let previous = sections.last_mut().expect("call has a section");
                        previous.groups.push(Group { parts });
                        sections.push(DetailSection {
                            title: TitleRow {
                                left: None,
                                title: label.clone(),
                                title_style: label_style,
                                time: None,
                                meta_style: Style::new().dim(),
                            },
                            groups: Vec::new(),
                        });
                    }
                    _ => push_section(&mut input, section, label_style, false),
                }
            }
            let mut groups = Vec::new();
            if !input.is_empty() {
                groups.push(Group { parts: input });
            }
            groups.push(Group {
                parts: vec![Part::new("", Body::Arrow)],
            });
            let mut output_parts = Vec::new();
            match &call.output {
                Some(output) => {
                    if output.is_empty() {
                        output_parts.push(Part::new("(no output)", Body::Text));
                    }
                    let style = if call.is_error {
                        error_style
                    } else {
                        label_style
                    };
                    for section in output {
                        push_section(&mut output_parts, section, style, true);
                    }
                }
                None => output_parts.push(Part::new("", Body::Running)),
            }
            groups.push(Group {
                parts: output_parts,
            });
            sections
                .last_mut()
                .expect("call has a section")
                .groups
                .extend(groups);
            sections
        })
        .collect()
}

fn push_section(parts: &mut Vec<Part>, section: &Section, label_style: Style, text_output: bool) {
    let (label, body) = match section {
        Section::Shell { text, prefix } => {
            parts.push(shell_part(text.clone(), *prefix));
            return;
        }
        Section::Prose { text, markdown } => {
            parts.push(Part::prose(text.clone(), *markdown));
            return;
        }
        Section::Label(text) => {
            parts.push(Part::new(text.clone(), Body::Label(label_style)));
            return;
        }
        Section::Json { text, .. } => {
            let value = serde_json::from_str(text).expect("provider JSON section is valid");
            parts.push(Part::json(value));
            return;
        }
        Section::Placeholder(text) => {
            parts.push(Part::new(text.clone(), Body::Label(Style::new().dim())));
            return;
        }
        Section::Code { label, lang, text } => {
            (label, Part::new(cells(text), Body::Code(lang.clone())))
        }
        Section::Diff { label, text } => (label, Part::new(cells(text), Body::Diff)),
        Section::Markdown { label, text } => (
            label,
            if text_output {
                Part::text(cells(text), true)
            } else {
                Part::prose(cells(text), true)
            },
        ),
        Section::Text { text, .. } => {
            if text_output {
                parts.extend(Part::text_parts(cells(text), false));
            } else {
                parts.push(Part::text(cells(text), false));
            }
            return;
        }
    };
    if matches!(section, Section::Diff { .. }) && !label.is_empty() {
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
        #[cfg(test)]
        record_json_scan(1);
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
                    #[cfg(test)]
                    record_json_scan(end - start);
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
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    fn event(calls: Vec<ToolCall>) -> Event {
        Event {
            source: None,
            completed_time: false,
            background: false,
            source_records: Vec::new(),
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

    fn flattened_parts(event: &Event) -> Vec<Part> {
        parts(event)
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.parts)
            .collect()
    }

    fn texts(parts: &[Part], width: usize) -> Vec<String> {
        parts
            .iter()
            .flat_map(|part| part.lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH))
            .map(|line| line.to_string())
            .collect()
    }

    #[test]
    fn section_titles_cover_single_tools_and_messages() {
        let parsed = crate::providers::claude::log::tail(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-agent-display.jsonl"),
            100,
        )
        .unwrap();
        for source in parsed.iter().filter(|event| !event.calls.is_empty()) {
            let mut event = source.clone();
            event.calls.truncate(1);
            let detail = detail(&event, "event time".into(), Style::new().dim());
            let title = &detail.sections[0].title;
            assert_eq!(title.time.as_deref(), Some("event time"));
            let body: Vec<_> = detail.sections[0]
                .groups
                .iter()
                .flat_map(|group| &group.parts)
                .map(|part| part.source.as_str())
                .collect();
            if let Some(left) = title.left.as_deref() {
                assert!(!body.contains(&left));
            }
        }
        let mut message = event(vec![]);
        message.kind = Kind::Assistant;
        message.sender = Some("peer".into());
        let detail = detail(&message, "event time".into(), Style::new().dim());
        let title = &detail.sections[0].title;
        assert_eq!(title.left.as_deref(), Some("peer"));
        assert_eq!(title.title, "Assistant");
        assert_eq!(title.time.as_deref(), Some("event time"));
    }

    #[test]
    fn tool_descriptions_are_centered_in_single_and_merged_title_rows() {
        let call = ToolCall {
            detail_left: "Bash".into(),
            detail_title: "Check the workspace".into(),
            show_details: false,
            id: "description-fixture".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![],
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
        };
        for count in [1, 2] {
            let detail = detail(
                &event(vec![call.clone(); count]),
                String::new(),
                Style::new(),
            );
            let title = &detail.sections[0].title;
            assert_eq!(title.title, "Check the workspace");
            assert_eq!(title.left.as_deref(), Some("Bash"));
        }
    }

    #[test]
    fn call_titles_keep_type_and_labels_out_of_the_body() {
        let mut call = ToolCall {
            detail_left: "general-purpose".into(),
            detail_title: "Inspect source".into(),
            show_details: false,
            id: "call-id".into(),
            title: "Agent".into(),
            name: "Agent".into(),
            input: vec![Section::Prose {
                text: "Find the entry".into(),
                markdown: true,
            }],
            output: None,
            exit_code: Some(1),
            is_error: true,
            duration: Some(chrono::Duration::seconds(12)),
        };
        for count in [1, 2] {
            let detail = detail(
                &event(vec![call.clone(); count]),
                "10:11:12".into(),
                Style::new().dim(),
            );
            assert_eq!(detail.sections.len(), count);
            for section in &detail.sections {
                let title = &section.title;
                assert_eq!(title.left.as_deref(), Some("general-purpose · exit 1"));
                assert_eq!(title.title, "Inspect source");
                assert_eq!(
                    title.time.as_deref(),
                    Some(if count == 1 { "10:11:12" } else { "12s" })
                );
                assert_eq!(
                    title.title_style,
                    super::super::style::log_styles(&Kind::Tool).0
                );
                assert_eq!(title.meta_style, Style::new().dim());
                assert_eq!(section.groups.len(), 3);
                assert_eq!(section.groups[0].parts.len(), 1);
                assert_eq!(section.groups[0].parts[0].source, "Find the entry");
                assert!(matches!(section.groups[1].parts[0].body, Body::Arrow));
                assert!(section.groups[2].parts[0].running());
            }
        }
        call.detail_title = "source-reader".into();
        assert_eq!(call_title(&call).1, "source-reader");
        call.detail_title = "Agent".into();
        assert_eq!(call_title(&call).1, "Agent");
        call.detail_left = "Task call-id".into();
        assert_eq!(
            call_title(&call).0.as_deref(),
            Some("Task call-id · exit 1")
        );
        call.detail_left = "Read".into();
        assert_eq!(call_title(&call).0.as_deref(), Some("Read · exit 1"));
    }

    #[test]
    fn event_details_cover_message_queue_text_and_empty_tools() {
        for kind in [
            Kind::User,
            Kind::Compaction,
            Kind::Assistant,
            Kind::Agent,
            Kind::Session,
            Kind::Error,
            Kind::Denied,
            Kind::QueuedUser,
            Kind::QueuedSystem,
            Kind::Tool,
        ] {
            let mut event = event(vec![]);
            event.kind = kind.clone();
            event.sender = Some("peer".into());
            event.body = "before {\"ok\":true} after".into();
            let detail = detail(&event, "event time".into(), Style::new().dim());
            assert_eq!(detail.sections.len(), 1);
            let title = &detail.sections[0].title;
            assert_eq!(title.time.as_deref(), Some("event time"));
            assert_eq!(title.left.as_deref(), Some("peer"));
            assert_eq!(
                title.title,
                if kind == Kind::Tool {
                    event.body.as_str()
                } else {
                    super::super::log::kind_label(&kind)
                }
            );
            if kind == Kind::Tool {
                assert!(detail.sections[0].groups[0].parts.is_empty());
                continue;
            }
            assert_eq!(detail.sections[0].groups.len(), 1);
            let parts = &detail.sections[0].groups[0].parts;
            if matches!(
                kind,
                Kind::User | Kind::Compaction | Kind::Assistant | Kind::Agent
            ) {
                assert_eq!(parts.len(), 1);
                assert!(parts[0].markdown());
            } else {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[1].body, Body::Json(_)));
            }
        }
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
        let parts = flattened_parts(&event(vec![ToolCall {
            detail_left: "Bash".into(),
            detail_title: "Bash".into(),
            show_details: false,
            id: "running-copy".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![],
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
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
        let column = 2;
        assert_eq!(text[first + 1], " ".repeat(column));
        assert_eq!(
            text[first + 2],
            format!(
                "{}三个区块单击定位、双击复制已实现。\",",
                " ".repeat(column)
            )
        );
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
            lines[first + 2]
                .spans
                .iter()
                .find(|span| span.content.contains("三个区块"))
                .unwrap()
                .style
                .fg,
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
    fn json_sections_use_multiline_highlighting_and_placeholders_are_dim() {
        let source = include_str!("../../tests/fixtures/codex-manager-await.jsonl").trim_end();
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Json {
                label: "input".into(),
                text: source.into(),
            },
            Style::default(),
            false,
        );
        let expected = Part::text_parts(source.into(), false);
        assert_eq!(parts[0].source, expected[0].source);
        assert_eq!(
            parts[0].lines(200, Style::default(), chrono::DateTime::UNIX_EPOCH),
            expected[0].lines(200, Style::default(), chrono::DateTime::UNIX_EPOCH)
        );
        parts.clear();
        push_section(
            &mut parts,
            &Section::Placeholder("[image image/png]".into()),
            Style::default(),
            true,
        );
        let lines = parts[0].lines(200, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(lines[0].to_string(), "[image image/png]");
        assert!(lines[0].spans.iter().all(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::DIM)
        }));
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
        let expected = pretty.replace("first\\r\\nsecond\\r\\n", "first\n  second\n  ");
        assert_eq!(part.source, expected);
        assert_eq!(texts(&[part], 200).join("\n"), expected);
    }

    #[test]
    fn unterminated_json_detection_is_bounded() {
        struct ScanBudget;
        impl Drop for ScanBudget {
            fn drop(&mut self) {
                JSON_SCAN_BUDGET.set(None);
            }
        }

        // Each failed parse used to revisit the remainder of this output.
        for repetitions in [100_000, 4_000_000] {
            let source = "[{".repeat(repetitions);
            JSON_SCAN_BUDGET.set(Some(2 * source.len()));
            let _budget = ScanBudget;
            let parts = Part::text_parts(source.clone(), false);
            let scanned = 2 * source.len() - JSON_SCAN_BUDGET.get().unwrap();
            assert!(
                scanned >= source.len(),
                "the whole candidate must be scanned"
            );
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].source, source);
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
    fn text_outputs_keep_literal_markup_and_code_keeps_language() {
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
                !Part::text_parts(source.into(), false)[0].markdown(),
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
            &Section::Shell {
                prefix: 0,
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
        for (row, (command, prefix)) in commands
            .iter()
            .zip(["┌ ", "│ ", "└ ", "┌ ", "└ "])
            .enumerate()
        {
            assert_eq!(lines[row].to_string(), format!("{prefix}{command}"));
            assert_eq!(maps[row].first().unwrap().0.start, 2);
            assert_eq!(
                maps[row].last().unwrap().0.end as usize,
                command.width() + 2
            );
        }
        assert_eq!(part.source, SHELL_SAMPLE);
        let part = command_part("a && bb && cccc; d");
        assert_eq!(
            part.lines(100, Style::default(), chrono::DateTime::UNIX_EPOCH)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["┌ a", "│ bb", "└ cccc", "  d"]
        );
        assert_eq!(command_part("a && b\n").source, "a && b\n");
    }

    #[test]
    fn shell_chain_prefixes_cover_multiline_commands_and_leave_source_bytes_unchanged() {
        let source = "pwd; printf 'first\nsecond' && echo done; whoami";
        let part = command_part(source);
        let lines = part.lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [
                "  pwd",
                "┌ printf 'first",
                "│ second'",
                "└ echo done",
                "  whoami"
            ]
        );
        for (line, cells) in lines.iter().zip(part.maps(&lines)) {
            assert_eq!(cells.first().unwrap().0.start, 2);
            let first = cells.first().unwrap().1.start;
            let last = cells.last().unwrap().1.end;
            assert_eq!(
                &line.to_string().graphemes(true).skip(2).collect::<String>(),
                &source[first..last]
            );
        }
        assert_eq!(part.source, source);
    }

    #[test]
    fn shell_without_chains_has_no_display_prefix() {
        for source in ["pwd; whoami", "echo 'a && b'", "printf 'first\nsecond'"] {
            let part = command_part(source);
            let lines = part.lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
            assert_eq!(
                lines[0].to_string(),
                source.split([';', '\n']).next().unwrap()
            );
            assert!(
                part.maps(&lines)
                    .iter()
                    .all(|cells| cells.first().unwrap().0.start == 0)
            );
        }
    }

    #[test]
    fn shell_lines_and_fields_keep_source_maps() {
        for command in [
            "echo 中文 && printf done; pwd",
            "python3 -c \"\nimport json;d=json.load(open('/tmp/x.json'))\nfor t in d['threads']:\n  if t['id'].startswith('01a147c6'): print(t)\n\"",
            "cd /some/dir && python3 - <<'EOF'\nimport json\nrows=[json.loads(l) for l in open('a.jsonl')]\nEOF",
            "node -e 'const x = \"中文\"; console.log(x)' && echo done",
            "echo 'a && b; c' && next",
            "echo $(a && b) || next && last",
            "cat <<'EOF'\na && b; c\nEOF\necho done && next",
            "  a && b  ",
        ] {
            let part = command_part(command);
            assert_eq!(part.source, command);
            for width in [3, 12, 80] {
                let lines = part.lines(width, Style::default(), chrono::DateTime::UNIX_EPOCH);
                let maps = part.maps(&lines);
                assert_eq!(maps.len(), lines.len());
                assert_eq!(
                    lines.len(),
                    part.lines(200, Style::default(), chrono::DateTime::UNIX_EPOCH)
                        .len()
                );
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
            &Section::Shell {
                text: "command: a && bb".into(),
                prefix: 9,
            },
            Style::default(),
            false,
        );
        push_section(
            &mut parts,
            &Section::Json {
                label: "input".into(),
                text: r#"{"timeout":1000}"#.into(),
            },
            Style::default(),
            false,
        );
        assert_eq!(
            parts
                .iter()
                .map(|part| part.source.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            "command: a && bb\n{\n  \"timeout\": 1000\n}"
        );
        assert_eq!(
            texts(&parts, 80),
            ["┌ command: a", "└ bb", "{", "  \"timeout\": 1000", "}"]
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
    fn wide_label_keeps_its_logical_line() {
        let part = Part::new(
            "/workspace/very/long/path/to/source.rs",
            Body::Label(Style::default()),
        );
        assert!(
            part.lines(8, Style::default(), chrono::DateTime::UNIX_EPOCH)
                .iter()
                .any(|line| line.to_string() == part.source)
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
    fn merged_calls_keep_separate_sections_and_groups() {
        let sections = parts(&event(vec![
            ToolCall {
                detail_left: "Bash".into(),
                detail_title: "Bash".into(),
                show_details: false,
                id: "Bash-fixture".into(),
                title: "Bash".into(),
                name: "Bash".into(),
                input: vec![Section::Shell {
                    prefix: 0,
                    text: "cargo test\n".into(),
                }],
                output: Some(vec![Section::Text {
                    label: "stdout".into(),
                    text: "running 3 tests\n...".into(),
                }]),
                exit_code: None,
                is_error: false,
                duration: None,
            },
            ToolCall {
                detail_left: "Read".into(),
                detail_title: "Read".into(),
                show_details: false,
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
                duration: None,
            },
        ]));
        assert_eq!(sections.len(), 2);
        for (section, title) in sections.iter().zip(["Bash", "Read"]) {
            assert_eq!(section.title.title.to_string(), title);
            assert_eq!(section.title.left.as_deref(), Some(title));
            assert_eq!(section.groups.len(), 3);
            assert!(matches!(section.groups[1].parts[0].body, Body::Arrow));
        }
        let parts: Vec<_> = sections
            .into_iter()
            .flat_map(|section| section.groups)
            .flat_map(|group| group.parts)
            .collect();
        assert_eq!(
            parts
                .iter()
                .map(|part| part.source.as_str())
                .collect::<Vec<_>>(),
            [
                "cargo test\n",
                "",
                "running 3 tests\n...",
                "agents/src/log.rs:1-40",
                "",
                ""
            ]
        );
        assert_eq!(
            texts(&parts, 80),
            [
                "cargo test",
                "↓",
                "running 3 tests",
                "...",
                "agents/src/log.rs:1-40",
                "↓",
                "•··"
            ]
        );
    }

    #[test]
    fn single_and_merged_arrows_keep_their_column_style_and_empty_copy_source() {
        for count in [1, 2] {
            let calls = vec![
                ToolCall {
                    detail_left: "Bash".into(),
                    detail_title: "Run checks".into(),
                    show_details: false,
                    id: "arrow-fixture".into(),
                    title: "Run checks".into(),
                    name: "Bash".into(),
                    input: vec![],
                    output: Some(vec![]),
                    exit_code: None,
                    is_error: false,
                    duration: None,
                };
                count
            ];
            let parts = flattened_parts(&event(calls));
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
    fn wide_code_and_diff_keep_logical_lines() {
        let parts = flattened_parts(&event(vec![ToolCall {
            detail_left: "Edit".into(),
            detail_title: "Edit".into(),
            show_details: false,
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
            duration: None,
        }]));
        assert_eq!(
            texts(&parts, 9),
            [
                "a.rs",
                "+中文字符串很长",
                "-short",
                "↓",
                "let value = \"abcdefghij\";"
            ]
        );
        let lines = parts[3].lines(9, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() > 1);
    }

    #[test]
    fn plain_outputs_keep_default_body_style_even_on_failure() {
        use crate::providers::Provider;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut claude = crate::providers::claude::ClaudeProvider::new(dir.path().into());
        let mut session = claude.refresh(1).sessions.remove(0);
        let codex = crate::providers::codex::CodexProvider::new(
            dir.path().into(),
            dir.path().join("manager"),
        );
        for fixture in ["codex-mcp-errors.jsonl", "codex-command-failed.jsonl"] {
            session.transcript = Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(fixture),
            );
            for mut event in codex.log(&session, usize::MAX, false).unwrap() {
                assert!(event.calls.iter().all(|call| call.is_error));
                for failed in [true, false] {
                    for call in &mut event.calls {
                        call.is_error = failed;
                        call.name = "arbitrary tool".into();
                    }
                    let detail = parts(&event);
                    for section in detail {
                        let output = section.groups.last().unwrap();
                        assert!(
                            output
                                .parts
                                .iter()
                                .any(|part| matches!(part.body, Body::Text))
                        );
                        for part in &output.parts {
                            if !matches!(part.body, Body::Text) {
                                continue;
                            }
                            let style = Style::default();
                            for width in [20, 160] {
                                for line in part.lines(width, Style::default(), event.started_at) {
                                    for span in line.spans {
                                        assert_eq!(
                                            span.style.fg, style.fg,
                                            "{fixture}: {}",
                                            span.content
                                        );
                                        assert_eq!(span.style.add_modifier, style.add_modifier);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn errors_label_output_in_the_error_style_and_tabs_become_spaces() {
        let parts = flattened_parts(&event(vec![ToolCall {
            detail_left: "Bash".into(),
            detail_title: "Bash".into(),
            show_details: false,
            id: "Bash-fixture".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![],
            output: Some(vec![
                Section::Label("stderr".into()),
                Section::Text {
                    label: String::new(),
                    text: "\tboom".into(),
                },
            ]),
            exit_code: None,
            is_error: true,
            duration: None,
        }]));
        let lines = parts[1].lines(80, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(
            lines[0].spans[0].style,
            super::super::style::log_styles(&Kind::Error).0
        );
        assert_eq!(parts[2].source, "\tboom");
    }

    #[test]
    fn json_renders_multiline_markdown_strings_and_keeps_other_strings_literal() {
        let value = serde_json::json!({
            "prompt": "# Plan\n\n- **ship** it",
            "log": "line one\nline two",
            "title": "**inline**",
        });
        let lines = json_lines(&value);
        let rows: Vec<_> = lines.iter().map(ToString::to_string).collect();
        assert_eq!(
            rows,
            [
                "{",
                "  \"log\": \"line one",
                "  line two\",",
                "  \"prompt\": \"Plan",
                "  • ship it\",",
                "  \"title\": \"**inline**\"",
                "}",
            ]
        );
        let bold = lines[4]
            .spans
            .iter()
            .find(|span| span.content == "ship")
            .unwrap();
        assert!(
            bold.style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }
}
