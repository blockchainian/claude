use chrono::{DateTime, Utc};
use serde_json::Value;

pub(crate) fn tool_kind(name: &str) -> Kind {
    match name.rsplit('.').next() {
        Some("AskUserQuestion" | "request_user_input") => Kind::Assistant,
        _ => Kind::Tool,
    }
}

pub(crate) fn tool_title(name: &str, input: &Value) -> String {
    let text = |key: &str| input[key].as_str().filter(|text| !text.trim().is_empty());
    let tool = name.rsplit('.').next().unwrap_or(name);
    match tool {
        "Agent" | "Task" | "spawn_agent" => {
            let agent = text("subagent_type")
                .map(|kind| format!("{kind} agent"))
                .unwrap_or_else(|| "Agent".to_owned());
            text("description")
                .map(|description| format!("{agent} {description}"))
                .unwrap_or(agent)
        }
        "Skill" => text("skill").unwrap_or(name).to_owned(),
        "SendMessage" | "send_message" | "followup_task" => text("to")
            .or_else(|| text("target"))
            .map(|recipient| format!("To {recipient}"))
            .unwrap_or_else(|| name.to_owned()),
        _ if name.starts_with("mcp__") => name.to_owned(),
        _ if text("description").is_some() => text("description").unwrap().to_owned(),
        "Bash" | "exec" | "exec_command" | "shell" => {
            let command = text("command").or_else(|| text("cmd")).unwrap_or("");
            let summary = crate::shell_summary::summarize_shell_commands(command);
            if summary.is_empty() {
                name.to_owned()
            } else {
                summary
            }
        }
        _ => name.to_owned(),
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

pub(crate) fn participant_text(name: Option<&str>, body: &str) -> String {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if body.is_empty() => format!("({name})"),
        Some(name) => format!("({name}) {body}"),
        None => body.to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    User,
    Assistant,
    Agent,
    Session,
    Tool,
    Error,
    Denied,
    QueuedUser,
    QueuedSystem,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub body: String,
    /// Byte ranges of the file paths inside `body`, in order.
    pub paths: Vec<std::ops::Range<usize>>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Known execution time; absent when a completed activity has no recorded start.
    pub duration: Option<chrono::Duration>,
    /// The preceding event's end, including hidden turn starters for Assistant replies.
    pub previous_end: Option<DateTime<Utc>>,
    /// Images attached to this prompt whose files still exist.
    pub images: Vec<Image>,
    /// The tool calls a Tool, Error or Denied row stands for, in the order they ran.
    pub calls: Vec<ToolCall>,
}

/// One tool call of a Tool, Error or Denied row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// The provider's stable call identifier.
    pub id: String,
    pub title: String,
    /// The provider's tool name, such as "Bash", "exec_command" or "apply_patch".
    pub name: String,
    pub input: Vec<Section>,
    /// `None` while the call is running.
    pub output: Option<Vec<Section>>,
    pub is_error: bool,
}

/// One labelled part of a tool call's input or output, as the open log event draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// `key: value` lines, in order.
    Fields(Vec<(String, String)>),
    /// Code highlighted by `lang`, a syntax token or file extension such as "bash" or "rs".
    Code {
        label: String,
        lang: String,
        text: String,
    },
    /// Unified diff hunks; the label is the changed file's path.
    Diff { label: String, text: String },
    /// Prose rendered as Markdown.
    Markdown { label: String, text: String },
    /// Plain text wrapped at the panel width.
    Text { label: String, text: String },
}

/// An image attached to a prompt: its placeholder in the prompt text, such as
/// `[Image #1]`, and the file it was attached from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub label: String,
    pub path: std::path::PathBuf,
}
