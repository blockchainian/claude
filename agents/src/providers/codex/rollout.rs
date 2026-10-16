use crate::log::{Event, Image, Kind, Section, ToolCall, participant_text, question_texts};
use crate::model::Activity;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: u64 = 64 * 1024;

#[derive(Default)]
pub(super) struct Summary {
    pub detail_sources: [Option<String>; 3],
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub updated_at: Option<DateTime<Utc>>,
    pub turn_ended: bool,
    pub pending_questions: Vec<String>,
}

/// Walk complete JSON records newest first, retaining records spanning chunks.
fn walk(path: &Path, mut visit: impl FnMut(Value, u64) -> io::Result<bool>) -> io::Result<()> {
    let mut file = File::open(path)?;
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
                && !visit(record, position + start as u64)?
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
    let input = if payload["type"] == "custom_tool_call" {
        vec![text_section(
            "input",
            payload["input"].as_str().unwrap_or(""),
        )]
    } else {
        let raw = payload["arguments"].as_str().unwrap_or("");
        match serde_json::from_str::<Value>(raw) {
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
    ToolCall {
        id: payload["call_id"].as_str().unwrap_or("").to_owned(),
        title: crate::log::tool_title(
            payload["name"].as_str().unwrap_or(""),
            &tool_arguments(payload).unwrap_or(Value::Null),
        ),
        name: payload["name"].as_str().unwrap_or("").to_owned(),
        input,
        output: output.map(|text| vec![text_section("output", text)]),
        is_error: false,
    }
}

fn result_section(text: &str) -> Section {
    text_section("result", text)
}

fn completed_call(item: &Value, command: Option<String>) -> ToolCall {
    let item_type = item["type"].as_str().unwrap();
    let (name, input, output) = match item_type {
        "CommandExecution" => {
            let mut input = vec![Section::Code {
                label: "command".to_owned(),
                lang: "bash".to_owned(),
                text: command.unwrap(),
            }];
            if let Some(code) = item["exit_code"].as_i64().filter(|code| *code != 0) {
                input.push(Section::Fields(vec![(
                    "exit code".to_owned(),
                    code.to_string(),
                )]));
            }
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
                    if let Some(blocks) = result["content"].as_array() {
                        blocks
                            .iter()
                            .filter(|block| block["type"] == "text")
                            .filter_map(|block| block["text"].as_str())
                            .map(result_section)
                            .collect()
                    } else if let Some(text) = result.as_str() {
                        vec![result_section(text)]
                    } else {
                        vec![text_section("result", &result.to_string())]
                    }
                });
            (
                format!(
                    "{}.{}",
                    item["server"].as_str().unwrap(),
                    item["tool"].as_str().unwrap()
                ),
                argument_fields(&item["arguments"]),
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
    let title = if name == "exec_command" {
        if let Some(Section::Code { text, .. }) = input.first() {
            crate::log::tool_title(&name, &serde_json::json!({ "command": text }))
        } else {
            name.clone()
        }
    } else {
        crate::log::tool_title(&name, &item["arguments"])
    };
    ToolCall {
        id: item["id"].as_str().unwrap_or("").to_owned(),
        title,
        name,
        input,
        output,
        is_error: item["status"] == "failed" || item["result"]["isError"] == true,
    }
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
                body.push_str(&format!(" (exit {code})"));
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
        source: None,
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
        if completed_user(&record) {
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
                        pending.push(question_texts(&args));
                    }
                }
                _ => {}
            }
        }
        if record["type"] == "event_msg" && payload["type"] == "task_started" {
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
                Some("function_call" | "custom_tool_call") => tool_body(payload).map(|label| {
                    (
                        crate::log::tool_kind(payload["name"].as_str().unwrap()),
                        label,
                    )
                }),
                Some("reasoning") => Some((Kind::Tool, "Thinking".to_owned())),
                _ => None,
            };
            if let Some((kind, label)) = activity {
                summary.detail_sources[0] = Some(source(&record, offset));
                summary.activity = Some(Activity { kind, label, at });
            }
        }
        Ok(!(boundary
            && summary.last_message.is_some()
            && summary.activity.is_some()
            && (summary.detail_sources[1].is_some() || message_source_closed)))
    })?;
    summary.pending_questions = pending.into_iter().rev().flatten().collect();
    Ok(summary)
}

pub(super) fn tail(path: &Path, count: usize) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let mut results = HashMap::new();
    let mut seen = HashSet::new();
    let mut completed_prompt = None;
    walk(path, |record, offset| {
        let Some(at) = timestamp(&record) else {
            return Ok(true);
        };
        if completed_user(&record) {
            completed_prompt = Some(record);
            return Ok(true);
        }
        let payload = &record["payload"];
        let mut images = Vec::new();
        let mut calls = Vec::new();
        if let Some(mut event) = completed_tool(&record)? {
            event.source = Some(source(&record, offset));
            if !seen.insert(required_text(&payload["item"], "id")?.to_owned()) {
                return Ok(true);
            }
            entries.push(event);
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
                    results.insert(id.to_owned(), (at, output));
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
                    ended.as_ref().map(|(_, text)| text.as_str()),
                ));
                (
                    crate::log::tool_kind(payload["name"].as_str().unwrap()),
                    body,
                    ended.map(|(at, _)| at),
                )
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
                    images = prompt_images(&prompt, payload, completed.as_ref());
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
        entries.push(Event {
            source: Some(source(&record, offset)),
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
        });
        Ok(entries.len() <= count)
    })?;
    entries.reverse();
    let mut previous_end = None;
    for entry in &mut entries {
        entry.previous_end = previous_end;
        previous_end = Some(entry.ended_at.unwrap_or(entry.started_at));
    }
    let skip = entries.len().saturating_sub(count);
    Ok(entries.into_iter().skip(skip).collect())
}
#[cfg(test)]
mod tests {
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
            Some(vec![text_section(
                "stderr",
                record["payload"]["item"]["stdout"].as_str().unwrap()
            )])
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
            matches!(&spawn[0].input[0], crate::log::Section::Fields(fields) if fields.iter().any(|(key, value)| key == "message" && value == "[scrubbed]"))
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
            matches!(&event.calls[0].input[0], crate::log::Section::Code { lang, text, .. } if lang == "bash" && text == command["payload"]["item"]["command"][2].as_str().unwrap())
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
            matches!(&event.calls[0].input[0], Section::Fields(fields) if fields == &[("text".to_owned(), "[scrubbed]".to_owned())])
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
        assert_eq!(
            event.calls[0].input[1],
            Section::Fields(vec![("exit code".to_owned(), "2".to_owned())])
        );
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
            let activity = summary(&path).unwrap().activity.unwrap();
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
        assert_eq!(input_summary(&records).pending_questions, ["Pick a color?"]);
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
            input_summary(&records).pending_questions,
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
