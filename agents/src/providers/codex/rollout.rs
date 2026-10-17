use crate::log::{Event, Image, Kind, Section, ToolCall, participant_text};
use crate::model::{Activity, SessionTask, TaskStatus};
use crate::providers::tool::question_texts;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn tool_kind(name: &str) -> Kind {
    match name.rsplit('.').next() {
        Some("AskUserQuestion" | "request_user_input") => Kind::Assistant,
        _ => Kind::Tool,
    }
}

fn tool_title(name: &str, input: &Value) -> String {
    use crate::providers::tool::Title;
    let kind = match name.rsplit('.').next().unwrap_or(name) {
        "AskUserQuestion" => Title::Questions,
        "Agent" | "Task" | "spawn_agent" => Title::Agent,
        "Skill" => Title::Skill,
        "SendMessage" | "send_message" | "followup_task" => Title::Recipient,
        "Bash" | "exec" | "exec_command" | "shell" => Title::Shell,
        _ => Title::Tool,
    };
    crate::providers::tool::title(name, input, kind)
}

const CHUNK: u64 = 64 * 1024;

#[derive(Default)]
pub(super) struct Summary {
    pub prompt_at: Option<DateTime<Utc>>,
    pub detail_sources: [Option<String>; 3],
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub updated_at: Option<DateTime<Utc>>,
    pub turn_ended: bool,
    pub pending_questions: Vec<crate::ask::Question>,
    pub tasks: Vec<SessionTask>,
}

/// Retains the last complete plan while reading only appended transcript lines.
#[derive(Default)]
pub(super) struct Tasks {
    offset: u64,
    identity: Option<(u64, u64)>,
    tasks: Vec<SessionTask>,
}

fn questions(input: &Value) -> io::Result<Vec<crate::ask::Question>> {
    #[derive(serde::Deserialize)]
    struct Input {
        questions: Vec<CodexQuestion>,
    }
    #[derive(serde::Deserialize)]
    struct CodexQuestion {
        id: String,
        header: String,
        question: String,
        options: Vec<crate::ask::QuestionOption>,
    }
    let input: Input = serde_json::from_value(input.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(input
        .questions
        .into_iter()
        .map(|question| crate::ask::Question {
            id: question.id,
            header: question.header,
            text: question.question,
            options: question.options,
            multi: false,
            other: false,
            secret: false,
        })
        .collect())
}

impl Tasks {
    pub(super) fn summary(&mut self, path: &Path) -> io::Result<Summary> {
        let mut result = summary(path)?;
        result.tasks = self.read(path)?;
        Ok(result)
    }

    fn read(&mut self, path: &Path) -> io::Result<Vec<SessionTask>> {
        use std::os::unix::fs::MetadataExt;
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let history = history_start(&mut file)?;
        let identity = (metadata.dev(), metadata.ino());
        if self.identity != Some(identity) || metadata.len() < self.offset {
            self.offset = 0;
            self.tasks.clear();
        }
        self.identity = Some(identity);
        let mut error = None;
        let offset = crate::jsonl::read_lines(&mut file, self.offset, |line| {
            if error.is_some() {
                return;
            }
            let Ok(record) = serde_json::from_slice::<Value>(line) else {
                return;
            };
            if inherited(&record, history) {
                return;
            }
            let payload = &record["payload"];
            if record["type"] != "response_item"
                || payload["type"] != "function_call"
                || payload["name"]
                    .as_str()
                    .and_then(|name| name.rsplit('.').next())
                    != Some("update_plan")
            {
                return;
            }
            match plan_tasks(payload) {
                Ok(tasks) => self.tasks = tasks,
                Err(invalid) => error = Some(invalid),
            }
        })?;
        if let Some(error) = error {
            return Err(error);
        }
        self.offset = offset;
        Ok(self.tasks.clone())
    }
}

fn plan_tasks(payload: &Value) -> io::Result<Vec<SessionTask>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid Codex update_plan");
    let arguments = tool_arguments(payload).ok_or_else(invalid)?;
    arguments["plan"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let status = match task["status"].as_str() {
                Some("pending") => TaskStatus::Pending,
                Some("in_progress") => TaskStatus::InProgress,
                Some("completed") => TaskStatus::Completed,
                _ => return Err(invalid()),
            };
            Ok(SessionTask {
                id: index.to_string(),
                subject: task["step"].as_str().ok_or_else(invalid)?.to_owned(),
                status,
            })
        })
        .collect()
}

/// Read the subagent boundary from the first session metadata record.
fn history_start(file: &mut File) -> io::Result<Option<u64>> {
    file.seek(SeekFrom::Start(0))?;
    let mut line = Vec::new();
    io::BufReader::new(file).read_until(b'\n', &mut line)?;
    let Ok(record) = serde_json::from_slice::<Value>(&line) else {
        return Ok(None);
    };
    if record["type"] != "session_meta" {
        return Ok(None);
    }
    record["payload"]
        .get("subagent_history_start_ordinal")
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid Codex subagent history boundary",
                )
            })
        })
        .transpose()
}

fn inherited(record: &Value, start: Option<u64>) -> bool {
    start.is_some_and(|start| {
        record["ordinal"]
            .as_u64()
            .is_some_and(|ordinal| ordinal < start)
    })
}

/// Walk complete JSON records newest first, retaining records spanning chunks.
fn walk(path: &Path, mut visit: impl FnMut(Value, u64) -> io::Result<bool>) -> io::Result<()> {
    let mut file = File::open(path)?;
    let history = history_start(&mut file)?;
    let mut position = file.metadata()?.len();
    let mut remainder = Vec::new();
    while position > 0 {
        let amount = position.min(CHUNK);
        position -= amount;
        file.seek(SeekFrom::Start(position))?;
        let mut bytes = vec![0; amount as usize];
        file.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&remainder);
        let first = if position == 0 {
            0
        } else if let Some(index) = bytes.iter().position(|b| *b == b'\n') {
            index + 1
        } else {
            remainder = bytes;
            continue;
        };
        let mut end = bytes.len();
        for line in bytes[first..].split(|b| *b == b'\n').rev() {
            let start = end - line.len();
            end = start.saturating_sub(1);
            if let Ok(record) = serde_json::from_slice::<Value>(line)
                && (inherited(&record, history) || !visit(record, position + start as u64)?)
            {
                return Ok(());
            }
        }
        remainder = bytes[..first].to_vec();
    }
    Ok(())
}

fn source(record: &Value, offset: u64) -> String {
    let payload = &record["payload"];
    payload["item"]["id"]
        .as_str()
        .or_else(|| payload["id"].as_str())
        .or_else(|| payload["call_id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("offset:{offset}"))
}

fn assistant_source(record: &Value, offset: u64) -> Option<String> {
    let payload = &record["payload"];
    ((record["type"] == "response_item"
        && payload["type"] == "message"
        && payload["role"] == "assistant")
        || (record["type"] == "event_msg"
            && payload["type"] == "item_completed"
            && payload["item"]["type"] == "AgentMessage"))
        .then(|| source(record, offset))
}

fn timestamp(record: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn printable(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

fn content_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return (!text.trim().is_empty()).then(|| text.to_owned());
    }
    let text = content
        .as_array()?
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

struct Prompt {
    text: Option<String>,
    attachments: Vec<(String, Option<String>)>,
}

impl Prompt {
    fn body(&self) -> Option<String> {
        self.text.clone().or_else(|| {
            (!self.attachments.is_empty()).then(|| {
                self.attachments
                    .iter()
                    .map(|(label, _)| label.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
    }
}

/// Image tags are separate blocks around an input_image, never prompt text.
fn user_prompt(content: &Value) -> Prompt {
    let Some(blocks) = content.as_array() else {
        return Prompt {
            text: content_text(content),
            attachments: Vec::new(),
        };
    };
    let mut wrappers = HashSet::new();
    let mut attachments = Vec::new();
    for (index, triple) in blocks.windows(3).enumerate() {
        if triple[0]["type"] != "input_text"
            || triple[1]["type"] != "input_image"
            || triple[2]["type"] != "input_text"
            || triple[2]["text"] != "</image>"
        {
            continue;
        }
        let Some(tag) = triple[0]["text"]
            .as_str()
            .and_then(|text| text.strip_prefix("<image name="))
            .and_then(|text| text.strip_suffix('>'))
        else {
            continue;
        };
        let Some((name, attributes)) = tag.split_once(']') else {
            continue;
        };
        let label = format!("{name}]");
        let path = attributes
            .strip_prefix(" path=\"")
            .and_then(|path| path.strip_suffix('"'))
            .map(str::to_owned);
        attachments.push((label, path));
        wrappers.insert(index);
        wrappers.insert(index + 2);
    }
    let text = blocks
        .iter()
        .enumerate()
        .filter(|(index, _)| !wrappers.contains(index))
        .filter_map(|(_, block)| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Prompt {
        text: (!text.trim().is_empty()).then_some(text),
        attachments,
    }
}

fn completed_user(record: &Value) -> bool {
    record["type"] == "event_msg"
        && record["payload"]["type"] == "item_completed"
        && record["payload"]["item"]["type"] == "UserMessage"
}

/// Pair by prompt text and, when present, turn ID. Consume each completed
/// event only at its preceding user message.
fn prompt_images(prompt: &Prompt, payload: &Value, completed: Option<&Value>) -> Vec<Image> {
    let paired = completed.filter(|record| {
        let response_turn =
            payload["internal_chat_message_metadata_passthrough"]["turn_id"].as_str();
        let event_turn = record["payload"]["turn_id"].as_str();
        (response_turn.is_none() || response_turn == event_turn)
            && content_text(&record["payload"]["item"]["content"]) == prompt.text
    });
    let paths: Vec<_> = paired
        .and_then(|record| record["payload"]["item"]["content"].as_array())
        .into_iter()
        .flatten()
        .filter(|block| matches!(block["type"].as_str(), Some("local_image" | "image")))
        .map(|block| block["path"].as_str())
        .collect();
    prompt
        .attachments
        .iter()
        .enumerate()
        .filter_map(|(index, (label, path))| {
            let path = path
                .as_deref()
                .filter(|path| Path::new(path).is_file())
                .or_else(|| {
                    paths
                        .get(index)
                        .copied()
                        .flatten()
                        .filter(|path| Path::new(path).is_file())
                })?;
            Some(Image {
                label: label.clone(),
                path: path.into(),
            })
        })
        .collect()
}

/// Whether a user-role message carries context Codex injected, such as
/// AGENTS.md or the environment, rather than what the user sent. Codex labels
/// each content item, and its own history hides a message holding any item
/// not labelled `user.*`.
fn is_context_message(payload: &Value) -> bool {
    payload["internal_chat_message_metadata_passthrough"]["content_item_kinds"]
        .as_array()
        .is_some_and(|kinds| {
            kinds
                .iter()
                .any(|kind| !kind.as_str().is_some_and(|kind| kind.starts_with("user.")))
        })
}

fn assistant_text(record: &Value) -> io::Result<Option<String>> {
    let payload = &record["payload"];
    Ok(match (record["type"].as_str(), payload["type"].as_str()) {
        (Some("response_item"), Some("message")) if payload["role"] == "assistant" => {
            content_text(&payload["content"])
        }
        (Some("event_msg"), Some("task_complete")) => payload["last_agent_message"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
        (Some("event_msg"), Some("agent_message")) => {
            payload["message"].as_str().map(str::to_owned)
        }
        (Some("event_msg"), Some("item_completed"))
            if payload["item"]["type"] == "AgentMessage" =>
        {
            required_text(&payload["item"], "id")?;
            completed_message(&payload["item"])?
        }
        _ => None,
    })
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum MessageContent {
    Text { text: String },
}

fn completed_message(item: &Value) -> io::Result<Option<String>> {
    let content: Vec<MessageContent> = field(item, "content")?;
    let text = content
        .into_iter()
        .map(|MessageContent::Text { text }| text)
        .collect::<Vec<_>>()
        .join("\n");
    Ok((!text.trim().is_empty()).then_some(text))
}

fn field<T: serde::de::DeserializeOwned>(value: &Value, key: &str) -> io::Result<T> {
    serde_json::from_value(value[key].clone()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid Codex activity {key}: {error}"),
        )
    })
}

fn optional<T: serde::de::DeserializeOwned>(value: &Value, key: &str) -> io::Result<Option<T>> {
    field(value, key)
}

fn tool_body(payload: &Value) -> Option<String> {
    let name = payload["name"].as_str()?;
    if name.rsplit('.').next() == Some("request_user_input") {
        return Some(question_texts(&tool_arguments(payload)?).join(" · "));
    }
    if matches!(
        name.rsplit('.').next(),
        Some("exec" | "exec_command" | "shell" | "apply_patch" | "view_image")
    ) {
        return None;
    }
    let detail = if payload["type"] == "custom_tool_call" {
        payload["input"].as_str().map(str::to_owned)
    } else {
        payload["arguments"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|args| {
                let tool = name.rsplit('.').next().unwrap_or(name);
                let participant = match tool {
                    "spawn_agent" => Some("task_name"),
                    "send_message" | "followup_task" | "interrupt_agent" => Some("target"),
                    "resume_agent" | "close_agent" => Some("id"),
                    _ => None,
                };
                if let Some(key) = participant {
                    return Some(participant_text(
                        args[key].as_str(),
                        args["message"].as_str().unwrap_or(""),
                    ));
                }
                ["cmd", "command", "path", "file_path", "query"]
                    .iter()
                    .find_map(|key| args[*key].as_str().map(str::to_owned))
            })
    };
    let label = if name.rsplit('.').next() == Some("send_message") {
        "To"
    } else {
        name
    };
    Some(match detail.filter(|text| !text.trim().is_empty()) {
        Some(detail) => format!("{label} {}", printable(&detail)),
        None => label.to_owned(),
    })
}

fn tool_arguments(payload: &Value) -> Option<Value> {
    serde_json::from_str(payload["arguments"].as_str()?).ok()
}

fn argument_fields(value: &Value) -> Vec<Section> {
    value
        .as_object()
        .map(|fields| {
            vec![Section::Fields(
                fields
                    .iter()
                    .filter(|(key, _)| key.as_str() != "description")
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string()),
                        )
                    })
                    .collect(),
            )]
        })
        .unwrap_or_default()
}

fn text_section(label: &str, text: &str) -> Section {
    Section::Text {
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

fn response_tool_call(payload: &Value, output: Option<&str>) -> ToolCall {
    let is_mcp = ["name", "namespace"].into_iter().any(|key| {
        payload[key]
            .as_str()
            .is_some_and(|name| name.starts_with("mcp__"))
    });
    let mut input = if payload["type"] == "custom_tool_call" {
        vec![text_section(
            "input",
            payload["input"].as_str().unwrap_or(""),
        )]
    } else {
        let raw = payload["arguments"].as_str().unwrap_or("");
        match serde_json::from_str::<Value>(raw) {
            Ok(value) if is_mcp => crate::providers::tool::json_input(&value),
            Ok(value) if value.is_object() => match payload["name"]
                .as_str()
                .unwrap_or("")
                .rsplit('.')
                .next()
                .unwrap_or("")
            {
                "Bash" | "exec" | "exec_command" | "shell" => vec![Section::Code {
                    label: "command".to_owned(),
                    lang: "bash".to_owned(),
                    text: value["command"]
                        .as_str()
                        .or_else(|| value["cmd"].as_str())
                        .unwrap_or("")
                        .to_owned(),
                }],
                "Agent" | "Task" => vec![Section::Markdown {
                    label: "prompt".to_owned(),
                    text: value["prompt"].as_str().unwrap_or("").to_owned(),
                }],
                "SendMessage" | "send_message" => vec![Section::Markdown {
                    label: "message".to_owned(),
                    text: value["message"].as_str().unwrap_or("").to_owned(),
                }],
                "Skill" => vec![text_section("args", value["args"].as_str().unwrap_or(""))],
                _ => argument_fields(&value),
            },
            _ => vec![text_section("input", raw)],
        }
    };
    if !is_mcp && let Some(arguments) = tool_arguments(payload) {
        crate::providers::tool::include_description(&mut input, &arguments);
    }
    let mut call = ToolCall {
        detail_left: crate::providers::tool::display_tool_name(
            payload["name"].as_str().unwrap_or(""),
        ),
        detail_title: String::new(),
        show_details: false,
        id: payload["call_id"].as_str().unwrap_or("").to_owned(),
        title: if is_mcp {
            payload["name"].as_str().unwrap_or("").to_owned()
        } else {
            tool_title(
                payload["name"].as_str().unwrap_or(""),
                &tool_arguments(payload).unwrap_or(Value::Null),
            )
        },
        name: payload["name"].as_str().unwrap_or("").to_owned(),
        input,
        output: output.map(|text| vec![text_section("output", text)]),
        exit_code: None,
        is_error: false,
        duration: None,
    };
    crate::providers::tool::annotate(&mut call);
    call
}

fn result_section(text: &str) -> Section {
    text_section("result", text)
}

fn completed_call(item: &Value, command: Option<String>) -> ToolCall {
    let item_type = item["type"].as_str().unwrap();
    let (name, mut input, output) = match item_type {
        "CommandExecution" => {
            let input = vec![Section::Code {
                label: "command".to_owned(),
                lang: "bash".to_owned(),
                text: command.unwrap(),
            }];
            (
                "exec_command".to_owned(),
                input,
                item["aggregated_output"]
                    .as_str()
                    .map(|text| vec![text_section("output", text)]),
            )
        }
        "FileChange" => {
            let input = item["changes"]
                .as_object()
                .unwrap()
                .iter()
                .filter_map(|(path, change)| {
                    Some(Section::Diff {
                        label: format!("{path} ({})", change["type"].as_str()?),
                        text: change["unified_diff"].as_str()?.to_owned(),
                    })
                })
                .collect();
            let output = ["stdout", "stderr"]
                .into_iter()
                .filter_map(|key| {
                    item[key]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .map(|text| text_section(key, text))
                })
                .collect();
            ("apply_patch".to_owned(), input, Some(output))
        }
        "McpToolCall" => {
            let output = item
                .get("result")
                .filter(|result| !result.is_null())
                .map(|result| {
                    if result.is_object() {
                        let mut sections = crate::providers::tool::mcp_content(&result["content"]);
                        if let Some(structured) = result
                            .get("structuredContent")
                            .filter(|value| !value.is_null())
                        {
                            sections.push(Section::Json {
                                label: "output".to_owned(),
                                text: structured.to_string(),
                            });
                        }
                        sections
                    } else if let Some(text) = result.as_str() {
                        vec![result_section(text)]
                    } else {
                        vec![text_section("result", &result.to_string())]
                    }
                })
                .or_else(|| {
                    item["error"]["message"]
                        .as_str()
                        .map(|text| vec![result_section(text)])
                });
            (
                format!(
                    "{}.{}",
                    item["server"].as_str().unwrap(),
                    item["tool"].as_str().unwrap()
                ),
                crate::providers::tool::json_input(&item["arguments"]),
                output,
            )
        }
        _ => (
            item_type.to_owned(),
            vec![Section::Fields(
                item.as_object()
                    .unwrap()
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                    .collect(),
            )],
            None,
        ),
    };
    if item_type != "McpToolCall" {
        crate::providers::tool::include_description(&mut input, &item["arguments"]);
    }
    let title = if item_type == "McpToolCall" {
        name.clone()
    } else if name == "exec_command" {
        if let Some(Section::Code { text, .. }) = input.first() {
            tool_title(&name, &serde_json::json!({ "command": text }))
        } else {
            name.clone()
        }
    } else {
        tool_title(&name, &item["arguments"])
    };
    let mut call = ToolCall {
        detail_left: crate::providers::tool::display_tool_name(&name),
        detail_title: String::new(),
        show_details: false,
        id: item["id"].as_str().unwrap_or("").to_owned(),
        title,
        name,
        input,
        output,
        exit_code: item["exit_code"]
            .as_i64()
            .filter(|code| item_type == "CommandExecution" && *code != 0),
        is_error: item["status"] == "failed" || item["result"]["isError"] == true,
        duration: None,
    };
    crate::providers::tool::annotate(&mut call);
    call
}

fn required_text<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value[key].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Codex activity requires {key}"),
        )
    })
}

fn item_time(payload: &Value, key: &str) -> io::Result<DateTime<Utc>> {
    let millis = if key == "completed_at_ms" && payload.get(key).is_none() {
        0
    } else {
        field::<i64>(payload, key)?
    };
    DateTime::from_timestamp_millis(millis).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid Codex activity {key}"),
        )
    })
}

fn completed_tool(record: &Value) -> io::Result<Option<Event>> {
    let payload = &record["payload"];
    if record["type"] != "event_msg" || payload["type"] != "item_completed" {
        return Ok(None);
    }
    let item = &payload["item"];
    let mut paths = Vec::new();
    let item_type = required_text(item, "type")?;
    let mut command_text = None;
    let body = match item_type {
        "CommandExecution" => {
            let command: Vec<_> = item["command"]
                .as_array()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "Codex command requires argv")
                })?
                .iter()
                .map(|arg| {
                    arg.as_str().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "Invalid Codex command argument")
                    })
                })
                .collect::<io::Result<_>>()?;
            let text = if command.len() == 3 && matches!(command[1], "-lc" | "-c") {
                command[2].to_owned()
            } else {
                command.join(" ")
            };
            command_text = Some(text.clone());
            let summary = crate::shell_summary::summarize_shell_commands(&text);
            let mut body = if summary.is_empty() {
                format!("exec_command {text}")
            } else {
                summary
            };
            if let Some(code) = optional::<i32>(item, "exit_code")?
                && code != 0
            {
                body.push_str(&format!(" → {code}"));
            }
            body
        }
        "FileChange" => {
            let changes = item["changes"].as_object().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Codex file change requires changes",
                )
            })?;
            let mut body = "apply_patch".to_owned();
            for (index, path) in changes.keys().enumerate() {
                body.push_str(if index == 0 { " " } else { " · " });
                paths.push(body.len()..body.len() + path.len());
                body.push_str(path);
            }
            body
        }
        "McpToolCall" => format!(
            "{}.{}",
            required_text(item, "server")?,
            required_text(item, "tool")?
        ),
        "Extension" => {
            let kind = required_text(item, "kind")?;
            let detail = match kind {
                "web.search" => Some(required_text(item, "query")?.to_owned()),
                "image_gen.generation" => optional::<String>(item, "revisedPrompt")?,
                _ => None,
            };
            match detail {
                Some(detail) => format!("{kind} {detail}"),
                None => kind.to_owned(),
            }
        }
        "ImageView" => {
            let image = required_text(item, "path")?;
            let image = image.strip_prefix("file://").unwrap_or(image);
            let body = "view_image ";
            paths.push(body.len()..body.len() + image.len());
            format!("{body}{image}")
        }
        "SubAgentActivity" => format!(
            "subagent {} {}",
            required_text(item, "kind")?,
            participant_text(Some(required_text(item, "agent_path")?), "")
        ),
        "CollabAgentToolCall" => {
            let tool = required_text(item, "tool")?;
            let targets = if item.get("receiver_thread_ids").is_some() {
                field::<Vec<String>>(item, "receiver_thread_ids")?
            } else {
                Vec::new()
            }
            .join(", ");
            if targets.is_empty() {
                tool.to_owned()
            } else {
                format!("{tool} {}", participant_text(Some(&targets), ""))
            }
        }
        _ => return Ok(None),
    };
    required_text(item, "id")?;
    let ended_at = item_time(payload, "completed_at_ms")?;
    let start = optional::<i64>(payload, "started_at_ms")?
        .map(|_| item_time(payload, "started_at_ms"))
        .transpose()?;
    let elapsed = if matches!(item_type, "CommandExecution" | "McpToolCall") {
        optional::<std::time::Duration>(item, "duration")?
            .map(chrono::Duration::from_std)
            .transpose()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    } else {
        None
    };
    let duration = start.map(|start| elapsed.unwrap_or(ended_at - start));
    let started_at = match duration {
        Some(elapsed) => ended_at.checked_sub_signed(elapsed).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Codex activity duration exceeds timestamp",
            )
        })?,
        None => ended_at,
    };
    Ok(Some(Event {
        completed_time: true,
        background: false,
        source: None,
        source_records: Vec::new(),
        sender: None,
        kind: Kind::Tool,
        body,
        paths,
        started_at,
        ended_at: Some(ended_at),
        duration,
        previous_end: None,
        images: Vec::new(),
        calls: vec![completed_call(item, command_text)],
    }))
}

/// Read only through the SQLite prompt and its following completed item.
/// Prompt text itself comes from SQLite; rollout records supply attachments.
pub(super) fn first_prompt(
    path: &Path,
    first_user_message: &str,
) -> io::Result<(Vec<Image>, Option<String>)> {
    let mut lines = io::BufReader::new(File::open(path)?).split(b'\n');
    let mut offset = 0u64;
    for line in lines.by_ref() {
        let line = line?;
        let start = offset;
        offset += line.len() as u64 + 1;
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if (record["type"] == "session_meta"
            && record["payload"]
                .get("subagent_history_start_ordinal")
                .is_some())
            || completed_user(&record)
        {
            return Ok((Vec::new(), None));
        }
        let payload = &record["payload"];
        if record["type"] != "response_item"
            || payload["type"] != "message"
            || payload["role"] != "user"
            || is_context_message(payload)
        {
            continue;
        }
        let prompt = user_prompt(&payload["content"]);
        // Injected AGENTS/environment messages may have no content-kind labels.
        // SQLite identifies the actual first prompt, truncated by session().
        if !prompt
            .body()
            .is_some_and(|text| text.chars().take(512).eq(first_user_message.chars()))
        {
            continue;
        }
        if prompt.attachments.is_empty() {
            return Ok((Vec::new(), Some(source(&record, start))));
        }
        let completed = lines
            .next()
            .transpose()?
            .and_then(|line| serde_json::from_slice::<Value>(&line).ok())
            .filter(completed_user);
        return Ok((
            prompt_images(&prompt, payload, completed.as_ref()),
            Some(source(&record, start)),
        ));
    }
    Ok((Vec::new(), None))
}

pub(super) fn summary(path: &Path) -> io::Result<Summary> {
    let mut summary = Summary::default();
    let mut boundary = false;
    let mut message_source_closed = false;
    let mut outputs = HashSet::new();
    let mut pending = Vec::new();
    walk(path, |record, offset| {
        let Some(at) = timestamp(&record) else {
            return Ok(true);
        };
        summary.updated_at.get_or_insert(at);
        let payload = &record["payload"];
        if !boundary && record["type"] == "response_item" {
            match payload["type"].as_str() {
                Some("function_call_output") => {
                    if let Some(id) = payload["call_id"].as_str() {
                        outputs.insert(id.to_owned());
                    }
                }
                Some("function_call")
                    if payload["name"]
                        .as_str()
                        .and_then(|name| name.rsplit('.').next())
                        == Some("request_user_input") =>
                {
                    if let Some(id) = payload["call_id"].as_str()
                        && !outputs.contains(id)
                        && let Some(args) = tool_arguments(payload)
                    {
                        pending.push(questions(&args)?);
                    }
                }
                _ => {}
            }
        }
        if record["type"] == "event_msg" && payload["type"] == "task_started" {
            summary.prompt_at.get_or_insert(at);
            message_source_closed = true;
        }
        if !boundary && record["type"] == "event_msg" {
            match payload["type"].as_str() {
                Some("task_complete" | "turn_aborted") => {
                    summary.turn_ended = true;
                    boundary = true;
                }
                Some("task_started") => {
                    boundary = true;
                }
                _ => {}
            }
        }
        let message = assistant_text(&record)?;
        if summary.last_message.is_none()
            && let Some(text) = message.as_ref()
        {
            summary.detail_sources[1] = assistant_source(&record, offset);
            summary.last_message = Some(printable(text));
            summary.last_message_at = Some(at);
        }
        // Completion notifications repeat the reply but have no log row. Keep
        // the identity of the latest message record in that turn instead.
        if summary.detail_sources[1].is_none() && !message_source_closed && message.is_some() {
            summary.detail_sources[1] = assistant_source(&record, offset);
        }
        if summary.activity.is_none()
            && let Some(event) = completed_tool(&record)?
        {
            summary.detail_sources[0] = Some(source(&record, offset));
            summary.activity = Some(Activity {
                kind: event.kind,
                label: event.body,
                at,
            });
        }
        if summary.activity.is_none() && record["type"] == "response_item" {
            let activity = match payload["type"].as_str() {
                Some("function_call" | "custom_tool_call") => tool_body(payload)
                    .map(|label| (tool_kind(payload["name"].as_str().unwrap()), label)),
                Some("reasoning") => Some((Kind::Tool, "Thinking".to_owned())),
                _ => None,
            };
            if let Some((kind, label)) = activity {
                summary.detail_sources[0] = Some(source(&record, offset));
                summary.activity = Some(Activity { kind, label, at });
            }
        }
        Ok(!(boundary
            && summary.prompt_at.is_some()
            && summary.last_message.is_some()
            && summary.activity.is_some()
            && (summary.detail_sources[1].is_some() || message_source_closed)))
    })?;
    summary.pending_questions = pending.into_iter().rev().flatten().collect();
    Ok(summary)
}

/// Parsed rollout records and the byte range retained for backward paging.
pub(crate) struct Reader {
    path: PathBuf,
    identity: Option<(u64, u64)>,
    history_start: Option<u64>,
    observed_len: u64,
    modified: Option<std::time::SystemTime>,
    oldest: u64,
    read: u64,
    pending_offset: Option<u64>,
    pending_bytes: Vec<u8>,
    records: Vec<(u64, Value)>,
    entries: Vec<(u64, Event)>,
    dependencies: HashMap<String, u64>,
    latest_prompt: Option<u64>,
    shown: usize,
    boundary: crate::jsonl::Boundary,
}

impl Reader {
    pub fn new(path: PathBuf, _sidechains: bool) -> Self {
        Self {
            path,
            identity: None,
            history_start: None,
            observed_len: 0,
            modified: None,
            oldest: 0,
            read: 0,
            pending_offset: None,
            pending_bytes: Vec::new(),
            records: Vec::new(),
            entries: Vec::new(),
            dependencies: HashMap::new(),
            latest_prompt: None,
            shown: 0,
            boundary: crate::jsonl::Boundary::default(),
        }
    }

    fn dependency(&mut self, record: &Value, offset: u64) -> u64 {
        let payload = &record["payload"];
        let mut affected = offset;
        for id in [
            payload["id"].as_str(),
            payload["call_id"].as_str(),
            payload["item"]["id"].as_str(),
        ]
        .into_iter()
        .flatten()
        {
            affected = affected.min(*self.dependencies.entry(id.to_owned()).or_insert(offset));
        }
        if completed_user(record) {
            affected = affected.min(self.latest_prompt.unwrap_or(offset));
        }
        if record["type"] == "response_item"
            && payload["type"] == "message"
            && payload["role"] == "user"
        {
            self.latest_prompt = Some(offset);
        }
        affected
    }

    fn read_pending(&mut self, file: &mut File) -> io::Result<u64> {
        file.seek(SeekFrom::Start(self.read))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if let Ok(record) = serde_json::from_slice::<Value>(&bytes) {
            if !inherited(&record, self.history_start) {
                self.pending_offset = Some(self.read);
                self.pending_bytes = bytes.clone();
                self.records.push((self.read, record));
            }
            Ok(bytes.len() as u64)
        } else {
            Ok(0)
        }
    }

    pub fn refresh(&mut self, requested: usize) -> io::Result<crate::log::Update> {
        use std::os::unix::fs::MetadataExt;
        let mut file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let modified = metadata.modified()?;
        let reset = self.identity != Some(identity)
            || metadata.len() < self.read
            || (metadata.len() == self.observed_len && self.modified != Some(modified))
            || (metadata.len() > self.observed_len && !self.boundary.matches(&mut file)?);
        let changed = self.observed_len != metadata.len() || self.modified != Some(modified);
        self.observed_len = metadata.len();
        self.modified = Some(modified);
        let old_shown = self.shown;
        let mut parsed_bytes = 0;
        let mut appended = 0;
        let mut replaced = 0;
        if reset {
            self.identity = Some(identity);
            self.history_start = history_start(&mut file)?;
            self.read = crate::jsonl::complete_end(&mut file)?;
            self.oldest = self.read;
            self.records.clear();
            self.entries.clear();
            self.dependencies.clear();
            self.latest_prompt = None;
            self.shown = 0;
            self.pending_offset = None;
            parsed_bytes += self.read_pending(&mut file)?;
            self.entries = events(&self.path, &self.records, usize::MAX)?;
        } else if changed && metadata.len() > self.read {
            let pending = self.pending_offset.take();
            let pending_bytes = std::mem::take(&mut self.pending_bytes);
            let pending_record =
                pending.and_then(|_| self.records.last().map(|(_, record)| record.clone()));
            let mut reused = 0;
            if let Some(pending) = pending {
                self.records.truncate(
                    self.records
                        .partition_point(|(offset, _)| *offset < pending),
                );
            }
            let mut offset = self.read;
            let mut incoming = Vec::new();
            let end = crate::jsonl::read_lines(&mut file, self.read, |line| {
                if pending == Some(offset) && line == pending_bytes {
                    if let Some(record) = &pending_record {
                        incoming.push((offset, record.clone()));
                        reused += line.len() as u64;
                    }
                } else if let Ok(record) = serde_json::from_slice::<Value>(line) {
                    incoming.push((offset, record));
                }
                offset += line.len() as u64 + 1;
            })?;
            parsed_bytes += end - self.read - reused;
            self.read = end;
            incoming.retain(|(_, record)| !inherited(record, self.history_start));
            let mut affected = pending.unwrap_or(end);
            for (offset, record) in &incoming {
                affected = affected.min(self.dependency(record, *offset));
            }
            self.records.extend(incoming);
            parsed_bytes += self.read_pending(&mut file)?;
            if let Some((offset, record)) = self.records.last().cloned() {
                affected = affected.min(self.dependency(&record, offset));
            }
            let prefix = self
                .entries
                .partition_point(|(offset, _)| *offset < affected);
            let start = self
                .records
                .partition_point(|(offset, _)| *offset < affected);
            let mut suffix = events(&self.path, &self.records[start..], usize::MAX)?;
            if let Some((_, previous)) = prefix
                .checked_sub(1)
                .and_then(|index| self.entries.get(index))
                && let Some((_, first)) = suffix.first_mut()
            {
                first.previous_end = Some(previous.ended_at.unwrap_or(previous.started_at));
            }
            let previous = self.entries.len() - prefix;
            appended = suffix.len().saturating_sub(previous);
            replaced = previous;
            self.entries.truncate(prefix);
            self.entries.extend(suffix);
        }
        let parsed_before_older = parsed_bytes;
        while self.entries.len() < requested && self.oldest > 0 {
            let end = self.oldest;
            let target = end.saturating_sub(CHUNK);
            let mut older = Vec::new();
            let mut oldest = end;
            crate::jsonl::read_lines_back(&mut file, end, |offset, line| {
                oldest = offset;
                parsed_bytes += line.len() as u64 + 1;
                if let Ok(record) = serde_json::from_slice::<Value>(line) {
                    if inherited(&record, self.history_start) {
                        oldest = 0;
                        return false;
                    }
                    older.push((offset, record));
                }
                offset > target
            })?;
            self.oldest = oldest;
            older.reverse();
            older.append(&mut self.records);
            self.records = older;
            self.entries = events(&self.path, &self.records, usize::MAX)?;
            self.dependencies.clear();
            self.latest_prompt = None;
            // Rebuild dependency positions only when the retained range expands backward.
            for index in 0..self.records.len() {
                let (offset, record) = self.records[index].clone();
                self.dependency(&record, offset);
            }
        }
        if parsed_bytes > parsed_before_older {
            replaced = replaced.max(old_shown);
        }
        let shown = self
            .entries
            .len()
            .min(requested.max(self.shown.saturating_add(appended)));
        let prepended = if reset {
            0
        } else {
            shown.saturating_sub(old_shown + appended)
        };
        self.shown = shown;
        let skip = self.entries.len() - shown;
        let events = self.entries[skip..]
            .iter()
            .map(|(_, event)| event.clone())
            .collect();
        self.boundary.capture(&mut file, metadata.len())?;
        Ok(crate::log::Update {
            events,
            prepended,
            appended: if reset { 0 } else { appended },
            replaced,
            reset,
            parsed_bytes,
            oldest_offset: self.oldest,
            read_offset: self.read,
        })
    }
}

impl crate::log::Reader for Reader {
    fn refresh(&mut self, requested: usize) -> io::Result<crate::log::Update> {
        Reader::refresh(self, requested)
    }
}

#[cfg(test)]
pub(super) fn tail(path: &Path, count: usize) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    walk(path, |record, offset| {
        records.push((offset, record));
        Ok(true)
    })?;
    records.reverse();
    Ok(events(path, &records, count)?
        .into_iter()
        .map(|(_, event)| event)
        .collect())
}

fn events(path: &Path, records: &[(u64, Value)], count: usize) -> io::Result<Vec<(u64, Event)>> {
    let mut entries = Vec::new();
    let mut results = HashMap::new();
    let mut seen = HashSet::new();
    let mut completed_prompt = None;
    let mut visit = |record: &Value, offset: u64| -> io::Result<bool> {
        let Some(at) = timestamp(record) else {
            return Ok(true);
        };
        if completed_user(record) {
            completed_prompt = Some((offset, record.clone()));
            return Ok(true);
        }
        let payload = &record["payload"];
        let mut images = Vec::new();
        let mut calls = Vec::new();
        let reference = |offset| crate::log::SourceRecord {
            path: path.to_path_buf(),
            offset,
        };
        let mut source_records = vec![reference(offset)];
        if let Some(mut event) = completed_tool(record)? {
            event.source = Some(source(record, offset));
            event.source_records = source_records;
            if !seen.insert(required_text(&payload["item"], "id")?.to_owned()) {
                return Ok(true);
            }
            entries.push((offset, event));
            return Ok(entries.len() <= count);
        }
        let (kind, body, ended_at) = match (record["type"].as_str(), payload["type"].as_str()) {
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                if let Some(id) = payload["call_id"].as_str() {
                    let output = if let Some(text) = payload["output"].as_str() {
                        text.to_owned()
                    } else {
                        payload["output"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|block| block["type"] == "input_text")
                            .filter_map(|block| block["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    results.insert(id.to_owned(), (at, output, offset));
                }
                return Ok(true);
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let Some(body) = tool_body(payload) else {
                    return Ok(true);
                };
                if let Some(id) = payload["call_id"].as_str()
                    && !seen.insert(id.to_owned())
                {
                    return Ok(true);
                }
                let ended = payload["call_id"]
                    .as_str()
                    .and_then(|id| results.remove(id));
                calls.push(response_tool_call(
                    payload,
                    ended.as_ref().map(|(_, text, _)| text.as_str()),
                ));
                if let Some((_, _, offset)) = &ended {
                    source_records.insert(0, reference(*offset));
                }
                (
                    tool_kind(payload["name"].as_str().unwrap()),
                    body,
                    ended.map(|(at, _, _)| at),
                )
            }
            (Some("response_item"), Some("agent_message")) => {
                let header = content_text(&payload["content"]).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing Codex agent message header",
                    )
                })?;
                let field = |prefix: &str| {
                    header
                        .lines()
                        .find_map(|line| line.strip_prefix(prefix))
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Invalid Codex agent message header",
                            )
                        })
                };
                let body = format!("{} from {}", field("Message Type: ")?, field("Sender: ")?);
                (Kind::Assistant, body, None)
            }
            (Some("response_item"), Some("message")) => {
                let kind = match payload["role"].as_str() {
                    Some("user") if !is_context_message(payload) => Kind::User,
                    Some("assistant") => Kind::Assistant,
                    _ => return Ok(true),
                };
                let completed = if payload["role"] == "user" {
                    completed_prompt.take()
                } else {
                    None
                };
                let Some(body) = (if kind == Kind::User {
                    let prompt = user_prompt(&payload["content"]);
                    if let Some((offset, _)) = &completed {
                        source_records.push(reference(*offset));
                    }
                    images = prompt_images(
                        &prompt,
                        payload,
                        completed.as_ref().map(|(_, record)| record),
                    );
                    prompt.body()
                } else {
                    content_text(&payload["content"])
                }) else {
                    return Ok(true);
                };
                if let Some(id) = payload["id"].as_str()
                    && !seen.insert(id.to_owned())
                {
                    return Ok(true);
                }
                (kind, body, None)
            }
            (Some("event_msg"), Some("item_completed"))
                if payload["item"]["type"] == "AgentMessage" =>
            {
                let item = &payload["item"];
                if !seen.insert(required_text(item, "id")?.to_owned()) {
                    return Ok(true);
                }
                let Some(body) = completed_message(item)? else {
                    return Ok(true);
                };
                (Kind::Assistant, body, None)
            }
            (Some("event_msg"), Some("turn_aborted")) => {
                (Kind::Error, "Turn aborted".to_owned(), Some(at))
            }
            _ => return Ok(true),
        };
        entries.push((
            offset,
            Event {
                completed_time: kind == Kind::Tool,
                background: body.ends_with(" *"),
                source: Some(source(record, offset)),
                source_records,
                sender: None,
                kind,
                body,
                paths: Vec::new(),
                started_at: at,
                ended_at,
                duration: ended_at.map(|end| end - at),
                previous_end: None,
                images,
                calls,
            },
        ));
        Ok(entries.len() <= count)
    };
    for (offset, record) in records.iter().rev() {
        if !visit(record, *offset)? {
            break;
        }
    }
    entries.reverse();
    let mut previous_end = None;
    for (_, entry) in &mut entries {
        entry.source_records.sort_by_key(|record| record.offset);
        entry.source_records.dedup();
        entry.previous_end = previous_end;
        previous_end = Some(entry.ended_at.unwrap_or(entry.started_at));
    }
    let skip = entries.len().saturating_sub(count);
    Ok(entries.into_iter().skip(skip).collect())
}
#[cfg(test)]
mod tests {
    #[test]
    fn raw_source_records_preserve_real_call_and_result_order() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut file,
            include_bytes!("../../../tests/fixtures/codex-raw-detail.jsonl"),
        )
        .unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let update = reader.refresh(usize::MAX).unwrap();
        let rows: Vec<serde_json::Value> =
            include_str!("../../../tests/fixtures/codex-raw-detail.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let expected = rows[1..]
            .iter()
            .map(|row| serde_json::to_string_pretty(row).unwrap())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(update.events.last().unwrap().raw_text().unwrap(), expected);
        assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        assert_eq!(
            update.events[0].raw_text().unwrap(),
            serde_json::to_string_pretty(&rows[0]).unwrap()
        );
    }

    use super::*;
    use crate::log::Kind;
    use std::io::Write;
    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-rollout.jsonl")
    }
    fn tool_records() -> Vec<Value> {
        include_str!("../../../tests/fixtures/codex-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn reader_extends_real_fixture_without_reparsing_bytes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for name in [
            "codex-tool-calls.jsonl",
            "codex-rollout.jsonl",
            "codex-inner/command.jsonl",
            "codex-inner/file-change.jsonl",
            "codex-images/wrapped.jsonl",
            "codex-images/fallback.jsonl",
        ] {
            let bytes = std::fs::read(root.join(name)).unwrap();
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let mut reader = Reader::new(file.path().to_path_buf(), false);
            let mut parsed = 0;
            for line in bytes.split_inclusive(|byte| *byte == b'\n') {
                for chunk in line.chunks(137) {
                    file.write_all(chunk).unwrap();
                    parsed += reader.refresh(usize::MAX).unwrap().parsed_bytes;
                }
                let update = reader.refresh(usize::MAX).unwrap();
                assert_eq!(
                    format!("{:?}", update.events),
                    format!("{:?}", tail(file.path(), usize::MAX).unwrap()),
                    "{name}"
                );
            }
            assert_eq!(parsed, bytes.len() as u64, "{name}");
            assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        }
    }

    #[test]
    fn reader_expands_backwards_and_resets_on_truncation() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!(
            "../../../tests/fixtures/codex-tool-calls.jsonl"
        ))
        .unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let first = reader.refresh(1).unwrap();
        assert!(first.reset);
        let expanded = reader.refresh(usize::MAX).unwrap();
        assert_eq!(
            expanded.events.len(),
            tail(file.path(), usize::MAX).unwrap().len()
        );
        assert_eq!(
            expanded.prepended,
            expanded.events.len() - first.events.len()
        );
        file.as_file_mut().set_len(0).unwrap();
        let reset = reader.refresh(1).unwrap();
        assert!(reset.reset);
        assert!(reset.events.is_empty());
    }

    #[test]
    fn reader_resets_when_source_file_is_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        std::fs::copy(fixture(), &path).unwrap();
        let mut reader = Reader::new(path.clone(), false);
        reader.refresh(usize::MAX).unwrap();
        let replacement = directory.path().join("replacement.jsonl");
        std::fs::copy(fixture(), &replacement).unwrap();
        std::fs::rename(replacement, &path).unwrap();
        let update = reader.refresh(usize::MAX).unwrap();
        assert!(update.reset);
        assert_eq!(
            format!("{:?}", update.events),
            format!("{:?}", tail(&path, usize::MAX).unwrap())
        );
    }

    #[test]
    fn reader_resets_on_same_length_overwrite() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let bytes = std::fs::read(fixture()).unwrap();
        file.write_all(&bytes).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        reader.refresh(usize::MAX).unwrap();
        let before = file.as_file().metadata().unwrap().modified().unwrap();
        let changed = String::from_utf8(bytes)
            .unwrap()
            .replace("Checks passed.", "Checks failed.");
        file.as_file_mut().seek(SeekFrom::Start(0)).unwrap();
        file.write_all(changed.as_bytes()).unwrap();
        file.as_file()
            .set_modified(before + std::time::Duration::from_secs(1))
            .unwrap();
        let update = reader.refresh(usize::MAX).unwrap();
        assert!(update.reset);
        assert_eq!(update.events.last().unwrap().body, "Checks failed.");
        assert!(!reader.refresh(usize::MAX).unwrap().reset);
    }

    #[test]
    fn reader_keeps_valid_unterminated_record_when_newline_arrives() {
        let text = std::fs::read_to_string(fixture()).unwrap();
        let end = text
            .lines()
            .scan(0usize, |offset, line| {
                *offset += line.len() + 1;
                let record: Value = serde_json::from_str(line).unwrap();
                Some((*offset, record))
            })
            .find(|(_, record)| record["payload"]["role"] == "assistant")
            .unwrap()
            .0;
        let bytes = text[..end].strip_suffix('\n').unwrap().as_bytes();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let initial = reader.refresh(usize::MAX).unwrap();
        let expected = tail(file.path(), usize::MAX).unwrap();
        assert_eq!(format!("{:?}", initial.events), format!("{expected:?}"));
        assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        file.write_all(b"\n").unwrap();
        let completed = reader.refresh(usize::MAX).unwrap();
        assert_eq!(format!("{:?}", completed.events), format!("{expected:?}"));
        assert_eq!(completed.appended, 0);
    }

    #[test]
    fn plans_replace_previous_tasks_incrementally() {
        let records: Vec<_> = include_str!("../../../tests/fixtures/codex-todos.jsonl")
            .lines()
            .collect();
        assert!(records.len() >= 2);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut tasks = Tasks::default();
        writeln!(file, "{}", records[0]).unwrap();
        let first = tasks.read(file.path()).unwrap();
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].subject, "Inspect input");
        assert_eq!(first[0].status, TaskStatus::InProgress);
        assert_eq!(first[1].status, TaskStatus::Pending);
        assert_eq!(first[2].status, TaskStatus::Pending);
        let offset = tasks.offset;
        assert_eq!(tasks.read(file.path()).unwrap(), first);
        assert_eq!(tasks.offset, offset);
        for record in &records[1..] {
            writeln!(file, "{record}").unwrap();
        }
        let last = tasks.read(file.path()).unwrap();
        assert_eq!(last.len(), 3);
        assert!(last.iter().all(|task| task.status == TaskStatus::Completed));
        assert_eq!(tasks.summary(file.path()).unwrap().tasks, last);
    }

    #[test]
    fn plans_wait_for_complete_lines_and_reset_on_truncation() {
        let record = include_str!("../../../tests/fixtures/codex-todos.jsonl")
            .lines()
            .next()
            .unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut tasks = Tasks::default();
        write!(file, "{record}").unwrap();
        assert!(tasks.read(file.path()).unwrap().is_empty());
        writeln!(file).unwrap();
        assert_eq!(tasks.read(file.path()).unwrap().len(), 3);
        file.as_file_mut().set_len(0).unwrap();
        assert!(tasks.read(file.path()).unwrap().is_empty());
    }

    #[test]
    fn detail_sources_identify_real_rollout_events() {
        let path = fixture();
        let details = summary(&path).unwrap();
        let (_, prompt) = first_prompt(&path, "Check the dashboard.").unwrap();
        let events = tail(&path, usize::MAX).unwrap();
        for (source, kind) in [
            (details.detail_sources[1].as_ref(), Kind::Assistant),
            (prompt.as_ref(), Kind::User),
        ] {
            let source = source.unwrap_or_else(|| panic!("fixture source {kind:?}"));
            let event = events
                .iter()
                .find(|event| event.source.as_ref() == Some(source))
                .unwrap();
            assert_eq!(event.kind, kind);
        }
        let tool_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-inner/command.jsonl");
        let tool_summary = summary(&tool_path).unwrap();
        let tool_events = tail(&tool_path, usize::MAX).unwrap();
        let source = tool_summary.detail_sources[0].as_ref().unwrap();
        assert!(
            tool_events
                .iter()
                .any(|event| event.source.as_ref() == Some(source))
        );
        let short = tail(&path, 1).unwrap();
        assert!(!short.iter().any(|event| event.source == prompt));
    }

    #[test]
    fn response_and_completed_calls_keep_titles_and_ids() {
        let records = tool_records();
        let response = &records[0]["payload"];
        let call = response_tool_call(response, None);
        assert_eq!(call.id, response["call_id"].as_str().unwrap());
        assert_eq!(call.title, "Agent");
        let record = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let call = &completed_tool(record).unwrap().unwrap().calls[0];
        assert_eq!(call.id, record["payload"]["item"]["id"].as_str().unwrap());
        assert_eq!(call.title, "cat, sed");
    }

    #[test]
    fn completed_file_change_has_output() {
        let records = tool_records();
        let record = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "FileChange")
            .unwrap();
        let event = completed_tool(record).unwrap().unwrap();
        assert_eq!(
            event.calls[0].output,
            Some(vec![text_section(
                "stdout",
                record["payload"]["item"]["stdout"].as_str().unwrap()
            )])
        );
        let mut empty = record.clone();
        empty["payload"]["item"]["stdout"] = "".into();
        empty["payload"]["item"]["stderr"] = "".into();
        assert_eq!(
            completed_tool(&empty).unwrap().unwrap().calls[0].output,
            Some(vec![])
        );
        empty["payload"]["item"]["stderr"] = record["payload"]["item"]["stdout"].clone();
        assert_eq!(
            completed_tool(&empty).unwrap().unwrap().calls[0].output,
            Some(vec![
                Section::Label("stderr".into()),
                text_section(
                    "stderr",
                    record["payload"]["item"]["stdout"].as_str().unwrap()
                )
            ])
        );
    }

    #[test]
    fn tool_calls_keep_arguments_outputs_and_running_state() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-tool-calls.jsonl");
        let events = tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        let spawn: Vec<_> = calls
            .iter()
            .filter(|call| call.name == "spawn_agent")
            .collect();
        assert_eq!(spawn.len(), 2);
        assert!(
            spawn[0].input.iter().any(|section| matches!(section, Section::Prose { text, markdown: true } if text == "[scrubbed]"))
        );
        assert!(spawn[0].output.is_some());
        assert_eq!(spawn[1].output, None);
        let custom = calls.iter().find(|call| call.name == "js_repl").unwrap();
        assert!(
            matches!(&custom.input[0], crate::log::Section::Text { text, .. } if text.starts_with("// codex-js-repl"))
        );
        assert!(
            matches!(&custom.output.as_ref().unwrap()[0], crate::log::Section::Text { text, .. } if text == "[]")
        );
        assert!(!calls.iter().any(|call| call.name == "exec"));
    }

    #[test]
    fn completed_calls_keep_command_output_and_file_diff() {
        let records = tool_records();
        let command = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let event = completed_tool(command).unwrap().unwrap();
        assert_eq!(event.calls.len(), 1);
        assert!(
            matches!(&event.calls[0].input[0], crate::log::Section::Shell { text, prefix: 0 } if text == command["payload"]["item"]["command"][2].as_str().unwrap())
        );
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], crate::log::Section::Text { text, .. } if text == command["payload"]["item"]["aggregated_output"].as_str().unwrap())
        );
        let file = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "FileChange")
            .unwrap();
        let event = completed_tool(file).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].input[0], crate::log::Section::Diff { label, text } if label.contains("/workspace/agents/src/processes.rs") && label.contains("update") && text.starts_with("@@ -132,2 +132,22 @@"))
        );
    }

    #[test]
    fn mcp_content_blocks_and_structured_results_are_retained() {
        for source in [
            include_str!("../../../tests/fixtures/mcp-codex/image.json"),
            include_str!("../../../tests/fixtures/mcp-codex/resource.json"),
            include_str!("../../../tests/fixtures/mcp-codex/structured.json"),
        ] {
            let record: Value = serde_json::from_str(source).unwrap();
            let call = &completed_tool(&record).unwrap().unwrap().calls[0];
            assert_eq!(call.output.as_ref().unwrap().len(), 2, "{}", call.name);
            assert_eq!(call.input.len(), 1);
            assert!(matches!(&call.input[0], Section::Json { label, text }
                if label == "input" && serde_json::from_str::<Value>(text).unwrap() == record["payload"]["item"]["arguments"]));
            let result = &record["payload"]["item"]["result"];
            assert!(
                matches!(&call.output.as_ref().unwrap()[0], Section::Text { text, .. }
                if text == result["content"][0]["text"].as_str().unwrap())
            );
            match &call.output.as_ref().unwrap()[1] {
                Section::Placeholder(text) => {
                    let mime = if call.name == "cua_repl.js" {
                        "[image image/png]"
                    } else {
                        "[resource text/html]"
                    };
                    assert_eq!(text, mime);
                }
                Section::Json { label, text } => {
                    assert_eq!(label, "output");
                    assert_eq!(
                        serde_json::from_str::<Value>(text).unwrap(),
                        result["structuredContent"]
                    );
                }
                section => panic!("unexpected MCP output: {section:?}"),
            }
        }
    }

    #[test]
    fn mcp_response_arguments_use_json_for_namespaced_tools() {
        let record: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/mcp-codex/input.json"))
                .unwrap();
        let call = response_tool_call(&record["payload"], None);
        assert_eq!(
            call.input,
            vec![Section::Json {
                label: "input".into(),
                text: "{}".into(),
            }]
        );
    }

    #[test]
    fn malformed_arguments_and_completed_mcp_results_keep_details() {
        let records = tool_records();
        let mut call = records
            .iter()
            .find(|record| record["payload"]["type"] == "function_call")
            .unwrap()["payload"]
            .clone();
        call["arguments"] = "{unfinished".into();
        let detail = response_tool_call(&call, None);
        assert_eq!(
            detail.input,
            [Section::Text {
                label: "input".to_owned(),
                text: "{unfinished".to_owned()
            }]
        );
        let mcp = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "McpToolCall")
            .unwrap();
        let event = completed_tool(mcp).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].input[0], Section::Json {label, text} if label == "input" && serde_json::from_str::<Value>(text).unwrap() == mcp["payload"]["item"]["arguments"])
        );
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], Section::Text { text, .. } if text == "Delivered to Claude.")
        );
        let mut no_result = mcp.clone();
        no_result["payload"]["item"]["result"] = Value::Null;
        assert_eq!(
            completed_tool(&no_result).unwrap().unwrap().calls[0].output,
            None
        );
        let mut json_result = mcp.clone();
        json_result["payload"]["item"]["result"]["content"][0]["text"] = r#"{"ok":true}"#.into();
        let event = completed_tool(&json_result).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], Section::Text { text, .. } if text == r#"{"ok":true}"#)
        );
        let mut command = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap()
            .clone();
        command["payload"]["item"]["exit_code"] = 2.into();
        let event = completed_tool(&command).unwrap().unwrap();
        assert_eq!(event.calls[0].input.len(), 1);
        assert_eq!(event.calls[0].exit_code, Some(2));
    }

    // Real timeout and rejection completions, with identifiers and project names scrubbed.
    // Sources (relative to Codex sessions/):
    // 2026/06/11/rollout-2026-06-11T19-39-04-019eb9b2-8282-7b02-827f-034576f5de4d.jsonl.zst:158
    // 2026/07/22/rollout-2026-07-22T09-19-33-019f8768-1762-79b3-b3ee-1db3f099c4f0.jsonl.zst:34
    fn mcp_error_records() -> Vec<Value> {
        include_str!("../../../tests/fixtures/codex-mcp-errors.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn mcp_completed_errors_keep_reason_and_failed_state() {
        for mut record in mcp_error_records() {
            let message = record["payload"]["item"]["error"]["message"]
                .as_str()
                .unwrap()
                .to_owned();
            for null_result in [false, true] {
                if null_result {
                    record["payload"]["item"]["result"] = Value::Null;
                }
                let event = completed_tool(&record).unwrap().unwrap();
                assert!(event.calls[0].is_error);
                assert_eq!(event.calls[0].output, Some(vec![result_section(&message)]));
            }
            record["payload"]["item"]["result"] = serde_json::json!({
                "content": [{"type": "text", "text": "result retained"}], "isError": true
            });
            let event = completed_tool(&record).unwrap().unwrap();
            assert_eq!(
                event.calls[0].output,
                Some(vec![text_section("output", "result retained")])
            );
            assert!(event.calls[0].is_error);
        }
    }

    #[test]
    fn mcp_completed_non_object_arguments_keep_json() {
        let mut record = mcp_error_records().remove(0);
        // No non-object arguments found in the source days' real MCP completions.
        // Vary only the arguments on a captured completion to cover JSON shapes.
        for arguments in [
            serde_json::json!(["text", {"key": 1}, null]),
            serde_json::json!("text"),
            serde_json::json!(42),
            serde_json::json!(true),
            Value::Null,
        ] {
            record["payload"]["item"]["arguments"] = arguments.clone();
            let event = completed_tool(&record).unwrap().unwrap();
            let expected = if arguments.is_null() {
                Vec::new()
            } else {
                vec![Section::Json {
                    label: "input".into(),
                    text: arguments.to_string(),
                }]
            };
            assert_eq!(event.calls[0].input, expected, "{arguments}");
        }
    }

    #[test]
    fn custom_output_arrays_join_text_and_ignore_images() {
        let mut records = tool_records();
        let mut call = records
            .iter()
            .find(|record| record["payload"]["name"] == "exec")
            .unwrap()
            .clone();
        call["payload"]["name"] = "js_repl".into();
        let mut output = records.pop().unwrap();
        output["payload"]["output"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type":"input_image","text":"skip"}));
        output["payload"]["output"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type":"input_text","text":"second text"}));
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{call}\n{output}").unwrap();
        let events = tail(file.path(), 100).unwrap();
        assert_eq!(events[0].calls.len(), 1);
        assert!(
            matches!(&events[0].calls[0].output.as_ref().unwrap()[0], crate::log::Section::Text { text, .. } if text.starts_with("Script completed") && !text.contains("skip") && text.ends_with("\nsecond text"))
        );
    }

    #[test]
    fn request_user_input_log_and_activity_show_question_texts() {
        for mode in ["default", "plan"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/codex-input/{mode}-pending.jsonl"));
            let events = tail(&path, 100).unwrap();
            let question = events
                .iter()
                .find(|event| event.body == "Pick a color?")
                .unwrap();
            assert_eq!(question.kind, Kind::Assistant);
            assert_eq!(question.body, "Pick a color?");
            assert!(question.ended_at.is_none());
            let summary = summary(&path).unwrap();
            assert_eq!(
                summary.pending_questions,
                vec![crate::ask::Question {
                    id: "color".into(),
                    header: "Color".into(),
                    text: "Pick a color?".into(),
                    options: vec![
                        crate::ask::QuestionOption {
                            label: "Red".into(),
                            description: "Choose red.".into(),
                            preview: None
                        },
                        crate::ask::QuestionOption {
                            label: "Blue".into(),
                            description: "Choose blue.".into(),
                            preview: None
                        },
                    ],
                    multi: false,
                    other: false,
                    secret: false,
                }]
            );
            let activity = summary.activity.unwrap();
            assert_eq!(activity.label, "Pick a color?");
            assert_eq!(activity.kind, Kind::Assistant);
        }
    }

    fn input_records(state: &str) -> Vec<Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/codex-input/default-{state}.jsonl"));
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn input_summary(records: &[Value]) -> Summary {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        summary(file.path()).unwrap()
    }

    #[test]
    fn request_user_input_output_clears_only_matching_call_before_turn_ends() {
        let mut records = input_records("pending");
        let output = input_records("answered")
            .into_iter()
            .find(|record| record["payload"]["type"] == "function_call_output")
            .unwrap();
        let mut other = output.clone();
        other["payload"]["call_id"] = "another-call".into();
        records.push(other);
        assert_eq!(
            input_summary(&records)
                .pending_questions
                .iter()
                .map(|question| question.text.as_str())
                .collect::<Vec<_>>(),
            ["Pick a color?"]
        );
        records.push(output);
        let result = input_summary(&records);
        assert!(result.pending_questions.is_empty());
        assert!(!result.turn_ended);
    }

    #[test]
    fn request_user_input_does_not_leak_across_turn_boundaries() {
        let mut records = input_records("pending");
        let started = records
            .iter()
            .find(|record| record["payload"]["type"] == "task_started")
            .unwrap()
            .clone();
        records.push(started.clone());
        assert!(input_summary(&records).pending_questions.is_empty());
        records.pop();
        let mut aborted = started;
        aborted["payload"]["type"] = "turn_aborted".into();
        records.push(aborted);
        assert!(input_summary(&records).pending_questions.is_empty());
    }

    #[test]
    fn request_user_input_multiple_questions_and_calls_keep_order() {
        let mut records = input_records("pending");
        let mut call = records
            .iter()
            .find(|record| record["payload"]["name"] == "request_user_input")
            .unwrap()
            .clone();
        let mut args = tool_arguments(&call["payload"]).unwrap();
        let mut second = args["questions"][0].clone();
        second["question"] = "Pick a size?".into();
        args["questions"].as_array_mut().unwrap().push(second);
        call["payload"]["arguments"] = args.to_string().into();
        call["payload"]["call_id"] = "second-call".into();
        call["payload"]["name"] = "functions.request_user_input".into();
        assert_eq!(
            tool_body(&call["payload"]).unwrap(),
            "Pick a color? · Pick a size?"
        );
        records.push(call);
        assert_eq!(
            input_summary(&records)
                .pending_questions
                .iter()
                .map(|question| question.text.as_str())
                .collect::<Vec<_>>(),
            ["Pick a color?", "Pick a color?", "Pick a size?"]
        );
    }
    #[test]
    fn real_agent_names_in_log_and_activity() {
        for (line, expected) in include_str!("../../../tests/fixtures/codex-agent-names.jsonl")
            .lines()
            .zip([
                "spawn_agent (real_scan) Review files.",
                "To (/root) Review files.",
                "followup_task (ux_probe) Review files.",
            ])
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, expected);
            assert_eq!(
                summary(file.path()).unwrap().activity.unwrap().label,
                expected
            );
        }
    }

    #[test]
    fn agent_targets_without_messages_and_namespaced_calls() {
        let record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/codex-agent-names.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        for (name, key) in [
            ("collaboration.send_message", "target"),
            ("interrupt_agent", "target"),
            ("resume_agent", "id"),
            ("close_agent", "id"),
        ] {
            let mut record = record.clone();
            record["payload"]["name"] = name.into();
            record["payload"]["arguments"] = serde_json::json!({key:"reviewer"}).to_string().into();
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), record.to_string()).unwrap();
            let label = if name == "collaboration.send_message" {
                "To"
            } else {
                name
            };
            let expected = format!("{label} (reviewer)");
            assert_eq!(tail(file.path(), 1).unwrap()[0].body, expected);
            assert_eq!(
                summary(file.path()).unwrap().activity.unwrap().label,
                expected
            );
        }
    }

    #[test]
    fn exec_wrappers_never_supply_activity() {
        assert!(
            tool_body(&serde_json::json!({"type":"custom_tool_call", "name":"exec"})).is_none()
        );
        assert!(
            tool_body(
                &serde_json::json!({"type":"custom_tool_call", "name":"exec", "input":"tools.exec_command({"})
            )
            .is_none()
        );
    }
    #[test]
    fn completed_rollout_summary() {
        let result = summary(&fixture()).unwrap();
        assert_eq!(result.last_message.as_deref(), Some("Checks passed."));
        assert!(result.last_message_at.is_some());
        assert!(result.updated_at.is_some());
        assert!(result.turn_ended);
    }
    #[test]
    fn log_maps_real_records_without_duplicate_messages() {
        let events = tail(&fixture(), 100).unwrap();
        assert_eq!(
            events.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            vec![&Kind::User, &Kind::Assistant]
        );
        assert_eq!(events[1].body, "Checks passed.");
        assert!(events[1].previous_end.is_some());
    }
    #[test]
    fn zero_count_reads_nothing() {
        assert!(tail(Path::new("missing"), 0).unwrap().is_empty());
    }
    #[test]
    fn backwards_reader_keeps_large_unicode_record_and_partial_tail() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut records: Vec<serde_json::Value> = std::fs::read_to_string(fixture())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        records.retain(|r| r["payload"]["type"] != "task_complete");
        let text = "中".repeat(70_000);
        let message = records
            .iter_mut()
            .find(|r| r["payload"]["role"] == "assistant")
            .unwrap();
        message["payload"]["content"][0]["text"] = serde_json::json!(text);
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        write!(file, "{{\"timestamp\":").unwrap();
        let result = summary(file.path()).unwrap();
        assert_eq!(result.last_message.unwrap(), text);
        assert!(!result.turn_ended);
    }
    #[test]
    fn duplicate_message_order_does_not_change_log() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let text = std::fs::read_to_string(fixture()).unwrap();
        let mut lines: Vec<_> = text.lines().collect();
        lines.swap(4, 5);
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        let events = tail(file.path(), 100).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn completed_item_without_response_still_supplies_message() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for line in std::fs::read_to_string(fixture()).unwrap().lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            if record["payload"]["role"] != "assistant"
                && record["payload"]["type"] != "task_complete"
            {
                writeln!(file, "{line}").unwrap();
            }
        }
        assert_eq!(
            summary(file.path()).unwrap().last_message.as_deref(),
            Some("Checks passed.")
        );
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events[0].kind, Kind::Assistant);
        assert_eq!(events[0].body, "Checks passed.");
    }

    #[test]
    fn latest_tool_activity_and_aborted_turn() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::copy(&mut std::fs::File::open(fixture()).unwrap(), &mut file).unwrap();
        writeln!(file,r#"{{"timestamp":"2026-10-13T18:00:00Z","type":"event_msg","payload":{{"type":"task_started"}}}}"#).unwrap();
        writeln!(file,r#"{{"timestamp":"2026-10-13T18:00:01Z","type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{{\"cmd\":\"cargo check\"}}","call_id":"test"}}}}"#).unwrap();
        let result = summary(file.path()).unwrap();
        assert!(!result.turn_ended);
        assert!(result.activity.is_none());
        writeln!(file,r#"{{"timestamp":"2026-10-13T18:00:02Z","type":"event_msg","payload":{{"type":"turn_aborted","reason":"interrupted"}}}}"#).unwrap();
        assert!(summary(file.path()).unwrap().turn_ended);
    }
}
