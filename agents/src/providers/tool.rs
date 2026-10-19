pub(crate) mod groups;
pub(crate) mod shell_summary;

use crate::log::{Section, ToolCall};
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
pub(crate) fn mcp_content(content: &Value) -> Vec<Section> {
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
        .map(|block| {
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

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Subject {
    Field(&'static str),
    ResultField {
        result: &'static str,
        input: &'static str,
    },
    QuestionHeaders,
    Recipient,
    Shell {
        description: bool,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum Input {
    Field(&'static str),
    QuestionList,
    Arguments,
    FetchPrompt,
    Shell,
}

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Text,
    Markdown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Output {
    Read,
    Write,
    Patch,
    Shell,
    Matches,
    Questions,
    Markdown,
    Fetch,
    Empty,
    Hidden,
    Agent,
    Wait,
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
    pub subject: Option<Subject>,
    pub input: Option<(Input, Format)>,
    pub output: Option<Output>,
    pub question: bool,
    pub hidden: bool,
    pub wrapper: bool,
    pub task: Option<TaskAction>,
    pub list_subject: bool,
    pub list_input: bool,
}

const DEFAULT_RULE: ToolRule = ToolRule {
    name: "",
    aliases: &[],
    subject: None,
    input: None,
    output: None,
    question: false,
    hidden: false,
    wrapper: false,
    task: None,
    list_subject: false,
    list_input: false,
};

pub(crate) const TOOL_RULES: &[ToolRule] = &[
    ToolRule {
        name: "Read",
        subject: Some(Subject::Field("file_path")),
        input: Some((Input::Field(""), Format::Text)),
        output: Some(Output::Read),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Write",
        subject: Some(Subject::Field("file_path")),
        input: Some((Input::Field("content"), Format::Text)),
        output: Some(Output::Write),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Edit",
        subject: Some(Subject::Field("file_path")),
        input: Some((Input::Field(""), Format::Text)),
        output: Some(Output::Patch),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Bash",
        subject: Some(Subject::Shell { description: true }),
        input: Some((Input::Shell, Format::Text)),
        output: Some(Output::Shell),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "exec_command",
        subject: Some(Subject::Shell { description: false }),
        input: Some((Input::Shell, Format::Text)),
        output: Some(Output::Shell),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "shell",
        subject: Some(Subject::Shell { description: false }),
        input: Some((Input::Shell, Format::Text)),
        output: Some(Output::Shell),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "CommandExecution",
        subject: Some(Subject::Shell { description: false }),
        input: Some((Input::Shell, Format::Text)),
        output: Some(Output::Shell),
        list_subject: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "ToolSearch",
        subject: Some(Subject::Field("query")),
        input: Some((Input::Field(""), Format::Text)),
        output: Some(Output::Matches),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "AskUserQuestion",
        subject: Some(Subject::QuestionHeaders),
        input: Some((Input::QuestionList, Format::Text)),
        output: Some(Output::Questions),
        question: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "request_user_input",
        subject: Some(Subject::QuestionHeaders),
        input: Some((Input::QuestionList, Format::Text)),
        output: Some(Output::Questions),
        question: true,
        ..DEFAULT_RULE
    },
    // Message owns spawn names and prompt bodies. Other input fields, including
    // subagent_type, run_in_background and isolation, are intentionally not displayed.
    ToolRule {
        name: "Agent",
        aliases: &["Task"],
        output: Some(Output::Agent),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "spawn_agent",
        output: Some(Output::Hidden),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "SendMessage",
        subject: Some(Subject::Recipient),
        input: Some((Input::Field("message"), Format::Markdown)),
        output: Some(Output::Markdown),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "send_message",
        subject: Some(Subject::Field("target")),
        input: Some((Input::Field("message"), Format::Markdown)),
        output: Some(Output::Empty),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "followup_task",
        subject: Some(Subject::Field("target")),
        input: Some((Input::Field("message"), Format::Markdown)),
        output: Some(Output::Empty),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "wait_agent",
        input: Some((Input::Field("timeout_ms"), Format::Text)),
        output: Some(Output::Wait),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "WebFetch",
        subject: Some(Subject::Field("url")),
        input: Some((Input::FetchPrompt, Format::Text)),
        output: Some(Output::Fetch),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "Skill",
        subject: Some(Subject::Field("skill")),
        input: Some((Input::Arguments, Format::Text)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "apply_patch",
        subject: Some(Subject::Field("paths")),
        input: Some((Input::Field(""), Format::Text)),
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
        name: "claude-client ask_claude",
        input: Some((Input::Field("text"), Format::Markdown)),
        list_input: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "claude-client notify_claude",
        input: Some((Input::Field("text"), Format::Markdown)),
        list_input: true,
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager start",
        subject: Some(Subject::Field("name")),
        input: Some((Input::Field("prompt"), Format::Markdown)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager send",
        subject: Some(Subject::Field("threadId")),
        input: Some((Input::Field("prompt"), Format::Markdown)),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager interrupt",
        subject: Some(Subject::Field("threadId")),
        ..DEFAULT_RULE
    },
    ToolRule {
        name: "codex-manager detach",
        subject: Some(Subject::ResultField {
            result: "name",
            input: "threadId",
        }),
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

pub(crate) fn tool_kind(name: &str) -> crate::log::Kind {
    if rule(name).question {
        crate::log::Kind::Assistant
    } else {
        crate::log::Kind::Tool
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
        Some((Input::Field(field), _)) => text(input, field),
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

pub(crate) fn detail_titles(name: &str, subject: Option<&str>) -> (String, String) {
    let display = display_tool_name(name);
    match subject.filter(|subject| !subject.trim().is_empty()) {
        Some(subject) => (display, subject.to_owned()),
        None => (String::new(), display),
    }
}

/// Read a declared subject from JSON output, leaving the input subject as fallback.
pub(crate) fn result_subject(name: &str, output: Option<&[Section]>) -> Option<String> {
    let Subject::ResultField { result, .. } = rule(name).subject? else {
        return None;
    };
    output?.iter().find_map(|section| {
        let source = match section {
            Section::Text { text, .. } | Section::Structured { text, .. } => text,
            _ => return None,
        };
        let value: Value = serde_json::from_str(source).ok()?;
        text(&value, result).map(str::to_owned)
    })
}

fn shell_command(input: &Value) -> Option<&str> {
    text(input, "command").or_else(|| text(input, "cmd"))
}

pub(crate) fn presentation(name: &str, input: &Value) -> (String, Option<String>, Vec<Section>) {
    let raw_name = name;
    let name = display_tool_name(name);
    let mut remaining = input.clone();
    let subject = match rule(raw_name).subject {
        Some(Subject::Field(field) | Subject::ResultField { input: field, .. }) => remaining
            .as_object_mut()
            .and_then(|object| object.remove(field))
            .and_then(|value| {
                value
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
            }),
        Some(Subject::Shell { description }) => description
            .then(|| text(input, "description"))
            .flatten()
            .map(str::to_owned)
            .or_else(|| {
                let summary = shell_summary::summarize_shell_commands(shell_command(input)?);
                (!summary.is_empty()).then_some(summary)
            }),
        Some(Subject::Recipient) => {
            let to = recipient(input).map(str::to_owned);
            if let Some(object) = remaining.as_object_mut() {
                object.remove("to");
            }
            to
        }
        Some(Subject::QuestionHeaders) => Some(
            input["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|question| text(question, "header"))
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .filter(|headers| !headers.is_empty()),
        None => None,
    };
    let input_rule = rule(raw_name).input;
    let sections = match input_rule {
        Some((Input::Field(""), _)) => Vec::new(),
        Some((method, format)) => {
            let (body, mut sections) = match method {
                Input::Shell => (shell_command(input).unwrap_or("").to_owned(), Vec::new()),
                Input::Field(field) => (
                    remaining
                        .get(field)
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_default(),
                    Vec::new(),
                ),
                Input::QuestionList => (String::new(), question_sections(input)),
                Input::Arguments => (input["args"].as_str().unwrap_or("").to_owned(), Vec::new()),
                Input::FetchPrompt => (
                    text(input, "prompt").unwrap_or("").to_owned(),
                    text(input, "url")
                        .map(|url| {
                            vec![Section::Text {
                                label: String::new(),
                                text: format!("url: {url}"),
                            }]
                        })
                        .unwrap_or_default(),
                ),
            };
            if !body.trim().is_empty() {
                let section = match format {
                    _ if matches!(method, Input::Shell) => Section::Shell {
                        text: body,
                        prefix: 0,
                    },
                    Format::Text if matches!(method, Input::Arguments) => Section::Text {
                        label: "args".into(),
                        text: body,
                    },
                    Format::Text => Section::Prose {
                        text: body,
                        markdown: false,
                    },
                    Format::Markdown => Section::Markdown {
                        label: match method {
                            Input::Field(field) => field,
                            _ => "",
                        }
                        .into(),
                        text: body,
                    },
                };
                sections.push(section);
            }
            sections
        }
        None => json_input(&remaining),
    };
    let sections = if matches!(rule(raw_name).output, Some(Output::Write)) {
        match (text(input, "file_path"), input["content"].as_str()) {
            (Some(path), Some(content)) => vec![file_code_section("input", path, content)],
            _ => sections,
        }
    } else {
        sections
    };
    (name, subject, sections)
}

pub(crate) struct ToolResult<'a> {
    pub content: &'a Value,
    pub value: &'a Value,
    pub text: &'a str,
    pub error: bool,
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

pub(crate) fn result_kind(kind: crate::log::Kind, state: &ResultState) -> crate::log::Kind {
    if state.denied {
        crate::log::Kind::Denied
    } else if state.is_error {
        crate::log::Kind::Error
    } else {
        kind
    }
}

pub(crate) fn text_output(text: &str, mode: Option<Output>, input: &Value) -> Vec<Section> {
    let value = serde_json::from_str(text).unwrap_or(Value::Null);
    let content = if value["content"].is_array() {
        value["content"].clone()
    } else {
        Value::String(text.to_owned())
    };
    let state = result_state(text, &Value::Null, value["isError"] == true);
    output(
        mode,
        input,
        &ToolResult {
            content: &content,
            value: &value,
            text,
            error: state.is_error,
        },
    )
}

/// Only recorded launch states suppress an agent's result body.
pub(crate) fn output_hidden(mode: Option<Output>, value: &Value) -> bool {
    match mode {
        Some(Output::Hidden) => true,
        Some(Output::Agent) => value["isAsync"] == true
            || matches!(value["status"].as_str(), Some("async_launched" | "teammate_spawned")),
        _ => false,
    }
}

pub(crate) fn output(mode: Option<Output>, input: &Value, result: &ToolResult<'_>) -> Vec<Section> {
    match mode {
        _ if !result.error && output_hidden(mode, result.value) => return Vec::new(),
        Some(Output::Empty) if !result.error => return Vec::new(),
        Some(Output::Wait) if !result.error => {
            let value = serde_json::from_str::<Value>(result.text).unwrap_or(Value::Null);
            let text = if value["timed_out"] == true {
                "(timed out)"
            } else {
                value["message"].as_str().unwrap_or(result.text)
            };
            return mcp_content(&Value::String(text.to_owned()));
        }
        _ => {}
    }

    let value = result.value;
    let generic = || {
        let mut content = result.content.clone();
        if let Some(text) = content
            .as_str()
            .filter(|text| text.starts_with("Exit code "))
        {
            content = text.split_once('\n').map_or("", |(_, body)| body).into();
        }
        mcp_content(&content)
    };
    if let Some(Output::Questions) = mode {
        let listed = if result.error {
            rejected_answers(&input["questions"], result.text)
                .or_else(|| rejected_questions(result.text))
        } else {
            serde_json::from_value(value["answers"].clone())
                .ok()
                .map(|answers| answer_list(&value["questions"], &answers))
        };
        if let Some(output) = listed {
            return vec![text_section("output", &output)];
        }
    }
    if result.error {
        return generic();
    }
    match mode {
        Some(Output::Markdown | Output::Agent) => vec![Section::Markdown {
            label: "output".to_owned(),
            text: result.text.to_owned(),
        }],
        Some(Output::Fetch) if value["result"].is_string() => vec![Section::Markdown {
            label: "output".to_owned(),
            text: value["result"].as_str().unwrap().to_owned(),
        }],
        Some(Output::Read) if value["type"] == "image" => {
            vec![Section::Placeholder(format!(
                "[image {}]",
                value["file"]["type"].as_str().unwrap_or("unknown")
            ))]
        }
        Some(Output::Read) if value["file"]["content"].is_string() => {
            vec![file_code_section(
                "output",
                value["file"]["filePath"].as_str().unwrap_or(""),
                value["file"]["content"].as_str().unwrap(),
            )]
        }
        Some(Output::Patch) => patch_section(value)
            .map(|section| vec![section])
            .unwrap_or_else(generic),
        Some(Output::Write) if value["type"] == "update" => patch_section(value)
            .map(|section| vec![section])
            .unwrap_or_else(generic),
        Some(Output::Shell) if value.get("stdout").is_some() || value.get("stderr").is_some() => {
            ["stdout", "stderr"]
                .into_iter()
                .filter_map(|key| {
                    value[key]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .map(|text| text_section(key, text))
                })
                .collect()
        }
        Some(Output::Matches) if value["matches"].is_array() => {
            vec![text_section(
                "output",
                &value["matches"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )]
        }
        _ => generic(),
    }
}

pub(crate) fn code_section(label: &str, lang: &str, text: &str) -> Section {
    Section::Code {
        label: label.to_owned(),
        lang: lang.to_owned(),
        text: text.to_owned(),
    }
}

pub(crate) fn file_code_section(label: &str, path: &str, text: &str) -> Section {
    let path = std::path::Path::new(path);
    let lang = path
        .extension()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");
    code_section(label, lang, text)
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
/// the answers its "Questions asked:" list reports; other errors keep their text.
pub(crate) fn rejected_answers(questions: &Value, text: &str) -> Option<String> {
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    let reason = text.strip_prefix("The user doesn't want to proceed with this tool use.")?;
    let Some((_, said)) = reason.split_once("the user said:\n") else {
        return Some("Rejected".to_owned());
    };
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

/// Add explicit headings for separately reported stderr output.
pub(super) fn annotate(call: &mut ToolCall) {
    if !call.is_error {
        match rule(&call.name).output {
            Some(Output::Hidden) => call.hide_output = true,
            Some(Output::Empty) => call.output = Some(Vec::new()),
            _ => {}
        }
    }
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
                let (_, subject, sections) = super::presentation(&name, input);
                assert_eq!(
                    sections,
                    vec![crate::log::Section::Markdown {
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
            let display = super::display_tool_name(name);
            for subject in [None, Some(""), Some(" \t ")] {
                assert_eq!(
                    super::detail_titles(name, subject),
                    (String::new(), display.clone())
                );
            }
            assert_eq!(
                super::detail_titles(name, Some("Inspect source")),
                (display, "Inspect source".into())
            );
        }
    }

    #[test]
    fn agent_and_task_share_the_spawn_rule() {
        assert!(std::ptr::eq(super::rule("Agent"), super::rule("Task")));
        for name in ["Agent", "Task", "spawn_agent"] {
            let rule = super::rule(name);
            assert!(rule.subject.is_none());
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
                    let message = crate::log::agent::tool_message(&raw, &input).unwrap();
                    assert_eq!(message.recipient.as_deref(), Some(subject));
                    assert_eq!(message.content.text(), input["message"].as_str().unwrap());
                    continue;
                }
                let (_, actual, sections) = super::presentation(&raw, &input);
                assert_eq!(actual.as_deref(), Some(subject));
                assert_eq!(
                    sections,
                    vec![crate::log::Section::Markdown {
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
        assert!(super::mcp_content(&content).is_empty());
    }

    #[test]
    fn result_subject_follows_the_declared_fields() {
        let records: Vec<serde_json::Value> =
            include_str!("../../tests/fixtures/claude-manager-detach-name.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let input = &records[0]["message"]["content"][0]["input"];
        let content = &records[1]["message"]["content"][0]["content"];
        let output = super::mcp_content(content);
        assert_eq!(
            super::result_subject("mcp__codex-manager__detach", Some(&output)).as_deref(),
            Some("模型和new挪位")
        );
        assert_eq!(
            super::presentation("mcp__codex-manager__detach", input)
                .1
                .as_deref(),
            input["threadId"].as_str()
        );
        let mut value: serde_json::Value =
            serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        for name in [
            serde_json::Value::Null,
            serde_json::json!(" "),
            serde_json::json!(42),
        ] {
            value["name"] = name;
            let output = vec![crate::log::Section::Structured {
                label: "output".into(),
                text: value.to_string(),
            }];
            assert_eq!(
                super::result_subject("mcp__codex-manager__detach", Some(&output)),
                None
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
        for kind in ["image", "audio", "resource_link", "future_content"] {
            let mut block = image.clone();
            block["type"] = kind.into();
            assert_eq!(
                super::mcp_content(&serde_json::Value::Array(vec![block])),
                vec![crate::log::Section::Placeholder(format!(
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
            let (name, subject, input) = super::presentation(name, &call["input"]);
            assert_eq!(name, "codex-manager start");
            assert_eq!(subject.as_deref(), Some("tool-calls"));
            assert_eq!(
                input,
                vec![crate::log::Section::Markdown {
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
