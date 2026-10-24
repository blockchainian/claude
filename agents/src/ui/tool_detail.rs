//! The calls of an open tool row as the parts the event list panel draws and copies.

use super::content::{Block, Kind as ContentKind};
use super::event_detail::{EventDetailContent, Group, Section as EventDetailSection, TitleRow};
use super::highlight;
#[cfg(test)]
use super::structured_text::json::{JSON_SCAN_BUDGET, json_lines};
use super::structured_text::{self, Document};
use crate::event::{Event, Kind, Section};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// One part of the open view: the text it is drawn from, which the selection copies, and
/// how that text turns into lines.
pub(super) struct Part {
    pub source: String,
    body: Body,
    pub file_links: Vec<crate::event::FileLink>,
}

enum Body {
    /// A section label, in the given style.
    Label(Style),
    /// A transcript image, with placeholder text as its copy source.
    Image {
        image: crate::images::Image,
        caption: bool,
    },
    /// An input/output arrow without selectable source bytes.
    Arrow,
    /// The dim animation that a call has no output yet.
    Running,
    /// Code highlighted as the given language.
    Code(String),
    /// Shell command ranges retain original bytes while separators are display-only.
    Shell(Vec<super::shell_commands::Command>),
    /// Parsed structured content with its format-owned rendering.
    Structured(Box<dyn Document>),
    Diff(String),
    Markdown,
    /// A message or free-text input, with a shared reading column.
    Prose(bool),
    /// Literal output with its original logical lines.
    Text,
    Note,
}

impl Body {
    fn content_kind(&self) -> ContentKind {
        match self {
            Self::Markdown | Self::Prose(_) => ContentKind::Prose,
            Self::Label(_)
            | Self::Image { .. }
            | Self::Arrow
            | Self::Running
            | Self::Code(_)
            | Self::Shell(_)
            | Self::Structured(_)
            | Self::Diff(_)
            | Self::Note
            | Self::Text => ContentKind::Preformatted,
        }
    }
}

impl Part {
    fn new(source: impl Into<String>, body: Body) -> Self {
        Part {
            source: source.into(),
            body,
            file_links: Vec::new(),
        }
    }

    pub fn image(image: crate::images::Image, caption: bool) -> Self {
        Self::new(image.label.clone(), Body::Image { image, caption })
    }

    pub fn shown_image(&self) -> Option<&crate::images::Image> {
        match &self.body {
            Body::Image { image, .. } => Some(image),
            _ => None,
        }
    }

    /// Split structured blocks from text, retaining each fragment's copy source.
    pub fn text_parts(source: String, markdown: bool) -> Vec<Self> {
        let source = crate::providers::text::printable(&source);
        let mut parts = Vec::new();
        let mut cursor = 0;
        let mut text_start = 0;
        let code_blocks = if markdown {
            structured_text::code_blocks(&source)
        } else {
            Vec::new()
        };
        let mut code_blocks = code_blocks.into_iter().peekable();
        while cursor < source.len() {
            if let Some(range) = code_blocks.peek() {
                if range.end <= cursor {
                    code_blocks.next();
                    continue;
                }
                if range.contains(&cursor) {
                    cursor = range.end;
                    continue;
                }
            }
            let (end, document) = structured_text::detect(&source, cursor);
            if let Some(document) =
                document.filter(|_| !markdown || structured_text::line_block(&source, cursor, end))
            {
                if text_start < cursor
                    && let Some(text) =
                        text_boundary(&source[text_start..cursor], text_start > 0, true)
                {
                    parts.push(Self::text_fragment(text.into(), markdown));
                }
                parts.push(Self::structured(document));
                text_start = end;
            }
            cursor = end;
        }
        if (text_start < source.len() || parts.is_empty())
            && let Some(text) = text_boundary(&source[text_start..], text_start > 0, false)
        {
            parts.push(Self::text_fragment(text.into(), markdown));
        }
        parts
    }

    fn structured(document: Box<dyn Document>) -> Self {
        Self::new(document.text(), Body::Structured(document))
    }

    fn text_fragment(source: String, markdown: bool) -> Self {
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    pub fn text(source: String, markdown: bool) -> Self {
        let source = crate::providers::text::printable(&source);
        if let Some(document) = structured_text::parse(&source) {
            return Self::structured(document);
        }
        Self::new(source, if markdown { Body::Markdown } else { Body::Text })
    }

    #[cfg(test)]
    pub fn prose(source: String, markdown: bool) -> Self {
        Self::text(source, markdown).into_prose(false)
    }

    pub fn prose_parts(source: String, markdown: bool) -> Vec<Self> {
        if let Some(document) = structured_text::parse(&source) {
            return vec![Self::structured(document)];
        }
        Self::text_parts(source, markdown)
            .into_iter()
            .map(|part| part.into_prose(true))
            .collect()
    }

    fn into_prose(mut self, detect_markdown: bool) -> Self {
        self.body = match self.body {
            Body::Structured(value) => Body::Structured(value),
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

    pub fn reading_column(&self) -> bool {
        matches!(self.body.content_kind(), ContentKind::Prose)
    }

    pub fn running(&self) -> bool {
        matches!(self.body, Body::Running)
    }

    /// Generated rules and title padding have no source bytes to select.
    #[cfg(test)]
    pub fn maps(&self, lines: &[Line<'_>]) -> Vec<Vec<super::text_selection_layout::CellMap>> {
        self.maps_in(lines, std::path::Path::new("/nonexistent"))
    }

    pub fn maps_in(
        &self,
        lines: &[Line<'_>],
        cwd: &std::path::Path,
    ) -> Vec<Vec<super::text_selection_layout::CellMap>> {
        if self.running() {
            return vec![Vec::new(); lines.len()];
        }
        if let Body::Shell(commands) = &self.body {
            return shell_maps(&self.source, commands, lines);
        }
        super::text_selection_layout::map_lines_in(
            &self.source,
            lines,
            self.markdown(),
            (!matches!(self.body, Body::Structured(_))).then_some(cwd),
        )
    }

    /// Provider ranges name literal content; formatting moves it to the copy source.
    pub fn remap_links(
        &self,
        original: &str,
        ranges: &[std::ops::Range<usize>],
        cwd: &std::path::Path,
        links: &mut Vec<super::links::Link>,
    ) {
        for range in ranges {
            let text = &original[range.clone()];
            if text.is_empty() {
                continue;
            }
            let mapped: Vec<_> = self
                .source
                .match_indices(text)
                .map(|(start, text)| start..start + text.len())
                .collect();
            for link in super::links::path_links(&self.source, &mapped, cwd) {
                if !links.iter().any(|other| other.range == link.range) {
                    links.push(link);
                }
            }
        }
    }

    /// Lay out prose at `width`, preserving logical lines in preformatted blocks.
    #[cfg(test)]
    pub fn lines(
        &self,
        width: usize,
        body_style: Style,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Line<'static>> {
        self.lines_in(width, body_style, now, std::path::Path::new("/nonexistent"))
    }

    pub fn lines_in(
        &self,
        width: usize,
        body_style: Style,
        now: chrono::DateTime<chrono::Utc>,
        cwd: &std::path::Path,
    ) -> Vec<Line<'static>> {
        self.layout_in(width, body_style, now, cwd).0
    }

    pub fn layout_in(
        &self,
        width: usize,
        body_style: Style,
        now: chrono::DateTime<chrono::Utc>,
        cwd: &std::path::Path,
    ) -> (Vec<Line<'static>>, Vec<Option<bool>>) {
        let text = crate::providers::text::printable(&self.source);
        let paths = super::paths::text_paths(&text, cwd);
        let display = super::line_truncation::abbreviate_paths(&text, &paths, usize::MAX, cwd);
        let numbered = matches!(&self.body, Body::Code(lang) if lang.starts_with("numbered:"));
        let drawn = if numbered {
            display
        } else {
            display.replace('\t', "    ")
        };
        let source = drawn.as_str();
        let options = super::line_truncation::prose_wrap_options(width);
        let kind = self.body.content_kind();
        let blocks = match &self.body {
            Body::Markdown | Body::Prose(true) => super::markdown::blocks(source),
            _ => vec![Block {
                lines: self.render_lines(source, width, now),
                indent: 0,
                kind,
            }],
        };
        let mut breaks = Vec::new();
        let lines: Vec<_> = blocks
            .into_iter()
            .flat_map(|mut block| {
                if matches!(block.kind, ContentKind::Prose) {
                    for line in &mut block.lines {
                        for span in &mut line.spans {
                            let paths = super::paths::text_paths(&span.content, cwd);
                            span.content = super::line_truncation::abbreviate_paths_for_wrap(
                                &span.content,
                                &paths,
                                width,
                                cwd,
                            )
                            .into();
                        }
                    }
                }
                let (lines, joins) = block.layout_with_breaks(options.clone());
                breaks.extend(joins);
                lines
            })
            .collect();
        if matches!(self.body, Body::Arrow) {
            return (lines, breaks);
        }
        let lines = lines
            .into_iter()
            .map(|mut line| {
                for span in &mut line.spans {
                    span.style = body_style.patch(span.style);
                }
                line
            })
            .collect();
        (lines, breaks)
    }

    fn render_lines(
        &self,
        source: &str,
        width: usize,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Line<'static>> {
        match &self.body {
            Body::Image { caption: false, .. } => Vec::new(),
            Body::Image { caption: true, .. } => vec![Line::from(Span::styled(
                source.to_owned(),
                Style::new().dim(),
            ))],
            Body::Label(style) => vec![Line::from(Span::styled(source.to_owned(), *style))],
            Body::Arrow => {
                let row = if width > 0 { "↓" } else { "" };
                vec![Line::styled(row, super::style::event_jump_style())]
            }
            Body::Running => vec![Line::from(Span::styled(
                running_frame(now),
                Style::new().dim(),
            ))],
            Body::Code(lang) => {
                let mut lines = highlight::code_lines(source, lang);
                if lang.starts_with("numbered:") {
                    for line in &mut lines {
                        for span in &mut line.spans {
                            span.content = span.content.replace('\t', "    ").into();
                        }
                    }
                }
                lines
            }
            Body::Shell(commands) => shell_lines(&self.source, commands),
            Body::Structured(document) => document.lines(width),
            Body::Diff(lang) => highlight::diff_lines(source, lang, width),
            Body::Markdown | Body::Prose(true) => unreachable!("Markdown renders semantic blocks"),
            Body::Note => source
                .split('\n')
                .map(|line| Line::styled(line.to_owned(), Style::new().dim()))
                .collect(),
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

pub(super) const SHELL_BRACKET: [&str; 3] = [
    ratatui::symbols::border::PLAIN.top_left,
    ratatui::symbols::border::PLAIN.vertical_left,
    ratatui::symbols::border::PLAIN.bottom_left,
];

pub(super) fn bracket_style() -> Style {
    Style::new().fg(ratatui::style::Color::Reset).dim()
}

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
                line.spans
                    .insert(0, Span::styled(format!("{bracket} "), bracket_style()));
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

/// Each raw record uses the shared document renderer under its own title,
/// styled like the event's title, with its timestamp in the time column.
pub(super) fn transcript_content(
    raw: &Result<String, String>,
    provider: crate::providers::ProviderId,
    kind: &Kind,
) -> EventDetailContent {
    let message = |text: String| EventDetailContent {
        sections: vec![EventDetailSection {
            title: TitleRow {
                left: None,
                exit_code: None,
                title: String::new(),
                title_style: Style::default(),
                time: None,
                meta_style: Style::new().dim(),
            },
            groups: vec![Group {
                parts: vec![Part::new(text, Body::Text)],
            }],
        }],
    };
    let raw = match raw {
        Ok(raw) if raw.is_empty() => return message("No transcript records".into()),
        Ok(raw) => raw,
        Err(error) => return message(format!("Cannot read transcript records: {error}")),
    };
    // raw_text retains one original JSON line per record.
    let sections = raw
        .lines()
        .map(|text| {
            let record: serde_json::Value =
                serde_json::from_str(text).expect("raw_text validates JSON");
            let mut title =
                super::formatting::title_words(record["type"].as_str().unwrap_or_default());
            if provider == crate::providers::ProviderId::Codex
                && let Some(kind) = record["payload"]["type"].as_str()
            {
                if !title.is_empty() {
                    title.push_str(" / ");
                }
                title.push_str(&super::formatting::title_words(kind));
            }
            let time = record["timestamp"]
                .as_str()
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                .map(|time| super::formatting::clock_text(time.with_timezone(&chrono::Utc)));
            EventDetailSection {
                title: TitleRow {
                    left: None,
                    exit_code: None,
                    title,
                    title_style: super::style::event_styles(kind).0,
                    time,
                    meta_style: Style::new().dim(),
                },
                groups: vec![Group {
                    parts: vec![Part::text(text.to_owned(), false)],
                }],
            }
        })
        .collect();
    EventDetailContent { sections }
}

/// Build one titled section per message or tool call.
pub(super) fn event_detail_content(
    event: &Event,
    time: String,
    meta_style: Style,
) -> EventDetailContent {
    let sections = if super::event_list::shows_calls(event) {
        let mut sections = parts(event);
        if let ([_], Some(section)) = (event.calls.as_slice(), sections.first_mut()) {
            if event.calls[0].started_at.is_none() {
                section.title.time = Some(time);
            }
            section.title.meta_style = meta_style;
        }
        sections
    } else {
        let mut parts = if matches!(
            event.kind,
            Kind::User | Kind::Compaction | Kind::Assistant | Kind::Agent
        ) {
            Part::prose_parts(
                event
                    .detail_body
                    .as_ref()
                    .and_then(|detail| detail.text.as_ref())
                    .unwrap_or(&event.body)
                    .clone(),
                true,
            )
        } else if matches!(event.kind, Kind::QueuedUser | Kind::QueuedSystem) {
            Part::prose_parts(event.body.clone(), true)
        } else if event.kind == Kind::Tool {
            Vec::new()
        } else {
            Part::text_parts(event.body.clone(), false)
        };
        if let Some(detail) = &event.detail_body {
            for note in &detail.notes {
                let mut part = Part::new(note.text.clone(), Body::Note);
                part.file_links = note.links.clone();
                parts.push(part);
            }
        }
        vec![EventDetailSection {
            title: TitleRow {
                left: event.sender.clone(),
                exit_code: None,
                title: if event.kind == Kind::Tool {
                    event.body.clone()
                } else {
                    super::event_list::kind_label(&event.kind).into()
                },
                title_style: super::style::event_styles(&event.kind).0,
                time: Some(time),
                meta_style,
            },
            groups: vec![Group { parts }],
        }]
    };
    EventDetailContent { sections }
}

/// Choose the tool section's metadata and centered title together.
pub(super) fn call_title(call: &crate::event::ToolCall) -> (Option<String>, String) {
    let mut left = call.detail_left();
    if let Some(code) = call.exit_code.filter(|code| *code != 0) {
        if left.is_empty() {
            left = code.to_string();
        } else {
            left.push_str(&format!(" · {code}"));
        }
    }
    (
        (!left.is_empty()).then_some(left),
        if call.title.trim().is_empty() {
            crate::providers::tool::display_tool_name(&call.name)
        } else {
            call.title.clone()
        },
    )
}

/// One section per call; arrows are independent alignment groups.
fn parts(event: &Event) -> Vec<EventDetailSection> {
    let label_style = super::style::event_styles(&event.kind).0;
    let error_style = super::style::event_styles(&Kind::Error).0;
    event
        .calls
        .iter()
        .flat_map(|call| {
            let (left, title) = call_title(call);
            let mut sections = vec![EventDetailSection {
                title: TitleRow {
                    left,
                    exit_code: call.exit_code.filter(|code| *code != 0),
                    title,
                    title_style: label_style,
                    time: call
                        .started_at
                        .map(super::formatting::clock_text)
                        .or_else(|| call.duration.map(super::formatting::format_event_duration)),
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
                        sections.push(EventDetailSection {
                            title: TitleRow {
                                left: None,
                                exit_code: None,
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
            if call.hide_output {
                sections
                    .last_mut()
                    .expect("call has a section")
                    .groups
                    .extend(groups);
                return sections;
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
                        // Headings share the following content block's alignment.
                        if !matches!(section, Section::Label(_)) {
                            groups.push(Group {
                                parts: std::mem::take(&mut output_parts),
                            });
                        }
                    }
                }
                None => output_parts.push(Part::new("", Body::Running)),
            }
            if !output_parts.is_empty() {
                groups.push(Group {
                    parts: output_parts,
                });
            }
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
        Section::OutputFile(path) => {
            push_section(
                parts,
                &Section::read_output_file(path),
                label_style,
                text_output,
            );
            return;
        }
        Section::Shell { text, prefix } => {
            parts.push(shell_part(text.clone(), *prefix));
            return;
        }
        Section::Prose { text, markdown } => {
            parts.extend(
                Part::text_parts(text.clone(), *markdown)
                    .into_iter()
                    .map(|part| part.into_prose(false)),
            );
            return;
        }
        Section::Label(text) => {
            parts.push(Part::new(text.clone(), Body::Label(label_style)));
            return;
        }
        Section::Structured { text, .. } => {
            let document =
                structured_text::parse(text).expect("provider structured section is valid");
            parts.push(Part::structured(document));
            return;
        }
        Section::Placeholder(text) => {
            parts.push(Part::new(text.clone(), Body::Label(Style::new().dim())));
            return;
        }
        Section::Image(image) => {
            parts.push(Part::image(image.clone(), true));
            return;
        }
        Section::Code { label, lang, text } => {
            (label, Part::new(cells(text), Body::Code(lang.clone())))
        }
        Section::Diff { label, lang, text } => {
            (label, Part::new(cells(text), Body::Diff(lang.clone())))
        }
        Section::Markdown { text, .. } => {
            // A Markdown block after other content starts after a blank line, as it would within one document.
            if !parts.is_empty() {
                parts.push(Part::new("", Body::Text));
            }
            parts.extend(if text_output {
                Part::text_parts(cells(text), true)
            } else {
                Part::prose_parts(cells(text), true)
            });
            return;
        }
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

/// A structured block boundary already supplies the adjacent line break.
fn text_boundary(mut source: &str, after_block: bool, before_block: bool) -> Option<&str> {
    if after_block && before_block && matches!(source, "\n" | "\r\n") {
        return None;
    }
    if after_block {
        source = source
            .strip_prefix("\r\n")
            .or_else(|| source.strip_prefix('\n'))
            .unwrap_or(source);
    }
    if before_block {
        source = source
            .strip_suffix("\r\n")
            .or_else(|| source.strip_suffix('\n'))
            .unwrap_or(source);
    }
    Some(source)
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
    use crate::event::ToolCall;
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    fn event(calls: Vec<ToolCall>) -> Event {
        Event {
            turn: None,
            source: None,
            completed_time: false,
            background: false,
            source_records: Vec::new(),
            sender: None,
            detail_body: None,
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

    fn codex_fixture_events(fixture: &str) -> Vec<Event> {
        use crate::providers::Provider;
        let directory = tempfile::tempdir().unwrap();
        let provider = crate::providers::codex::CodexProvider::new(
            directory.path().into(),
            directory.path().join("manager"),
        );
        let database = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
        database
            .execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
            .unwrap();
        let mut provider = provider;
        let mut session = provider.refresh(1).sessions.remove(0);
        session.transcript = Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture));
        provider.event_panel(&session, usize::MAX, false).unwrap()
    }

    #[test]
    fn markdown_file_calls_render_headings_and_keep_input_output_semantics() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[6]["message"]["content"][0]["input"];
        let content = input["content"].as_str().unwrap();
        let mut snapshot = String::new();
        for (name, indexes, extension) in [
            ("Write", [6, 7], "md"),
            ("Read", [2, 3], "md"),
            ("cat", [0, 1], "md"),
            ("head", [0, 1], "markdown"),
            ("cat", [0, 1], "MD"),
            ("Read", [2, 3], "markdown"),
        ] {
            // Replay the captured calls with the captured Write file as their content.
            let path = format!("/workspace/agents/plan.{extension}");
            let mut call = records[indexes[0]].clone();
            let mut result = records[indexes[1]].clone();
            match name {
                "Write" => call["message"]["content"][0]["input"]["file_path"] = path.into(),
                "Read" => {
                    call["message"]["content"][0]["input"]["file_path"] = path.clone().into();
                    result["toolUseResult"]["file"]["filePath"] = path.into();
                    result["toolUseResult"]["file"]["content"] = content.into();
                }
                _ => {
                    let command = if name == "head" {
                        format!("head -n {} {path}", content.lines().count())
                    } else {
                        format!("{name} {path}")
                    };
                    call["message"]["content"][0]["input"]["command"] = command.into();
                    result["toolUseResult"]["stdout"] = content.into();
                    result["toolUseResult"]["stderr"] = "".into();
                }
            }
            let transcript = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(transcript.path(), format!("{call}\n{result}\n")).unwrap();
            let events =
                crate::providers::claude::event_reader::tail(transcript.path(), 1).unwrap();
            let call = &events[0].calls[0];
            let sections = if name == "Write" {
                &call.input
            } else {
                call.output.as_ref().unwrap()
            };
            let label = if name == "Write" {
                "input"
            } else if name == "Read" {
                "output"
            } else {
                "stdout"
            };
            assert_eq!(
                sections,
                &[Section::Markdown {
                    label: label.into(),
                    text: content.into()
                }],
                "{name} .{extension}"
            );
            let parts = flattened_parts(&events[0]);
            let part = parts
                .iter()
                .find(|part| part.source == content.trim_end())
                .unwrap();
            assert!(if name == "Write" {
                matches!(part.body, Body::Prose(true))
            } else {
                matches!(part.body, Body::Markdown)
            });
            let lines = part.lines(100, Style::default(), chrono::DateTime::UNIX_EPOCH);
            assert_eq!(
                lines[0].to_string().trim(),
                "agents: clickable paths and image links"
            );
            assert!(lines[0].spans.iter().any(|span| {
                span.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
            }));
            snapshot.push_str(&format!("{name} .{extension}\n"));
            for line in lines.iter().take(8) {
                snapshot.push_str(line.to_string().trim_end());
                snapshot.push('\n');
            }
            snapshot.push('\n');
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui_snapshots/markdown_file_calls.txt");
        if std::env::var_os("UPDATE_MARKDOWN_FILE_SNAPSHOT").is_some() {
            std::fs::write(&path, &snapshot).unwrap();
        }
        assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
    }

    #[test]
    fn chrome_output_blocks_have_independent_alignment_groups() {
        let events = crate::providers::claude::event_reader::tail(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-chrome-javascript.jsonl"),
            usize::MAX,
        )
        .unwrap();
        let event = events.iter().find(|event| !event.calls.is_empty()).unwrap();
        let output = event.calls[0].output.as_ref().unwrap();
        assert_eq!(output.len(), 2);
        let detail = parts(event);
        let groups = &detail[0].groups;
        let arrow = groups
            .iter()
            .position(|group| matches!(group.parts[0].body, Body::Arrow))
            .unwrap();
        assert_eq!(groups.len() - arrow - 1, output.len());
        let result: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-chrome-javascript.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let blocks = result["message"]["content"][0]["content"]
            .as_array()
            .unwrap();
        for (group, block) in groups[arrow + 1..].iter().zip(blocks) {
            assert_eq!(group.parts.len(), 1);
            assert_eq!(
                group.parts[0].source,
                block["text"].as_str().unwrap().trim_end()
            );
        }
    }

    #[test]
    fn captured_stderr_heading_stays_with_its_output_group() {
        let events = crate::providers::claude::event_reader::tail(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-tool-calls.jsonl"),
            usize::MAX,
        )
        .unwrap();
        let event = events
            .iter()
            .find(|event| {
                event.calls.iter().any(|call| {
                    call.output.as_ref().is_some_and(|output| {
                        output.iter().any(
                            |section| matches!(section, Section::Label(label) if label == "stderr"),
                        )
                    })
                })
            })
            .unwrap();
        let detail = parts(event);
        let groups = detail.iter().flat_map(|section| &section.groups);
        let stderr = groups
            .clone()
            .find(|group| group.parts.iter().any(|part| part.source == "stderr"))
            .unwrap();
        assert!(matches!(stderr.parts[0].body, Body::Label(_)));
        assert_eq!(stderr.parts[0].source, "stderr");
        assert!(stderr.parts[1].source.contains("Shell cwd was reset"));
        assert!(
            !stderr
                .parts
                .iter()
                .any(|part| part.source.contains("decks.txt"))
        );
        assert!(groups.into_iter().any(|group| {
            group
                .parts
                .iter()
                .any(|part| part.source.contains("decks.txt"))
        }));
    }

    #[test]
    fn claude_advisor_detail_shows_encrypted_output() {
        let events = crate::providers::claude::event_reader::tail_session(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-advisor.jsonl"),
            20,
            true,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        let parts = flattened_parts(&events[0]);
        let output: Vec<_> = parts
            .iter()
            .skip_while(|part| !matches!(part.body, Body::Arrow))
            .skip(1)
            .map(|part| part.source.as_str())
            .collect();
        assert_eq!(output, ["(encrypted)"]);
    }

    #[test]
    fn codex_agent_message_has_a_named_title_and_no_output_arrow() {
        let events = codex_fixture_events("tests/fixtures/codex-agent-messages/new-task.jsonl");
        let event = events.first().unwrap();
        let detail = event_detail_content(event, "09:55:00".into(), Style::new().dim());
        assert_eq!(detail.sections.len(), 1);
        let section = &detail.sections[0];
        assert_eq!(section.title.left.as_deref(), None);
        assert_eq!(section.title.title, "root → demo_sub");
        assert_eq!(section.title.time.as_deref(), Some("09:55:00"));
        let parts: Vec<_> = section
            .groups
            .iter()
            .flat_map(|group| &group.parts)
            .collect();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].source, "(encrypted)");
        assert!(!matches!(parts[0].body, Body::Arrow));
    }

    #[test]
    fn merged_codex_messages_have_individual_clock_titles() {
        let events = codex_fixture_events("tests/fixtures/codex-agent-messages/sequence.jsonl");
        assert_eq!(events.len(), 1);
        let event = &events[0];
        let detail = event_detail_content(event, "merged time".into(), Style::new().dim());
        assert_eq!(detail.sections.len(), 4);
        let expected_times = ["21:22:10", "21:22:35", "21:22:53", "21:22:53"];
        for (section, expected) in detail.sections.iter().zip(expected_times) {
            assert_eq!(section.title.left.as_deref(), None);
            assert_eq!(section.title.title, "tests_audit → root");
            assert_eq!(section.title.time.as_deref(), Some(expected));
            let body: Vec<_> = section
                .groups
                .iter()
                .flat_map(|group| &group.parts)
                .collect();
            assert_eq!(body.len(), 1);
            assert_eq!(body[0].source, "(encrypted)");
        }
    }

    #[test]
    fn section_titles_cover_single_tools_and_messages() {
        let parsed = crate::providers::claude::event_reader::tail(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-agent-display.jsonl"),
            100,
        )
        .unwrap();
        for source in parsed.iter().filter(|event| !event.calls.is_empty()) {
            let mut event = source.clone();
            event.calls.truncate(1);
            let detail = event_detail_content(&event, "event time".into(), Style::new().dim());
            let title = &detail.sections[0].title;
            let clock = event.calls[0].started_at.map(|at| {
                at.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            });
            assert_eq!(
                title.time.as_deref(),
                clock.as_deref().or(Some("event time"))
            );
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
        let detail = event_detail_content(&message, "event time".into(), Style::new().dim());
        let title = &detail.sections[0].title;
        assert_eq!(title.left.as_deref(), Some("peer"));
        assert_eq!(title.title, "Assistant");
        assert_eq!(title.time.as_deref(), Some("event time"));
    }

    #[test]
    fn tool_descriptions_are_centered_in_single_and_merged_title_rows() {
        let call = ToolCall {
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            title: "Check the workspace".into(),
            show_event_detail: false,
            id: "description-fixture".into(),
            summary: "Bash".into(),
            name: "Bash".into(),
            input: vec![],
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
        };
        for count in [1, 2] {
            let detail = event_detail_content(
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
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            title: "Inspect source".into(),
            show_event_detail: false,
            id: "call-id".into(),
            summary: "Agent".into(),
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
            let detail = event_detail_content(
                &event(vec![call.clone(); count]),
                "10:11:12".into(),
                Style::new().dim(),
            );
            assert_eq!(detail.sections.len(), count);
            for section in &detail.sections {
                let title = &section.title;
                assert_eq!(title.left.as_deref(), Some("Agent · 1"));
                assert_eq!(title.title, "Inspect source");
                assert_eq!(
                    title.time.as_deref(),
                    Some(if count == 1 { "10:11:12" } else { "12s" })
                );
                assert_eq!(
                    title.title_style,
                    super::super::style::event_styles(&Kind::Tool).0
                );
                assert_eq!(title.meta_style, Style::new().dim());
                assert_eq!(section.groups.len(), 3);
                assert_eq!(section.groups[0].parts.len(), 1);
                assert_eq!(section.groups[0].parts[0].source, "Find the entry");
                assert!(matches!(section.groups[1].parts[0].body, Body::Arrow));
                assert!(section.groups[2].parts[0].running());
            }
        }
        call.title = "source-reader".into();
        assert_eq!(call_title(&call).1, "source-reader");
        call.title = "Agent".into();
        assert_eq!(call_title(&call).1, "Agent");
        call.name = "Task call-id".into();
        assert_eq!(call_title(&call).0.as_deref(), Some("Task call-id · 1"));
        call.name = "Read".into();
        assert_eq!(call_title(&call).0.as_deref(), Some("Read · 1"));
        call.name = "Bash".into();
        assert_eq!(call_title(&call).0.as_deref(), Some("Bash · 1"));
        call.title.clear();
        assert_eq!(call_title(&call).0.as_deref(), Some("1"));
        assert_eq!(call_title(&call).1, "Bash");
        call.exit_code = None;
        for source in [
            include_str!("../../tests/fixtures/claude-client/ask_claude.jsonl"),
            include_str!("../../tests/fixtures/claude-client/notify_claude.jsonl"),
            include_str!("../../tests/fixtures/tool-list/claude.jsonl"),
        ] {
            let record: serde_json::Value =
                serde_json::from_str(source.lines().next().unwrap()).unwrap();
            call.name = record["payload"]["item"]["tool"]
                .as_str()
                .or_else(|| record["message"]["content"][0]["name"].as_str())
                .unwrap()
                .into();
            for subject in ["", " \t "] {
                call.title = subject.into();
                assert_eq!(
                    call_title(&call),
                    (None, crate::providers::tool::display_tool_name(&call.name))
                );
            }
            call.title = "Inspect source".into();
            assert_eq!(call_title(&call).1, "Inspect source");
        }
        call.title = "Inspect source".into();
        for name in ["advisor", "background"] {
            call.name = name.into();
            assert_eq!(call_title(&call), (None, "Inspect source".into()));
        }
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
            let detail = event_detail_content(&event, "event time".into(), Style::new().dim());
            assert_eq!(detail.sections.len(), 1);
            let title = &detail.sections[0].title;
            assert_eq!(title.time.as_deref(), Some("event time"));
            assert_eq!(title.left.as_deref(), Some("peer"));
            assert_eq!(
                title.title,
                if kind == Kind::Tool {
                    event.body.clone()
                } else {
                    super::super::event_list::kind_label(&kind)
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
                Kind::User
                    | Kind::Compaction
                    | Kind::Assistant
                    | Kind::Agent
                    | Kind::QueuedUser
                    | Kind::QueuedSystem
            ) {
                assert_eq!(parts.len(), 1);
                assert!(parts[0].markdown());
            } else {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[1].body, Body::Structured(_)));
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
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            title: "Bash".into(),
            show_event_detail: false,
            id: "running-copy".into(),
            summary: "Bash".into(),
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
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        highlight::use_theme(false, None).unwrap();
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
            format!("{}三个区块单击定位、双击复制已实现。\"", " ".repeat(column))
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
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let source = include_str!("../../tests/fixtures/codex-manager-await.jsonl").trim_end();
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Structured {
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
    fn json_documents_keep_record_key_order() {
        let raw = include_str!("../../tests/fixtures/codex-raw-detail.jsonl")
            .lines()
            .nth(1)
            .unwrap();
        let part = Part::text(raw.into(), false);
        let positions = [
            "\"type\"",
            "\"timestamp\"",
            "\"payload\"",
            "\"name\"",
            "\"arguments\"",
            "\"call_id\"",
        ]
        .map(|key| part.source.find(key).unwrap());
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{}",
            part.source
        );
    }

    #[test]
    fn json_documents_wrap_complete_lines_with_styles() {
        let raw = include_str!("../../tests/fixtures/codex-raw-detail.jsonl")
            .lines()
            .nth(2)
            .unwrap();
        let part = Part::text(raw.into(), false);
        let narrow = part.lines(33, Style::default(), chrono::Utc::now());
        assert!(narrow.iter().all(|line| line.width() <= 33));
        let styled = |lines: Vec<Line<'static>>| {
            lines
                .into_iter()
                .flat_map(|line| line.spans)
                .flat_map(|span| {
                    span.content
                        .bytes()
                        .map(|byte| (byte, span.style))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            styled(narrow),
            styled(part.lines(usize::MAX, Style::default(), chrono::Utc::now()))
        );
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
        assert!(matches!(parts[1].body, Body::Structured(_)));
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
                if matches!(part.body, Body::Structured(_)) {
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

    #[test]
    fn adjacent_output_blocks_keep_only_source_lines() {
        let mut parts = Vec::new();
        for section in [
            crate::providers::tool::code_section("stdout", "rs", "fn main() {}\n"),
            Section::Text {
                label: "stdout".into(),
                text: "---\n".into(),
            },
            crate::providers::tool::code_section("stdout", "py", "print('ready')\n"),
            crate::providers::tool::code_section("stdout", "toml", "[package]"),
        ] {
            push_section(&mut parts, &section, Style::new(), true);
        }
        assert_eq!(
            texts(&parts, 80),
            ["fn main() {}", "---", "print('ready')", "[package]"]
        );
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
            &Section::Structured {
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
    fn numbered_code_keeps_selection_and_logical_lines() {
        let source = "     1\tlet answer = \"ready\";\n     2\tprintln!(\"done\");";
        let mut parts = Vec::new();
        push_section(
            &mut parts,
            &Section::Code {
                label: String::new(),
                lang: "numbered:rs".into(),
                text: source.into(),
            },
            Style::default(),
            true,
        );
        let part = &parts[0];
        assert_eq!(part.source, source);
        let lines = part.lines(10, Style::default(), chrono::DateTime::UNIX_EPOCH);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), "     1    let answer = \"ready\";");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::DIM)
        );
        let maps = part.maps(&lines);
        for (row, expected) in source.lines().enumerate() {
            let cells = &maps[row];
            let first = cells.first().unwrap().1.start;
            let last = cells.last().unwrap().1.end;
            assert_eq!(&part.source[first..last], expected);
        }
    }

    #[test]
    fn markdown_after_other_output_starts_after_a_blank_line() {
        let mut parts = Vec::new();
        for text in [
            "| Name | State |\n| --- | --- |\n| a | waiting |",
            "| Thread | Attention |\n| --- | --- |\n| t | q |",
        ] {
            push_section(
                &mut parts,
                &Section::Markdown {
                    label: "output".into(),
                    text: text.into(),
                },
                Style::default(),
                true,
            );
        }
        let lines = texts(&parts, 80);
        let second = lines
            .iter()
            .position(|line| line.contains("Thread"))
            .unwrap();
        assert!(
            second > 0 && lines[second - 1].trim().is_empty(),
            "{lines:?}"
        );
        assert!(!lines[0].trim().is_empty(), "{lines:?}");
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
                agent_message: None,
                started_at: None,
                hide_output: false,
                model_settings: None,
                agent_thread_id: None,
                agent_path: None,
                title: "Bash".into(),
                show_event_detail: false,
                id: "Bash-fixture".into(),
                summary: "Bash".into(),
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
                agent_message: None,
                started_at: None,
                hide_output: false,
                model_settings: None,
                agent_thread_id: None,
                agent_path: None,
                title: "Read".into(),
                show_event_detail: false,
                id: "Read-fixture".into(),
                summary: "Read".into(),
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
                    agent_message: None,
                    started_at: None,
                    hide_output: false,
                    model_settings: None,
                    agent_thread_id: None,
                    agent_path: None,
                    title: "Run checks".into(),
                    show_event_detail: false,
                    id: "arrow-fixture".into(),
                    summary: "Run checks".into(),
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
                    assert_eq!(lines[0].style, super::super::style::event_jump_style());
                    assert!(part.maps(&lines)[0].is_empty());
                }
            }
        }
    }

    #[test]
    fn wide_code_and_diff_keep_logical_lines() {
        let parts = flattened_parts(&event(vec![ToolCall {
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            title: "Edit".into(),
            show_event_detail: false,
            id: "Edit-fixture".into(),
            summary: "Edit".into(),
            name: "Edit".into(),
            input: vec![Section::Diff {
                lang: "rs".into(),
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
                "-short   ",
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
            for mut event in codex.event_panel(&session, usize::MAX, false).unwrap() {
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
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            title: "Bash".into(),
            show_event_detail: false,
            id: "Bash-fixture".into(),
            summary: "Bash".into(),
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
            super::super::style::event_styles(&Kind::Error).0
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
                "  \"prompt\": \"Plan",
                "  • ship it\",",
                "  \"log\": \"line one",
                "  line two\",",
                "  \"title\": \"**inline**\"",
                "}",
            ]
        );
        let bold = lines[2]
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

#[cfg(test)]
#[path = "structured_text_tests.rs"]
mod structured_text_tests;

#[cfg(test)]
#[path = "tool_detail_output_tests.rs"]
mod output_tests;
