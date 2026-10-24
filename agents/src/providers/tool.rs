pub(crate) mod groups;
mod shell_output;
pub(crate) mod shell_summary;

use shell_output::{Stream, sections as shell_output_sections};

use crate::event::{ImageBlock, Section, SourceRecord, ToolCall};
use serde_json::Value;
use std::collections::BTreeMap;

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
    content
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, block)| {
            if let Some(text) = block["text"].as_str().filter(|_| block["type"] == "text") {
                return Section::Text {
                    label: "output".into(),
                    text: text.into(),
                };
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
                return Section::Image(ImageBlock {
                    media_type: mime.into(),
                    record: record.clone(),
                    pointer: format!("{pointer}/{index}/{data}"),
                });
            }
            Section::Placeholder(match mime {
                Some(mime) => format!("[{kind} {mime}]"),
                None => format!("[{kind}]"),
            })
        })
        .filter(|section| !matches!(section, Section::Text { text, .. } if text.is_empty()))
        .collect()
}

fn tool_name(name: &str) -> String {
    if name.contains(' ') {
        return name.to_owned();
    }
    let Some((server, tool)) = name
        .strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
    else {
        return name
            .rsplit_once('.')
            .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
            .map(|(server, tool)| {
                let mcp = format!("{server} {tool}");
                if TOOL_RULES.iter().any(|rule| rule.name == mcp) {
                    mcp
                } else {
                    tool.to_owned()
                }
            })
            .unwrap_or_else(|| name.to_owned());
    };
    let server = if let Some(rest) = server.strip_prefix("plugin_") {
        match rest.split_once('_') {
            Some((plugin, server)) if !plugin.is_empty() => server,
            _ => return name.to_owned(),
        }
    } else {
        server
    };
    if server.is_empty() || tool.is_empty() || tool.contains("__") {
        name.to_owned()
    } else {
        [server, tool].join(" ")
    }
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
    /// A JSON array as a Markdown table: one row per element, one column per field.
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

/// All tool-specific field and display choices are declared once, independent of provider.
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
    /// Appends the codex-manager requests still waiting for Claude to the output.
    pub attention: bool,
    shell_stream: Stream,
}

const DEFAULT_RULE: ToolRule = ToolRule {
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
    attention: false,
    shell_stream: Stream {
        merged_stderr: false,
        trimmed: false,
        exit_code: None,
        is_error: false,
    },
};

pub(crate) const TOOL_RULES: &[ToolRule] = &[
    ToolRule {
        name: "task-notification",
        label: false,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "advisor",
        label: false,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "background",
        label: false,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SubagentHandback",
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::Field(Source::Result, "/message"), Format::Text)),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Read",
        title: &[
            Pick::With(read_title),
            Pick::Field(Source::Result, "/file/filePath"),
            Pick::Field(Source::Input, "/file_path"),
        ],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(read_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Write",
        title: &[Pick::Field(Source::Input, "/file_path")],
        input: Some((Pick::With(write_input), Format::Text)),
        output: Some((Pick::With(write_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Edit",
        title: &[Pick::Field(Source::Input, "/file_path")],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(patch_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Bash",
        title: &[Pick::With(shell_description)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: false,
            trimmed: true,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "exec_command",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "shell",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "CommandExecution",
        title: &[Pick::With(shell_title)],
        input: Some((Pick::With(shell_input), Format::Text)),
        output: Some((Pick::With(merged_shell_output), Format::Text)),
        list_subject: true,
        shell_stream: Stream {
            merged_stderr: true,
            trimmed: false,
            exit_code: None,
            is_error: false,
        },
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "ToolSearch",
        title: &[Pick::Field(Source::Input, "/query")],
        input: Some((Pick::With(empty_input), Format::Text)),
        output: Some((Pick::With(matches_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "AskUserQuestion",
        title: &[Pick::With(question_title)],
        input: Some((Pick::With(questions_input), Format::Text)),
        output: Some((Pick::With(questions_output), Format::Text)),
        question: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "request_user_input",
        title: &[Pick::With(question_title)],
        input: Some((Pick::With(questions_input), Format::Text)),
        output: Some((Pick::With(questions_output), Format::Text)),
        question: true,
        ..DEFAULT_RULE
    },
    // Message owns spawn names and prompt bodies. Other input fields, including
    // subagent_type, run_in_background and isolation, are intentionally not displayed.
    ToolRule {
        name: "Agent",
        aliases: &["Task"],
        output: Some((Pick::With(agent_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "spawn_agent",
        output: Some((Pick::With(hidden_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SendMessage",
        title: &[Pick::With(recipient_title)],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::With(markdown_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "send_message",
        title: &[Pick::Field(Source::Input, "/target")],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::With(empty_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "followup_task",
        title: &[Pick::Field(Source::Input, "/target")],
        input: Some((Pick::Field(Source::Input, "/message"), Format::Markdown)),
        output: Some((Pick::With(empty_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "wait_agent",
        input: Some((Pick::Field(Source::Input, "/timeout_ms"), Format::Text)),
        output: Some((Pick::With(wait_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "WebSearch",
        input: Some((Pick::Field(Source::Input, "/query"), Format::Text)),
        output: Some((Pick::With(web_search_output), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "WebFetch",
        title: &[Pick::Field(Source::Input, "/url")],
        input: Some((Pick::With(fetch_input), Format::Text)),
        output: Some((Pick::Field(Source::Result, "/result"), Format::Markdown)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Skill",
        title: &[Pick::Field(Source::Input, "/skill")],
        input: Some((Pick::With(arguments_input), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SendUserFile",
        input: Some((Pick::Field(Source::Input, "/caption"), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "apply_patch",
        title: &[Pick::Field(Source::Input, "/paths")],
        input: Some((Pick::With(empty_input), Format::Text)),
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "exec",
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "view_image",
        wrapper: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "update_plan",
        task: Some(TaskAction::Plan),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskCreate",
        hidden: true,
        task: Some(TaskAction::Create),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskUpdate",
        hidden: true,
        task: Some(TaskAction::Update),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskList",
        hidden: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "TaskGet",
        hidden: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "ListAgents",
        hidden: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-in-chrome javascript_tool",
        input: Some((Pick::Field(Source::Input, "/text"), Format::Code("js"))),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-in-chrome find",
        input: Some((Pick::Field(Source::Input, "/query"), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-in-chrome computer",
        input: Some((Pick::With(claude_in_chrome_computer_input), Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-client ask_claude",
        input: Some((Pick::Field(Source::Input, "/text"), Format::Markdown)),
        output: Some((Pick::With(mcp_markdown_output), Format::Text)),
        list_input: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-client notify_claude",
        input: Some((Pick::Field(Source::Input, "/text"), Format::Markdown)),
        output: Some((Pick::With(mcp_markdown_output), Format::Text)),
        list_input: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager start",
        title: &[Pick::Field(Source::Input, "/name")],
        input: Some((Pick::Field(Source::Input, "/prompt"), Format::Markdown)),
        output: Some((Pick::Field(Source::Result, "/threadId"), Format::Text)),
        attention: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager send",
        title: &[Pick::Field(Source::Input, "/threadId")],
        input: Some((Pick::Field(Source::Input, "/prompt"), Format::Markdown)),
        attention: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager reply",
        title: &[Pick::Field(Source::Input, "/threadId")],
        input: Some((Pick::Field(Source::Input, "/ask"), Format::Text)),
        output: Some((Pick::With(codex_reply_output), Format::Text)),
        attention: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager list",
        input: Some((Pick::Field(Source::Input, "/state"), Format::Table)),
        output: Some((Pick::With(codex_thread_table), Format::Text)),
        attention: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager interrupt",
        title: &[Pick::Field(Source::Input, "/threadId")],
        attention: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager detach",
        title: &[
            Pick::Field(Source::Result, "/name"),
            Pick::Field(Source::Input, "/threadId"),
        ],
        output: Some((Pick::Field(Source::Result, "/threadId"), Format::Text)),
        attention: true,
        ..DEFAULT_RULE
    },
];

pub(crate) fn rule(name: &str) -> &'static ToolRule {
    let name = tool_name(name);
    TOOL_RULES
        .iter()
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

fn text<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input[field].as_str().filter(|text| !text.trim().is_empty())
}

/// A message's named recipient; a peer session's socket address names nobody.
fn recipient(input: &Value) -> Option<&str> {
    text(input, "to").filter(|to| !to.starts_with("uds:"))
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

fn shell_command(input: &Value) -> Option<&str> {
    text(input, "command").or_else(|| text(input, "cmd"))
}

fn field<'a>(data: &'a ToolData<'_>, source: Source, pointer: &str) -> Option<&'a Value> {
    match source {
        Source::Input => data.input.pointer(pointer),
        Source::Result => data.result?.structured.pointer(pointer),
    }
}

fn raw_input(input: &Value) -> Vec<Section> {
    match input.as_str() {
        Some(text) if !text.is_empty() => vec![text_section("input", text)],
        Some(_) => Vec::new(),
        None => json_input(input),
    }
}

fn picked_sections(value: &Value, pointer: &str, format: Format, output: bool) -> Vec<Section> {
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
        Format::Code(lang) => code_section(label, lang, &format_code(lang, &text)),
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

fn format_code(lang: &str, text: &str) -> String {
    match lang {
        "js" => {
            use dprint_plugin_typescript::configuration::{
                Configuration, ConfigurationBuilder, QuoteStyle,
            };
            use dprint_plugin_typescript::{FormatTextOptions, format_text};

            static CONFIG: std::sync::OnceLock<Configuration> = std::sync::OnceLock::new();
            let config = CONFIG.get_or_init(|| {
                ConfigurationBuilder::new()
                    .quote_style(QuoteStyle::PreferSingle)
                    .line_width(80)
                    .build()
            });
            match format_text(FormatTextOptions {
                path: std::path::Path::new("input.js"),
                extension: None,
                text: text.to_owned(),
                config,
                external_formatter: None,
            }) {
                Ok(Some(formatted)) => formatted,
                Ok(None) | Err(_) => text.to_owned(),
            }
        }
        _ => text.to_owned(),
    }
}

pub(crate) fn presentation(name: &str, data: &ToolData<'_>) -> Presentation {
    let rule = rule(name);
    let title = rule.title.iter().find_map(|pick| {
        let title = match pick {
            Pick::Field(source, pointer) => field(data, *source, pointer)?.as_str()?.to_owned(),
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
                object.remove(&key);
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
    .unwrap_or_else(|| raw_input(&remaining));
    let mut output = if data.result.is_some() {
        Some(output(rule.output, data))
    } else {
        match rule.output {
            Some((Pick::With(function), _)) => function(data),
            _ => None,
        }
    };
    if rule.attention
        && let Some(Body::Sections(sections)) = &mut output
    {
        sections.extend(codex_attention_section(data));
    }
    Presentation {
        title,
        input,
        output,
    }
}

fn shell_title(data: &ToolData<'_>) -> Option<String> {
    let title = shell_summary::summarize_shell_commands(shell_command(data.input)?);
    (!title.is_empty()).then_some(title)
}
fn shell_description(data: &ToolData<'_>) -> Option<String> {
    text(data.input, "description")
        .map(str::to_owned)
        .or_else(|| shell_title(data))
}
fn recipient_title(data: &ToolData<'_>) -> Option<String> {
    recipient(data.input).map(str::to_owned)
}
fn question_title(data: &ToolData<'_>) -> Option<String> {
    let title = data.input["questions"]
        .as_array()?
        .iter()
        .filter_map(|question| text(question, "header"))
        .collect::<Vec<_>>()
        .join(" · ");
    (!title.is_empty()).then_some(title)
}
fn read_title(data: &ToolData<'_>) -> Option<String> {
    if data.input.get("offset").is_none() && data.input.get("limit").is_none() {
        return None;
    }
    let file = &data.result?.structured["file"];
    let path = text(file, "filePath")?;
    let start = file["startLine"].as_u64()?;
    let count = file["numLines"].as_u64()?;
    Some(format!(
        "{path}:{start}-{}",
        start.saturating_add(count.saturating_sub(1))
    ))
}
/// The file path of a file title, without the `:start-end` line range of a partial Read.
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
fn empty_input(_: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(Vec::new())
}
fn questions_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(question_sections(data.input))
}
fn shell_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        shell_command(data.input)
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                vec![Section::Shell {
                    text: text.into(),
                    prefix: 0,
                }]
            })
            .unwrap_or_default(),
    )
}
fn arguments_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        text(data.input, "args")
            .map(|text| vec![text_section("args", text)])
            .unwrap_or_default(),
    )
}
fn fetch_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let mut sections = Vec::new();
    if let Some(url) = text(data.input, "url") {
        sections.push(text_section("", &format!("url: {url}")));
    }
    if let Some(prompt) = text(data.input, "prompt") {
        sections.push(Section::Prose {
            text: prompt.into(),
            markdown: false,
        });
    }
    Some(sections)
}
fn claude_in_chrome_computer_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    let action = text(data.input, "action")?;
    let text = match text(data.input, "action_summary") {
        Some(summary) => format!("{action}  {summary}"),
        None => action.to_owned(),
    };
    Some(vec![Section::Prose {
        text,
        markdown: false,
    }])
}
fn write_input(data: &ToolData<'_>) -> Option<Vec<Section>> {
    Some(
        match (
            text(data.input, "file_path"),
            data.input["content"].as_str(),
        ) {
            (Some(path), Some(content)) => vec![file_section("input", path, content)],
            _ => data
                .input
                .get("content")
                .map(|value| picked_sections(value, "/content", Format::Text, false))
                .unwrap_or_default(),
        },
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

fn raw_output(result: &ToolResult) -> Vec<Section> {
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

fn output(mode: Option<(Pick<Body>, Format)>, data: &ToolData<'_>) -> Body {
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

fn hidden_output(data: &ToolData<'_>) -> Option<Body> {
    (!data.result.is_some_and(|result| result.error)).then_some(Body::Hidden)
}
fn empty_output(data: &ToolData<'_>) -> Option<Body> {
    (!data.result.is_some_and(|result| result.error)).then_some(Body::Sections(Vec::new()))
}
fn agent_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    if value["isAsync"] == true
        || matches!(
            value["status"].as_str(),
            Some("async_launched" | "teammate_spawned")
        )
    {
        Some(Body::Hidden)
    } else {
        markdown_output(data)
    }
}
fn wait_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    let text = if value["timed_out"] == true {
        "(timed out)".to_owned()
    } else {
        value["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| result.text())
    };
    Some(Body::Sections(mcp_content(&Value::String(text), None)))
}
fn questions_output(data: &ToolData<'_>) -> Option<Body> {
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
fn read_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let value = &result.structured;
    if value["type"] == "image" {
        // Use the message copy so Read and MCP images share the same record pointer.
        Some(Body::Sections(mcp_content(
            &result.content,
            result.source.as_ref(),
        )))
    } else {
        Some(Body::Sections(vec![file_section(
            "output",
            value["file"]["filePath"].as_str().unwrap_or(""),
            value["file"]["content"].as_str()?,
        )]))
    }
}
fn patch_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    Some(Body::Sections(vec![patch_section(&result.structured)?]))
}
fn write_output(data: &ToolData<'_>) -> Option<Body> {
    if data.result?.structured["type"] == "update" {
        patch_output(data)
    } else {
        None
    }
}
fn matches_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let text = result.structured["matches"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    Some(Body::Sections(vec![text_section("output", &text)]))
}
fn shell_output(data: &ToolData<'_>) -> Option<Body> {
    shell_sections(data, rule("Bash").shell_stream)
}
pub(super) fn merged_shell_output(data: &ToolData<'_>) -> Option<Body> {
    shell_sections(data, rule("exec_command").shell_stream)
}
fn shell_sections(data: &ToolData<'_>, stream: Stream) -> Option<Body> {
    let result = data.result?;
    let value = &result.structured;
    if !result.error
        && value["backgroundTaskId"].is_string()
        && let Some((_, path)) = result.text().split_once("Output is being written to: ")
        && let Some((path, _)) = path.split_once(". You will be notified")
        && !path.is_empty()
    {
        return Some(Body::Sections(vec![Section::OutputFile(path.into())]));
    }
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

fn markdown_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    Some(Body::Sections(vec![Section::Markdown {
        label: "output".into(),
        text: result.text(),
    }]))
}
fn mcp_markdown_output(data: &ToolData<'_>) -> Option<Body> {
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
fn web_search_output(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct SearchLink {
        title: String,
        url: String,
    }
    let result_text = result.text();
    let (_, links) = result_text.split_once("\n\nLinks: ")?;
    let mut stream = serde_json::Deserializer::from_str(links).into_iter::<Vec<SearchLink>>();
    let links_found = stream.next()?.ok()?;
    let mut text = String::from("| Title |\n| --- |\n");
    for link in links_found {
        let title = table_cell(&link.title);
        let url = link
            .url
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
            .replace('|', "%7C");
        text.push_str(&format!("| [{title}](<{url}>) |\n"));
    }
    text.pop();
    text.push_str(&links[stream.byte_offset()..]);
    Some(Body::Sections(vec![Section::Markdown {
        label: "output".into(),
        text,
    }]))
}

/// Literal text inside a Markdown table cell.
fn table_cell(text: &str) -> String {
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
fn markdown_table(value: &Value, name: &str) -> Option<String> {
    let rows = value.as_array().filter(|rows| !rows.is_empty())?;
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

fn codex_thread_table(data: &ToolData<'_>) -> Option<Body> {
    let result = data.result?;
    if result.error {
        return None;
    }
    let threads: Vec<Value> = result.structured["threads"]
        .as_array()?
        .iter()
        .map(|thread| serde_json::json!({ "Name": thread["name"], "State": thread["state"] }))
        .collect();
    Some(Body::Sections(vec![Section::Markdown {
        label: "output".into(),
        text: markdown_table(&Value::Array(threads), "threads")?,
    }]))
}

/// The answer Claude sent, shown below the request it answers.
fn codex_reply_output(data: &ToolData<'_>) -> Option<Body> {
    if data.result.is_some_and(|result| result.error) {
        return None;
    }
    Some(Body::Sections(vec![Section::Prose {
        text: text(data.input, "reply")?.to_owned(),
        markdown: false,
    }]))
}

/// The requests still waiting for Claude, one row per thread.
fn codex_attention_section(data: &ToolData<'_>) -> Option<Section> {
    let result = data.result.filter(|result| !result.error)?;
    let entries: Vec<Value> = result.structured["attention"]
        .as_array()?
        .iter()
        .map(|entry| serde_json::json!({ "Attention": entry["text"], "Thread": entry["threadId"] }))
        .collect();
    Some(Section::Markdown {
        label: "attention".into(),
        text: markdown_table(&Value::Array(entries), "attention")?,
    })
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

fn patch_section(value: &serde_json::Value) -> Option<Section> {
    let hunks = value["structuredPatch"].as_array()?;
    let text = hunks
        .iter()
        .map(|hunk| {
            std::iter::once(format!(
                "@@ -{},{} +{},{} @@",
                hunk["oldStart"], hunk["oldLines"], hunk["newStart"], hunk["newLines"]
            ))
            .chain(
                hunk["lines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|line| line.as_str().map(str::to_owned)),
            )
            .collect::<Vec<_>>()
            .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(Section::Diff {
        lang: file_language(value["filePath"].as_str()?).to_owned(),
        label: String::new(),
        text,
    })
}

fn text_section(label: &str, text: &str) -> Section {
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
    let lines: Vec<String> = questions
        .iter()
        .filter_map(|question| {
            let answer = answers.get(question["question"].as_str()?)?;
            let header = text(question, "header").or_else(|| text(question, "question"))?;
            Some(format!("● {header} → {answer}"))
        })
        .collect();
    if !lines.is_empty() {
        blocks.push(lines.join("\n"));
    }
    blocks.join("\n\n")
}

/// Shows a rejected ask as `Rejected` with the user's note, or as `Chat about this` with
/// any answers its "Questions asked:" list reports; dismissed messages have no answers.
/// Other errors keep their text.
pub(crate) fn rejected_answers(questions: &Value, text: &str) -> Option<String> {
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    let dismissed = |reason: &str| {
        reason.starts_with("The user dismissed these questions because the message you sent with them was not shown.")
    };
    let said = if let Some(reason) = text.strip_prefix("PreToolUse:AskUserQuestion hook error: ") {
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
        return Some(format!(
            "Chat about this\n\n{}",
            answer_list(questions, &BTreeMap::new())
        ));
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
    Some(format!(
        "Chat about this\n\n{}",
        answer_list(questions, &answers)
    ))
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
fn question_sections(input: &Value) -> Vec<Section> {
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
    use serde_json::Value;
    #[test]
    fn captured_edit_patch_carries_file_language() {
        let record = include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["toolUseResult"]["structuredPatch"].is_array())
            .unwrap();
        let section = super::patch_section(&record["toolUseResult"]).unwrap();
        assert!(
            matches!(section, crate::event::Section::Diff { lang, label, .. } if lang == "rs" && label.is_empty())
        );
    }

    #[test]
    fn hook_rejected_questions_show_chat_and_answers() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-questions-hook-rejected.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let questions = &records[0]["message"]["content"][0]["input"]["questions"];
        let result = records[1]["message"]["content"][0]["content"]
            .as_str()
            .unwrap();
        assert_eq!(
            super::rejected_answers(questions, result).as_deref(),
            Some("Chat about this\n\nYou have not answered all questions")
        );
        assert_eq!(
            super::rejected_answers(questions, records[1]["toolUseResult"].as_str().unwrap()),
            super::rejected_answers(questions, result)
        );
        assert_eq!(
            super::rejected_answers(
                questions,
                &result.replace("(No answer provided)", "Answer: Red")
            )
            .as_deref(),
            Some("Chat about this\n\n● Color → Red")
        );
        assert_eq!(
            super::rejected_answers(
                questions,
                "PreToolUse:AskUserQuestion hook error: unrelated error"
            ),
            None
        );
    }

    #[test]
    fn recorded_native_rejected_questions_show_chat_and_answers() {
        for fixture in [
            include_str!("../../tests/fixtures/claude-questions-rejected.jsonl"),
            include_str!("../../tests/fixtures/claude-questions-hook-rejected.jsonl"),
        ] {
            let records: Vec<serde_json::Value> = fixture
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let questions = &records[0]["message"]["content"][0]["input"]["questions"];
            let header = questions[0]["header"].as_str().unwrap();
            for result in [
                records[1]["message"]["content"][0]["content"]
                    .as_str()
                    .unwrap(),
                records[1]["toolUseResult"].as_str().unwrap(),
            ] {
                // Keep the recorded four-line explanation and its actual question list.
                let (prefix, asked) = result
                    .split_once("The user wants to clarify these questions.")
                    .unwrap();
                let (_, list) = asked.split_once("Questions asked:").unwrap();
                let short = format!(
                    "{prefix}The user wants to clarify these questions. Start by asking them what they would like to clarify.\n\n    Questions asked:{list}"
                );
                for text in [result, short.as_str()] {
                    assert_eq!(
                        super::rejected_answers(questions, text).as_deref(),
                        Some("Chat about this\n\nYou have not answered all questions")
                    );
                    assert_eq!(
                        super::rejected_answers(
                            questions,
                            &text.replace("(No answer provided)", "Answer: Red")
                        ),
                        Some(format!("Chat about this\n\n● {header} → Red"))
                    );
                }
            }
        }
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
    fn chrome_javascript_formats_recorded_text() {
        let records: Vec<Value> =
            include_str!("../../tests/fixtures/claude-chrome-javascript.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = records[1]["message"]["content"][0]["content"].clone();
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: Value::Null,
            error: false,
        };
        for name in [
            "claude-in-chrome javascript_tool",
            "mcp__claude-in-chrome__javascript_tool",
            "mcp__plugin_claude-in-chrome_claude-in-chrome__javascript_tool",
            "claude-in-chrome.javascript_tool",
        ] {
            let shown = super::presentation(
                name,
                &super::ToolData {
                    input,
                    result: Some(&result),
                },
            );
            let [crate::event::Section::Code { label, lang, text }] = shown.input.as_slice() else {
                panic!("JavaScript must be a code section: {:?}", shown.input);
            };
            assert_eq!(label, "text");
            assert_eq!(lang, "js");
            assert!(text.lines().count() > 1, "{text}");
            assert!(text.contains(" => "), "{text}");
            assert!(text.contains("'resource'"), "{text}");
            assert!(text.contains("' '"), "{text}");
            let Some(super::Body::Sections(output)) = shown.output else {
                panic!("missing output")
            };
            assert_eq!(output, super::mcp_content(&content, None));
            assert_eq!(output.len(), 2);

            let mut formatted = input.clone();
            formatted["text"] = text.clone().into();
            assert_eq!(
                super::presentation(
                    name,
                    &super::ToolData {
                        input: &formatted,
                        result: None
                    }
                )
                .input,
                shown.input,
                "Already formatted JavaScript stays unchanged"
            );
        }
    }

    #[test]
    fn chrome_find_shows_only_its_query() {
        let input = serde_json::json!({
            "query": "Refresh link near the calendar/summary panel",
            "tabId": 665979853
        });
        let shown = super::presentation(
            "mcp__claude-in-chrome__find",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(
            shown.input,
            [crate::event::Section::Prose {
                text: "Refresh link near the calendar/summary panel".into(),
                markdown: false,
            }]
        );
    }

    #[test]
    fn chrome_computer_shows_its_action_and_summary() {
        for (input, shown) in [
            (
                serde_json::json!({
                    "action": "left_click",
                    "tabId": 665979853,
                    "ref": "ref_21",
                    "action_summary": "点搜索按钮执行航班搜索"
                }),
                "left_click  点搜索按钮执行航班搜索",
            ),
            (
                serde_json::json!({"action": "wait", "tabId": 665979853, "duration": 4}),
                "wait",
            ),
        ] {
            assert_eq!(
                super::presentation(
                    "mcp__claude-in-chrome__computer",
                    &super::ToolData {
                        input: &input,
                        result: None,
                    },
                )
                .input,
                [crate::event::Section::Prose {
                    text: shown.into(),
                    markdown: false,
                }]
            );
        }
    }

    #[test]
    fn chrome_javascript_invalid_text_stays_code_and_absent_text_uses_default_input() {
        let record: Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-chrome-javascript.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let mut input = record["message"]["content"][0]["input"].clone();
        let invalid = format!("{}(", input["text"].as_str().unwrap());
        input["text"] = invalid.clone().into();
        let shown = super::presentation(
            "mcp__claude-in-chrome__javascript_tool",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(shown.input[0], super::code_section("text", "js", &invalid));
        input.as_object_mut().unwrap().remove("text");
        assert_eq!(
            super::presentation(
                "mcp__claude-in-chrome__javascript_tool",
                &super::ToolData {
                    input: &input,
                    result: None
                }
            )
            .input,
            super::raw_input(&input)
        );
    }

    #[test]
    fn claude_client_text_is_markdown_and_follows_the_list_tool_name() {
        for source in [
            include_str!("../../tests/fixtures/claude-client/ask_claude.jsonl"),
            include_str!("../../tests/fixtures/claude-client/notify_claude.jsonl"),
        ] {
            let record: serde_json::Value = serde_json::from_str(source).unwrap();
            let item = &record["payload"]["item"];
            let input = &item["arguments"];
            let text = input["text"].as_str().unwrap();
            let tool = item["tool"].as_str().unwrap();
            for name in [
                format!("claude-client {tool}"),
                format!("mcp__claude-client__{tool}"),
                format!("mcp__plugin_claude_claude-client__{tool}"),
            ] {
                let super::Presentation {
                    title: subject,
                    input: sections,
                    ..
                } = super::presentation(
                    &name,
                    &super::ToolData {
                        input,
                        result: None,
                    },
                );
                assert_eq!(
                    sections,
                    vec![crate::event::Section::Markdown {
                        label: "text".into(),
                        text: text.into(),
                    }]
                );
                assert_eq!(
                    super::list_title(&name, subject.as_deref(), input, None),
                    format!("{} {text}", super::display_tool_name(&name))
                );
            }
        }
    }

    #[test]
    fn tool_detail_titles_center_the_tool_name_without_a_subject() {
        for source in [
            include_str!("../../tests/fixtures/claude-client/ask_claude.jsonl"),
            include_str!("../../tests/fixtures/claude-client/notify_claude.jsonl"),
            include_str!("../../tests/fixtures/tool-list/claude.jsonl"),
        ] {
            let record: serde_json::Value =
                serde_json::from_str(source.lines().next().unwrap()).unwrap();
            let name = record["payload"]["item"]["tool"]
                .as_str()
                .or_else(|| record["message"]["content"][0]["name"].as_str())
                .unwrap();
            let mut call = crate::event::ToolCall {
                model_settings: None,
                agent_message: None,
                started_at: None,
                hide_output: false,
                agent_thread_id: None,
                agent_path: None,
                title: String::new(),
                show_event_detail: true,
                id: String::new(),
                summary: String::new(),
                name: String::new(),
                input: Vec::new(),
                exit_code: None,
                output: None,
                is_error: false,
                duration: None,
            };
            call.name = name.into();
            let display = super::display_tool_name(name);
            for title in ["", " \t "] {
                call.title = title.into();
                assert_eq!(call.detail_left(), "");
            }
            call.title = "Inspect source".into();
            assert_eq!(call.detail_left(), display);
        }
    }

    #[test]
    fn agent_and_task_share_the_spawn_rule() {
        assert!(std::ptr::eq(super::rule("Agent"), super::rule("Task")));
        for name in ["Agent", "Task", "spawn_agent"] {
            let rule = super::rule(name);
            assert!(rule.title.is_empty());
            assert!(rule.input.is_none());
        }
    }

    #[test]
    fn tool_declarations_are_unique_and_keep_captured_agent_fields() {
        let mut names = std::collections::HashSet::new();
        for rule in super::TOOL_RULES {
            assert!(
                names.insert(rule.name),
                "duplicate declaration: {}",
                rule.name
            );
        }
        for line in include_str!("../../tests/fixtures/codex-agent-names.jsonl").lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            let payload = &record["payload"];
            let name = payload["name"].as_str().unwrap();
            let input: serde_json::Value =
                serde_json::from_str(payload["arguments"].as_str().unwrap()).unwrap();
            let subject = input[if name == "spawn_agent" {
                "task_name"
            } else {
                "target"
            }]
            .as_str()
            .unwrap();
            for raw in [
                name.to_owned(),
                format!("collaboration.{name}"),
                format!("functions.collaboration.{name}"),
            ] {
                if name == "spawn_agent" {
                    let message = crate::event::agent::tool_message(&raw, &input).unwrap();
                    assert_eq!(message.recipient.as_deref(), Some(subject));
                    assert_eq!(message.content.text(), input["message"].as_str().unwrap());
                    continue;
                }
                let super::Presentation {
                    title: actual,
                    input: sections,
                    ..
                } = super::presentation(
                    &raw,
                    &super::ToolData {
                        input: &input,
                        result: None,
                    },
                );
                assert_eq!(actual.as_deref(), Some(subject));
                assert_eq!(
                    sections,
                    vec![crate::event::Section::Markdown {
                        label: "message".into(),
                        text: input["message"].as_str().unwrap().to_owned(),
                    }]
                );
            }
        }
    }

    #[test]
    fn captured_wait_name_keeps_a_capitalized_tool_label() {
        let record: serde_json::Value =
            include_str!("../../tests/fixtures/codex-perspective/beta.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|record| record["payload"]["name"] == "wait")
                .unwrap();
        let name = record["payload"]["name"].as_str().unwrap();
        assert_eq!(super::display_tool_name(name), "Wait");
    }

    #[test]
    fn captured_mcp_names_preserve_server_and_tool_case() {
        let record: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/mcp-codex/image.json"))
                .unwrap();
        let item = &record["payload"]["item"];
        let server = item["server"].as_str().unwrap();
        let tool = item["tool"].as_str().unwrap();
        let raw = format!("mcp__{server}__{tool}");
        assert_eq!(
            super::display_tool_name(&raw),
            format!("{server} {}", tool.replace('_', " "))
        );
        assert_eq!(
            super::display_tool_name("mcp__claude-client__ask_claude"),
            "claude-client ask claude"
        );
    }

    #[test]
    fn captured_tool_names_share_one_display_rule() {
        let record: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/codex-agent-names.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let name = record["payload"]["name"].as_str().unwrap();
        assert_eq!(super::display_tool_name(name), "Send message");
        assert_eq!(
            super::display_tool_name(&format!("collaboration.{name}")),
            "Send message"
        );
        for line in include_str!("../../tests/fixtures/claude-tool-calls.jsonl").lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some(name @ ("WebFetch" | "Bash" | "Read" | "Skill")) =
                record["message"]["content"][0]["name"].as_str()
            {
                assert_eq!(super::display_tool_name(name), name);
            }
        }
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
    fn codex_manager_list_shows_a_thread_table() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-list.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let name = records[0]["message"]["content"][0]["name"]
            .as_str()
            .unwrap();
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let super::Body::Sections(output) = super::output(
            super::rule(name).output,
            &super::ToolData {
                input: &serde_json::Value::Null,
                result: Some(&result),
            },
        ) else {
            panic!("expected visible body")
        };
        let [crate::event::Section::Markdown { text, .. }] = output.as_slice() else {
            panic!("expected one Markdown table, got {output:?}");
        };
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows[..2], ["| Name | State |", "| --- | --- |"]);
        assert_eq!(rows[2], r"| 规则\:拆逗号 | completed |");
        assert_eq!(rows.last().unwrap(), &"| 框架补三处 | inProgress |");
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn codex_manager_reply_shows_ask_reply_and_attention_by_thread() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-reply.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let presentation = super::presentation(
            call["name"].as_str().unwrap(),
            &super::ToolData {
                input: &call["input"],
                result: Some(&result),
            },
        );
        assert_eq!(
            presentation.title.as_deref(),
            call["input"]["threadId"].as_str()
        );
        let [crate::event::Section::Prose { text: ask, .. }] = presentation.input.as_slice() else {
            panic!("expected the ask, got {:?}", presentation.input);
        };
        assert_eq!(ask, call["input"]["ask"].as_str().unwrap());
        let Some(super::Body::Sections(output)) = presentation.output else {
            panic!("expected visible body")
        };
        let [
            crate::event::Section::Prose { text: reply, .. },
            crate::event::Section::Markdown { text, .. },
        ] = output.as_slice()
        else {
            panic!("expected the reply and the attention section, got {output:?}");
        };
        assert_eq!(reply, call["input"]["reply"].as_str().unwrap());
        let attention = result.structured["attention"].as_array().unwrap();
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows[..2], ["| Attention | Thread |", "| --- | --- |"]);
        assert_eq!(rows.len(), 2 + attention.len());
        for (row, entry) in rows[2..].iter().zip(attention) {
            assert_eq!(
                *row,
                format!(
                    "| {} | {} |",
                    super::table_cell(entry["text"].as_str().unwrap()),
                    super::table_cell(entry["threadId"].as_str().unwrap())
                )
            );
        }
    }

    #[test]
    fn codex_manager_list_input_shows_its_states_as_a_table() {
        let input = serde_json::json!({ "state": ["waiting", "inProgress"] });
        let presentation = super::presentation(
            "mcp__plugin_codex_codex-manager__list",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        let [crate::event::Section::Markdown { text, .. }] = presentation.input.as_slice() else {
            panic!("expected one Markdown table, got {:?}", presentation.input);
        };
        assert_eq!(text, "| state |\n| --- |\n| waiting |\n| inProgress |");
    }

    #[test]
    fn codex_manager_start_shows_only_the_thread_id() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-start-result.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let call = &records[0]["message"]["content"][0];
        let content = &records[1]["message"]["content"][0]["content"];
        let result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        let presentation = super::presentation(
            call["name"].as_str().unwrap(),
            &super::ToolData {
                input: &call["input"],
                result: Some(&result),
            },
        );
        let Some(super::Body::Sections(output)) = presentation.output else {
            panic!("expected visible body")
        };
        let [crate::event::Section::Text { text, .. }] = output.as_slice() else {
            panic!("expected only the thread id, got {output:?}");
        };
        assert_eq!(text, result.structured["threadId"].as_str().unwrap());
    }

    #[test]
    fn send_user_file_input_shows_only_the_caption() {
        let input = serde_json::json!({
            "caption": "放大的那几列",
            "files": ["/tmp/crop.png"],
            "status": "normal",
        });
        let super::Presentation {
            title: subject,
            input: sections,
            ..
        } = super::presentation(
            "SendUserFile",
            &super::ToolData {
                input: &input,
                result: None,
            },
        );
        assert_eq!(subject, None);
        assert!(matches!(
            sections.as_slice(),
            [crate::event::Section::Prose { text, .. }] if text == "放大的那几列"
        ));
    }

    #[test]
    fn result_title_follows_the_declared_fields() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-detach-name.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = &records[1]["message"]["content"][0]["content"];
        let mut result = super::ToolResult {
            source: None,
            content: content.clone(),
            structured: serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap(),
            error: false,
        };
        assert_eq!(
            super::presentation(
                "mcp__codex-manager__detach",
                &super::ToolData {
                    input,
                    result: Some(&result)
                }
            )
            .title
            .as_deref(),
            Some("模型和new挪位")
        );
        assert_eq!(
            super::presentation(
                "mcp__codex-manager__detach",
                &super::ToolData {
                    input,
                    result: None
                }
            )
            .title
            .as_deref(),
            input["threadId"].as_str()
        );
        for name in [
            serde_json::Value::Null,
            serde_json::json!(" "),
            serde_json::json!(42),
        ] {
            result.structured["name"] = name;
            assert_eq!(
                super::presentation(
                    "mcp__codex-manager__detach",
                    &super::ToolData {
                        input,
                        result: Some(&result)
                    }
                )
                .title
                .as_deref(),
                input["threadId"].as_str()
            );
        }
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

    #[test]
    fn mcp_presentation_uses_shared_tables_and_removes_title_fields() {
        let record: serde_json::Value =
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .find(|record: &serde_json::Value| {
                    record["message"]["content"][0]["name"]
                        == "mcp__plugin_codex_codex-manager__start"
                })
                .unwrap();
        let call = &record["message"]["content"][0];
        for name in [call["name"].as_str().unwrap(), "codex-manager.start"] {
            let super::Presentation {
                title: subject,
                input,
                ..
            } = super::presentation(
                name,
                &super::ToolData {
                    input: &call["input"],
                    result: None,
                },
            );
            let name = super::display_tool_name(name);
            assert_eq!(name, "codex-manager start");
            assert_eq!(subject.as_deref(), Some("tool-calls"));
            assert_eq!(
                input,
                vec![crate::event::Section::Markdown {
                    label: "prompt".into(),
                    text: call["input"]["prompt"].as_str().unwrap().into(),
                }]
            );
        }
    }

    #[test]
    fn mcp_titles_show_server_and_tool() {
        for (name, expected) in [
            (
                "mcp__plugin_codex_codex-manager__send",
                "codex-manager send",
            ),
            (
                "mcp__plugin_codex_codex-manager__start",
                "codex-manager start",
            ),
            (
                "mcp__claude-in-chrome__navigate",
                "claude-in-chrome navigate",
            ),
            ("mcp__context7__query-docs", "context7 query-docs"),
            ("mcp__tool", "mcp__tool"),
            ("mcp____send", "mcp____send"),
            ("mcp__server__", "mcp__server__"),
            ("mcp__plugin_codex__send", "mcp__plugin_codex__send"),
            ("mcp__plugin__server__send", "mcp__plugin__server__send"),
            ("mcp__plugin_codex___send", "mcp__plugin_codex___send"),
        ] {
            assert_eq!(super::display_tool_name(name), expected);
        }
    }
}
