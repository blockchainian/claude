use crate::event::{Event, Kind, Section, ToolCall};
use crate::images::{Image, ImageSource};
use crate::model::{Activity, SessionTask, TaskStatus};
use crate::providers::text::printable;
use crate::providers::tool::{self, Body, ToolData, ToolResult, list_title, tool_kind};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(test)]
fn response_output(name: &str, text: &str) -> Vec<Section> {
    tool::text_output(text, tool::rule(name).output, &Value::Null)
}

#[cfg(test)]
fn output_sections(text: &str, mode: Option<(tool::Pick<Body>, tool::Format)>) -> Vec<Section> {
    tool::text_output(text, mode, &Value::Null)
}

const CHUNK: u64 = 64 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Summary {
    pub prompt_at: Option<DateTime<Utc>>,
    pub cache_expires_at: Option<DateTime<Utc>>,
    pub task_details_sources: [Option<String>; 3],
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub updated_at: Option<DateTime<Utc>>,
    pub turn_ended: bool,
    pub pending_questions: Vec<crate::ask::Question>,
    pub pending_question_call: Option<(String, DateTime<Utc>)>,
    pub tasks: Vec<SessionTask>,
}

/// Reuses a file generation's decoded records, summary and last complete plan.
#[derive(Default)]
pub(super) struct Tasks {
    #[cfg(test)]
    parsed_bytes: u64,
    reader: Option<Reader>,
    summary: Option<Summary>,
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
        let result = self.refresh_summary(path);
        if result.is_err() {
            self.reader = None;
            self.summary = None;
            self.tasks.clear();
            self.offset = 0;
        }
        result
    }

    fn refresh_summary(&mut self, path: &Path) -> io::Result<Summary> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path)?;
        if let Some(reader) = &self.reader
            && reader.identity == Some((metadata.dev(), metadata.ino()))
            && reader.observed_len == metadata.len()
            && reader.modified == Some(metadata.modified()?)
            && let Some(summary) = &self.summary
        {
            return Ok(summary.clone());
        }
        let reader = self
            .reader
            .get_or_insert_with(|| Reader::new(path.to_owned(), false));
        let update = reader.read_summary()?;
        #[cfg(test)]
        {
            self.parsed_bytes += update.parsed_bytes;
        }
        if update.reset {
            self.tasks.clear();
            self.offset = 0;
        }
        let start = reader
            .records
            .partition_point(|(offset, _)| *offset < self.offset);
        for (offset, record) in &reader.records[start..] {
            if *offset >= reader.read {
                continue;
            }
            let payload = &record["payload"];
            if record["type"] == "response_item"
                && payload["type"] == "function_call"
                && payload["name"]
                    .as_str()
                    .is_some_and(|name| tool::rule(name).task == Some(tool::TaskAction::Plan))
            {
                self.tasks = plan_tasks(payload)?;
            }
        }
        self.offset = reader.read;
        self.identity = reader.identity;
        let latest = latest_activity_records(path, &reader.records, reader.metadata.as_ref())?;
        let mut result = summary_records(
            reader
                .records
                .iter()
                .rev()
                .map(|(offset, record)| (record, *offset)),
            latest.as_ref(),
        )?;
        result.tasks = self.tasks.clone();
        self.summary = Some(result.clone());
        Ok(result)
    }

    pub(super) fn first_prompt(
        &mut self,
        prompt: &str,
    ) -> io::Result<(Vec<Image>, Option<String>)> {
        let reader = self
            .reader
            .as_ref()
            .ok_or_else(|| io::Error::other("Codex summary is not loaded"))?;
        Ok(if reader.history_start.is_some() {
            (Vec::new(), None)
        } else {
            first_prompt_records(
                &reader.path,
                reader
                    .records
                    .iter()
                    .map(|(offset, record)| (*offset, record)),
                &reader.line_ends,
                prompt,
            )
        })
    }

    #[cfg(test)]
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
                || !payload["name"]
                    .as_str()
                    .is_some_and(|name| tool::rule(name).task == Some(tool::TaskAction::Plan))
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
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid Codex plan");
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
fn read_metadata(file: &mut File) -> io::Result<Option<Value>> {
    file.seek(SeekFrom::Start(0))?;
    let mut line = Vec::new();
    io::BufReader::new(file).read_until(b'\n', &mut line)?;
    Ok(serde_json::from_slice(&line).ok())
}

#[cfg(test)]
fn history_start(file: &mut File) -> io::Result<Option<u64>> {
    read_metadata(file)?
        .as_ref()
        .map(history_from_record)
        .transpose()
        .map(Option::flatten)
}

fn history_from_record(record: &Value) -> io::Result<Option<u64>> {
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
#[cfg(test)]
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

fn record_ids(record: &Value) -> impl Iterator<Item = &str> {
    let payload = &record["payload"];
    [
        payload["item"]["id"].as_str(),
        payload["id"].as_str(),
        payload["call_id"].as_str(),
    ]
    .into_iter()
    .flatten()
}

fn source(record: &Value, offset: u64) -> String {
    record_ids(record)
        .next()
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

fn completed_assistant(record: &Value) -> bool {
    record["type"] == "event_msg"
        && record["payload"]["type"] == "item_completed"
        && record["payload"]["item"]["type"] == "AgentMessage"
}

fn timestamp(record: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
        .ok()
        .map(|at| at.with_timezone(&Utc))
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
fn prompt_images(
    prompt: &Prompt,
    payload: &Value,
    completed: Option<&Value>,
    source: crate::event::SourceRecord,
) -> Vec<Image> {
    let paired = completed.filter(|record| {
        let response_turn =
            payload["internal_chat_message_metadata_passthrough"]["turn_id"].as_str();
        let event_turn = record["payload"]["turn_id"].as_str();
        (response_turn.is_none() || response_turn == event_turn)
            && content_text(&record["payload"]["item"]["content"]) == prompt.text
    });
    let local_paths: Vec<_> = paired
        .and_then(|record| record["payload"]["item"]["content"].as_array())
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "local_image")
        .filter_map(|block| block["path"].as_str())
        .collect();
    let mut seen = Vec::new();
    let images: Vec<_> = payload["content"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, block)| block["type"] == "input_image")
        .enumerate()
        .flat_map(|(index, (content_index, block))| {
            let encoded = block["image_url"].as_str();
            let pointer = format!("/payload/content/{content_index}/image_url");
            let first = encoded.and_then(|encoded| {
                seen.iter()
                    .find(|(data, _)| *data == encoded)
                    .map(|(_, pointer): &(&str, String)| pointer.clone())
            });
            let duplicate = first.is_some();
            let pointer = first.unwrap_or(pointer);
            if !duplicate && let Some(encoded) = encoded {
                seen.push((encoded, pointer.clone()));
            }
            let image = Image {
                label: prompt.attachments.get(index).map_or_else(
                    || format!("[Image #{}]", index + 1),
                    |(label, _)| label.clone(),
                ),
                source: ImageSource::Record {
                    record: source.clone(),
                    pointer,
                },
            };
            let mut images = Vec::new();
            if !duplicate {
                images.push(image.clone());
            }
            for path in prompt
                .attachments
                .get(index)
                .and_then(|(_, path)| path.as_deref())
                .into_iter()
                .chain(local_paths.get(index).copied())
            {
                if prompt
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains(path))
                    && !images.iter().any(|image| image.label == path)
                {
                    images.push(Image {
                        label: path.into(),
                        source: image.source.clone(),
                    });
                }
            }
            images
        })
        .collect();
    if !images.is_empty() {
        return images;
    }
    paired
        .and_then(|record| record["payload"]["item"]["content"].as_array())
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "local_image")
        .enumerate()
        .filter_map(|(index, block)| {
            let path = PathBuf::from(block["path"].as_str()?);
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(Image {
                label: format!("[Image #{}]", index + 1),
                source: ImageSource::File {
                    path,
                    len: metadata.len(),
                    modified: metadata.modified().ok()?,
                },
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

fn response_tool_name(payload: &Value) -> String {
    let name = payload["name"].as_str().unwrap_or("");
    match payload["namespace"]
        .as_str()
        .filter(|namespace| namespace.starts_with("mcp__"))
    {
        Some(namespace) if !name.starts_with("mcp__") => format!("{namespace}__{name}"),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
fn tool_body(payload: &Value) -> Option<String> {
    let name = payload["name"].as_str()?;
    // Wrapper calls are represented by their completed inner items.
    if tool::rule(name).wrapper || tool::tool_hidden(name) {
        return None;
    }
    let call = response_tool_call(payload, None);
    Some(
        call.agent_message
            .as_ref()
            .map(|message| message.content.text().to_owned())
            .unwrap_or(call.summary),
    )
}

const ENCRYPTED_CONTENT: &str = "(encrypted)";

fn is_fernet_token(text: &str) -> bool {
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(text) else {
        return false;
    };
    // Version, timestamp, IV, at least one AES block, and SHA-256 HMAC.
    bytes.first() == Some(&0x80) && bytes.len() >= 73 && (bytes.len() - 57).is_multiple_of(16)
}

fn tool_arguments(payload: &Value) -> Option<Value> {
    let mut arguments: Value = serde_json::from_str(payload["arguments"].as_str()?).ok()?;
    if let Some(message) = arguments.get_mut("message")
        && message.as_str().is_some_and(is_fernet_token)
    {
        *message = ENCRYPTED_CONTENT.into();
    }
    Some(arguments)
}

#[cfg(test)]
fn text_section(label: &str, text: &str) -> Section {
    Section::Text {
        label: label.to_owned(),
        text: text.to_owned(),
    }
}

#[cfg(test)]
fn agent_name(path: &str) -> &str {
    crate::event::agent::name(path)
}

fn parsed_object(text: &str) -> Value {
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
        .unwrap_or(Value::Null)
}

fn normalize_function_result(text: &str) -> ToolResult {
    let structured = parsed_object(text);
    let content = structured
        .get("content")
        .filter(|content| content.is_array())
        .cloned()
        .unwrap_or_else(|| Value::String(text.to_owned()));
    let error = tool::result_state(text, &Value::Null, structured["isError"] == true).is_error;
    ToolResult {
        source: None,
        content,
        structured,
        error,
    }
}

fn normalize_mcp_result(item: &Value) -> Option<ToolResult> {
    let result = item.get("result").filter(|result| !result.is_null());
    let Some(result) = result else {
        return item["error"]["message"].as_str().map(|text| ToolResult {
            source: None,
            content: text.into(),
            structured: Value::Null,
            error: tool::result_state(text, item, true).is_error,
        });
    };
    if !result.is_object() {
        let text = result
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| result.to_string());
        return Some(ToolResult {
            source: None,
            content: text.clone().into(),
            structured: Value::Null,
            error: tool::result_state(&text, item, false).is_error,
        });
    }
    let mut content = result["content"].clone();
    let structured = if let Some(structured) = result
        .get("structuredContent")
        .filter(|value| !value.is_null())
    {
        let block = serde_json::json!({"type":"text", "text":structured.to_string()});
        if let Some(blocks) = content.as_array_mut() {
            blocks.push(block);
        } else if content.is_null() {
            content = Value::Array(vec![block]);
        }
        structured.clone()
    } else {
        content
            .as_array()
            .filter(|blocks| blocks.len() == 1)
            .and_then(|blocks| {
                (blocks[0]["type"] == "text")
                    .then(|| blocks[0]["text"].as_str())
                    .flatten()
            })
            .map(parsed_object)
            .unwrap_or(Value::Null)
    };
    let text = content_text(&content).unwrap_or_default();
    Some(ToolResult {
        source: None,
        content,
        structured,
        error: tool::result_state(&text, item, result["isError"] == true).is_error,
    })
}

fn presentation_output(output: Option<Body>, completed: bool) -> (Option<Vec<Section>>, bool) {
    match output {
        Some(Body::Sections(sections)) => (Some(sections), false),
        Some(Body::Hidden) => (completed.then(Vec::new), true),
        None => (None, false),
    }
}

fn response_tool_call(payload: &Value, output: Option<&str>) -> ToolCall {
    let raw = payload["arguments"].as_str().unwrap_or("");
    let arguments = tool_arguments(payload);
    let raw_name = response_tool_name(payload);
    let input_value = if payload["type"] == "custom_tool_call" {
        Value::String(payload["input"].as_str().unwrap_or("").to_owned())
    } else {
        arguments
            .clone()
            .unwrap_or_else(|| Value::String(raw.to_owned()))
    };
    let normalized = output.map(normalize_function_result);
    let state = output.map(|text| {
        tool::result_state(
            text,
            &Value::Null,
            normalized.as_ref().is_some_and(|result| result.error),
        )
    });
    let presentation = tool::presentation(
        &raw_name,
        &ToolData {
            input: &input_value,
            result: normalized.as_ref(),
        },
    );
    let subject = presentation.title;
    let (output, hide_output) = presentation_output(presentation.output, normalized.is_some());
    let mut call = ToolCall {
        agent_message: None,
        title: subject.clone().unwrap_or_default(),
        show_event_detail: true,
        id: payload["call_id"].as_str().unwrap_or("").to_owned(),
        summary: list_title(
            &raw_name,
            subject.as_deref(),
            arguments.as_ref().unwrap_or(&Value::Null),
            state.as_ref().and_then(|state| state.exit_code),
        ),
        name: raw_name.clone(),
        started_at: None,
        hide_output,
        model_settings: None,
        agent_thread_id: None,
        agent_path: None,
        input: presentation.input,
        output,
        exit_code: state.as_ref().and_then(|state| state.exit_code),
        is_error: state.is_some_and(|state| state.is_error),
        duration: None,
    };
    call.agent_message = arguments
        .as_ref()
        .and_then(|input| crate::event::agent::tool_message(&raw_name, input));
    if let Some(message) = &mut call.agent_message
        && serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|args| args["message"].as_str().map(is_fernet_token))
            == Some(true)
    {
        message.content = crate::event::agent::Content::Encrypted;
    }
    call.agent_path = call
        .agent_message
        .as_ref()
        .and_then(|message| message.recipient.clone());
    crate::event::agent::present(&mut call);
    crate::providers::tool::annotate(&mut call);
    call
}

#[cfg(test)]
fn result_section(text: &str) -> Section {
    text_section("output", text)
}

fn image_data(value: &str) -> &str {
    if value.starts_with("data:") {
        value.split_once(',').map_or(value, |(_, data)| data)
    } else {
        value
    }
}

#[cfg(test)]
thread_local! {
    static IMAGE_HASH_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static IMAGE_INTERVAL_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn image_hash(value: &str) -> [u8; 32] {
    #[cfg(test)]
    IMAGE_HASH_BYTES.with(|count| count.set(count.get() + image_data(value).len()));
    Sha256::digest(image_data(value)).into()
}

fn output_image_blocks(output: &Value) -> impl Iterator<Item = (usize, &str)> {
    output
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, block)| block["type"] == "input_image")
        .filter_map(|(index, block)| Some((index, block["image_url"].as_str()?)))
}

struct RecordImages {
    generated: Option<[u8; 32]>,
    output: HashMap<usize, [u8; 32]>,
}

impl RecordImages {
    fn new(record: &Value) -> Self {
        let payload = &record["payload"];
        Self {
            generated: (payload["item"]["kind"] == "image_gen.generation")
                .then(|| payload["item"]["result"].as_str())
                .flatten()
                .map(image_hash),
            output: output_image_blocks(&payload["output"])
                .map(|(index, url)| (index, image_hash(url)))
                .collect(),
        }
    }

    fn hashes(&self) -> impl Iterator<Item = &[u8; 32]> {
        self.generated.iter().chain(self.output.values())
    }
}

type ImageHashes = HashMap<u64, RecordImages>;

fn output_image(record: &crate::event::SourceRecord, index: usize, url: &str) -> Image {
    let label = url
        .strip_prefix("data:")
        .and_then(|data| data.split_once(';'))
        .filter(|(mime, _)| !mime.is_empty())
        .map_or_else(
            || "[image]".to_owned(),
            |(mime, _)| format!("[image {mime}]"),
        );
    Image {
        label,
        source: ImageSource::Record {
            record: record.clone(),
            pointer: format!("/payload/output/{index}/image_url"),
        },
    }
}

fn image_file(value: &str) -> Option<Image> {
    let path = if value.starts_with("file:") {
        url::Url::parse(value).ok()?.to_file_path().ok()?
    } else {
        PathBuf::from(value)
    };
    Image::file(path)
}

fn record_turn(record: &Value) -> Option<&str> {
    let payload = &record["payload"];
    payload["turn_id"]
        .as_str()
        .or_else(|| payload["internal_chat_message_metadata_passthrough"]["turn_id"].as_str())
}

/// Only a single ImageView and a single output image establish an unambiguous source.
fn image_views(path: &Path, records: &[(u64, Value)]) -> HashMap<u64, (Image, [u64; 2])> {
    let outputs: HashMap<_, _> = records
        .iter()
        .filter(|(_, record)| {
            record["type"] == "response_item"
                && matches!(
                    record["payload"]["type"].as_str(),
                    Some("function_call_output" | "custom_tool_call_output")
                )
        })
        .filter_map(|(offset, record)| {
            Some((record["payload"]["call_id"].as_str()?, (*offset, record)))
        })
        .collect();
    let calls: Vec<_> = records
        .iter()
        .filter(|(_, record)| {
            record["type"] == "response_item"
                && matches!(
                    record["payload"]["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                )
                && record["payload"]["name"]
                    .as_str()
                    .is_some_and(|name| tool::rule(name).wrapper)
        })
        .filter_map(|(start, record)| {
            let id = record["payload"]["call_id"].as_str()?;
            let (end, output) = outputs.get(id)?;
            let turn = record_turn(record)?;
            (record_turn(output) == Some(turn)).then_some((*start, *end, turn, *output))
        })
        .collect();
    // Sweep each turn's strict call intervals. Endpoint ordering excludes both
    // boundaries and the active set preserves the existing ambiguity rule.
    let mut turns = HashMap::<&str, Vec<(u64, u8, usize)>>::new();
    for (index, (start, end, turn, _)) in calls.iter().enumerate() {
        if start < end {
            let points = turns.entry(turn).or_default();
            points.push((*start, 2, index));
            points.push((*end, 0, index));
        }
    }
    for (index, (offset, record)) in records.iter().enumerate() {
        if record["type"] == "event_msg"
            && record["payload"]["item"]["type"] == "ImageView"
            && let Some(turn) = record_turn(record)
        {
            turns.entry(turn).or_default().push((*offset, 1, index));
        }
    }
    let mut owners = HashMap::new();
    let mut view_counts = vec![0; calls.len()];
    for points in turns.values_mut() {
        points.sort_unstable();
        let mut active = std::collections::BTreeSet::new();
        let mut views = 0;
        for &(offset, kind, index) in points.iter() {
            match kind {
                0 => {
                    view_counts[index] = views - view_counts[index];
                    active.remove(&index);
                }
                2 => {
                    view_counts[index] = views;
                    active.insert(index);
                }
                _ => {
                    views += 1;
                    #[cfg(test)]
                    IMAGE_INTERVAL_CHECKS.with(|count| count.set(count.get() + 1));
                    if active.len() == 1 {
                        owners.insert(offset, calls[*active.first().unwrap()].0);
                    }
                }
            }
        }
    }
    let mut positions = HashMap::<u64, Vec<u64>>::new();
    for (offset, _) in records {
        if let Some(start) = owners.get(offset) {
            positions.entry(*start).or_default().push(*offset);
        }
    }
    let mut images = HashMap::new();
    for (call_index, (start, end, _, output)) in calls.into_iter().enumerate() {
        if view_counts[call_index] != 1 {
            continue;
        }
        let Some(positions) = positions
            .get(&start)
            .filter(|positions| positions.len() == 1)
        else {
            continue;
        };
        let mut blocks = output_image_blocks(&output["payload"]["output"]);
        let Some((index, url)) = blocks.next() else {
            continue;
        };
        if blocks.next().is_some() {
            continue;
        }
        let record = crate::event::SourceRecord {
            path: path.to_owned(),
            offset: end,
        };
        images.insert(
            positions[0],
            (output_image(&record, index, url), [start, end]),
        );
    }
    images
}

fn completed_call(
    item: &Value,
    command: Option<String>,
    record: Option<&crate::event::SourceRecord>,
) -> ToolCall {
    let item_type = item["type"].as_str().unwrap();
    let mut normalized = (item_type == "McpToolCall")
        .then(|| normalize_mcp_result(item))
        .flatten();
    if let Some(result) = &mut normalized {
        result.source = record
            .cloned()
            .map(|record| (record, "/payload/item/result/content".into()));
    }
    let (name, arguments, output) = match item_type {
        "CommandExecution" => {
            let arguments = serde_json::json!({"command": command.unwrap()});
            let output = item["aggregated_output"].as_str().and_then(|text| {
                let result = ToolResult {
                    source: None,
                    content: text.into(),
                    structured: Value::Null,
                    error: tool::result_state("", item, false).is_error,
                };
                match super::tools::merged_shell_output(&ToolData {
                    input: &arguments,
                    result: Some(&result),
                }) {
                    Some(Body::Sections(sections)) => Some(sections),
                    Some(Body::Hidden) => unreachable!("shell output cannot be hidden"),
                    None => None,
                }
            });
            ("exec_command".to_owned(), arguments, output)
        }
        "FileChange" => {
            let changes = item["changes"].as_object().unwrap();
            let output = changes
                .iter()
                .filter_map(|(path, change)| {
                    Some(Section::Diff {
                        lang: crate::providers::tool::file_language(path).to_owned(),
                        label: String::new(),
                        text: file_change_diff(change)?,
                    })
                })
                .collect();
            (
                "apply_patch".to_owned(),
                serde_json::json!({"paths": changes.keys().cloned().collect::<Vec<_>>().join(" · ")}),
                Some(output),
            )
        }
        "McpToolCall" => {
            let name = format!(
                "mcp__{}__{}",
                item["server"].as_str().unwrap(),
                item["tool"].as_str().unwrap()
            );
            (name, item["arguments"].clone(), None)
        }
        "Extension" if item["kind"] == "image_gen.generation" => {
            let mut arguments = item.clone();
            arguments.as_object_mut().unwrap().remove("result");
            let output = item["result"].as_str().zip(record).map(|(_, record)| {
                vec![Section::Image(Image {
                    label: "[image]".into(),
                    source: ImageSource::Record {
                        record: record.clone(),
                        pointer: "/payload/item/result".into(),
                    },
                })]
            });
            (item_type.to_owned(), arguments, output)
        }
        "ImageView" => (
            item_type.to_owned(),
            item.clone(),
            item["path"]
                .as_str()
                .and_then(image_file)
                .map(|image| vec![Section::Image(image)]),
        ),
        _ => (item_type.to_owned(), item.clone(), None),
    };
    let raw_name = name;
    let presentation = tool::presentation(
        &raw_name,
        &ToolData {
            input: &arguments,
            result: normalized.as_ref(),
        },
    );
    let subject = presentation.title;
    let (table_output, hide_output) = presentation_output(presentation.output, true);
    let output = if item_type == "McpToolCall" {
        table_output
    } else {
        output
    };
    let state = tool::result_state(
        "",
        item,
        normalized.as_ref().is_some_and(|result| result.error),
    );
    let exit_code = state.exit_code;
    let mut call = ToolCall {
        agent_message: None,
        title: subject.clone().unwrap_or_default(),
        show_event_detail: true,
        id: item["id"].as_str().unwrap_or("").to_owned(),
        summary: list_title(&raw_name, subject.as_deref(), &arguments, exit_code),
        name: raw_name.clone(),
        started_at: None,
        hide_output,
        model_settings: None,
        agent_thread_id: None,
        agent_path: None,
        input: presentation.input,
        // Completed items can have no result body; an empty output still marks completion.
        output: Some(output.unwrap_or_default()),
        exit_code,
        is_error: state.is_error,
        duration: None,
    };
    crate::providers::tool::annotate(&mut call);
    call
}

// Codex records an added or deleted file as its whole content, not a diff.
fn file_change_diff(change: &Value) -> Option<String> {
    let sign = match change["type"].as_str()? {
        "add" => '+',
        "delete" => '-',
        _ => return change["unified_diff"].as_str().map(str::to_owned),
    };
    let lines: Vec<&str> = change["content"].as_str()?.lines().collect();
    let range = match sign {
        '+' => format!("@@ -0,0 +1,{} @@", lines.len()),
        _ => format!("@@ -1,{} +0,0 @@", lines.len()),
    };
    Some(
        std::iter::once(range)
            .chain(lines.iter().map(|line| format!("{sign}{line}")))
            .collect::<Vec<_>>()
            .join("\n"),
    )
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

fn completed_tool(
    record: &Value,
    source: Option<&crate::event::SourceRecord>,
) -> io::Result<Option<Event>> {
    let payload = &record["payload"];
    if record["type"] != "event_msg" || payload["type"] != "item_completed" {
        return Ok(None);
    }
    let item = &payload["item"];
    let item_type = required_text(item, "type")?;
    let command_text = match item_type {
        "CommandExecution" => {
            optional::<i32>(item, "exit_code")?;
            let command: Vec<String> = field(item, "command")?;
            Some(
                if command.len() == 3 && matches!(command[1].as_str(), "-lc" | "-c") {
                    command[2].clone()
                } else {
                    command.join(" ")
                },
            )
        }
        "FileChange" => {
            if !item["changes"].is_object() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Codex file change requires changes",
                ));
            }
            None
        }
        "McpToolCall" => {
            required_text(item, "server")?;
            required_text(item, "tool")?;
            None
        }
        "Extension" => {
            match required_text(item, "kind")? {
                "web.search" => {
                    required_text(item, "query")?;
                }
                "image_gen.generation" => {
                    optional::<String>(item, "revisedPrompt")?;
                }
                _ => {}
            }
            None
        }
        "ImageView" => {
            required_text(item, "path")?;
            None
        }
        "SubAgentActivity" => {
            required_text(item, "kind")?;
            required_text(item, "agent_path")?;
            None
        }
        "CollabAgentToolCall" => {
            required_text(item, "tool")?;
            if item.get("receiver_thread_ids").is_some() {
                field::<Vec<String>>(item, "receiver_thread_ids")?;
            }
            None
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
    let call = completed_call(item, command_text, source);
    let body = call.summary.clone();
    let paths = if item_type == "FileChange" && !call.detail_left().is_empty() {
        let mut offset = call.detail_left().len() + 1;
        call.title
            .split(" · ")
            .map(|path| {
                let range = offset..offset + path.len();
                offset += path.len() + " · ".len();
                range
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(Some(Event {
        turn: None,
        completed_time: true,
        background: false,
        source: None,
        source_records: Vec::new(),
        sender: None,
        detail_body: None,
        kind: tool::result_kind(Kind::Tool, &tool::result_state("", item, call.is_error)),
        body,
        paths,
        started_at,
        ended_at: Some(ended_at),
        duration,
        previous_end: None,
        images: Vec::new(),
        calls: vec![call],
    }))
}

/// Read only through the SQLite prompt and its following completed item.
/// Prompt text itself comes from SQLite; rollout records supply attachments.
#[cfg(test)]
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
        let completed = lines
            .next()
            .transpose()?
            .and_then(|line| serde_json::from_slice::<Value>(&line).ok())
            .filter(completed_user);
        return Ok((
            prompt_images(
                &prompt,
                payload,
                completed.as_ref(),
                crate::event::SourceRecord {
                    path: path.to_owned(),
                    offset: start,
                },
            ),
            Some(source(&record, start)),
        ));
    }
    Ok((Vec::new(), None))
}

fn first_prompt_records<'a>(
    path: &Path,
    records: impl IntoIterator<Item = (u64, &'a Value)>,
    line_ends: &HashMap<u64, u64>,
    first_user_message: &str,
) -> (Vec<Image>, Option<String>) {
    let mut records = records.into_iter();
    while let Some((offset, record)) = records.next() {
        if (record["type"] == "session_meta"
            && record["payload"]
                .get("subagent_history_start_ordinal")
                .is_some())
            || completed_user(record)
        {
            return (Vec::new(), None);
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
        if !prompt
            .body()
            .is_some_and(|text| text.chars().take(512).eq(first_user_message.chars()))
        {
            continue;
        }
        let completed = records
            .next()
            .filter(|(next, _)| line_ends.get(&offset) == Some(next))
            .map(|(_, record)| record)
            .filter(|record| completed_user(record));
        let images = prompt_images(
            &prompt,
            payload,
            completed,
            crate::event::SourceRecord {
                path: path.to_owned(),
                offset,
            },
        );
        return (images, Some(source(record, offset)));
    }
    (Vec::new(), None)
}

/// Uses the already decoded tail, expanding only until the last call is paired.
fn latest_activity_records(
    path: &Path,
    records: &[(u64, Value)],
    metadata: Option<&Value>,
) -> io::Result<Option<Event>> {
    let mut start = records.partition_point(|(offset, _)| {
        *offset
            < records
                .last()
                .map_or(0, |(offset, _)| offset.saturating_sub(CHUNK))
    });
    let mut requested = 16;
    loop {
        let entries = events_with_context(path, &records[start..], usize::MAX, None, metadata)?;
        if start == 0
            || (entries.len() > requested && !missing_assistant_context(&records[start..]))
        {
            if let Some((index, (_, event))) = entries
                .iter()
                .enumerate()
                .rev()
                .find(|(_, (_, event))| !event.calls.is_empty())
                && (index > 0 || start == 0)
            {
                return Ok(Some(event.clone()));
            }
            if start == 0 {
                return Ok(None);
            }
            requested = entries.len().saturating_add(16);
        }
        let target = records[start].0.saturating_sub(CHUNK);
        let next = records[..start]
            .partition_point(|(offset, _)| *offset < target)
            .min(start - 1);
        debug_assert!(next < start, "summary tail did not advance");
        start = next;
    }
}

/// Read the same parsed rows as the event list, including call/result and message pairing.
#[cfg(test)]
fn latest_activity(path: &Path) -> io::Result<Option<Event>> {
    let mut reader = Reader::new(path.to_path_buf(), false);
    let mut requested = 16;
    loop {
        reader.refresh(requested)?;
        if let Some((index, (_, event))) = reader
            .entries
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (_, event))| !event.calls.is_empty())
            && (index > 0 || reader.oldest == 0)
        {
            return Ok(Some(event.clone()));
        }
        if reader.oldest == 0 {
            return Ok(None);
        }
        requested = reader.entries.len().saturating_add(16);
    }
}

#[cfg(test)]
pub(super) fn summary(path: &Path) -> io::Result<Summary> {
    let latest = latest_activity(path)?;
    let mut records = Vec::new();
    walk(path, |record, offset| {
        records.push((offset, record));
        Ok(true)
    })?;
    summary_records(
        records.iter().map(|(offset, record)| (record, *offset)),
        latest.as_ref(),
    )
}

fn summary_records<'a>(
    records: impl IntoIterator<Item = (&'a Value, u64)>,
    latest: Option<&Event>,
) -> io::Result<Summary> {
    let mut summary = Summary::default();
    if let Some(event) = latest {
        summary.task_details_sources[0] = event.source.clone();
        summary.activity = Some(event.activity());
    }
    let mut boundary = false;
    let mut message_source_closed = false;
    let mut message_pending_completion = false;
    let mut outputs = HashSet::new();
    let mut pending = Vec::new();
    let mut cache_response_seen = false;
    let mut cache_response_id = None;
    let mut cache_request_at = None;
    let mut cache_resolved = false;
    for (record, offset) in records {
        let Some(at) = timestamp(record) else {
            continue;
        };
        summary.updated_at.get_or_insert(at);
        let payload = &record["payload"];
        if !cache_resolved {
            if record["type"] == "event_msg" && payload["type"] == "raw_response_completed" {
                let response_id = payload["response_id"].as_str().map(str::to_owned);
                if !cache_response_seen {
                    cache_response_seen = true;
                    cache_response_id = response_id;
                } else if response_id.is_some() && response_id == cache_response_id {
                    // A repeated completion belongs to the same request; find
                    // its boundary before the first recorded completion.
                    cache_request_at = None;
                } else {
                    cache_resolved = true;
                }
            } else if cache_response_seen {
                if record["type"] == "response_item"
                    && matches!(
                        payload["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    )
                {
                    cache_request_at.get_or_insert(at);
                } else if record["type"] == "event_msg" && payload["type"] == "task_started" {
                    cache_request_at.get_or_insert(at);
                    cache_resolved = true;
                }
            }
        }
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
                        summary
                            .pending_question_call
                            .get_or_insert_with(|| (id.to_owned(), at));
                        pending.push(questions(&args)?);
                    }
                }
                _ => {}
            }
        }
        if record["type"] == "event_msg" && payload["type"] == "task_started" {
            summary.prompt_at.get_or_insert(at);
            message_source_closed = true;
            message_pending_completion = false;
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
        let message = assistant_text(record)?;
        let message_source = assistant_source(record, offset);
        // Both stored forms share a message id. A turn can contain several
        // replies, so only the completion for the selected id can replace it.
        let selected_completion = completed_assistant(record)
            && message_source.is_some()
            && message_source == summary.task_details_sources[1];
        let resolves_source = !message_source_closed
            && summary.task_details_sources[1].is_none()
            && message_source.is_some();
        if let Some(text) = message.as_ref()
            && (summary.last_message.is_none() || resolves_source || selected_completion)
        {
            summary.task_details_sources[1] = message_source.clone();
            summary.last_message = Some(printable(text));
            summary.last_message_at = Some(at);
            message_pending_completion = message_source.is_some() && !completed_assistant(record);
        }
        // Completion notifications repeat the reply but have no event row. Keep
        // the identity of the latest message record in that turn instead.
        if summary.task_details_sources[1].is_none() && !message_source_closed && message.is_some()
        {
            summary.task_details_sources[1] = assistant_source(record, offset);
        }
        if boundary
            && cache_resolved
            && summary.prompt_at.is_some()
            && summary.last_message.is_some()
            && summary.activity.is_some()
            && !message_pending_completion
            && (summary.task_details_sources[1].is_some() || message_source_closed)
        {
            break;
        }
    }
    summary.cache_expires_at = cache_request_at.map(|at| at + chrono::Duration::minutes(30));
    summary.pending_questions = pending.into_iter().rev().flatten().collect();
    Ok(summary)
}

/// Retain only fields read by summary, event pairing and first-prompt selection.
/// Raw response bodies and injected context can repeat the whole conversation.
fn summary_record(mut record: Value) -> Value {
    if record["type"] == "session_meta" {
        return record;
    }
    let payload = &record["payload"];
    let context = record["type"] == "response_item"
        && payload["type"] == "message"
        && payload["role"] == "user"
        && is_context_message(payload);
    let keep_payload = event_candidate(&record)
        || matches!(
            (record["type"].as_str(), payload["type"].as_str()),
            (
                Some("response_item"),
                Some("function_call_output" | "custom_tool_call_output")
            ) | (
                Some("event_msg"),
                Some("task_started" | "task_complete" | "agent_message")
            )
        )
        || matches!(
            payload["item"]["type"].as_str(),
            Some("SubAgentActivity" | "CollabAgentToolCall")
        );
    let cache = record["type"] == "event_msg" && payload["type"] == "raw_response_completed";
    if let Some(payload) = record.get_mut("payload").and_then(Value::as_object_mut) {
        if context {
            payload.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "type" | "role" | "internal_chat_message_metadata_passthrough"
                )
            });
            if let Some(metadata) = payload
                .get_mut("internal_chat_message_metadata_passthrough")
                .and_then(Value::as_object_mut)
            {
                metadata.retain(|key, _| key == "content_item_kinds");
            }
        } else if !keep_payload {
            payload.retain(|key, _| key == "type" || (cache && key == "response_id"));
        }
    }
    if let Some(record) = record.as_object_mut() {
        record
            .retain(|key, _| matches!(key.as_str(), "type" | "timestamp" | "ordinal" | "payload"));
    }
    record
}

/// Parsed rollout records and the byte range retained for backward paging.
pub(crate) struct Reader {
    path: PathBuf,
    identity: Option<(u64, u64)>,
    history_start: Option<u64>,
    metadata: Option<Value>,
    observed_len: u64,
    modified: Option<std::time::SystemTime>,
    oldest: u64,
    read: u64,
    pending_offset: Option<u64>,
    pending_bytes: Vec<u8>,
    records: Vec<(u64, Value)>,
    line_ends: HashMap<u64, u64>,
    entries: Vec<(u64, Event)>,
    dependencies: HashMap<String, u64>,
    image_hashes: ImageHashes,
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
            metadata: None,
            observed_len: 0,
            modified: None,
            oldest: 0,
            read: 0,
            pending_offset: None,
            pending_bytes: Vec::new(),
            records: Vec::new(),
            line_ends: HashMap::new(),
            entries: Vec::new(),
            dependencies: HashMap::new(),
            image_hashes: ImageHashes::new(),
            latest_prompt: None,
            shown: 0,
            boundary: crate::jsonl::Boundary::default(),
        }
    }

    /// The session summary and plan share one decoded forward stream.
    fn read_summary(&mut self) -> io::Result<crate::event::Update> {
        use std::os::unix::fs::MetadataExt;
        let mut file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let modified = metadata.modified()?;
        let reset = self.identity != Some(identity)
            || metadata.len() < self.observed_len
            || (metadata.len() == self.observed_len && self.modified != Some(modified))
            || !self.boundary.matches(&mut file)?;
        if reset {
            self.history_start = None;
            self.metadata = None;
            self.line_ends.clear();
            self.records.clear();
            self.read = 0;
            self.pending_offset = None;
            self.pending_bytes.clear();
        }
        let pending = self.pending_offset.take();
        let pending_bytes = std::mem::take(&mut self.pending_bytes);
        let mut pending_record = pending.and_then(|offset| {
            self.records.last().filter(|(last, _)| *last == offset)?;
            self.records.pop().map(|(_, record)| record)
        });
        let mut offset = self.read;
        let mut parsed_bytes = 0;
        let mut invalid_history = None;
        self.read = crate::jsonl::read_lines(&mut file, self.read, |line| {
            let record = if pending == Some(offset) && line == pending_bytes {
                pending_record.take()
            } else {
                parsed_bytes += line.len() as u64 + 1;
                serde_json::from_slice::<Value>(line).ok()
            };
            if let Some(record) = record {
                if offset == 0 {
                    self.metadata = Some(record.clone());
                    match history_from_record(&record) {
                        Ok(history) => self.history_start = history,
                        Err(error) => invalid_history = Some(error),
                    }
                }
                if !inherited(&record, self.history_start) {
                    self.line_ends
                        .insert(offset, offset + line.len() as u64 + 1);
                    self.records.push((offset, summary_record(record)));
                }
            }
            offset += line.len() as u64 + 1;
        })?;
        if let Some(error) = invalid_history {
            return Err(error);
        }
        parsed_bytes += self.read_pending(&mut file)?;
        if let Some((offset, record)) = self.records.last()
            && Some(*offset) == self.pending_offset
        {
            self.line_ends.insert(*offset, metadata.len());
            if *offset == 0 {
                self.metadata = Some(record.clone());
                self.history_start = history_from_record(record)?;
            }
        }
        if let Some((offset, record)) = self.records.last_mut()
            && Some(*offset) == self.pending_offset
        {
            *record = summary_record(std::mem::take(record));
        }
        self.records
            .retain(|(_, record)| !inherited(record, self.history_start));
        self.identity = Some(identity);
        self.modified = Some(modified);
        self.observed_len = metadata.len();
        self.boundary.capture(&mut file, self.read)?;
        Ok(crate::event::Update {
            reset,
            parsed_bytes,
            events: Vec::new(),
            appended: 0,
            replaced: 0,
            prepended: 0,
            has_older: false,
            oldest_offset: 0,
            read_offset: self.read,
        })
    }

    fn dependency(
        entries: &[(u64, Event)],
        dependencies: &mut HashMap<String, u64>,
        image_hashes: &mut ImageHashes,
        latest_prompt: &mut Option<u64>,
        record: &Value,
        offset: u64,
    ) -> u64 {
        let payload = &record["payload"];
        let mut affected = offset;
        if record["type"] == "response_item"
            && (payload["type"] == "agent_message"
                || payload["name"]
                    .as_str()
                    .and_then(|name| {
                        crate::event::agent::tool_message(
                            name,
                            &tool_arguments(payload).unwrap_or(Value::Null),
                        )
                    })
                    .is_some_and(|message| message.kind == "MESSAGE"))
            && let Some((previous_offset, previous)) = entries.last()
            && previous.calls.first().is_some_and(|call| {
                call.agent_message
                    .as_ref()
                    .is_some_and(|message| message.kind == "MESSAGE")
            })
        {
            affected = affected.min(*previous_offset);
        }
        for id in [
            payload["id"].as_str(),
            payload["call_id"].as_str(),
            payload["item"]["id"].as_str(),
        ]
        .into_iter()
        .flatten()
        {
            affected = affected.min(
                *dependencies
                    .entry(id.to_owned())
                    .and_modify(|previous| *previous = (*previous).min(offset))
                    .or_insert(offset),
            );
        }
        for hash in image_hashes
            .entry(offset)
            .or_insert_with(|| RecordImages::new(record))
            .hashes()
        {
            affected = affected.min(
                *dependencies
                    .entry(format!("image:{hash:02x?}"))
                    .and_modify(|previous| *previous = (*previous).min(offset))
                    .or_insert(offset),
            );
        }
        if completed_user(record) {
            affected = affected.min(latest_prompt.unwrap_or(offset));
        }
        if record["type"] == "response_item"
            && payload["type"] == "message"
            && payload["role"] == "user"
        {
            *latest_prompt = Some(latest_prompt.map_or(offset, |previous| previous.max(offset)));
        }
        affected
    }

    fn read_pending(&mut self, file: &mut File) -> io::Result<u64> {
        file.seek(SeekFrom::Start(self.read))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if let Ok(record) = serde_json::from_slice::<Value>(&bytes) {
            if !inherited(&record, self.history_start) {
                self.image_hashes.remove(&self.read);
                self.pending_offset = Some(self.read);
                self.pending_bytes = bytes.clone();
                self.records.push((self.read, record));
            }
            Ok(bytes.len() as u64)
        } else {
            Ok(0)
        }
    }

    pub fn refresh(&mut self, requested: usize) -> io::Result<crate::event::Update> {
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
            self.metadata = read_metadata(&mut file)?;
            self.history_start = self
                .metadata
                .as_ref()
                .map(history_from_record)
                .transpose()?
                .flatten();
            self.read = crate::jsonl::complete_end(&mut file)?;
            self.oldest = self.read;
            self.records.clear();
            self.entries.clear();
            self.dependencies.clear();
            self.image_hashes.clear();
            self.latest_prompt = None;
            self.shown = 0;
            self.pending_offset = None;
            parsed_bytes += self.read_pending(&mut file)?;
            self.entries = events_with_images(
                &self.path,
                &self.records,
                usize::MAX,
                None,
                self.metadata.as_ref(),
                &mut self.image_hashes,
            )?;
        } else if changed && metadata.len() > self.read {
            if self.read == 0 {
                self.metadata = read_metadata(&mut file)?;
                self.history_start = self
                    .metadata
                    .as_ref()
                    .map(history_from_record)
                    .transpose()?
                    .flatten();
            }
            let pending = self.pending_offset.take();
            let pending_bytes = std::mem::take(&mut self.pending_bytes);
            let mut pending_record = pending.and_then(|offset| {
                self.records
                    .last()
                    .filter(|(last_offset, _)| *last_offset == offset)?;
                self.records.pop().map(|(_, record)| record)
            });
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
                    if let Some(record) = pending_record.take() {
                        incoming.push((offset, record));
                        reused += line.len() as u64;
                    }
                } else if let Ok(record) = serde_json::from_slice::<Value>(line) {
                    self.image_hashes.remove(&offset);
                    incoming.push((offset, record));
                }
                offset += line.len() as u64 + 1;
            })?;
            parsed_bytes += end - self.read - reused;
            self.read = end;
            incoming.retain(|(_, record)| !inherited(record, self.history_start));
            let mut affected = pending.unwrap_or(end);
            for (offset, record) in &incoming {
                affected = affected.min(Self::dependency(
                    &self.entries,
                    &mut self.dependencies,
                    &mut self.image_hashes,
                    &mut self.latest_prompt,
                    record,
                    *offset,
                ));
            }
            self.records.extend(incoming);
            parsed_bytes += self.read_pending(&mut file)?;
            if let Some((offset, record)) = self.records.last() {
                affected = affected.min(Self::dependency(
                    &self.entries,
                    &mut self.dependencies,
                    &mut self.image_hashes,
                    &mut self.latest_prompt,
                    record,
                    *offset,
                ));
            }
            let prefix = self
                .entries
                .partition_point(|(offset, _)| *offset < affected);
            let start = self
                .records
                .partition_point(|(offset, _)| *offset < affected);
            let mut suffix = events_with_images(
                &self.path,
                &self.records[start..],
                usize::MAX,
                None,
                self.metadata.as_ref(),
                &mut self.image_hashes,
            )?;
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
        let mut context_minimum = 0;
        while self.oldest > 0
            && (self.entries.len() <= requested || missing_assistant_context(&self.records))
        {
            let end = self.oldest;
            let mut older = Vec::new();
            let mut candidates = 0usize;
            // Delay context scans until enough candidates have arrived. If pairing or
            // merging needs another batch, double its record range to bound rescans.
            loop {
                let end = self.oldest;
                let target = end.saturating_sub(CHUNK);
                let mut oldest = end;
                crate::jsonl::read_lines_back(&mut file, end, |offset, line| {
                    oldest = offset;
                    parsed_bytes += line.len() as u64 + 1;
                    if let Ok(record) = serde_json::from_slice::<Value>(line) {
                        if inherited(&record, self.history_start) {
                            oldest = 0;
                            return false;
                        }
                        candidates += usize::from(event_candidate(&record));
                        older.push((offset, record));
                    }
                    offset > target
                })?;
                self.oldest = oldest;
                if oldest == 0
                    || (candidates > requested.saturating_sub(self.entries.len())
                        && older.len() + self.records.len() >= context_minimum)
                {
                    break;
                }
            }
            older.reverse();
            let added = older.len();
            let mut selected: HashSet<_> = older.iter().map(|(offset, _)| *offset).collect();
            let ids: HashSet<_> = older
                .iter()
                .flat_map(|(_, record)| record_ids(record).map(str::to_owned))
                .collect();
            for (offset, record) in &older {
                self.image_hashes
                    .entry(*offset)
                    .or_insert_with(|| RecordImages::new(record));
            }
            let images: HashSet<_> = older
                .iter()
                .flat_map(|(offset, _)| self.image_hashes[offset].hashes().copied())
                .collect();
            older.append(&mut self.records);
            self.records = older;
            // Rebuild retained rows when either half of a paired event arrives,
            // including every member of a merged message group.
            for (offset, record) in &self.records[added..] {
                if record_ids(record).any(|id| ids.contains(id))
                    || self.image_hashes[offset]
                        .hashes()
                        .any(|hash| images.contains(hash))
                {
                    selected.insert(*offset);
                }
            }
            self.entries.retain(|(offset, event)| {
                if selected.contains(offset)
                    || event
                        .source_records
                        .iter()
                        .any(|record| selected.contains(&record.offset))
                {
                    selected.extend(event.source_records.iter().map(|record| record.offset));
                    false
                } else {
                    true
                }
            });
            let incoming = events_with_images(
                &self.path,
                &self.records,
                usize::MAX,
                Some(&selected),
                self.metadata.as_ref(),
                &mut self.image_hashes,
            )?;
            self.entries.extend(incoming);
            merge_entries(&self.records, &mut self.entries);
            for (offset, record) in &self.records[..added] {
                Self::dependency(
                    &self.entries,
                    &mut self.dependencies,
                    &mut self.image_hashes,
                    &mut self.latest_prompt,
                    record,
                    *offset,
                );
            }
            context_minimum = self.records.len().saturating_mul(2);
            debug_assert!(self.oldest < end);
        }
        if parsed_bytes > parsed_before_older {
            replaced = replaced.max(old_shown);
        }
        if changed || parsed_bytes > 0 {
            let turns = crate::activity::EventTurns::new(
                crate::providers::ProviderId::Codex,
                self.records
                    .iter()
                    .map(|(offset, record)| (*offset, record)),
            );
            for (offset, event) in &mut self.entries {
                let turn = turns.at(*offset);
                if event.turn != turn {
                    replaced = replaced.max(old_shown);
                    event.turn = turn;
                }
            }
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
        let cut_turn = skip
            .checked_sub(1)
            .and_then(|index| self.entries[index].1.turn.map(|turn| turn.id));
        let events = self.entries[skip..]
            .iter()
            .map(|(_, event)| {
                let mut event = event.clone();
                if let Some(turn) = &mut event.turn
                    && Some(turn.id) == cut_turn
                {
                    turn.started = false;
                }
                event
            })
            .collect();
        self.boundary.capture(&mut file, metadata.len())?;
        Ok(crate::event::Update {
            events,
            prepended,
            appended: if reset { 0 } else { appended },
            replaced,
            reset,
            parsed_bytes,
            has_older: self.oldest > 0 || skip > 0,
            oldest_offset: self.oldest,
            read_offset: self.read,
        })
    }
}

fn event_candidate(record: &Value) -> bool {
    let payload = &record["payload"];
    matches!(
        (record["type"].as_str(), payload["type"].as_str()),
        (
            Some("response_item"),
            Some("message" | "agent_message" | "function_call" | "custom_tool_call")
        ) | (Some("event_msg"), Some("turn_aborted" | "item_completed"))
    )
}

fn merge_entries(records: &[(u64, Value)], entries: &mut Vec<(u64, Event)>) {
    entries.sort_by_key(|(offset, _)| *offset);
    let turns = crate::activity::EventTurns::new(
        crate::providers::ProviderId::Codex,
        records.iter().map(|(offset, record)| (*offset, record)),
    );
    let offsets: HashMap<_, _> = entries
        .iter_mut()
        .map(|(offset, event)| {
            event.turn = turns.at(*offset);
            (event.source.clone(), *offset)
        })
        .collect();
    let events = crate::event::agent::merge(
        std::mem::take(entries)
            .into_iter()
            .map(|(_, event)| event)
            .collect(),
    );
    let mut previous_end = None;
    *entries = events
        .into_iter()
        .map(|mut event| {
            event.source_records.sort_by_key(|record| record.offset);
            event.source_records.dedup();
            event.previous_end = previous_end;
            previous_end = Some(event.ended_at.unwrap_or(event.started_at));
            (offsets[&event.source], event)
        })
        .collect();
}

/// A page starting inside a reply must include its completion or turn boundary.
fn missing_assistant_context(records: &[(u64, Value)]) -> bool {
    let mut pending = HashSet::new();
    let mut completed = HashSet::new();
    let mut boundary = false;
    for (_, record) in records {
        let payload = &record["payload"];
        if completed_assistant(record) {
            if let Some(id) = payload["item"]["id"].as_str() {
                completed.insert(id);
            }
        } else if (record["type"] == "event_msg" && payload["type"] == "task_started")
            || (record["type"] == "response_item"
                && payload["type"] == "message"
                && payload["role"] == "user")
        {
            boundary = true;
        } else if !boundary
            && record["type"] == "response_item"
            && payload["type"] == "message"
            && payload["role"] == "assistant"
            && let Some(id) = payload["id"].as_str()
        {
            pending.insert(id);
        }
    }
    pending.iter().any(|id| !completed.contains(id))
}

impl crate::event::Reader for Reader {
    fn refresh(&mut self, requested: usize) -> io::Result<crate::event::Update> {
        Reader::refresh(self, requested)
    }

    fn read_events(
        &mut self,
        groups: &[Vec<crate::event::SourceRecord>],
    ) -> io::Result<Vec<Option<Event>>> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(vec![None; groups.len()]);
            }
            Err(error) => return Err(error),
        };
        let metadata = read_metadata(&mut file)?;
        Ok(groups
            .iter()
            .map(|group| {
                let decode = || -> io::Result<Option<Event>> {
                    if group.is_empty() || group.iter().any(|record| record.path != self.path) {
                        return Ok(None);
                    }
                    let mut file = io::BufReader::new(std::fs::File::open(&self.path)?);
                    let mut records = Vec::with_capacity(group.len());
                    for record in group {
                        file.seek(SeekFrom::Start(record.offset))?;
                        let mut line = Vec::new();
                        file.read_until(b'\n', &mut line)?;
                        records.push((
                            record.offset,
                            serde_json::from_slice::<serde_json::Value>(&line)?,
                        ));
                    }
                    records.sort_by_key(|(offset, _)| *offset);
                    records.dedup_by_key(|(offset, _)| *offset);
                    let selected = records.iter().map(|(offset, _)| *offset).collect();
                    let events = events_with_context(
                        &self.path,
                        &records,
                        usize::MAX,
                        Some(&selected),
                        metadata.as_ref(),
                    )?
                    .into_iter()
                    .map(|(_, mut event)| {
                        event.turn = None;
                        event.previous_end = None;
                        event
                    })
                    .collect();
                    let mut events = crate::event::agent::merge(events);
                    if events.len() != 1 {
                        return Ok(None);
                    }
                    Ok(Some(events.remove(0)))
                };
                decode().ok().flatten()
            })
            .collect())
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

fn memory_notes(item: &Value) -> io::Result<Vec<crate::event::Note>> {
    let Some(citation) = item.get("memory_citation").filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        path: PathBuf,
        line_start: u64,
        line_end: u64,
        note: String,
    }
    #[derive(serde::Deserialize)]
    struct Citation {
        entries: Vec<Entry>,
    }
    let citation: Citation = serde_json::from_value(citation.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if citation.entries.is_empty() {
        return Ok(Vec::new());
    }
    let home = match std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?,
        )
        .join(".codex"),
    };
    citation
        .entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if entry.line_start == 0
                || entry.line_end < entry.line_start
                || entry.path.as_os_str().is_empty()
                || entry
                    .path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid Codex memory citation",
                ));
            }
            let location = if entry.line_start == entry.line_end {
                format!("{}:{}", entry.path.display(), entry.line_start)
            } else {
                format!(
                    "{}:{}-{}",
                    entry.path.display(),
                    entry.line_start,
                    entry.line_end
                )
            };
            let prefix = if index == 0 { "Memory: " } else { "        " };
            let end = prefix.len() + location.len();
            Ok(crate::event::Note {
                text: format!("{prefix}{location} · {}", entry.note),
                links: vec![crate::event::FileLink {
                    range: prefix.len()..end,
                    path: home.join("memories").join(entry.path),
                    line: entry.line_start,
                }],
            })
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    static EVENT_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn events_selected(
    path: &Path,
    records: &[(u64, Value)],
    count: usize,
    selected: &HashSet<u64>,
) -> io::Result<Vec<(u64, Event)>> {
    let metadata = read_metadata(&mut File::open(path)?)?;
    events_with_context(path, records, count, Some(selected), metadata.as_ref())
}

#[cfg(test)]
fn events(path: &Path, records: &[(u64, Value)], count: usize) -> io::Result<Vec<(u64, Event)>> {
    let metadata = read_metadata(&mut File::open(path)?)?;
    events_with_context(path, records, count, None, metadata.as_ref())
}

/// Converts selected records while borrowing every retained record for pairing and deduplication.
/// Selected entries stay unmerged so callers can combine them with retained entries first.
fn events_with_context(
    path: &Path,
    records: &[(u64, Value)],
    count: usize,
    selected: Option<&HashSet<u64>>,
    metadata: Option<&Value>,
) -> io::Result<Vec<(u64, Event)>> {
    events_with_images(
        path,
        records,
        count,
        selected,
        metadata,
        &mut ImageHashes::new(),
    )
}

fn events_with_images<'a>(
    path: &Path,
    records: &'a [(u64, Value)],
    count: usize,
    selected: Option<&HashSet<u64>>,
    metadata: Option<&Value>,
    image_hashes: &mut ImageHashes,
) -> io::Result<Vec<(u64, Event)>> {
    let sender = metadata
        .as_ref()
        .filter(|record| record["type"] == "session_meta")
        .and_then(|record| record["payload"]["agent_path"].as_str())
        .unwrap_or("/root");
    let turns = crate::activity::EventTurns::new(
        crate::providers::ProviderId::Codex,
        records.iter().map(|(offset, record)| (*offset, record)),
    );
    let mut entries = Vec::new();
    let mut results = HashMap::new();
    let mut seen = HashSet::new();
    let mut completed_prompt = None;
    let mut completed_messages = HashMap::new();
    let mut collaboration = HashMap::new();
    let mut sources: HashMap<&str, Vec<u64>> = HashMap::new();
    let views = image_views(path, records);
    for (offset, record) in records {
        image_hashes
            .entry(*offset)
            .or_insert_with(|| RecordImages::new(record));
    }
    let generated: HashSet<_> = records
        .iter()
        .filter(|(_, record)| record["payload"]["item"]["kind"] == "image_gen.generation")
        .filter_map(|(offset, _)| image_hashes[offset].generated)
        .collect();
    let completed_mcp: HashSet<_> = records
        .iter()
        .filter(|(_, record)| record["payload"]["item"]["type"] == "McpToolCall")
        .filter_map(|(_, record)| record["payload"]["item"]["id"].as_str())
        .collect();
    for (offset, record) in records {
        for id in record_ids(record) {
            sources.entry(id).or_default().push(*offset);
        }
        let item = &record["payload"]["item"];
        if matches!(
            item["type"].as_str(),
            Some("SubAgentActivity" | "CollabAgentToolCall")
        ) && let Some(id) = item["id"].as_str()
        {
            collaboration.insert(id.to_owned(), (*offset, record));
        }
        if completed_assistant(record) {
            let id = required_text(&record["payload"]["item"], "id")?;
            completed_messages.insert(id, (*offset, record));
        }
    }
    let mut visit = |record: &'a Value, offset: u64| -> io::Result<bool> {
        let Some(at) = timestamp(record) else {
            return Ok(true);
        };
        if completed_user(record) {
            completed_prompt = Some((offset, record));
            return Ok(true);
        }
        let mut images = Vec::new();
        let mut calls = Vec::new();
        let detail_body = None;
        let reference = |offset| crate::event::SourceRecord {
            path: path.to_path_buf(),
            offset,
        };
        let mut source_records = vec![reference(offset)];
        // The raw response may be persisted after its processed completion.
        // Resolve by id before deduplication, independent of append order.
        let record = assistant_source(record, offset)
            .and_then(|id| completed_messages.get(id.as_str()))
            .map(|(completed_offset, completed)| {
                source_records.push(reference(*completed_offset));
                *completed
            })
            .unwrap_or(record);
        source_records.extend(
            record_ids(record)
                .filter_map(|id| sources.get(id))
                .flatten()
                .map(|offset| reference(*offset)),
        );
        let payload = &record["payload"];
        if selected.is_some_and(|selected| !selected.contains(&offset)) {
            match (record["type"].as_str(), payload["type"].as_str()) {
                (
                    Some("response_item"),
                    Some("function_call_output" | "custom_tool_call_output"),
                ) => {
                    if let Some(id) = payload["call_id"].as_str() {
                        results.insert(id.to_owned(), (at, &payload["output"], offset));
                    }
                }
                (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                    if payload["name"]
                        .as_str()
                        .is_some_and(|name| !tool::rule(name).wrapper && !tool::tool_hidden(name))
                        && let Some(id) = payload["call_id"].as_str()
                        && seen.insert(id.to_owned())
                    {
                        results.remove(id);
                    }
                }
                (Some("response_item"), Some("message"))
                    if (payload["role"] == "user" && !is_context_message(payload))
                        || payload["role"] == "assistant" =>
                {
                    if payload["role"] == "user" {
                        completed_prompt.take();
                    }
                    let content = &payload["content"];
                    let has_text = content.as_str().is_some_and(|text| !text.trim().is_empty())
                        || content.as_array().is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block["text"]
                                    .as_str()
                                    .is_some_and(|text| !text.trim().is_empty())
                            })
                        });
                    if has_text && let Some(id) = payload["id"].as_str() {
                        seen.insert(id.to_owned());
                    }
                }
                (Some("event_msg"), Some("item_completed"))
                    if matches!(
                        payload["item"]["type"].as_str(),
                        Some(
                            "AgentMessage"
                                | "CommandExecution"
                                | "FileChange"
                                | "McpToolCall"
                                | "Extension"
                                | "ImageView"
                        )
                    ) =>
                {
                    seen.insert(required_text(&payload["item"], "id")?.to_owned());
                }
                _ => {}
            }
            return Ok(true);
        }
        #[cfg(test)]
        EVENT_CONVERSIONS.with(|count| count.set(count.get() + 1));
        if matches!(
            payload["item"]["type"].as_str(),
            Some("SubAgentActivity" | "CollabAgentToolCall")
        ) {
            completed_tool(record, Some(&reference(offset)))?;
            return Ok(true);
        }
        if let Some(mut event) = completed_tool(record, Some(&reference(offset)))? {
            event.turn = turns.at(offset);
            event.source = Some(source(record, offset));
            if let Some((image, offsets)) = views.get(&offset) {
                event.calls[0].output = Some(vec![Section::Image(image.clone())]);
                source_records.extend(offsets.iter().map(|offset| reference(*offset)));
            }
            event.source_records = source_records;
            if !seen.insert(required_text(&payload["item"], "id")?.to_owned()) {
                return Ok(true);
            }
            entries.push((offset, event));
            return Ok(true);
        }
        let (kind, body, ended_at) = match (record["type"].as_str(), payload["type"].as_str()) {
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                if let Some(id) = payload["call_id"].as_str() {
                    results.insert(id.to_owned(), (at, &payload["output"], offset));
                }
                return Ok(true);
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                if !payload["name"]
                    .as_str()
                    .is_some_and(|name| !tool::rule(name).wrapper && !tool::tool_hidden(name))
                {
                    return Ok(true);
                }
                if payload["call_id"]
                    .as_str()
                    .is_some_and(|id| completed_mcp.contains(id))
                {
                    return Ok(true);
                }
                if let Some(id) = payload["call_id"].as_str()
                    && !seen.insert(id.to_owned())
                {
                    return Ok(true);
                }
                let image_output = payload["call_id"]
                    .as_str()
                    .and_then(|id| results.get(id))
                    .filter(|(_, output, _)| output_image_blocks(output).next().is_some())
                    .map(|(_, output, offset)| {
                        let source = reference(*offset);
                        let mut seen_images = Vec::new();
                        output
                            .as_array()
                            .unwrap()
                            .iter()
                            .enumerate()
                            .filter_map(|(index, block)| match block["type"].as_str()? {
                                "input_text" => Some(Section::Text {
                                    label: "output".into(),
                                    text: block["text"].as_str()?.to_owned(),
                                }),
                                "input_image" => {
                                    let url = block["image_url"].as_str()?;
                                    if seen_images.contains(&url) {
                                        return None;
                                    }
                                    seen_images.push(url);
                                    (!(payload["name"] == "wait"
                                        && image_hashes[offset]
                                            .output
                                            .get(&index)
                                            .is_some_and(|hash| generated.contains(hash))))
                                    .then(|| Section::Image(output_image(&source, index, url)))
                                }
                                _ => None,
                            })
                            .collect()
                    });
                let ended = payload["call_id"]
                    .as_str()
                    .and_then(|id| results.remove(id))
                    .map(|(at, output, offset)| {
                        let text = if let Some(text) = output.as_str() {
                            text.to_owned()
                        } else {
                            output
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|block| block["type"] == "input_text")
                                .filter_map(|block| block["text"].as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        };
                        (at, text, offset)
                    });
                let mut call =
                    response_tool_call(payload, ended.as_ref().map(|(_, text, _)| text.as_str()));
                if let Some(output) = image_output {
                    call.output = Some(output);
                    call.hide_output = false;
                }
                if let Some(message) = &mut call.agent_message {
                    message.author = metadata
                        .as_ref()
                        .and_then(|record| record["payload"]["agent_path"].as_str())
                        .map(str::to_owned);
                }
                crate::event::agent::present(&mut call);
                let body = call
                    .agent_message
                    .as_ref()
                    .map(|m| m.content.text().to_owned())
                    .unwrap_or_else(|| call.summary.clone());
                call.started_at = call.agent_path.as_ref().map(|_| at);
                let mut completed_at = ended.as_ref().map(|(at, _, _)| *at);
                if let Some((offset, record)) = payload["call_id"]
                    .as_str()
                    .and_then(|id| collaboration.get(id))
                {
                    source_records.push(reference(*offset));
                    let completion = &record["payload"];
                    let item = &completion["item"];
                    if item["type"] == "SubAgentActivity" && item["kind"] == "started" {
                        call.agent_thread_id = item["agent_thread_id"].as_str().map(str::to_owned);
                        call.agent_path = item["agent_path"].as_str().map(str::to_owned);
                        call.model_settings = Some(crate::event::ModelSettings {
                            provider: crate::providers::ProviderId::Codex,
                            model: item["model"].as_str().map(str::to_owned),
                            effort: item["reasoning_effort"].as_str().map(str::to_owned),
                        });
                    } else if item["type"] == "CollabAgentToolCall" {
                        let start = item_time(completion, "started_at_ms")?;
                        let end = item_time(completion, "completed_at_ms")?;
                        call.duration = Some(end - start);
                        completed_at = Some(end);
                    }
                }
                if let Some((_, _, offset)) = &ended {
                    source_records.insert(0, reference(*offset));
                }
                let is_error = call.is_error;
                calls.push(call);
                (
                    tool::result_kind(
                        tool_kind(payload["name"].as_str().unwrap()),
                        &tool::result_state(
                            ended
                                .as_ref()
                                .map(|(_, text, _)| text.as_str())
                                .unwrap_or(""),
                            &Value::Null,
                            is_error,
                        ),
                    ),
                    body,
                    completed_at,
                )
            }

            (Some("response_item"), Some("agent_message")) => {
                let header = content_text(&payload["content"]).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing Codex agent message header",
                    )
                })?;
                let (header, body) = header.split_once("\nPayload:\n").ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing Codex agent message payload",
                    )
                })?;
                let encrypted = body.trim().is_empty()
                    && payload["content"].as_array().is_some_and(|blocks| {
                        blocks
                            .iter()
                            .any(|block| block["type"] == "encrypted_content")
                    });
                let text = if encrypted {
                    ENCRYPTED_CONTENT.to_owned()
                } else {
                    body.to_owned()
                };
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
                let message_type = field("Message Type: ")?;
                let author = required_text(payload, "author")?;
                let recipient = required_text(payload, "recipient")?;
                let message = crate::event::agent::Message {
                    author: Some(author.to_owned()),
                    recipient: Some(recipient.to_owned()),
                    direction: if author == sender {
                        crate::event::agent::Direction::Outgoing
                    } else if recipient == sender {
                        crate::event::agent::Direction::Incoming
                    } else {
                        crate::event::agent::Direction::Between
                    },
                    kind: message_type.to_owned(),
                    content: if encrypted {
                        crate::event::agent::Content::Encrypted
                    } else {
                        crate::event::agent::Content::Text(text.clone())
                    },
                };
                let body = text.clone();
                calls.push(ToolCall {
                    agent_message: Some(message),

                    title: String::new(),
                    show_event_detail: true,
                    id: required_text(payload, "id")?.to_owned(),
                    summary: body.clone(),
                    name: message_type.to_owned(),
                    started_at: Some(at),
                    hide_output: true,
                    model_settings: None,
                    agent_thread_id: None,
                    agent_path: Some(if author == "/root" { recipient } else { author }.to_owned()),
                    input: vec![Section::Markdown {
                        label: String::new(),
                        text,
                    }],
                    output: None,
                    exit_code: None,
                    is_error: false,
                    duration: None,
                });
                crate::event::agent::present(calls.last_mut().unwrap());
                (Kind::Agent, body, None)
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
                        completed.as_ref().map(|(_, record)| *record),
                        reference(offset),
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
                turn: turns.at(offset),
                completed_time: kind == Kind::Tool
                    && !calls.iter().any(|call| call.agent_path.is_some()),
                background: false,
                source: Some(source(record, offset)),
                source_records,
                sender: None,
                kind,
                body,
                detail_body: {
                    let notes = memory_notes(&payload["item"])?;
                    (detail_body.is_some() || !notes.is_empty()).then_some(
                        crate::event::MessageDetail {
                            text: detail_body,
                            notes,
                        },
                    )
                },
                paths: Vec::new(),
                started_at: at,
                ended_at,
                duration: calls
                    .first()
                    .and_then(|call| call.duration)
                    .or_else(|| ended_at.map(|end| end - at)),
                previous_end: None,
                images,
                calls,
            },
        ));
        Ok(true)
    };
    for (offset, record) in records.iter().rev() {
        if !visit(record, *offset)? {
            break;
        }
    }
    entries.reverse();
    let offsets: HashMap<_, _> = entries
        .iter()
        .map(|(offset, event)| (event.source.clone(), *offset))
        .collect();
    let mut entries: Vec<_> = if selected.is_none() {
        crate::event::agent::merge(entries.into_iter().map(|(_, event)| event).collect())
            .into_iter()
            .map(|event| (offsets[&event.source], event))
            .collect()
    } else {
        entries
    };
    let mut previous_end = None;
    for (_, entry) in &mut entries {
        entry.source_records.sort_by_key(|record| record.offset);
        entry.source_records.dedup();
        entry.previous_end = previous_end;
        previous_end = Some(entry.ended_at.unwrap_or(entry.started_at));
    }
    let skip = entries.len().saturating_sub(count);
    let cut_turn = skip
        .checked_sub(1)
        .and_then(|index| entries[index].1.turn.map(|turn| turn.id));
    Ok(entries
        .into_iter()
        .skip(skip)
        .map(|(offset, mut event)| {
            if let Some(turn) = &mut event.turn
                && Some(turn.id) == cut_turn
            {
                turn.started = false;
            }
            (offset, event)
        })
        .collect())
}
#[cfg(test)]
mod tests {
    fn completed_tool(record: &Value) -> std::io::Result<Option<Event>> {
        super::completed_tool(
            record,
            Some(&crate::event::SourceRecord {
                path: "fixture.jsonl".into(),
                offset: 0,
            }),
        )
    }
    fn completed_call(item: &Value, command: Option<String>) -> ToolCall {
        super::completed_call(
            item,
            command,
            Some(&crate::event::SourceRecord {
                path: "fixture.jsonl".into(),
                offset: 0,
            }),
        )
    }

    #[test]
    fn turn_groups_mark_a_page_without_the_prompt_as_partial() {
        let records: Vec<_> = include_str!("../../../tests/fixtures/codex-rollout.jsonl")
            .lines()
            .enumerate()
            .skip(3)
            .map(|(offset, line)| (offset as u64, serde_json::from_str(line).unwrap()))
            .collect();
        let entries = events(&fixture(), &records, usize::MAX).unwrap();
        assert_eq!(entries.len(), 1);
        let turn = entries[0].1.turn.expect("partial turn marker");
        assert!(!turn.started && turn.complete);
        assert!(entries.iter().all(|(_, event)| event.turn == Some(turn)));
    }

    #[test]
    fn turn_groups_hide_the_start_when_the_returned_page_cuts_a_turn() {
        let mut reader = Reader::new(fixture(), false);
        let update = reader.refresh(1).unwrap();
        assert_eq!(update.events.len(), 1);
        assert!(update.events.iter().all(|event| {
            event
                .turn
                .is_some_and(|turn| !turn.started && turn.complete)
        }));
        let expanded = reader.refresh(2).unwrap();
        assert!(
            expanded
                .events
                .iter()
                .all(|event| { event.turn.is_some_and(|turn| turn.started && turn.complete) })
        );
    }

    #[test]
    fn turn_groups_follow_real_root_turn_records() {
        let mut reader = Reader::new(fixture(), false);
        let update = reader.refresh(usize::MAX).unwrap();
        assert_eq!(update.events.len(), 2);
        let turn = update.events[0].turn.expect("root turn marker");
        assert_eq!(turn.id, 0);
        assert!(turn.started && turn.complete);
        assert!(update.events.iter().all(|event| event.turn == Some(turn)));
    }

    #[test]
    fn turn_groups_close_existing_events_on_boundary_only_append() {
        use std::io::Write;
        let fixture = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let lines: Vec<_> = fixture.lines().collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", lines[..lines.len() - 1].join("\n")).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let running = reader.refresh(usize::MAX).unwrap();
        assert!(running.events.iter().all(|event| {
            event
                .turn
                .is_some_and(|turn| turn.started && !turn.complete)
        }));
        writeln!(file, "{}", lines.last().unwrap()).unwrap();
        let finished = reader.refresh(usize::MAX).unwrap();
        assert_eq!(finished.events.len(), running.events.len());
        assert_eq!(finished.appended, 0);
        assert_eq!(finished.replaced, running.events.len());
        assert!(
            finished
                .events
                .iter()
                .all(|event| { event.turn.is_some_and(|turn| turn.started && turn.complete) })
        );
        assert_eq!(
            finished.events[0].turn.unwrap().id,
            running.events[0].turn.unwrap().id
        );
    }

    #[test]
    fn turn_groups_keep_identity_when_appending_records() {
        use std::io::Write;
        let fixture = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let lines: Vec<_> = fixture.lines().collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", lines[..4].join("\n")).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        let initial = reader.refresh(usize::MAX).unwrap();
        let turn = initial.events[0].turn.expect("root turn marker");
        writeln!(file, "{}", lines[4..7].join("\n")).unwrap();
        let updated = reader.refresh(usize::MAX).unwrap();
        assert!(updated.events.iter().all(|event| event.turn == Some(turn)));
        assert!(
            updated
                .events
                .iter()
                .any(|event| event.kind == Kind::Assistant)
        );
    }
    #[test]
    fn real_activity_uses_the_latest_event() {
        let fixture = include_str!("../../../tests/fixtures/codex-latest-activity.jsonl");
        for count in [11, 12, 28, 142, 143, 144, 145, 148, 149, 156] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let prefix = fixture.lines().take(count).collect::<Vec<_>>().join("\n");
            std::fs::write(file.path(), format!("{prefix}\n")).unwrap();
            let events = tail(file.path(), usize::MAX).unwrap();
            let event = events
                .iter()
                .rev()
                .find(|event| !event.calls.is_empty())
                .unwrap();
            let result = summary(file.path()).unwrap();
            let activity = result.activity.unwrap();
            assert_eq!(activity.label, event.summary(), "prefix {count}");
            assert_eq!(activity.kind, event.kind, "prefix {count}");
            assert_eq!(activity.at, event.started_at, "prefix {count}");
            assert_eq!(
                result.task_details_sources[0], event.source,
                "prefix {count}"
            );
        }
    }

    #[test]
    fn detach_subject_uses_json_output_with_input_fallback() {
        let records: Vec<Value> =
            include_str!("../../../tests/fixtures/claude-manager-detach-name.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let block = &records[0]["message"]["content"][0];
        let input = &block["input"];
        let output = records[1]["message"]["content"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        let payload = serde_json::json!({"type":"function_call", "name": block["name"], "arguments":input.to_string(), "call_id":block["id"]});
        for (output, expected) in [
            (Some(output), "模型和new挪位"),
            (None, input["threadId"].as_str().unwrap()),
            (Some("{}"), input["threadId"].as_str().unwrap()),
        ] {
            let call = response_tool_call(&payload, output);
            assert_eq!(call.title, expected);
            assert!(call.summary.contains(expected));
            if expected == "模型和new挪位" {
                assert_eq!(
                    call.output,
                    Some(vec![text_section(
                        "output",
                        input["threadId"].as_str().unwrap()
                    )])
                );
            }
            let mut records: Vec<Value> =
                include_str!("../../../tests/fixtures/codex-tool-calls.jsonl")
                    .lines()
                    .take(2)
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
            records[0]["payload"] = payload.clone();
            records[1]["payload"]["call_id"] = block["id"].clone();
            if let Some(output) = output {
                records[1]["payload"]["output"] = output.into();
            } else {
                records.truncate(1);
            }
            let records: Vec<_> = records
                .into_iter()
                .enumerate()
                .map(|(index, record)| (index as u64, record))
                .collect();
            let file = tempfile::NamedTempFile::new().unwrap();
            let events = events(file.path(), &records, 10).unwrap();
            assert!(events[0].1.body.contains(expected), "{}", events[0].1.body);
        }
    }

    #[test]
    fn real_command_list_preserves_summary_and_failure_status() {
        let record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-command-summary.jsonl"
        ))
        .unwrap();
        let call = completed_call(
            &record["payload"]["item"],
            Some(
                record["payload"]["item"]["command"][2]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            ),
        );
        assert_eq!(call.summary, "sed, cat");
        assert_eq!(call.detail_left(), "Exec command");
        assert_eq!(call.title, "sed, cat");
        let mut failed = record["payload"]["item"].clone();
        failed["exit_code"] = 2.into();
        assert_eq!(
            completed_call(&failed, Some("yarn tsc".into())).summary,
            "yarn tsc · 2"
        );
        let mut empty = record["payload"]["item"].clone();
        empty["exit_code"] = 0.into();
        assert_eq!(
            completed_call(&empty, Some("$command".into())).summary,
            "Exec command"
        );
    }

    #[test]
    fn command_rules_keep_descriptions_and_detail_subjects() {
        let record = tool_records()
            .into_iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let command = record["payload"]["item"]["command"][2].as_str().unwrap();
        let parent = include_str!("../../../tests/fixtures/codex-interaction/parent.jsonl");
        let mut payload = parent
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| record["payload"]["type"] == "function_call")
            .unwrap()["payload"]
            .clone();
        payload["arguments"] =
            serde_json::json!({"command": command, "description": "A command description"})
                .to_string()
                .into();
        for name in [
            "exec_command",
            "shell",
            "CommandExecution",
            "functions.exec_command",
        ] {
            payload["name"] = name.into();
            let call = response_tool_call(&payload, Some(""));
            assert_eq!(
                tool_body(&payload).as_deref(),
                Some("A command description")
            );
            assert_eq!(call.summary, "A command description");
            assert!(!call.title.is_empty());
            assert!(matches!(&call.input[0], Section::Shell { text, .. } if text == command));
        }
    }

    #[test]
    fn captured_fetch_uses_the_shared_url_and_prompt_format() {
        let record: Value = include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| record["message"]["content"][0]["name"] == "WebFetch")
            .unwrap();
        let block = &record["message"]["content"][0];
        let mut payload = block.clone();
        payload["arguments"] = block["input"].to_string().into();
        let call = response_tool_call(&payload, None);
        assert!(
            matches!(&call.input[0], Section::Text { text, .. } if text == &format!("url: {}", block["input"]["url"].as_str().unwrap()))
        );
    }

    #[test]
    fn output_rules_declare_agent_result_behavior() {
        for name in ["spawn_agent", "send_message", "followup_task", "wait_agent"] {
            assert!(
                tool::rule(name).output.is_some(),
                "missing output rule: {name}"
            );
        }
    }

    #[test]
    fn command_rules_declare_summaries() {
        for name in ["exec_command", "shell", "CommandExecution"] {
            assert!(
                matches!(tool::rule(name).title, [tool::Pick::With(_)]),
                "missing command subject: {name}"
            );
        }
    }

    #[test]
    fn output_rules_handle_namespaces_from_real_wait_payload() {
        let record = include_str!("../../../tests/fixtures/codex-interaction/parent.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| {
                record["payload"]["type"] == "function_call_output"
                    && record["payload"]["output"]
                        .as_str()
                        .is_some_and(|text| text.contains("Wait completed."))
            })
            .unwrap();
        let raw = record["payload"]["output"].as_str().unwrap();
        for (name, expected) in [
            (
                "collaboration.wait_agent",
                vec![text_section("output", "Wait completed.")],
            ),
            ("collaboration.send_message", Vec::new()),
            ("collaboration.spawn_agent", Vec::new()),
        ] {
            assert_eq!(response_output(name, raw), expected, "{name}");
        }
        let mut timed_out: Value = serde_json::from_str(raw).unwrap();
        timed_out["timed_out"] = true.into();
        assert_eq!(
            output_sections(&timed_out.to_string(), tool::rule("wait_agent").output),
            vec![text_section("output", "(timed out)")]
        );
        assert_eq!(
            output_sections(raw, None),
            vec![Section::Structured {
                label: "output".into(),
                text: raw.to_owned()
            }]
        );
    }

    #[test]
    fn agent_message_outputs_are_empty_without_result_records() {
        for fixture in [
            include_str!("../../../tests/fixtures/codex-collaboration/send_message.jsonl"),
            include_str!("../../../tests/fixtures/codex-collaboration/followup_task.jsonl"),
        ] {
            let record: Value = serde_json::from_str(fixture).unwrap();
            let call = response_tool_call(&record["payload"], None);
            assert_eq!(call.output, Some(Vec::new()));
        }
    }

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
        let raw = update.events.last().unwrap().raw_text().unwrap();
        let actual = serde_json::Deserializer::from_str(&raw)
            .into_iter::<serde_json::Value>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(actual, rows[1..]);
        assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&update.events[0].raw_text().unwrap())
                .unwrap(),
            rows[0]
        );
    }

    use super::*;
    use crate::event::Kind;
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
    fn trailing_asterisk_does_not_mark_a_message_as_background() {
        let mut record = include_str!("../../../tests/fixtures/codex-rollout.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| {
                record["type"] == "response_item"
                    && record["payload"]["type"] == "message"
                    && record["payload"]["role"] == "assistant"
            })
            .unwrap();
        record["payload"]["content"][0]["text"] = "literal *".into();
        let file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file.as_file(), "{record}").unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events[0].body, "literal *");
        assert!(!events[0].background);
    }

    #[test]
    fn captured_shell_truncated_reads_keep_known_code_sections() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!(
            "../../../tests/fixtures/codex-shell-truncated.jsonl"
        ))
        .unwrap();
        let events = tail(file.path(), 20).unwrap();
        let output = events[0].calls[0].output.as_ref().unwrap();
        assert!(output.iter().any(|section| matches!(section,
            Section::Code { lang, text, .. }
                if lang == "rs" && text.starts_with("//! Asks the terminal"))));
        assert!(output.iter().any(|section| matches!(section,
            Section::Text { text, .. } if text.contains("…133 tokens truncated…"))));
    }

    #[test]
    fn captured_shell_diff_uses_diff_section() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!(
            "../../../tests/fixtures/codex-shell-diff.jsonl"
        ))
        .unwrap();
        let events = tail(file.path(), 20).unwrap();
        assert!(matches!(&events[0].calls[0].output.as_ref().unwrap()[0],
            Section::Diff { text, .. } if text.starts_with("diff --git ") && text.contains("@@ -35,7 +35,11 @@")));
    }

    #[test]
    fn shell_file_reads_highlight_stdout() {
        let record = tool_records()
            .into_iter()
            .find(|r| r["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        for (command, highlighted) in [
            ("sed -n '10,80p' src/foo.rs", true),
            ("cat 'src/foo bar.rs'", true),
            ("head -n 50 src/foo.rs", true),
            ("tail src/foo.rs", true),
            ("cat foo.unknown-language", false),
            ("sed -n 1,5p a.rs | grep x", false),
        ] {
            let mut record = record.clone();
            record["payload"]["item"]["command"][2] = command.into();
            record["payload"]["item"]["aggregated_output"] = "fn main() {}".into();
            let file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file.as_file(), "{record}").unwrap();
            let events = tail(file.path(), 20).unwrap();
            let output = events[0].calls[0].output.as_ref().unwrap();
            if highlighted {
                assert_eq!(
                    output[0],
                    tool::code_section("output", "rs", "fn main() {}")
                );
            } else {
                assert!(matches!(&output[0], Section::Text { .. }));
            }
            let payload = serde_json::json!({
                "type": "function_call", "name": "exec_command", "call_id": "shell",
                "arguments": serde_json::json!({"cmd": command}).to_string(),
            });
            let call = response_tool_call(&payload, Some("Exit code 0\nfn main() {}"));
            let section = &call.output.as_ref().unwrap()[0];
            if highlighted {
                assert_eq!(section, &tool::code_section("output", "rs", "fn main() {}"));
            } else {
                assert!(matches!(section, Section::Text { .. }));
            }
        }
    }

    #[test]
    fn shell_known_separator_keeps_mixed_language_block_plain() {
        let mut record = tool_records()
            .into_iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let command = "sed -n '1p' src/main.rs\necho '---'; head -n 1 probe.py && cat Cargo.toml";
        let stdout = "fn main() {}\n---\nprint('ready')\n[package]\nname = \"agents\"";
        record["payload"]["item"]["command"][2] = command.into();
        record["payload"]["item"]["aggregated_output"] = stdout.into();
        let file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file.as_file(), "{record}").unwrap();
        let events = tail(file.path(), 20).unwrap();
        let output = events[0].calls[0].output.as_ref().unwrap();
        assert_eq!(
            output,
            &vec![
                tool::code_section("output", "rs", "fn main() {}\n"),
                Section::Text {
                    label: "output".into(),
                    text: "---\n".into(),
                },
                Section::Text {
                    label: "output".into(),
                    text: "print('ready')\n[package]\nname = \"agents\"".into(),
                },
            ]
        );
    }

    #[test]
    fn shell_file_reads_before_unknown_commands_stay_plain() {
        let mut record = tool_records()
            .into_iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        record["payload"]["item"]["command"][2] = "sed -n '1p' src/main.rs; rg foo src".into();
        record["payload"]["item"]["aggregated_output"] = "let foo = 1;\nsrc/main.rs:foo\n".into();
        let file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file.as_file(), "{record}").unwrap();
        let events = tail(file.path(), 20).unwrap();
        assert_eq!(
            events[0].calls[0].output.as_ref().unwrap(),
            &vec![Section::Text {
                label: "output".into(),
                text: "let foo = 1;\nsrc/main.rs:foo\n".into(),
            }]
        );
    }

    #[test]
    fn shell_response_titles_use_command_subject_and_shell_event_detail() {
        let record = tool_records()
            .into_iter()
            .find(|r| r["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let command = record["payload"]["item"]["command"][2].as_str().unwrap();
        for (name, field) in [
            ("exec_command", "cmd"),
            ("exec_command", "command"),
            ("shell", "command"),
            ("CommandExecution", "command"),
        ] {
            let payload = serde_json::json!({
                "type": "function_call", "name": name, "call_id": "shell",
                "arguments": serde_json::json!({field: command}).to_string(),
            });
            let call = response_tool_call(&payload, Some("output"));
            assert_eq!(call.name, name);
            assert_eq!(call.summary, "cat, sed");
            assert_eq!(call.title, "cat, sed");
            assert_eq!(call.detail_left(), crate::providers::tool::title_name(name));
            let arguments = tool_arguments(&payload).unwrap();
            let presentation = tool::presentation(
                name,
                &ToolData {
                    input: &arguments,
                    result: None,
                },
            );
            assert_eq!(call.input, presentation.input);
            assert_eq!(call.output, Some(response_output(name, "output")));
        }
    }

    #[test]
    fn shell_function_calls_supply_command_subjects() {
        let record = tool_records()
            .into_iter()
            .find(|r| r["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let command = record["payload"]["item"]["command"][2].as_str().unwrap();
        for field in ["cmd", "command"] {
            let payload = serde_json::json!({
                "type": "function_call", "name": "exec_command", "call_id": "shell",
                "arguments": serde_json::json!({field: command}).to_string(),
            });
            assert_eq!(tool_body(&payload).as_deref(), Some("cat, sed"));
            let file = tempfile::NamedTempFile::new().unwrap();
            writeln!(
                file.as_file(),
                "{}",
                serde_json::json!({
                    "type": "response_item", "timestamp": record["timestamp"], "payload": payload,
                })
            )
            .unwrap();
            let events = tail(file.path(), 100).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, "cat, sed");
            assert_eq!(events[0].calls[0].detail_left(), "Exec command");
        }
    }

    #[test]
    fn read_subject_and_file_change_use_captured_records() {
        let records = tool_records();
        let read = records
            .iter()
            .find(|r| r["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let event = completed_tool(read).unwrap().unwrap();
        assert_eq!(event.calls[0].title, "cat, sed");
        assert_eq!(event.calls[0].detail_left(), "Exec command");
        assert_eq!(event.body, "cat, sed");
        let file = records
            .iter()
            .find(|r| r["payload"]["item"]["type"] == "FileChange")
            .unwrap();
        let event = completed_tool(file).unwrap().unwrap();
        assert_eq!(event.calls[0].title, "/workspace/agents/src/processes.rs");
        assert!(event.calls[0].input.is_empty());
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], Section::Diff { text, .. } if text == file["payload"]["item"]["changes"]["/workspace/agents/src/processes.rs"]["unified_diff"].as_str().unwrap())
        );
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
    fn task_details_sources_identify_real_rollout_events() {
        let path = fixture();
        let task_details = summary(&path).unwrap();
        let (_, prompt) = first_prompt(&path, "Check the dashboard.").unwrap();
        let events = tail(&path, usize::MAX).unwrap();
        for (source, kind) in [
            (
                task_details.task_details_sources[1].as_ref(),
                Kind::Assistant,
            ),
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
        let source = tool_summary.task_details_sources[0].as_ref().unwrap();
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
        assert_eq!(call.summary, "Spawn agent claude_calls");
        let record = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        let call = &completed_tool(record).unwrap().unwrap().calls[0];
        assert_eq!(call.id, record["payload"]["item"]["id"].as_str().unwrap());
        assert_eq!(call.summary, "cat, sed");
    }

    #[test]
    fn completed_file_change_has_output() {
        let records = tool_records();
        let record = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "FileChange")
            .unwrap();
        let event = completed_tool(record).unwrap().unwrap();
        assert!(event.calls[0].input.is_empty());
        assert_eq!(event.calls[0].output, Some(vec![Section::Diff {
            lang: "rs".into(),
            label: String::new(),
            text: record["payload"]["item"]["changes"]["/workspace/agents/src/processes.rs"]["unified_diff"].as_str().unwrap().to_owned(),
        }]));
    }

    #[test]
    fn added_and_deleted_files_show_their_content_as_diff() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/codex-file-changes.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let outputs: Vec<_> = records
            .iter()
            .map(|record| {
                completed_tool(record).unwrap().unwrap().calls[0]
                    .output
                    .clone()
            })
            .collect();
        assert_eq!(outputs, [
            Some(vec![Section::Diff {
                lang: "mjs".into(),
                label: String::new(),
                text: "@@ -0,0 +1,2 @@\n+// Ported from wakaru v1.14.0 SimplifySequence (Apache-2.0).\n+export default { name: 'SimplifySequence', visitor() { return {}; } };".to_owned(),
            }]),
            Some(vec![Section::Diff {
                lang: "txt".into(),
                label: String::new(),
                text: "@@ -1,7 +0,0 @@\n-                 Prompt\n-Current prompt Current prompt Current\n-prompt Current prompt Current prompt\n-Current prompt Current prompt Current\n-prompt Current prompt Current prompt\n-Current prompt Current prompt\n-Queued prompt that extends beyond the".to_owned(),
            }]),
        ]);
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
        assert!(spawn[0].input.iter().any(
            |section| matches!(section, Section::Markdown { text, .. } if text == "[scrubbed]")
        ));
        assert!(spawn[0].output.is_some());
        assert_eq!(spawn[1].output, None);
        let custom = calls.iter().find(|call| call.name == "js_repl").unwrap();
        assert!(
            matches!(&custom.input[0], crate::event::Section::Text { text, .. } if text.starts_with("// codex-js-repl"))
        );
        assert!(
            matches!(&custom.output.as_ref().unwrap()[0], crate::event::Section::Structured { text, .. } if text == "[]")
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
            matches!(&event.calls[0].input[0], crate::event::Section::Shell { text, prefix: 0 } if text == command["payload"]["item"]["command"][2].as_str().unwrap())
        );
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], crate::event::Section::Text { text, .. } if text == command["payload"]["item"]["aggregated_output"].as_str().unwrap())
        );
        let file = records
            .iter()
            .find(|record| record["payload"]["item"]["type"] == "FileChange")
            .unwrap();
        let event = completed_tool(file).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], crate::event::Section::Diff { label, text, .. } if label.is_empty() && text.starts_with("@@ -132,2 +132,22 @@"))
        );
    }

    #[test]
    fn inline_image_pointer_resolves_captured_mcp_record() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mcp-codex/image.json");
        let record: Value = serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
        let entries = events(&path, &[(0, record.clone())], usize::MAX).unwrap();
        let output = entries[0].1.calls[0].output.as_ref().unwrap();
        let Section::Image(image) = &output[1] else {
            panic!("expected image")
        };
        assert_eq!(image.label, "[image image/png]");
        let ImageSource::Record {
            record: source,
            pointer,
        } = &image.source
        else {
            panic!("expected record source")
        };
        assert_eq!(*source, crate::event::SourceRecord { path, offset: 0 });
        assert_eq!(pointer, "/payload/item/result/content/1/data");
        assert_eq!(
            record.pointer(pointer).unwrap().as_str().unwrap(),
            record["payload"]["item"]["result"]["content"][1]["data"]
                .as_str()
                .unwrap()
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
            if record["payload"]["item"]["server"] == "appium-mcp" {
                assert!(
                    matches!(&call.input[0], Section::Markdown { text, .. }
                    if text.contains("platform") && !text.contains("action")),
                    "appium session calls title their action and table the rest: {:?}",
                    call.input
                );
            } else {
                assert!(matches!(&call.input[0], Section::Structured { label, text }
                    if label == "input" && serde_json::from_str::<Value>(text).unwrap() == record["payload"]["item"]["arguments"]));
            }
            let result = &record["payload"]["item"]["result"];
            assert!(
                matches!(&call.output.as_ref().unwrap()[0], Section::Text { text, .. }
                if text == result["content"][0]["text"].as_str().unwrap())
            );
            match &call.output.as_ref().unwrap()[1] {
                Section::Image(image) => {
                    assert_eq!(image.label, "[image image/png]");
                    let ImageSource::Record { pointer, .. } = &image.source else {
                        panic!("expected record source")
                    };
                    assert_eq!(record.pointer(pointer), Some(&result["content"][1]["data"]));
                }
                Section::Placeholder(text) => assert_eq!(text, "[resource text/html]"),
                Section::Structured { label, text } => {
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
        assert!(call.input.is_empty());
        assert_eq!(call.name, "mcp__chrome_devtools__list_pages");
    }

    #[test]
    fn malformed_arguments_and_completed_mcp_results_keep_event_detail() {
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
        let mcp: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-client/notify.jsonl"
        ))
        .unwrap();
        let event = completed_tool(&mcp).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].input[0], Section::Markdown {label, text} if label == "text" && text == mcp["payload"]["item"]["arguments"]["text"].as_str().unwrap())
        );
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], Section::Markdown { text, .. } if text == "Delivered to Claude.")
        );
        let mut no_result = mcp.clone();
        no_result["payload"]["item"]["result"] = Value::Null;
        assert_eq!(
            completed_tool(&no_result).unwrap().unwrap().calls[0].output,
            Some(Vec::new())
        );
        let mut json_result = mcp.clone();
        json_result["payload"]["item"]["result"]["content"][0]["text"] = r#"{"ok":true}"#.into();
        let event = completed_tool(&json_result).unwrap().unwrap();
        assert!(
            matches!(&event.calls[0].output.as_ref().unwrap()[0], Section::Markdown { text, .. } if text == r#"{"ok":true}"#)
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
    fn captured_mcp_failures_and_rejections_set_the_row_kind() {
        for record in mcp_error_records() {
            let event = completed_tool(&record).unwrap().unwrap();
            let message = record["payload"]["item"]["error"]["message"]
                .as_str()
                .unwrap();
            assert_eq!(
                event.kind,
                if message == "user rejected MCP tool call" {
                    Kind::Denied
                } else {
                    Kind::Error
                }
            );
        }
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
            } else if let Some(text) = arguments.as_str() {
                vec![text_section("input", text)]
            } else {
                vec![Section::Structured {
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
            matches!(&events[0].calls[0].output.as_ref().unwrap()[0], crate::event::Section::Text { text, .. } if text.starts_with("Script completed") && !text.contains("skip") && text.ends_with("\nsecond text"))
        );
    }

    #[test]
    fn question_rejection_uses_the_shared_rejection_format() {
        let payload = input_records("pending")
            .into_iter()
            .find(|record| record["payload"]["name"] == "request_user_input")
            .unwrap()["payload"]
            .clone();
        let result: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-questions-rejected.jsonl")
                .lines()
                .nth(1)
                .unwrap(),
        )
        .unwrap();
        let output = result["message"]["content"][0]["content"].as_str().unwrap();
        let call = response_tool_call(&payload, Some(output));
        assert_eq!(
            call.output.unwrap(),
            vec![text_section("output", "Chat about this")]
        );
        assert!(call.is_error);
    }

    #[test]
    fn real_question_details_use_headers_and_question_list() {
        let record = input_records("pending")
            .into_iter()
            .find(|record| record["payload"]["name"] == "request_user_input")
            .unwrap();
        let call = response_tool_call(&record["payload"], None);
        assert_eq!(call.detail_left(), "Request user input");
        assert_eq!(call.title, "Color");
        assert_eq!(call.summary, "Request user input Color");
        assert!(call.show_event_detail);
        assert_eq!(
            call.input,
            vec![
                Section::Label("Color".into()),
                Section::Prose {
                    text: "Pick a color?\n1. Red: Choose red.\n2. Blue: Choose blue.".into(),
                    markdown: false,
                }
            ]
        );
    }

    #[test]
    fn message_redaction_requires_fernet_structure_and_preserves_other_fields() {
        let record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-collaboration/spawn_agent.jsonl"
        ))
        .unwrap();
        let mut payload = record["payload"].clone();
        let original: Value = serde_json::from_str(payload["arguments"].as_str().unwrap()).unwrap();
        let token = original["message"].as_str().unwrap();
        let bytes = base64::engine::general_purpose::URL_SAFE
            .decode(token)
            .unwrap();
        let mut wrong_version = bytes.clone();
        wrong_version[0] = 0x81;
        for message in [
            "Review files.".to_owned(),
            "gAAAAA is a token prefix.".to_owned(),
            format!("{token} trailing prose"),
            format!("Review {token}"),
            base64::engine::general_purpose::URL_SAFE.encode(&bytes[..57]),
            base64::engine::general_purpose::URL_SAFE.encode(&bytes[..bytes.len() - 1]),
            base64::engine::general_purpose::URL_SAFE.encode(wrong_version),
        ] {
            let arguments = serde_json::json!({"message": message, "target": "reviewer"});
            payload["arguments"] = arguments.to_string().into();
            assert_eq!(tool_arguments(&payload).unwrap(), arguments);
        }
        // Redaction depends on the field value, not a tool name or namespace.
        payload["name"] = "other_message_tool".into();
        payload["arguments"] = serde_json::json!({
            "message": token, "target": token, "task_name": "demo_sub"
        })
        .to_string()
        .into();
        let arguments = tool_arguments(&payload).unwrap();
        assert_eq!(arguments["message"], ENCRYPTED_CONTENT);
        assert_eq!(arguments["target"], token);
        assert_eq!(arguments["task_name"], "demo_sub");
    }

    #[test]
    fn real_collaboration_messages_hide_fernet_tokens() {
        for (fixture, subject) in [
            (
                include_str!("../../../tests/fixtures/codex-collaboration/spawn_agent.jsonl"),
                "demo_sub",
            ),
            (
                include_str!("../../../tests/fixtures/codex-collaboration/send_message.jsonl"),
                "rename_gating",
            ),
            (
                include_str!("../../../tests/fixtures/codex-collaboration/followup_task.jsonl"),
                "ux_probe",
            ),
        ] {
            let record: Value = serde_json::from_str(fixture).unwrap();
            let mut payload = record["payload"].clone();
            let tool = payload["name"].as_str().unwrap().to_owned();
            for name in [tool.clone(), format!("collaboration.{tool}")] {
                payload["name"] = name.into();
                let call = response_tool_call(&payload, Some("Plaintext result"));
                let expected = if tool == "send_message" {
                    format!("{subject} ← message")
                } else {
                    let name = crate::providers::tool::display_tool_name(
                        payload["name"].as_str().unwrap(),
                    );
                    if tool == "spawn_agent" {
                        format!("{name} {subject}")
                    } else {
                        format!("{subject} ← {}", name.to_lowercase())
                    }
                };
                assert_eq!(tool_body(&payload).unwrap(), "(encrypted)");
                assert_eq!(call.summary, expected);
                assert_eq!(
                    call.title,
                    if tool == "spawn_agent" {
                        subject.to_owned()
                    } else {
                        format!("← {}", agent_name(subject))
                    }
                );
                let input = vec![Section::Markdown {
                    label: String::new(),
                    text: "(encrypted)".into(),
                }];
                assert_eq!(call.input, input);
                assert_eq!(call.output, Some(Vec::new()));
                let file = tempfile::NamedTempFile::new().unwrap();
                let mut row = record.clone();
                row["payload"] = payload.clone();
                std::fs::write(file.path(), row.to_string()).unwrap();
                assert_eq!(tail(file.path(), 1).unwrap()[0].summary(), expected);
                assert_eq!(
                    summary(file.path()).unwrap().activity.unwrap().label,
                    expected
                );
            }
        }
    }

    #[test]
    fn real_agent_details_use_markdown_messages() {
        for (line, (tool, subject)) in
            include_str!("../../../tests/fixtures/codex-agent-names.jsonl")
                .lines()
                .zip([
                    ("spawn_agent", "real_scan"),
                    ("send_message", "/root"),
                    ("followup_task", "ux_probe"),
                ])
        {
            let record: Value = serde_json::from_str(line).unwrap();
            for name in [tool.to_owned(), format!("collaboration.{tool}")] {
                let mut payload = record["payload"].clone();
                payload["name"] = name.clone().into();
                let call = response_tool_call(&payload, None);
                assert_eq!(call.detail_left(), "");
                assert_eq!(
                    call.title,
                    if tool == "spawn_agent" {
                        subject.to_owned()
                    } else {
                        format!("← {}", agent_name(subject))
                    }
                );
                let expected = vec![Section::Markdown {
                    label: String::new(),
                    text: "Review files.".into(),
                }];
                assert_eq!(call.input, expected);
            }
        }
    }

    #[test]
    fn daemon_question_metadata_matches_replayed_request() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-input");
        let request: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-input/daemon-request.json"
        ))
        .unwrap();
        let pending = summary(&root.join("daemon-pending.jsonl")).unwrap();
        assert_eq!(
            pending.pending_question_call,
            Some((
                request["params"]["itemId"].as_str().unwrap().to_owned(),
                "2026-10-20T02:59:57.613Z".parse().unwrap(),
            ))
        );
        assert_eq!(pending.pending_questions[0].id, "color");
        let answered = summary(&root.join("daemon-answered.jsonl")).unwrap();
        assert!(answered.pending_question_call.is_none());
        assert!(answered.pending_questions.is_empty());
        assert!(!answered.turn_ended);
    }

    #[test]
    fn request_user_input_event_and_activity_show_name_and_headers() {
        for mode in ["default", "plan"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/codex-input/{mode}-pending.jsonl"));
            let events = tail(&path, 100).unwrap();
            let question = events
                .iter()
                .find(|event| {
                    event
                        .calls
                        .iter()
                        .any(|call| call.name == "request_user_input")
                })
                .unwrap();
            assert_eq!(question.kind, Kind::Assistant);
            assert_eq!(question.body, "Request user input Color");
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
            assert_eq!(activity.label, "Request user input Color");
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
    fn ended_turn_without_question_output_has_no_pending_call() {
        let mut records = input_records("pending");
        let completed = input_records("answered")
            .into_iter()
            .find(|record| record["payload"]["type"] == "task_complete")
            .unwrap();
        for kind in ["task_complete", "turn_aborted"] {
            let mut ended = completed.clone();
            ended["payload"]["type"] = kind.into();
            records.push(ended);
            let result = input_summary(&records);
            assert!(result.turn_ended);
            assert!(result.pending_question_call.is_none());
            assert!(result.pending_questions.is_empty());
            records.pop();
        }
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
            "Request user input Color · Color"
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
    fn real_agent_names_in_event_and_activity() {
        for (line, expected) in include_str!("../../../tests/fixtures/codex-agent-names.jsonl")
            .lines()
            .zip([
                "Spawn agent real_scan",
                "root ← message",
                "ux_probe ← followup task",
            ])
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].summary(), expected);
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
            let expected = if name == "collaboration.send_message" {
                "reviewer ← message".to_owned()
            } else {
                crate::providers::tool::title_name(&crate::providers::tool::display_tool_name(name))
            };
            assert_eq!(tail(file.path(), 1).unwrap()[0].summary(), expected);
            assert_eq!(
                summary(file.path()).unwrap().activity.unwrap().label,
                expected
            );
        }
    }

    #[test]
    fn image_generation_input_excludes_result() {
        let record: Value =
            include_str!("../../../tests/fixtures/codex-images/tool-generation.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|record| record["payload"]["item"]["kind"] == "image_gen.generation")
                .unwrap();
        let item = &record["payload"]["item"];
        assert!(!item["result"].as_str().unwrap().is_empty());
        let call = completed_tool(&record).unwrap().unwrap().calls.remove(0);
        let [Section::Structured { label, text }] = call.input.as_slice() else {
            panic!(
                "expected structured image generation input: {:?}",
                call.input
            );
        };
        assert_eq!(label, "input");
        let input: Value = serde_json::from_str(text).unwrap();
        let mut expected = item.clone();
        expected.as_object_mut().unwrap().remove("result");
        assert_eq!(input, expected);
    }

    #[test]
    fn image_view_exec_calls_do_not_emit_events() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex-images/tool-view.jsonl");
        let events = tail(&path, usize::MAX).unwrap();
        assert_eq!(events.len(), 1);
        let [call] = events[0].calls.as_slice() else {
            panic!("expected the image view call");
        };
        assert_eq!(call.name, "ImageView");
        assert_eq!(call.id, "exec-a6b963e9-ca3f-48c6-9a37-5ffae408af41");
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
    fn event_maps_real_records_without_duplicate_messages() {
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
        for record in &mut records {
            if completed_assistant(record) {
                record["payload"]["item"]["content"][0]["text"] = serde_json::json!(text);
            } else if record["payload"]["role"] == "assistant" {
                record["payload"]["content"][0]["text"] = serde_json::json!(text);
            }
        }
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        write!(file, "{{\"timestamp\":").unwrap();
        let result = summary(file.path()).unwrap();
        assert_eq!(result.last_message.unwrap(), text);
        assert!(!result.turn_ended);
    }
    #[test]
    fn duplicate_message_order_does_not_change_event() {
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
        assert_eq!(result.activity.unwrap().label, "cargo check");
        writeln!(file,r#"{{"timestamp":"2026-10-13T18:00:02Z","type":"event_msg","payload":{{"type":"turn_aborted","reason":"interrupted"}}}}"#).unwrap();
        assert!(summary(file.path()).unwrap().turn_ended);
    }
}

#[cfg(test)]
mod timeout_test {
    use super::*;

    #[test]
    fn real_wait_result_timeout_only_shows_annotation() {
        let record = include_str!("../../../tests/fixtures/codex-interaction/parent.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| {
                record["payload"]["type"] == "function_call_output"
                    && record["payload"]["output"]
                        .as_str()
                        .is_some_and(|text| text.contains("Wait completed."))
            })
            .unwrap();
        let mut output: serde_json::Value =
            serde_json::from_str(record["payload"]["output"].as_str().unwrap()).unwrap();
        output["timed_out"] = true.into();
        assert_eq!(
            response_output("wait_agent", &output.to_string()),
            vec![crate::event::Section::Text {
                label: "output".into(),
                text: "(timed out)".into()
            }]
        );
    }
}

#[cfg(test)]
mod perspective_tests {
    use super::*;

    fn fixture(name: &str) -> Vec<Event> {
        tail(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/codex-perspective/{name}.jsonl")),
            200,
        )
        .unwrap()
    }

    #[test]
    fn perspective_sentence_names_from_real_records() {
        let records = include_str!("../../../tests/fixtures/codex-perspective/root.jsonl");
        for (name, expected_left, expected_title) in [
            ("spawn_agent", "", "alpha"),
            ("wait_agent", "", ""),
            ("followup_task", "", "← alpha"),
        ] {
            let record: Value = records
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|r| r["payload"]["name"] == name)
                .unwrap();
            let call = response_tool_call(&record["payload"], None);
            assert_eq!(call.detail_left(), expected_left);
            assert_eq!(call.title, expected_title);
        }
        assert_eq!(
            crate::providers::tool::title_name("FINAL_ANSWER"),
            "Final answer"
        );
        assert_eq!(
            crate::providers::tool::title_name("exec_command"),
            "Exec command"
        );
    }

    #[test]
    fn perspective_assignments_use_function_names() {
        let events = fixture("root");
        for (name, title) in [
            ("spawn_agent", "Spawn agent alpha"),
            ("followup_task", "alpha ← followup task"),
        ] {
            assert!(
                events
                    .iter()
                    .any(|e| e.calls.iter().any(|c| c.name == name) && e.summary() == title),
                "{name}"
            );
        }
    }

    #[test]
    fn perspective_root_messages() {
        let events = fixture("root");
        for title in ["alpha → message", "beta ← message", "beta → beta 完成"] {
            assert!(
                events.iter().any(|e| e.summary() == title),
                "{title}: {:?}",
                events.iter().map(|e| e.summary()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn perspective_children_and_grandchild() {
        let events = fixture("alpha");
        assert_eq!(
            events
                .iter()
                .filter(|e| e.summary() == "root → new task")
                .count(),
            2
        );
        for title in [
            "Spawn agent alpha_kid",
            "alpha_kid → 孙子完成",
            "beta ← message",
            "root ← message",
        ] {
            assert!(events.iter().any(|e| e.summary() == title), "{title}");
        }
        assert!(
            fixture("beta")
                .iter()
                .any(|e| e.summary() == "alpha → message")
        );
        assert!(
            fixture("beta")
                .iter()
                .any(|e| e.summary() == "root ← 2 messages")
        );
        assert!(
            fixture("alpha_kid")
                .iter()
                .any(|e| e.summary() == "alpha → new task")
        );
    }

    #[test]
    fn perspective_incoming_messages_merge_from_real_records() {
        let records = include_str!("../../../tests/fixtures/codex-perspective/root.jsonl");
        let mut lines = records.lines();
        let metadata = lines.next().unwrap();
        let messages: Vec<_> = lines
            .filter(|line| {
                let record: Value = serde_json::from_str(line).unwrap();
                let payload = &record["payload"];
                payload["type"] == "agent_message"
                    && payload["author"] == "/root/beta"
                    && payload["content"][0]["text"]
                        .as_str()
                        .is_some_and(|text| text.starts_with("Message Type: MESSAGE\n"))
            })
            .collect();
        assert_eq!(messages.len(), 2);
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            format!("{metadata}\n{}\n", messages.join("\n")),
        )
        .unwrap();
        let events = tail(file.path(), 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary(), "beta → 2 messages");
        assert_eq!(events[0].calls.len(), 2);
        assert!(
            events[0]
                .calls
                .iter()
                .all(|call| call.title == "beta → root")
        );
    }

    #[test]
    fn perspective_foreign_route_keeps_both_participants() {
        let root = include_str!("../../../tests/fixtures/codex-perspective/root.jsonl")
            .lines()
            .next()
            .unwrap();
        let message = include_str!("../../../tests/fixtures/codex-perspective/beta.jsonl")
            .lines()
            .find(|line| {
                let r: Value = serde_json::from_str(line).unwrap();
                r["payload"]["author"] == "/root/alpha" && r["payload"]["recipient"] == "/root/beta"
            })
            .unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), format!("{root}\n{message}\n")).unwrap();
        assert_eq!(
            tail(file.path(), 10).unwrap()[0].summary(),
            "alpha → beta · message"
        );
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    #[test]
    fn codex_agent_exchange_keeps_routes_out_of_body() {
        let events = tail(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codex-agent-exchange.jsonl"),
            100,
        )
        .unwrap();
        let incoming = events.iter().find(|e| e.kind == Kind::Agent).unwrap();
        assert!(!incoming.body.contains(" → "), "{}", incoming.body);
        assert_eq!(
            incoming.calls[0].input,
            vec![Section::Markdown {
                label: String::new(),
                text: incoming.body.clone()
            }]
        );
    }

    #[test]
    fn codex_agent_exchange_records_route_type_and_encryption() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-agent-exchange.jsonl");
        let events = tail(&path, 100).unwrap();
        let calls: Vec<_> = events.iter().flat_map(|event| &event.calls).collect();
        assert_eq!(calls.len(), 10);
        for line in include_str!("../../../tests/fixtures/codex-agent-exchange.jsonl").lines() {
            let record: Value = serde_json::from_str(line).unwrap();
            let payload = &record["payload"];
            if payload["type"] == "agent_message" {
                let call = calls
                    .iter()
                    .find(|call| Some(call.id.as_str()) == payload["id"].as_str())
                    .unwrap();
                let message = call.agent_message.as_ref().unwrap();
                assert_eq!(message.author.as_deref(), payload["author"].as_str());
                assert_eq!(message.recipient.as_deref(), payload["recipient"].as_str());
                let text = payload["content"][0]["text"].as_str().unwrap();
                let (header, body) = text.split_once("\nPayload:\n").unwrap();
                assert_eq!(
                    message.kind,
                    header
                        .lines()
                        .next()
                        .unwrap()
                        .strip_prefix("Message Type: ")
                        .unwrap()
                );
                if payload["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|block| block["type"] == "encrypted_content")
                {
                    assert_eq!(message.content, crate::event::agent::Content::Encrypted);
                } else {
                    assert_eq!(message.content.text(), body);
                }
            } else if payload["type"] == "function_call" {
                let call = calls
                    .iter()
                    .find(|call| Some(call.id.as_str()) == payload["call_id"].as_str())
                    .unwrap();
                assert_eq!(call.agent_message.as_ref().unwrap().author, None);
            }
        }
        let merged = events.iter().find(|event| event.calls.len() == 2).unwrap();
        assert_eq!(merged.summary(), "beta → 2 messages");
        assert_eq!(merged.calls.len(), 2);
        assert_ne!(merged.calls[0].id, merged.calls[1].id);
        assert_eq!(merged.started_at, merged.calls[1].started_at.unwrap());
    }

    #[test]
    fn plaintext_marker_is_not_an_encrypted_field() {
        let mut record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-agent-messages/final-answer.jsonl"
        ))
        .unwrap();
        let text = record["payload"]["content"][0]["text"].as_str().unwrap();
        let header = text.split_once("\nPayload:\n").unwrap().0;
        record["payload"]["content"][0]["text"] = format!("{header}\nPayload:\n(encrypted)").into();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), record.to_string()).unwrap();
        let events = tail(file.path(), 10).unwrap();
        assert_eq!(
            events[0].calls[0].agent_message.as_ref().unwrap().content,
            crate::event::agent::Content::Text("(encrypted)".to_owned())
        );
    }
}

#[cfg(test)]
mod prompt_cache_tests {
    use super::*;

    fn request_prefix(lines: usize) -> Summary {
        let fixture = include_str!("../../../tests/fixtures/prompt-cache/codex-requests.jsonl");
        let file = tempfile::NamedTempFile::new().unwrap();
        let prefix = fixture.lines().take(lines).collect::<Vec<_>>().join("\n");
        std::fs::write(file.path(), format!("{prefix}\n")).unwrap();
        summary(file.path()).unwrap()
    }

    #[test]
    fn prompt_cache_starts_at_turn_start_instead_of_response_completion() {
        let expected = "2026-10-19T07:24:18.845Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(request_prefix(2).cache_expires_at, Some(expected));
    }

    #[test]
    fn prompt_cache_starts_followup_at_tool_output_and_ignores_uncompleted_request() {
        let first = request_prefix(2).cache_expires_at;
        assert_eq!(request_prefix(3).cache_expires_at, first);
        assert_eq!(request_prefix(4).cache_expires_at, first);
        let expected = "2026-10-19T07:24:24.225Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(request_prefix(5).cache_expires_at, Some(expected));
        assert_eq!(request_prefix(7).cache_expires_at, Some(expected));
    }

    #[test]
    fn prompt_cache_repeated_completion_keeps_the_original_request_start() {
        let fixture = include_str!("../../../tests/fixtures/prompt-cache/codex-requests.jsonl");
        let lines: Vec<_> = fixture.lines().collect();
        let file = tempfile::NamedTempFile::new().unwrap();
        let replay = [lines[0], lines[1], lines[2], lines[1]].join("\n");
        std::fs::write(file.path(), format!("{replay}\n")).unwrap();
        assert_eq!(
            summary(file.path()).unwrap().cache_expires_at,
            request_prefix(2).cache_expires_at
        );
    }

    #[test]
    fn prompt_cache_requires_a_completed_request() {
        assert_eq!(request_prefix(1).cache_expires_at, None);
    }
}

#[cfg(test)]
mod summary_cache_tests {
    #[test]
    fn summary_cache_keeps_only_observed_fields_of_context_and_raw_responses() {
        use super::*;
        let text = include_str!("../../../tests/fixtures/codex-latest-activity.jsonl");
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), text).unwrap();
        let mut tasks = Tasks::default();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
        let records = &tasks.reader.as_ref().unwrap().records;
        assert!(
            records
                .iter()
                .any(|(_, record)| record["payload"]["type"] == "raw_response_completed")
        );
        for (_, record) in records {
            if record["payload"]["type"] == "raw_response_completed" {
                assert!(record["payload"].as_object().unwrap().len() <= 2);
            } else if record["type"] == "turn_context" {
                assert!(record["payload"].as_object().unwrap().len() <= 1);
            }
        }
    }

    #[test]
    fn summary_tail_advances_past_a_record_larger_than_a_page() {
        use super::*;
        use std::io::Write;
        let text = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let mut context: Value = serde_json::from_str(text.lines().nth(1).unwrap()).unwrap();
        let content = context["payload"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .repeat(1024);
        context["payload"]["content"][0]["text"] = content.into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{context}\n{}", text.lines().next().unwrap()).unwrap();
        let mut tasks = Tasks::default();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
    }

    use super::*;
    use std::io::Write;

    fn expected(path: &Path) -> Summary {
        let mut result = summary(path).unwrap();
        result.tasks = Tasks::default().read(path).unwrap();
        result
    }

    #[test]
    fn summaries_parse_real_fixture_once_across_appends_and_unchanged_refreshes() {
        for text in [
            include_str!("../../../tests/fixtures/codex-rollout.jsonl"),
            include_str!("../../../tests/fixtures/codex-todos.jsonl"),
            include_str!("../../../tests/fixtures/codex-latest-activity.jsonl"),
            include_str!("../../../tests/fixtures/codex-input/default-pending.jsonl"),
            include_str!("../../../tests/fixtures/codex-input/default-answered.jsonl"),
            include_str!("../../../tests/fixtures/codex-subagents/rollout.jsonl"),
            include_str!("../../../tests/fixtures/prompt-cache/codex-requests.jsonl"),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let mut tasks = Tasks::default();
            for line in text.split_inclusive('\n') {
                file.write_all(line.as_bytes()).unwrap();
                let want = expected(file.path());
                assert_eq!(tasks.summary(file.path()).unwrap(), want);
                let parsed = tasks.parsed_bytes;
                assert_eq!(tasks.summary(file.path()).unwrap(), want);
                assert_eq!(
                    tasks.parsed_bytes, parsed,
                    "unchanged file was parsed again"
                );
            }
            assert_eq!(tasks.parsed_bytes, text.len() as u64);
        }
    }

    #[test]
    fn summary_errors_remain_errors_on_unchanged_retries() {
        let text = include_str!("../../../tests/fixtures/codex-todos.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut tasks = Tasks::default();
        file.write_all(text.as_bytes()).unwrap();
        tasks.summary(file.path()).unwrap();
        let mut invalid: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        let mut args: Value =
            serde_json::from_str(invalid["payload"]["arguments"].as_str().unwrap()).unwrap();
        args["plan"][0]["status"] = "invalid".into();
        invalid["payload"]["arguments"] = args.to_string().into();
        writeln!(file, "{invalid}").unwrap();
        for _ in 0..2 {
            assert!(tasks.summary(file.path()).is_err());
        }
    }

    #[test]
    fn summary_reads_subagent_metadata_after_an_empty_initial_refresh() {
        let text = include_str!("../../../tests/fixtures/codex-subagents/rollout.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut tasks = Tasks::default();
        tasks.summary(file.path()).unwrap();
        for bytes in text.as_bytes().chunks(137) {
            file.write_all(bytes).unwrap();
            assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
        }
    }

    #[test]
    fn prompt_images_keep_record_sources_after_local_files_disappear() {
        // Real prompt-image probe records, with the recorded base64 truncation retained.
        let records: Vec<Value> =
            include_str!("../../../tests/fixtures/codex-images/prompt-record.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let context = include_str!("../../../tests/fixtures/codex-images/context.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{context}").unwrap();
        let offset = context.len() as u64;
        for record in &records {
            writeln!(file, "{record}").unwrap();
        }
        let prompt = user_prompt(&records[0]["payload"]["content"])
            .body()
            .unwrap();
        let expected = vec![Image {
            label: "[Image #1]".into(),
            source: ImageSource::Record {
                record: crate::event::SourceRecord {
                    path: file.path().to_owned(),
                    offset,
                },
                pointer: "/payload/content/1/image_url".into(),
            },
        }];
        assert_eq!(tail(file.path(), 1).unwrap()[0].images, expected);
        assert_eq!(first_prompt(file.path(), &prompt).unwrap().0, expected);
        let mut tasks = Tasks::default();
        tasks.summary(file.path()).unwrap();
        assert_eq!(tasks.first_prompt(&prompt).unwrap().0, expected);
        // The completed local-image copy is not required for the Record source.
        file.as_file_mut().set_len(offset).unwrap();
        file.as_file_mut().seek(SeekFrom::Start(offset)).unwrap();
        writeln!(file, "{}", records[0]).unwrap();
        assert_eq!(tail(file.path(), 1).unwrap()[0].images, expected);
        assert_eq!(first_prompt(file.path(), &prompt).unwrap().0, expected);
    }

    #[test]
    fn prompt_images_keep_each_content_index_and_label() {
        let mut record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/codex-images/prompt-record.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let mut second = record["payload"]["content"].as_array().unwrap()[..3].to_vec();
        let distinct: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/codex-images/wrapped.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        second[1]["image_url"] = distinct["payload"]["content"][1]["image_url"].clone();
        second[0]["text"] = second[0]["text"]
            .as_str()
            .unwrap()
            .replace("[Image #1]", "[Image #2]")
            .into();
        record["payload"]["content"]
            .as_array_mut()
            .unwrap()
            .extend(second);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        let events = tail(file.path(), 1).unwrap();
        let images = &events[0].images;
        assert_eq!(images.len(), 2);
        for (image, (label, index)) in images.iter().zip([("[Image #1]", 1), ("[Image #2]", 5)]) {
            assert_eq!(image.label, label);
            assert_eq!(
                image.source,
                ImageSource::Record {
                    record: crate::event::SourceRecord {
                        path: file.path().to_owned(),
                        offset: 0
                    },
                    pointer: format!("/payload/content/{index}/image_url"),
                }
            );
        }
    }

    #[test]
    fn prompt_attachments_recheck_paths_and_keep_physical_line_pairing() {
        let mut records: Vec<Value> =
            include_str!("../../../tests/fixtures/codex-images/fallback.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        records[0]["payload"]["content"] =
            Value::Array(vec![records[0]["payload"]["content"][3].clone()]);
        let mut completed = records[1].clone();
        let image = tempfile::NamedTempFile::new().unwrap();
        completed["payload"]["item"]["content"][1]["path"] = image.path().to_str().unwrap().into();
        let prompt = user_prompt(&records[0]["payload"]["content"])
            .body()
            .unwrap();
        let prompt: String = prompt.chars().take(512).collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}\n{completed}", records[0]).unwrap();
        let mut tasks = Tasks::default();
        tasks.summary(file.path()).unwrap();
        let metadata = image.as_file().metadata().unwrap();
        assert_eq!(
            tasks.first_prompt(&prompt).unwrap().0,
            vec![Image {
                label: "[Image #1]".into(),
                source: ImageSource::File {
                    path: image.path().to_owned(),
                    len: metadata.len(),
                    modified: metadata.modified().unwrap(),
                },
            }]
        );
        image.close().unwrap();
        assert!(tasks.first_prompt(&prompt).unwrap().0.is_empty());
        std::fs::write(
            file.path(),
            format!("{}\ninvalid json\n{completed}\n", records[0]),
        )
        .unwrap();
        let image = tempfile::NamedTempFile::new().unwrap();
        completed["payload"]["item"]["content"][1]["path"] = image.path().to_str().unwrap().into();
        std::fs::write(
            file.path(),
            format!("{}\ninvalid json\n{completed}\n", records[0]),
        )
        .unwrap();
        tasks.summary(file.path()).unwrap();
        assert_eq!(
            tasks.first_prompt(&prompt).unwrap(),
            first_prompt(file.path(), &prompt).unwrap()
        );
    }

    #[test]
    fn summary_cache_resets_on_truncate_replace_and_same_length_rewrite() {
        let text = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(text.as_bytes()).unwrap();
        let mut tasks = Tasks::default();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
        std::fs::write(
            file.path(),
            text.replace("Checks passed.", "Checks failed."),
        )
        .unwrap();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
        std::fs::write(file.path(), text.lines().next().unwrap()).unwrap();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
        let mut replacement =
            tempfile::NamedTempFile::new_in(file.path().parent().unwrap()).unwrap();
        replacement.write_all(text.as_bytes()).unwrap();
        replacement.persist(file.path()).unwrap();
        assert_eq!(tasks.summary(file.path()).unwrap(), expected(file.path()));
    }
}

#[cfg(test)]
mod paging_tests {
    #[test]
    fn expanding_pages_reuse_previously_converted_real_events() {
        use super::*;
        use std::io::Write;
        fn ids(value: &mut Value, suffix: usize) {
            match value {
                Value::Object(object) => {
                    for (key, value) in object {
                        if matches!(key.as_str(), "id" | "call_id" | "turn_id" | "root_turn_id") {
                            if let Some(text) = value.as_str() {
                                *value = format!("{text}-{suffix}").into();
                            }
                        } else {
                            ids(value, suffix);
                        }
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        ids(value, suffix);
                    }
                }
                _ => {}
            }
        }
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for suffix in 0..80 {
            for line in include_str!("../../../tests/fixtures/codex-rollout.jsonl").lines() {
                let mut record: Value = serde_json::from_str(line).unwrap();
                ids(&mut record, suffix);
                writeln!(file, "{record}").unwrap();
            }
        }
        let mut reader = Reader::new(file.path().to_owned(), false);
        EVENT_CONVERSIONS.with(|count| count.set(0));
        for requested in [1, 20, 40, 80, usize::MAX] {
            let update = reader.refresh(requested).unwrap();
            let converted = EVENT_CONVERSIONS.with(|count| count.get());
            // A retained row is decoded again when an earlier source arrives.
            let paired_sources: usize = reader
                .entries
                .iter()
                .map(|(_, event)| event.source_records.len().saturating_sub(1))
                .sum();
            assert!(
                converted <= reader.records.len() + paired_sources,
                "converted {converted} records for {} retained records and {paired_sources} paired sources",
                reader.records.len()
            );
            let before = converted;
            assert_eq!(update.events, tail(file.path(), requested).unwrap());
            EVENT_CONVERSIONS.with(|count| count.set(before));
        }
    }

    #[test]
    fn backwards_paging_converts_each_record_once() {
        use super::*;
        use std::io::Write;
        let text = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for _ in 0..80 {
            file.write_all(text.as_bytes()).unwrap();
        }
        let mut reader = Reader::new(file.path().to_owned(), false);
        EVENT_CONVERSIONS.with(|count| count.set(0));
        let update = reader.refresh(usize::MAX).unwrap();
        let converted = EVENT_CONVERSIONS.with(|count| count.get());
        assert!(
            converted <= reader.records.len(),
            "converted {converted} records for {} retained records",
            reader.records.len()
        );
        assert_eq!(update.events, tail(file.path(), usize::MAX).unwrap());
    }

    use super::*;
    use std::io::Write;

    #[test]
    fn backward_pages_match_full_real_rollouts_across_record_boundaries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for name in [
            "codex-rollout.jsonl",
            "codex-agent-exchange.jsonl",
            "codex-agent-messages/sequence.jsonl",
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for line in std::fs::read_to_string(root.join(name)).unwrap().lines() {
                let mut record: Value = serde_json::from_str(line).unwrap();
                // Keep the captured payload intact and put adjacent records in separate pages.
                record["paging_padding"] = "x".repeat(CHUNK as usize).into();
                writeln!(file, "{record}").unwrap();
            }
            let expected = tail(file.path(), usize::MAX).unwrap();
            let mut reader = Reader::new(file.path().to_path_buf(), false);
            reader.refresh(1).unwrap();
            let expanded = reader.refresh(usize::MAX).unwrap();
            assert_eq!(expanded.events, expected, "{name}");
            assert_eq!(reader.refresh(usize::MAX).unwrap().parsed_bytes, 0);
        }
    }

    #[test]
    fn completing_pending_record_reuses_its_owned_value() {
        let fixture = include_str!("../../../tests/fixtures/codex-rollout.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(fixture.trim_end().as_bytes()).unwrap();
        let mut reader = Reader::new(file.path().to_path_buf(), false);
        reader.refresh(usize::MAX).unwrap();
        assert!(reader.pending_offset.is_some());
        file.write_all(b"\n").unwrap();
        let completed = reader.refresh(usize::MAX).unwrap();
        assert_eq!(completed.events, tail(file.path(), usize::MAX).unwrap());
        assert_eq!(completed.parsed_bytes, 1);
        assert!(reader.pending_offset.is_none());
    }
}

#[cfg(test)]
mod selective_event_tests {
    use super::*;

    #[test]
    fn selected_events_match_full_events_without_converting_context_records() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for fixture in ["codex-rollout.jsonl", "codex-tool-calls.jsonl"] {
            let path = root.join(fixture);
            let records: Vec<_> = std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .enumerate()
                .map(|(offset, line)| (offset as u64, serde_json::from_str(line).unwrap()))
                .collect();
            let full = events(&path, &records, usize::MAX).unwrap();
            assert!(!full.is_empty());
            for (offset, expected) in full {
                EVENT_CONVERSIONS.with(|count| count.set(0));
                let selected = HashSet::from([offset]);
                let mut actual = events_selected(&path, &records, usize::MAX, &selected).unwrap();
                assert_eq!(actual.len(), 1, "{fixture} offset {offset}");
                let mut expected = expected;
                // Retained and selected entries receive their shared predecessor after combining.
                expected.previous_end = None;
                actual[0].1.previous_end = None;
                assert_eq!(
                    actual,
                    vec![(offset, expected)],
                    "{fixture} offset {offset}"
                );
                assert_eq!(EVENT_CONVERSIONS.with(std::cell::Cell::get), 1);
            }
            EVENT_CONVERSIONS.with(|count| count.set(0));
            assert!(
                events_selected(&path, &records, usize::MAX, &HashSet::new())
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(EVENT_CONVERSIONS.with(std::cell::Cell::get), 0);
        }
    }
}

#[cfg(test)]
#[path = "rollout_source_tests.rs"]
mod source_tests;

#[cfg(test)]
#[path = "rollout_pinned_reader_tests.rs"]
mod pinned_reader_tests;

#[cfg(test)]
#[path = "rollout_image_tests.rs"]
mod image_tests;
