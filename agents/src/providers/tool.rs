pub(crate) mod groups;
mod shell_output;
pub(crate) mod shell_summary;

pub(crate) use shell_output::Stream;
use shell_output::sections as shell_output_sections;

use crate::code_format::Language;
use crate::event::{Section, SourceRecord, ToolCall};
use crate::images::{Image, ImageSource};
use crate::shell_commands::{ScriptKind, SyntaxSource, scripts};
use serde_json::Value;
use std::collections::BTreeMap;

/// Reads background output as plain text, retaining file-read error placeholders.
pub(crate) fn read_plain_output_file(path: &std::path::Path) -> Section {
    match std::fs::read_to_string(path) {
        Ok(text) => Section::Text {
            label: String::new(),
            text,
        },
        Err(error) => Section::Placeholder(if error.kind() == std::io::ErrorKind::NotFound {
            "File no longer exists".to_owned()
        } else {
            error.to_string()
        }),
    }
}

pub(crate) fn json_input(value: &Value) -> Vec<Section> {
    if value.is_null() || value.as_object().is_some_and(|object| object.is_empty()) {
        Vec::new()
    } else {
        vec![Section::Structured {
            label: "input".into(),
            text: value.to_string(),
        }]
    }
}

/// Decode MCP and Anthropic content blocks without exposing binary payloads.
pub(crate) fn mcp_content(
    content: &Value,
    source: Option<&(SourceRecord, String)>,
) -> Vec<Section> {
    if let Some(text) = content.as_str() {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![Section::Text {
                label: "output".into(),
                text: text.into(),
            }]
        };
    }
    let mut seen = Vec::new();
    content
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, block)| {
            if let Some(text) = block["text"].as_str().filter(|_| block["type"] == "text") {
                return Some(Section::Text {
                    label: "output".into(),
                    text: text.into(),
                });
            }
            let kind = block["type"].as_str().unwrap_or("unknown");
            let mime = block["mimeType"]
                .as_str()
                .or_else(|| block["source"]["media_type"].as_str())
                .or_else(|| block["resource"]["mimeType"].as_str());
            if kind == "image"
                && let Some((record, pointer)) = source
                && let Some(mime) = mime
            {
                let data = if block["source"]["data"].is_string() {
                    "source/data"
                } else {
                    "data"
                };
                let encoded = block.pointer(&format!("/{data}")).and_then(Value::as_str);
                if let Some(encoded) = encoded {
                    if seen.contains(&encoded) {
                        return None;
                    }
                    seen.push(encoded);
                }
                return Some(Section::Image(Image {
                    label: format!("[image {mime}]"),
                    source: ImageSource::Record {
                        record: record.clone(),
                        pointer: format!("{pointer}/{index}/{data}"),
                    },
                }));
            }
            Some(Section::Placeholder(match mime {
                Some(mime) => format!("[{kind} {mime}]"),
                None => format!("[{kind}]"),
            }))
        })
        .filter(|section| !matches!(section, Section::Text { text, .. } if text.is_empty()))
        .collect()
}

fn tool_name(name: &str) -> String {
    if let Some((server, tool)) = super::mcp::parse(name) {
        let declared = super::TOOL_RULES
            .iter()
            .flat_map(|rules| rules.servers)
            .any(|candidate| {
                candidate.name == server
                    && candidate
                        .tools
                        .iter()
                        .any(|rule| rule.name == tool || rule.aliases.contains(&tool))
            });
        if name.starts_with("mcp__") || name.contains(' ') || declared {
            return format!("{server} {tool}");
        }
        return tool.to_owned();
    }
    if name.starts_with("mcp__") || name.contains(' ') {
        return name.to_owned();
    }
    name.rsplit_once('.')
        .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
        .map_or_else(|| name.to_owned(), |(_, tool)| tool.to_owned())
}

/// One display-name rule for both providers; protocol namespaces are not tool names.
pub(crate) fn display_tool_name(name: &str) -> String {
    let name = tool_name(name);
    if name.starts_with("mcp__") {
        return name;
    }
    title_name(&name)
}

/// Preserve MCP server spelling and camel case; capitalize native tool labels.
pub(crate) fn title_name(name: &str) -> String {
    let name = tool_name(name);
    if let Some((server, tool)) = name.split_once(' ') {
        return format!("{server} {}", tool.replace('_', " "));
    }
    if !name.contains('_') && name.chars().skip(1).any(char::is_uppercase) {
        return name;
    }
    let text = if name.contains('_') {
        name.split('_')
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    } else {
        name
    };
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// The display rules used by one provider's transcripts.
pub(crate) struct ToolRules {
    pub native: &'static [ToolRule],
    pub servers: &'static [super::mcp::McpServer],
}

pub(crate) struct ToolData<'a> {
    pub input: &'a Value,
    pub result: Option<&'a ToolResult>,
}

pub(crate) struct ToolResult {
    /// The transcript record and JSON pointer of the content array.
    pub source: Option<(SourceRecord, String)>,
    pub content: Value,
    pub structured: Value,
    pub error: bool,
}

impl ToolResult {
    pub fn text(&self) -> String {
        self.content.as_str().map(str::to_owned).unwrap_or_else(|| {
            self.content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| match block["type"].as_str() {
                    Some("text") => block["text"].as_str(),
                    Some("tool_reference") => block["tool_name"].as_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Input,
    Result,
}

pub(crate) enum Pick<T> {
    Field(Source, &'static str),
    With(fn(&ToolData<'_>) -> Option<T>),
}

impl<T> Copy for Pick<T> {}
impl<T> Clone for Pick<T> {
    fn clone(&self) -> Self {
        *self
    }
}

pub(crate) enum Body {
    Sections(Vec<Section>),
    Hidden,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Format {
    Text,
    Markdown,
    /// Formats source code in the given language as a code section.
    Code(&'static str),
    /// A JSON array as a Markdown table, one row per element and one column per field;
    /// a JSON object is a single row.
    Table,
}

pub(crate) struct Presentation {
    pub title: Option<String>,
    pub input: Vec<Section>,
    pub output: Option<Body>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TaskAction {
    Create,
    Update,
    Plan,
}

/// A tool's field and display choices.
#[derive(Clone, Copy)]
pub(crate) struct ToolRule {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub title: &'static [Pick<String>],
    pub label: bool,
    pub input: Option<(Pick<Vec<Section>>, Format)>,
    pub output: Option<(Pick<Body>, Format)>,
    pub question: bool,
    pub hidden: bool,
    pub wrapper: bool,
    pub task: Option<TaskAction>,
    pub list_subject: bool,
    pub list_input: bool,
    /// How the input fields no title or input pick shows are drawn; JSON when unset.
    pub remaining_input: Option<Format>,
    pub shell_stream: Stream,
}

pub(crate) const DEFAULT_RULE: ToolRule = ToolRule {
    name: "",
    aliases: &[],
    title: &[],
    label: true,
    input: None,
    output: None,
    question: false,
    hidden: false,
    wrapper: false,
    task: None,
    list_subject: false,
    list_input: false,
    remaining_input: None,
    shell_stream: Stream {
        merged_stderr: false,
        trimmed: false,
        exit_code: None,
        is_error: false,
    },
};

pub(crate) fn rule(name: &str) -> &'static ToolRule {
    if let Some((server, tool)) = super::mcp::parse(name) {
        if let Some(server) = super::TOOL_RULES
            .iter()
            .flat_map(|rules| rules.servers)
            .find(|candidate| candidate.name == server)
        {
            return server
                .tools
                .iter()
                .find(|rule| rule.name == tool || rule.aliases.contains(&tool))
                .unwrap_or(&DEFAULT_RULE);
        }
        if name.starts_with("mcp__") || name.contains(' ') {
            return &DEFAULT_RULE;
        }
    }
    let name = tool_name(name);
    super::TOOL_RULES
        .iter()
        .flat_map(|rules| rules.native)
        .find(|rule| rule.name == name || rule.aliases.contains(&name.as_str()))
        .unwrap_or(&DEFAULT_RULE)
}

pub(crate) fn tool_kind(name: &str) -> crate::event::Kind {
    if rule(name).question {
        crate::event::Kind::Assistant
    } else {
        crate::event::Kind::Tool
    }
}

pub(crate) fn tool_hidden(name: &str) -> bool {
    rule(name).hidden
}

pub(crate) fn text<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input[field].as_str().filter(|text| !text.trim().is_empty())
}

/// A human description stands alone; shell calls use their declared command subject.
pub(crate) fn list_label(
    name: &str,
    subject: Option<&str>,
    input: &Value,
    exit_code: Option<i64>,
) -> (String, Option<String>) {
    let display = display_tool_name(name);
    let rule = rule(name);
    let subject = subject.filter(|subject| !subject.trim().is_empty());
    let input_text = match rule.input {
        Some((Pick::Field(Source::Input, field), _)) => input
            .pointer(field)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty()),
        _ => None,
    };
    let mut label = if let Some(description) = text(input, "description") {
        (description.to_owned(), None)
    } else if rule.list_subject {
        (subject.or(input_text).unwrap_or(&display).to_owned(), None)
    } else {
        (
            display,
            subject
                .or_else(|| rule.list_input.then_some(input_text).flatten())
                .map(str::to_owned),
        )
    };
    if let Some(code) = exit_code.filter(|code| *code != 0) {
        label
            .1
            .as_mut()
            .unwrap_or(&mut label.0)
            .push_str(&format!(" · {code}"));
    }
    label
}

pub(crate) fn list_title(
    name: &str,
    subject: Option<&str>,
    input: &Value,
    exit_code: Option<i64>,
) -> String {
    let (head, member) = list_label(name, subject, input, exit_code);
    let mut groups = groups::Groups::default();
    groups.add(&head, member.as_deref());
    groups.finish().unwrap()
}

pub(crate) fn shell_command(input: &Value) -> Option<&str> {
    text(input, "command").or_else(|| text(input, "cmd"))
}

fn field<'a>(data: &'a ToolData<'_>, source: Source, pointer: &str) -> Option<&'a Value> {
    match source {
        Source::Input => data.input.pointer(pointer),
        Source::Result => data.result?.structured.pointer(pointer),
    }
}

pub(crate) fn raw_input(input: &Value) -> Vec<Section> {
    match input.as_str() {
        Some(text) if !text.is_empty() => vec![text_section("input", text)],
        Some(_) => Vec::new(),
        None => json_input(input),
    }
}

pub(crate) fn picked_sections(
    value: &Value,
    pointer: &str,
    format: Format,
    output: bool,
) -> Vec<Section> {
    let label = pointer.rsplit('/').next().unwrap_or("");
    if let Format::Table = format {
        return markdown_table(value, label)
            .map(|text| Section::Markdown {
                label: label.into(),
                text,
            })
            .into_iter()
            .collect();
    }
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![match format {
        Format::Code(lang) => code_section(
            label,
            lang,
            &Language::from_extension(lang)
                .and_then(|l| l.format(&text))
                .unwrap_or(text),
        ),
        Format::Markdown => Section::Markdown {
            label: label.into(),
            text,
        },
        Format::Text if output => text_section("output", &text),
        Format::Text | Format::Table => Section::Prose {
            text,
            markdown: false,
        },
    }]
}

/// A string or number, or a list of them joined with commas; a true flag is its field name.
fn title_text(value: &Value, pointer: &str) -> Option<String> {
    let scalar = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    match value {
        Value::Bool(true) => pointer.rsplit('/').next().map(str::to_owned),
        Value::Array(items) => items
            .iter()
            .map(scalar)
            .collect::<Option<Vec<_>>>()
            .map(|items| items.join(", ")),
        _ => scalar(value),
    }
}

pub(crate) fn presentation(name: &str, data: &ToolData<'_>) -> Presentation {
    let rule = rule(name);
    let title = rule.title.iter().find_map(|pick| {
        let title = match pick {
            Pick::Field(source, pointer) => title_text(field(data, *source, pointer)?, pointer)?,
            Pick::With(function) => function(data)?,
        };
        (!title.trim().is_empty()).then_some(title)
    });
    let mut remaining = data.input.clone();
    for pick in rule.title {
        if let Pick::Field(Source::Input, pointer) = pick
            && let Some((parent, key)) = pointer.rsplit_once('/')
        {
            let key = key.replace("~1", "/").replace("~0", "~");
            if let Some(object) = remaining.pointer_mut(parent).and_then(Value::as_object_mut) {
                object.shift_remove(&key);
            }
        }
    }
    let input = match rule.input {
        Some((Pick::Field(source, pointer), format)) => {
            field(data, source, pointer).map(|value| picked_sections(value, pointer, format, false))
        }
        Some((Pick::With(function), _)) => function(data),
        None => None,
    }
    .unwrap_or_else(|| match rule.remaining_input {
        Some(format) => picked_sections(&remaining, "/input", format, false),
        None => raw_input(&remaining),
    });
    let output = if data.result.is_some() {
        Some(output(rule.output, data))
    } else {
        match rule.output {
            Some((Pick::With(function), _)) => function(data),
            _ => None,
        }
    };
    Presentation {
        title,
        input,
        output,
    }
}

pub(crate) fn shell_title(data: &ToolData<'_>) -> Option<String> {
    let title = shell_summary::summarize_shell_commands(shell_command(data.input)?);
    (!title.is_empty()).then_some(title)
}

pub(crate) fn question_title(data: &ToolData<'_>) -> Option<String> {
    let title = data.input["questions"]
        .as_array()?
        .iter()
        .filter_map(|question| text(question, "header"))
        .collect::<Vec<_>>()
        .join(" · ");
    (!title.is_empty()).then_some(title)
}

/// The file path of a file title, without its `:start-end` line range.
pub(crate) fn title_path(title: &str) -> &str {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    title
        .rsplit_once(':')
        .filter(|(_, lines)| {
            lines
                .split_once('-')
                .is_some_and(|(start, end)| digits(start) && digits(end))
        })
        .map_or(title, |(path, _)| path)
}
pub(crate) fn empty_input(_: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(Vec::new())
}
pub(crate) fn questions_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(question_sections(data.input))
}
fn formatted_shell(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for script in scripts(text) {
        let ScriptKind::Heredoc {
            literal: true,
            strip_tabs,
        } = script.kind
        else {
            continue;
        };
        let language = match script.syntax {
            SyntaxSource::Interpreter(name) => Language::from_interpreter(name),
            SyntaxSource::Extension(extension) => Language::from_extension(extension),
        };
        let Some(language) = language else {
            continue;
        };
        let body = &text[script.range.clone()];
        let tabs = if strip_tabs {
            body.lines()
                .filter(|line| !line.is_empty())
                .map(|line| line.bytes().take_while(|&byte| byte == b'\t').count())
                .min()
                .unwrap_or(0)
        } else {
            0
        };
        let prefix = "\t".repeat(tabs);
        let source = if strip_tabs {
            body.split_inclusive('\n')
                .map(|line| line.trim_start_matches('\t'))
                .collect()
        } else {
            body.to_owned()
        };
        let Some(mut formatted) = language.format(&source) else {
            continue;
        };
        if !formatted.ends_with('\n') {
            formatted.push('\n');
        }
        output.push_str(&text[cursor..script.range.start]);
        for line in formatted.split_inclusive('\n') {
            if line != "\n" {
                output.push_str(&prefix);
            }
            output.push_str(line);
        }
        cursor = script.range.end;
    }
    output.push_str(&text[cursor..]);
    output
}

pub(crate) fn shell_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        shell_command(data.input)
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                vec![Section::Shell {
                    text: formatted_shell(text),
                    prefix: 0,
                }]
            })
            .unwrap_or_default(),
    )
}

#[derive(Default)]
pub(crate) struct ResultState {
    pub exit_code: Option<i64>,
    pub is_error: bool,
    pub denied: bool,
}

/// Read explicit outcome metadata; output prose does not determine success.
pub(crate) fn result_state(text: &str, value: &Value, error: bool) -> ResultState {
    let exit_code = value["exit_code"]
        .as_i64()
        .or_else(|| {
            text.lines()
                .next()?
                .strip_prefix("Exit code ")?
                .parse()
                .ok()
        })
        .or_else(|| {
            text.starts_with("Chunk ID:").then_some(())?;
            text.split_once("\nOutput:\n")?
                .0
                .lines()
                .find_map(|line| line.strip_prefix("Process exited with code ")?.parse().ok())
        })
        .filter(|code| *code != 0);
    let reason = value["error"]["message"].as_str().unwrap_or(text);
    let denied = reason == "user rejected MCP tool call"
        || reason
            .strip_prefix("Error: ")
            .unwrap_or(reason)
            .starts_with("The user doesn't want to proceed with this tool use.");
    ResultState {
        exit_code,
        is_error: error
            || denied
            || exit_code.is_some()
            || value["isError"] == true
            || value["result"]["isError"] == true
            || value["status"] == "failed"
            || value["error"]["message"].is_string()
            || text.starts_with("Error: "),
        denied,
    }
}

pub(crate) fn result_kind(kind: crate::event::Kind, state: &ResultState) -> crate::event::Kind {
    if state.denied {
        crate::event::Kind::Denied
    } else if state.is_error {
        crate::event::Kind::Error
    } else {
        kind
    }
}

#[cfg(test)]
pub(crate) fn text_output(
    text: &str,
    mode: Option<(Pick<Body>, Format)>,
    input: &Value,
) -> Vec<Section> {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let content = if value["content"].is_array() {
        value["content"].clone()
    } else {
        Value::String(text.into())
    };
    let error = result_state(text, &value, false).is_error;
    let structured = if value.is_object() {
        value
    } else {
        Value::Null
    };
    let result = ToolResult {
        source: None,
        content,
        structured,
        error,
    };
    match output(
        mode,
        &ToolData {
            input,
            result: Some(&result),
        },
    ) {
        Body::Sections(sections) => sections,
        Body::Hidden => Vec::new(),
    }
}

pub(crate) fn raw_output(result: &ToolResult) -> Vec<Section> {
    let mut content = result.content.clone();
    if let Some(text) = content
        .as_str()
        .filter(|text| text.starts_with("Exit code "))
    {
        content = text.split_once('\n').map_or("", |(_, body)| body).into();
    }
    mcp_content(&content, result.source.as_ref())
        .into_iter()
        .map(|section| match section {
            Section::Text { label, text } if serde_json::from_str::<Value>(&text).is_ok() => {
                Section::Structured { label, text }
            }
            section => section,
        })
        .collect()
}

pub(crate) fn output(mode: Option<(Pick<Body>, Format)>, data: &ToolData<'_>) -> Body {
    let result = data
        .result
        .expect("output requires a completed tool result");
    let picked = match mode {
        Some((Pick::Field(source, pointer), format)) if !result.error => {
            field(data, source, pointer)
                .map(|value| Body::Sections(picked_sections(value, pointer, format, true)))
        }
        Some((Pick::With(function), _)) => function(data),
        _ => None,
    };
    picked.unwrap_or_else(|| Body::Sections(raw_output(result)))
}

pub(crate) fn questions_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    let value = &result.structured;
    let listed = if result.error {
        rejected_answers(&data.input["questions"], &result.text())
            .or_else(|| rejected_questions(&result.text()))
    } else {
        serde_json::from_value(value["answers"].clone())
            .ok()
            .map(|answers| answer_list(&value["questions"], &answers))
    }?;
    Some(Body::Sections(vec![text_section("output", &listed)]))
}

pub(crate) fn shell_sections(data: &ToolData<'_>, stream: Stream) -> Option<Body> {
    let result = data.result?;
    let value = &result.structured;
    let sections =
        if !result.error && (value.get("stdout").is_some() || value.get("stderr").is_some()) {
            ["stdout", "stderr"]
                .into_iter()
                .filter_map(|key| text(value, key).map(|text| text_section(key, text)))
                .collect()
        } else {
            raw_output(result)
        };
    let Some(command) = shell_command(data.input) else {
        return Some(Body::Sections(sections));
    };
    let state = result_state(&result.text(), value, result.error);
    Some(Body::Sections(
        sections
            .into_iter()
            .flat_map(|section| {
                if let Section::Text { label, text } = &section
                    && matches!(label.as_str(), "stdout" | "output")
                    && let Some(sections) = shell_output_sections(
                        command,
                        label,
                        text,
                        Stream {
                            exit_code: state.exit_code,
                            is_error: result.error,
                            ..stream
                        },
                    )
                {
                    return sections;
                }
                vec![section]
            })
            .collect(),
    ))
}

pub(crate) fn markdown_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    Some(Body::Sections(vec![Section::Markdown {
        label: "output".into(),
        text: result.text(),
    }]))
}
pub(crate) fn mcp_markdown_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    Some(Body::Sections(
        mcp_content(&result.content, result.source.as_ref())
            .into_iter()
            .map(|section| match section {
                Section::Text { label, text } => Section::Markdown { label, text },
                section => section,
            })
            .collect(),
    ))
}

/// Literal text inside a Markdown table cell.
pub(crate) fn table_cell(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character.is_ascii_punctuation() {
                vec!['\\', character]
            } else if matches!(character, '\n' | '\r') {
                vec![' ']
            } else {
                vec![character]
            }
        })
        .collect()
}

/// A Markdown table of a JSON array: objects give one column per field, in first-seen order;
/// scalars give a single column headed `name`.
pub(crate) fn markdown_table(value: &Value, name: &str) -> Option<String> {
    let rows = match value {
        Value::Object(object) if !object.is_empty() => std::slice::from_ref(value),
        _ => value.as_array().filter(|rows| !rows.is_empty())?.as_slice(),
    };
    let cell = |value: &Value| {
        table_cell(
            &value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned),
        )
    };
    let mut columns: Vec<&str> = Vec::new();
    for row in rows {
        for key in row.as_object().into_iter().flat_map(|object| object.keys()) {
            if !columns.contains(&key.as_str()) {
                columns.push(key);
            }
        }
    }
    let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
    if columns.is_empty() {
        let mut lines = vec![line(vec![table_cell(name)]), "| --- |".to_owned()];
        lines.extend(rows.iter().map(|row| line(vec![cell(row)])));
        return Some(lines.join("\n"));
    }
    let mut lines = vec![
        line(columns.iter().map(|column| table_cell(column)).collect()),
        line(vec!["---".to_owned(); columns.len()]),
    ];
    lines.extend(rows.iter().map(|row| {
        line(
            columns
                .iter()
                .map(|column| row.get(column).map_or_else(String::new, cell))
                .collect(),
        )
    }));
    Some(lines.join("\n"))
}

pub(crate) fn code_section(label: &str, lang: &str, text: &str) -> Section {
    Section::Code {
        label: label.to_owned(),
        lang: lang.to_owned(),
        text: text.to_owned(),
    }
}

pub(crate) fn file_language(path: &str) -> &str {
    let path = std::path::Path::new(path);
    path.extension()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("")
}

pub(crate) fn file_section(label: &str, path: &str, text: &str) -> Section {
    let lang = file_language(path);
    if lang.eq_ignore_ascii_case("md") || lang.eq_ignore_ascii_case("markdown") {
        Section::Markdown {
            label: label.to_owned(),
            text: text.to_owned(),
        }
    } else {
        code_section(label, lang, text)
    }
}

pub(crate) fn text_section(label: &str, text: &str) -> Section {
    Section::Text {
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

pub(crate) fn question_texts(input: &Value) -> Vec<String> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|question| question["question"].as_str())
        .map(str::to_owned)
        .collect()
}

/// Lists an ask's answers the way the question view's Submit tab does: a warning while
/// questions are unanswered, then one `● header → answer` line per answered question.
pub(crate) fn answer_list(questions: &Value, answers: &BTreeMap<String, String>) -> String {
    let questions = questions.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut blocks = Vec::new();
    if questions
        .iter()
        .any(|question| !answers.contains_key(question["question"].as_str().unwrap_or("")))
    {
        blocks.push("You have not answered all questions".to_owned());
    }
    let lines = answered_lines(questions, answers);
    if !lines.is_empty() {
        blocks.push(lines.join("\n"));
    }
    blocks.join("\n\n")
}

/// One `● header → answer` line per answered question.
fn answered_lines(questions: &[Value], answers: &BTreeMap<String, String>) -> Vec<String> {
    questions
        .iter()
        .filter_map(|question| {
            let answer = answers.get(question["question"].as_str()?)?;
            let header = text(question, "header").or_else(|| text(question, "question"))?;
            Some(format!("● {header} → {answer}"))
        })
        .collect()
}

/// Shows an ask the user chose to chat about as `Chat about this`, followed by the
/// answers given before they did.
fn chat_about_this(questions: &Value, answers: &BTreeMap<String, String>) -> String {
    let questions = questions.as_array().map(Vec::as_slice).unwrap_or_default();
    let lines = answered_lines(questions, answers);
    if lines.is_empty() {
        return "Chat about this".to_owned();
    }
    format!("Chat about this\n\n{}", lines.join("\n"))
}

/// Shows a rejected ask as `Rejected` with the user's note, or as `Chat about this` with
/// any answers its "Questions asked:" list reports; dismissed messages have no answers.
/// Other errors keep their text.
pub(crate) fn rejected_answers(questions: &Value, text: &str) -> Option<String> {
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    let dismissed = |reason: &str| {
        reason.starts_with("The user dismissed these questions because the message you sent with them was not shown.")
    };
    let said = if let Some(reason) = text
        .strip_prefix("PreToolUse:")
        .and_then(|text| text.split_once(" hook error: ").map(|(_, reason)| reason))
    {
        if !dismissed(reason) && !reason.starts_with("The user wants to clarify these questions.") {
            return None;
        }
        reason
    } else {
        let reason = text.strip_prefix("The user doesn't want to proceed with this tool use.")?;
        let Some((_, said)) = reason.split_once("the user said:\n") else {
            return Some("Rejected".to_owned());
        };
        said
    };
    if dismissed(said) {
        return Some(chat_about_this(questions, &BTreeMap::new()));
    }
    if !said.starts_with("The user wants to clarify these questions.") {
        return Some(format!("Rejected\n\n{}", said.trim()));
    }
    let mut answers = BTreeMap::new();
    let (_, asked) = said.split_once("Questions asked:")?;
    let mut lines = asked.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        let Some(question) = line
            .strip_prefix("- \"")
            .and_then(|line| line.strip_suffix('"'))
        else {
            continue;
        };
        if let Some(answer) = lines.peek().and_then(|line| line.strip_prefix("Answer: ")) {
            answers.insert(question.to_owned(), answer.to_owned());
        }
    }
    Some(chat_about_this(questions, &answers))
}

pub(crate) fn rejected_questions(text: &str) -> Option<String> {
    let (explanation, questions) = text.split_once("Questions asked:")?;
    Some(format!(
        "Questions asked:{}\n\n{}",
        questions.trim_end(),
        explanation.trim_end()
    ))
}

/// One titled section per question: its text, then its numbered options.
pub(crate) fn question_sections(input: &Value) -> Vec<Section> {
    let mut sections = Vec::new();
    for question in input["questions"].as_array().into_iter().flatten() {
        let Some(asked) = text(question, "question") else {
            continue;
        };
        if let Some(header) = text(question, "header") {
            sections.push(Section::Label(header.to_owned()));
        }
        let suffix = if question["multiSelect"] == true {
            " (multi-select)"
        } else {
            ""
        };
        let options = question["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                text(option, "label").map(|label| (label, text(option, "description")))
            });
        let lines: Vec<String> = std::iter::once(format!("{asked}{suffix}"))
            .chain(
                options
                    .enumerate()
                    .map(|(index, (label, description))| match description {
                        Some(description) => format!("{}. {label}: {description}", index + 1),
                        None => format!("{}. {label}", index + 1),
                    }),
            )
            .collect();
        sections.push(Section::Prose {
            text: lines.join("\n"),
            markdown: false,
        });
    }
    sections
}

fn printf_separator(format: &str, args: &[&str]) -> Option<String> {
    let mut output = String::new();
    let mut argument = 0;
    loop {
        let mut chars = format.chars();
        let start = argument;
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => output.push(match chars.next()? {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    _ => return None,
                }),
                '%' => match chars.next()? {
                    '%' => output.push('%'),
                    's' => {
                        output.push_str(args.get(argument).copied().unwrap_or(""));
                        argument += 1;
                    }
                    _ => return None,
                },
                ch => output.push(ch),
            }
        }
        if argument >= args.len() {
            return Some(output);
        }
        if start == argument {
            return None;
        }
    }
}

/// Add headings for separately reported stderr output.
pub(super) fn annotate(call: &mut ToolCall) {
    if let Some(output) = &mut call.output {
        *output = std::mem::take(output)
            .into_iter()
            .filter(|section| !matches!(section, Section::Text { text, .. } | Section::Markdown { text, .. } | Section::Prose { text, .. } | Section::Code { text, .. } | Section::Diff { text, .. } | Section::Shell { text, .. } | Section::Structured { text, .. } if text.is_empty()))
            .flat_map(label_section)
            .collect();
    }
}

fn label_section(section: Section) -> Vec<Section> {
    let label = match &section {
        Section::Code { label, .. }
        | Section::Markdown { label, .. }
        | Section::Text { label, .. }
            if label == "stderr" =>
        {
            Some(label.clone())
        }
        _ => None,
    };
    label
        .map(Section::Label)
        .into_iter()
        .chain(std::iter::once(section))
        .collect()
}

#[cfg(test)]
mod tests {
    fn recorded_heredoc_command() -> String {
        let record: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/claude-shell-heredoc.jsonl"
        ))
        .unwrap();
        record["message"]["content"][0]["input"]["command"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn shown_shell(command: &str) -> String {
        let input = serde_json::json!({"command": command});
        let sections = super::shell_input(&super::ToolData {
            input: &input,
            result: None,
        })
        .unwrap();
        let [crate::event::Section::Shell { text, prefix: 0 }] = sections.as_slice() else {
            panic!("expected shell section: {sections:?}");
        };
        text.clone()
    }

    #[test]
    fn shell_heredoc_formats_recorded_body_and_preserves_shell_bytes() {
        let command = recorded_heredoc_command();
        let script = crate::shell_commands::scripts(&command).remove(0);
        let body = &command[script.range.clone()];
        let formatted = crate::code_format::Language::JavaScript
            .format(body)
            .unwrap();
        let expected = format!(
            "{}{}{}",
            &command[..script.range.start],
            formatted,
            &command[script.range.end..]
        );
        assert_ne!(expected, command);
        assert_eq!(shown_shell(&command), expected);
        let node = command.replacen("cat > render.mjs", "node", 1);
        assert_eq!(
            shown_shell(&node),
            expected.replacen("cat > render.mjs", "node", 1)
        );
        assert_eq!(shown_shell(&expected), expected);
    }

    #[test]
    fn shell_heredoc_keeps_expansion_arguments_and_invalid_code() {
        let command = recorded_heredoc_command();
        let script = crate::shell_commands::scripts(&command).remove(0);
        let body = &command[script.range.clone()];
        for changed in [
            command
                .replace("<<'EOF'", "<<EOF")
                .replace(body, "console.log('$HOME')\n"),
            format!("node -e '{}'", body.replace('\'', "\"")),
            command.replace(body, "const broken = (\n"),
            command.replace("render.mjs", "render.py"),
        ] {
            assert_eq!(shown_shell(&changed), changed);
        }
    }

    #[test]
    fn shell_heredoc_formats_multiple_bodies_in_source_order() {
        let command = recorded_heredoc_command();
        let script = crate::shell_commands::scripts(&command).remove(0);
        let body = &command[script.range.clone()];
        let formatted = crate::code_format::Language::JavaScript
            .format(body)
            .unwrap();
        let command = format!("node <<'EOF' <<'SECOND'\n{body}EOF\n{body}SECOND\necho done");
        let expected =
            format!("node <<'EOF' <<'SECOND'\n{formatted}EOF\n{formatted}SECOND\necho done");
        assert_eq!(shown_shell(&command), expected);
    }

    #[test]
    fn shell_heredoc_restores_shortest_tab_prefix() {
        let command = recorded_heredoc_command();
        let script = crate::shell_commands::scripts(&command).remove(0);
        let body = format!("\n{}", &command[script.range.clone()]);
        let formatted = crate::code_format::Language::JavaScript
            .format(&body)
            .unwrap();
        for prefix in ["", "\t", "\t\t"] {
            let indented = body
                .lines()
                .enumerate()
                .map(|(index, line)| {
                    if line.is_empty() {
                        "\n".to_owned()
                    } else {
                        format!("{prefix}{}{line}\n", if index == 1 { "\t" } else { "" })
                    }
                })
                .collect::<String>();
            let expected = formatted
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        "\n".to_owned()
                    } else {
                        format!("{prefix}{line}\n")
                    }
                })
                .collect::<String>();
            let command = format!("node <<-'EOF'\n{indented}\tEOF\necho done");
            assert_eq!(
                shown_shell(&command),
                format!("node <<-'EOF'\n{expected}\tEOF\necho done")
            );
        }
    }
    #[test]
    fn field_titles_accept_numbers_and_numbers_in_arrays() {
        for (value, expected) in [
            (serde_json::json!(400), "400"),
            (serde_json::json!(-2), "-2"),
            (serde_json::json!(2.5), "2.5"),
            (
                serde_json::json!([400, "wide", -2, 2.5]),
                "400, wide, -2, 2.5",
            ),
        ] {
            assert_eq!(
                super::title_text(&value, "/size").as_deref(),
                Some(expected)
            );
        }
        for value in [
            serde_json::json!(false),
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!([400, false]),
            serde_json::json!([[400]]),
        ] {
            assert_eq!(super::title_text(&value, "/size"), None);
        }
        assert_eq!(
            super::title_text(&serde_json::json!(true), "/options/createIfEmpty").as_deref(),
            Some("createIfEmpty")
        );
    }

    fn shell_output(command: &str, text: &str) -> Vec<crate::event::Section> {
        super::shell_output_sections(command, "stdout", text, super::Stream::default())
            .unwrap_or_else(|| {
                vec![crate::event::Section::Text {
                    label: "stdout".into(),
                    text: text.into(),
                }]
            })
    }

    #[test]
    fn shell_output_cd_before_sed() {
        let text = "export const check = true;\n".repeat(36);
        assert_eq!(
            shell_output("cd /a/b/tests && sed -n 35,70p checks.test.mjs", &text),
            vec![super::code_section("stdout", "mjs", &text)]
        );
    }

    #[test]
    fn empty_captured_output_has_no_sections() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let mut content = record["message"]["content"][0]["content"].clone();
        content = serde_json::Value::String(content.as_str().unwrap().chars().take(0).collect());
        assert!(super::mcp_content(&content, None).is_empty());
    }

    #[test]
    fn unsupported_content_blocks_keep_type_and_mime_without_payload() {
        let record: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/mcp-codex/image.json"))
                .unwrap();
        let image = &record["payload"]["item"]["result"]["content"][1];
        // These types were absent from local transcripts; vary a captured media block.
        for kind in ["audio", "resource_link", "future_content"] {
            let mut block = image.clone();
            block["type"] = kind.into();
            assert_eq!(
                super::mcp_content(&serde_json::Value::Array(vec![block]), None),
                vec![crate::event::Section::Placeholder(format!(
                    "[{kind} image/png]"
                ))]
            );
        }
    }
}

#[cfg(test)]
mod f4_tests {
    #[test]
    fn identical_mcp_images_are_emitted_once() {
        let value: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-read-image.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let block = value["message"]["content"][0]["content"][0].clone();
        let source = (
            crate::event::SourceRecord {
                path: std::path::PathBuf::from("unused"),
                offset: 0,
            },
            "/message/content/0/content".to_owned(),
        );
        let sections = super::mcp_content(&serde_json::json!([block, block]), Some(&source));
        assert_eq!(sections.len(), 1);
    }
}
