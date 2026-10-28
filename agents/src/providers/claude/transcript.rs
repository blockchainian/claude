use crate::event::SourceRecord;
use crate::model::{Activity, BackgroundTask, PullRequest, SessionTask, TaskStatus};
use crate::providers::pull_request::{CreatedPullRequest, creates_pull_request};
use crate::providers::text::printable;
use crate::providers::tool::question_texts;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const CHUNK: u64 = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
    pub tasks: Vec<SessionTask>,
    task_replay: TaskReplay,
    pr_replay: PullRequestReplay,
    model_history: ModelHistory,
    prompt_cache: PromptCache,
    pub task_details_sources: [Option<String>; 3],
    pub started_at: Option<DateTime<Utc>>,
    pub subagent_records: Vec<Value>,
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub pr: Option<PullRequest>,
    pub prompt: String,
    pub prompt_images: Vec<crate::images::Image>,
    pub queued: Vec<String>,
    pub prompt_at: Option<DateTime<Utc>>,
    pub turn_messages: usize,
    pub ai_title: Option<String>,
    pub custom_title: Option<String>,
    pub last_prompt: Option<String>,
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub pending_questions: Vec<String>,
    pub background: Vec<BackgroundTask>,
    pub completed_background: HashSet<String>,
    pub background_ids: HashSet<String>,
    pub model: Option<String>,
    /// Human-readable names keyed by the model ids named in the transcript.
    pub model_identities: HashMap<String, String>,
    pub effort: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub cache_expires_at: Option<DateTime<Utc>>,
}

/// Replayed alongside task history so bounded tails retain the request and TTL.
#[derive(Clone, Debug, Default)]
struct PromptCache {
    ttl: Option<chrono::Duration>,
    input_at: Option<DateTime<Utc>>,
    request_at: Option<DateTime<Utc>>,
    message_id: Option<String>,
}

impl PromptCache {
    fn record(&mut self, record: &Value, expires_at: &mut Option<DateTime<Utc>>) {
        if record["type"] == "user" {
            self.input_at = record["timestamp"]
                .as_str()
                .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&Utc));
            return;
        }
        if record["type"] != "assistant" {
            return;
        }
        let message_id = record["message"]["id"].as_str();
        // Streamed blocks share an id; later blocks must keep the first anchor.
        if message_id != self.message_id.as_deref() || self.request_at.is_none() {
            self.request_at = self.input_at;
            self.message_id = message_id.map(str::to_owned);
        }
        let creation = &record["message"]["usage"]["cache_creation"];
        if creation["ephemeral_1h_input_tokens"]
            .as_u64()
            .is_some_and(|tokens| tokens > 0)
        {
            self.ttl = Some(chrono::Duration::hours(1));
        } else if creation["ephemeral_5m_input_tokens"]
            .as_u64()
            .is_some_and(|tokens| tokens > 0)
        {
            self.ttl = Some(chrono::Duration::minutes(5));
        }
        *expires_at = self.request_at.zip(self.ttl).map(|(at, ttl)| at + ttl);
    }
}

#[derive(Clone, Debug, Default)]
struct TaskReplay {
    offset: u64,
    creates: HashSet<String>,
}

impl TaskReplay {
    fn record(&mut self, record: &Value, tasks: &mut Vec<SessionTask>) {
        let Some(blocks) = record["message"]["content"].as_array() else {
            return;
        };
        for block in blocks {
            if record["type"] == "assistant" && block["type"] == "tool_use" {
                let input = &block["input"];
                match block["name"]
                    .as_str()
                    .and_then(|name| crate::providers::tool::rule(name).task)
                {
                    Some(crate::providers::tool::TaskAction::Create) => {
                        if let Some(id) = block["id"].as_str() {
                            self.creates.insert(id.to_owned());
                        }
                    }
                    Some(crate::providers::tool::TaskAction::Update) => {
                        if let Some(id) = input["taskId"].as_str()
                            && let Some(task) = tasks.iter_mut().find(|task| task.id == id)
                        {
                            if let Some(subject) = text(input, "subject") {
                                task.subject = subject;
                            }
                            if let Some(status) = input["status"].as_str().and_then(task_status) {
                                task.status = status;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if record["type"] == "user"
                && block["type"] == "tool_result"
                && block["is_error"] != true
                && let Some(tool) = block["tool_use_id"].as_str()
                && self.creates.remove(tool)
                && let Some(id) = text(&record["toolUseResult"]["task"], "id")
                && let Some(subject) = text(&record["toolUseResult"]["task"], "subject")
            {
                if let Some(index) = tasks.iter().position(|task| task.id == id) {
                    if tasks[index].status == TaskStatus::Deleted {
                        tasks.remove(index);
                    } else {
                        // A fresh successful create can restart numbering within a transcript.
                        tasks.clear();
                    }
                }
                tasks.push(SessionTask {
                    id,
                    subject,
                    status: TaskStatus::Pending,
                });
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct PullRequestReplay {
    created: CreatedPullRequest,
    calls: HashSet<String>,
    background: HashSet<String>,
}

impl PullRequestReplay {
    fn record(&mut self, record: &Value, cwd: Option<&Path>) -> io::Result<()> {
        let Some(cwd) = cwd else {
            return Ok(());
        };
        if record["type"] == "assistant" {
            if let Some(blocks) = record["message"]["content"].as_array() {
                for block in blocks {
                    if block["type"] == "tool_use"
                        && block["name"] == "Bash"
                        && block["input"]["command"]
                            .as_str()
                            .is_some_and(creates_pull_request)
                        && let Some(id) = block["id"].as_str()
                    {
                        self.calls.insert(id.to_owned());
                    }
                }
            }
        } else if record["type"] == "user" {
            if let Some(blocks) = record["message"]["content"].as_array() {
                for block in blocks {
                    if block["type"] != "tool_result" {
                        continue;
                    }
                    let Some(id) = block["tool_use_id"]
                        .as_str()
                        .filter(|id| self.calls.remove(*id))
                    else {
                        continue;
                    };
                    let result = &record["toolUseResult"];
                    let has_pr = self
                        .created
                        .record(cwd, result["stdout"].as_str().unwrap_or_default());
                    if !has_pr && result["backgroundTaskId"].as_str().is_some() {
                        self.background.insert(id.to_owned());
                    }
                }
            }
        } else if record["type"] == "queue-operation"
            && record["operation"] == "enqueue"
            && let Some(content) = record["content"].as_str()
            && content.trim_start().starts_with("<task-notification>")
            && let Some(id) = notification_field(content, "tool-use-id")
            && self.background.contains(id)
            && let Some(path) = notification_field(content, "output-file")
        {
            match std::fs::read_to_string(path) {
                Ok(output) => {
                    self.created.record(cwd, &output);
                    self.background.remove(id);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn notification_field<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    content
        .split_once(&format!("<{name}>"))?
        .1
        .split_once(&format!("</{name}>"))
        .map(|(value, _)| value.trim())
}

fn task_status(status: &str) -> Option<TaskStatus> {
    match status {
        "pending" => Some(TaskStatus::Pending),
        "in_progress" => Some(TaskStatus::InProgress),
        "completed" => Some(TaskStatus::Completed),
        "deleted" => Some(TaskStatus::Deleted),
        _ => None,
    }
}

fn read_history(
    path: &Path,
    len: u64,
    previous: Option<&Summary>,
    sidechains: bool,
    summary: &mut Summary,
) -> io::Result<()> {
    if let Some(previous) = previous.filter(|previous| previous.task_replay.offset <= len) {
        summary
            .model_identities
            .extend(previous.model_identities.clone());
        summary.tasks = previous.tasks.clone();
        summary.task_replay = previous.task_replay.clone();
        summary.pr_replay = previous.pr_replay.clone();
        summary.model_history = previous.model_history.clone();
        summary.prompt_cache = previous.prompt_cache.clone();
        summary.cache_expires_at = previous.cache_expires_at;
    }
    if previous.is_none() {
        // Model attachments occur near the start, outside the bounded message tail.
        let head = BufReader::new(File::open(path)?.take(CHUNK));
        for line in head.lines().take(32) {
            let line = line?;
            if let Ok(record) = serde_json::from_str::<Value>(&line)
                && (sidechains || record["isSidechain"] != true)
                && let Some((id, name)) = model_identity(&record)
            {
                summary.model_identities.insert(id, name);
                break;
            }
        }
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(summary.task_replay.offset))?;
    let mut reader = BufReader::new(file.take(len - summary.task_replay.offset));
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader.read_until(b'\n', &mut line)?;
        if count == 0 || line.last() != Some(&b'\n') {
            break;
        }
        summary.task_replay.offset += count as u64;
        if let Ok(record) = serde_json::from_slice::<Value>(&line)
            && (sidechains || record["isSidechain"] != true)
        {
            summary.task_replay.record(&record, &mut summary.tasks);
            summary.pr_replay.record(&record, summary.cwd.as_deref())?;
            summary
                .model_history
                .record(&record, &mut summary.model_identities);
            summary
                .prompt_cache
                .record(&record, &mut summary.cache_expires_at);
            if let Some((id, name)) = model_identity(&record) {
                summary.model_identities.insert(id, name);
            }
        }
    }
    summary.pr = summary.pr_replay.created.pr.clone();
    summary.model = summary.model_history.model.clone();
    Ok(())
}

/// Local commands change the selected model before another assistant reply exists.
#[derive(Clone, Debug, Default)]
struct ModelHistory {
    model: Option<String>,
    command: Option<(String, String)>,
}

impl ModelHistory {
    fn record(&mut self, record: &Value, identities: &mut HashMap<String, String>) {
        if record["type"] == "assistant" {
            if let Some(model) = assistant_model(record) {
                self.model = Some(model);
            }
            return;
        }
        if record["type"] != "user" {
            return;
        }
        let Some(content) = record["message"]["content"].as_str() else {
            return;
        };
        if let Some(args) = content
            .strip_prefix("<command-name>/model</command-name>")
            .and_then(|content| content.split_once("<command-args>"))
            .and_then(|(_, args)| args.strip_suffix("</command-args>"))
            .filter(|args| !args.is_empty() && !args.chars().any(char::is_whitespace))
        {
            self.command = text(record, "uuid").map(|id| (id, args.to_owned()));
        } else if let Some(name) = content
            .strip_prefix("<local-command-stdout>Set model to `")
            .and_then(|output| output.strip_suffix("</local-command-stdout>"))
            .and_then(|output| output.split_once('`'))
            .map(|(name, _)| name)
            && let Some((id, model)) = &self.command
            && record["parentUuid"].as_str() == Some(id)
        {
            self.model = Some(model.clone());
            identities.insert(model.clone(), name.to_owned());
            self.command = None;
        }
    }
}

/// Reads only through the first timestamped record, including sidechains.
pub(super) fn start_time(path: &Path) -> io::Result<Option<DateTime<Utc>>> {
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(timestamp) = record.get("timestamp") else {
            continue;
        };
        if record.get("forkedFrom").is_some() {
            return Ok(File::open(path)?.metadata()?.created().ok().map(Into::into));
        }
        let timestamp = timestamp.as_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Transcript timestamp must be a string",
            )
        })?;
        return DateTime::parse_from_rfc3339(timestamp)
            .map(|at| Some(at.with_timezone(&Utc)))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }
    Ok(None)
}

impl Summary {
    fn record_metadata(&mut self, record: &Value) {
        if let Some(timestamp) = record["timestamp"]
            .as_str()
            .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        {
            self.timestamp = Some(timestamp.with_timezone(&Utc));
        }
        // A head chunk swallowed by one huge record still leaves these in the tail.
        if self.cwd.is_none() {
            self.cwd = record["cwd"]
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from);
        }
        if matches!(record["type"].as_str(), Some("user" | "assistant")) {
            if self.cwd.is_none() {
                self.cwd = record["cwd"]
                    .as_str()
                    .filter(|cwd| !cwd.is_empty())
                    .map(PathBuf::from);
            }
            if self.git_branch.is_none() {
                self.git_branch = text(record, "gitBranch");
            }
        }
        match record["type"].as_str() {
            Some("ai-title") => {
                if let Some(title) = text(record, "aiTitle") {
                    self.ai_title = Some(title);
                }
            }
            Some("custom-title") => {
                if let Some(title) = text(record, "customTitle") {
                    self.custom_title = Some(title);
                }
            }
            Some("last-prompt") => {
                if let Some(prompt) = text(record, "lastPrompt") {
                    self.last_prompt = Some(prompt);
                }
            }
            Some("assistant") => self.record_assistant_metadata(record),
            _ => {}
        }
    }

    fn record_assistant_metadata(&mut self, record: &Value) {
        if let Some(model) = assistant_model(record) {
            self.model = Some(model);
        }
        if let Some(effort) = text(record, "effort") {
            self.effort = Some(effort);
        }
    }
}

/// The model id and human-readable name of a model attachment, such as "Opus 5.5".
fn model_identity(record: &Value) -> Option<(String, String)> {
    let attachment = &record["attachment"];
    if attachment["type"] != "model" {
        return None;
    }
    let identity = &attachment["identity"];
    Some((text(identity, "modelId")?, text(identity, "marketingName")?))
}

fn assistant_model(record: &Value) -> Option<String> {
    text(record, "requestedModel").or_else(|| text(&record["message"], "model"))
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)?
        .as_str()
        .map(printable)
        .filter(|text| !text.trim().is_empty())
}

pub(super) fn message_source(record: &Value) -> Option<String> {
    let uuid = record["uuid"].as_str()?;
    let index = record["message"]["content"]
        .as_array()?
        .iter()
        .rposition(|block| block["type"] == "text" && block["text"].is_string())?;
    Some(format!("{uuid}:text:{index}"))
}

pub(super) fn content_text(record: &Value, last: bool) -> Option<String> {
    let content = record.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let blocks = content.as_array()?;
    let mut texts = blocks
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block.get("text").and_then(Value::as_str));
    if last {
        texts.next_back().map(str::to_owned)
    } else {
        Some(texts.collect::<String>())
    }
}

fn records(bytes: &[u8]) -> impl DoubleEndedIterator<Item = Value> + '_ {
    records_with_sidechains(bytes, false)
}

fn records_with_sidechains(
    bytes: &[u8],
    sidechains: bool,
) -> impl DoubleEndedIterator<Item = Value> + '_ {
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(move |record| sidechains || record["isSidechain"] != true)
}

enum QueueOperation {
    Enqueue(String),
    Dequeue,
    Remove(String),
}

impl QueueOperation {
    fn from_record(record: &Value) -> Option<Self> {
        if record["type"] != "queue-operation" {
            return None;
        }
        match record["operation"].as_str()? {
            "enqueue" => Some(Self::Enqueue(record["content"].as_str()?.to_owned())),
            "dequeue" => Some(Self::Dequeue),
            "remove" => Some(Self::Remove(record["content"].as_str()?.to_owned())),
            _ => None,
        }
    }
}

/// Text of a prompt the model acted on: a typed user message, or a queued
/// message absorbed into a running turn, which Claude Code stores as an
/// attachment instead of a user record. A message from another session or
/// agent is a prompt only when `peers` is set, as in a subagent, whose
/// prompts all come from its parent.
fn prompt_text(record: &Value, peers: bool) -> Option<String> {
    let peer = |(name, body)| peers.then(|| crate::event::participant_text(name, body));
    match record["type"].as_str()? {
        "user" => {
            if let Some(message) = super::peer::origin_message(record) {
                return peer(message);
            }
            if record["isMeta"] == true {
                return None;
            }
            let text = content_text(record, false)?;
            let text = if text.trim().is_empty() {
                super::images::prompt_text(record)?
            } else {
                text
            };
            match super::peer::wrapped_message(&text) {
                Some((_, message)) => peer(message),
                None => Some(typed_text(text)),
            }
        }
        "attachment" => {
            super::images::queued_prompt(record)?;
            if let Some(message) = super::peer::origin_message(record) {
                return peer(message);
            }
            super::images::prompt_text(record).map(typed_text)
        }
        _ => None,
    }
}

/// Text the user typed, with pasted blocks shown as their content.
fn typed_text(text: String) -> String {
    super::event_reader::prompt_body(&text)
}

fn queued_prompts(operations: Vec<QueueOperation>) -> Vec<String> {
    let mut queue = VecDeque::new();
    for operation in operations.into_iter().rev() {
        match operation {
            QueueOperation::Enqueue(content) => queue.push_back(content),
            QueueOperation::Dequeue => {
                queue.pop_front();
            }
            QueueOperation::Remove(content) => {
                if let Some(index) = queue.iter().position(|item| item == &content) {
                    queue.remove(index);
                }
            }
        }
    }
    queue
        .into_iter()
        .filter(|content| {
            super::peer::wrapped_message(content).is_none()
                && !content.trim_start().starts_with("<task-notification>")
        })
        .map(typed_text)
        .collect()
}

#[derive(Default)]
struct Background {
    completed: HashSet<String>,
    pending: HashMap<String, (String, PathBuf)>,
    tasks: Vec<(String, BackgroundTask)>,
    requested: HashMap<String, PathBuf>,
}

impl Background {
    fn record(&mut self, record: &Value, at: Option<DateTime<Utc>>) {
        if record["type"] == "attachment" {
            let attachment = &record["attachment"];
            if attachment["type"] == "queued_command"
                && attachment["commandMode"] == "task-notification"
                && let Some(prompt) = attachment["prompt"].as_str()
                && let Some((_, rest)) = prompt.split_once("<task-id>")
                && let Some((task, _)) = rest.split_once("</task-id>")
            {
                self.completed.insert(task.to_owned());
                self.requested.remove(task);
                self.pending.retain(|_, (id, _)| id != task);
            }
        }
        let Some(blocks) = record["message"]["content"].as_array() else {
            return;
        };
        for block in blocks.iter().rev() {
            if record["type"] == "user"
                && block["type"] == "tool_result"
                && let Some(id) = block["tool_use_id"].as_str()
                && let Some(task) = record["toolUseResult"]["backgroundTaskId"].as_str()
                && !self.completed.contains(task)
                && let Some(output) = self.requested.remove(task)
            {
                self.pending
                    .insert(id.to_owned(), (task.to_owned(), output));
            }

            if record["type"] == "assistant"
                && block["type"] == "tool_use"
                && block["name"].as_str().is_some_and(|name| {
                    crate::providers::tool::presentation(
                        name,
                        &crate::providers::tool::ToolData {
                            input: &block["input"],
                            result: None,
                        },
                    )
                    .input
                    .iter()
                    .any(|section| matches!(section, crate::event::Section::Shell { .. }))
                })
                && let Some(id) = block["id"].as_str()
                && let Some((task, output)) = self.pending.remove(id)
                && let Some(started_at) = at
                && let Some(description) = block["input"]["description"]
                    .as_str()
                    .filter(|description| !description.trim().is_empty())
                    .or_else(|| block["input"]["command"].as_str()?.lines().next())
            {
                self.tasks.push((
                    task,
                    BackgroundTask {
                        description: description.to_owned(),
                        output: Some(output),
                        started_at,
                    },
                ));
            }
        }
    }
}

/// Resolve only file-confirmed running ids, continuing through queue quiet points.
pub(super) fn background_tasks(
    path: &Path,
    len: u64,
    requested: &HashMap<String, PathBuf>,
) -> io::Result<(HashMap<String, BackgroundTask>, u64)> {
    let mut file = CountingReader {
        inner: File::open(path)?,
        bytes: 0,
    };
    let start = len.saturating_sub(CHUNK);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    (&mut file).take(len - start).read_to_end(&mut tail)?;
    let background = read_messages(
        &mut file,
        SourceRecord {
            path: path.to_owned(),
            offset: 0,
        },
        start,
        tail,
        &mut Summary::default(),
        false,
        Background {
            requested: requested.clone(),
            ..Background::default()
        },
    )?;
    Ok((background.tasks.into_iter().collect(), file.bytes))
}

struct CountingReader<R> {
    inner: R,
    bytes: u64,
}
impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.bytes += count as u64;
        Ok(count)
    }
}
impl<R: Seek> Seek for CountingReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

/// Walk complete transcript records newest first with their starting byte offsets.
/// Retain a split first line for the preceding block.
/// The callback returns false once it has all the records it needs.
pub(super) fn walk_records(
    file: &mut (impl Read + Seek),
    mut start: u64,
    mut bytes: Vec<u8>,
    sidechains: bool,
    mut visit: impl FnMut(u64, Value) -> bool,
) -> io::Result<()> {
    let mut chunk = CHUNK;
    loop {
        // Keep the incomplete first line for the next block instead of dropping it.
        let skip = if start == 0 {
            0
        } else {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |index| index + 1)
        };
        let mut line_start = bytes.len();
        for line in bytes[skip..].split_inclusive(|byte| *byte == b'\n').rev() {
            line_start -= line.len();
            let line = line.strip_suffix(b"\n").unwrap_or(line);
            for record in records_with_sidechains(line, sidechains) {
                if !visit(start + line_start as u64, record) {
                    return Ok(());
                }
            }
        }
        if start == 0 {
            return Ok(());
        }
        bytes.truncate(skip);
        let previous = start.saturating_sub(chunk);
        file.seek(SeekFrom::Start(previous))?;
        let mut block = Vec::new();
        (&mut *file)
            .take(start - previous)
            .read_to_end(&mut block)?;
        block.extend_from_slice(&bytes);
        bytes = block;
        start = previous;
        chunk = chunk.saturating_mul(2);
    }
}

fn read_messages(
    file: &mut (impl Read + Seek),
    origin: SourceRecord,
    start: u64,
    bytes: Vec<u8>,
    summary: &mut Summary,
    sidechains: bool,
    mut background: Background,
) -> io::Result<Background> {
    let descriptions_only = !background.requested.is_empty();
    let mut prompt_found = false;
    let mut turn_messages = 0;
    // An absorbed queued prompt waits for its `remove` record, which carries
    // the moment the running turn actually picked it up.
    let mut absorbed = None;
    let mut operations = Vec::new();
    let mut next_user = None;
    let mut dequeued_user = None;
    let mut quiet_point = false;
    let mut questions_found = false;
    let mut model_found = false;
    let mut effort_found = false;
    let mut tool_results = HashSet::new();
    let mut images = super::images::PromptImages::default();
    let mut activity = super::event_reader::ActivityEvent::default();
    walk_records(file, start, bytes, sidechains, |offset, record| {
        let source = SourceRecord {
            path: origin.path.clone(),
            offset: origin.offset + offset,
        };
        if !descriptions_only && !activity.ready() {
            activity.record(record.clone());
        }
        if record["type"] == "assistant" {
            if !model_found && let Some(model) = assistant_model(&record) {
                summary.model = Some(model);
                model_found = true;
            }
            if !effort_found && let Some(effort) = text(&record, "effort") {
                summary.effort = Some(effort);
                effort_found = true;
            }
        }
        images.record(&source, &record);
        if !questions_found && let Some(blocks) = record["message"]["content"].as_array() {
            for block in blocks.iter().rev() {
                if record["type"] == "user"
                    && block["type"] == "tool_result"
                    && let Some(id) = block["tool_use_id"].as_str()
                {
                    tool_results.insert(id.to_owned());
                }
                if record["type"] == "assistant"
                    && block["type"] == "tool_use"
                    && block["name"]
                        .as_str()
                        .is_some_and(|name| crate::providers::tool::rule(name).question)
                    && let Some(id) = block["id"].as_str()
                {
                    questions_found = true;
                    if !tool_results.contains(id) {
                        summary.pending_questions = question_texts(&block["input"]);
                    }
                    break;
                }
            }
        }
        if !quiet_point {
            if record["type"] == "user" {
                next_user = content_text(&record, false);
            }
            if let Some(operation) = QueueOperation::from_record(&record) {
                // Enqueue → dequeue → matching user means FIFO had nothing
                // older pending before this enqueue; replay can start here.
                quiet_point = matches!(&operation, QueueOperation::Enqueue(content)
                        if dequeued_user.as_ref() == Some(content));
                dequeued_user = match &operation {
                    QueueOperation::Dequeue => next_user.clone(),
                    _ => None,
                };
                operations.push(operation);
            }
        }
        let at = record["timestamp"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));
        background.record(&record, at);
        if descriptions_only && background.requested.is_empty() && background.pending.is_empty() {
            return false;
        }
        if let Some(id) = record["toolUseResult"]["backgroundTaskId"].as_str() {
            summary.background_ids.insert(id.to_owned());
        }
        if !prompt_found && record["type"] == "assistant" {
            turn_messages += 1;
        }
        if absorbed.is_some()
            && record["type"] == "queue-operation"
            && record["operation"] == "remove"
            && record["commandUuid"].as_str() == absorbed.as_deref()
        {
            summary.prompt_at = at;
            absorbed = None;
        }
        if !prompt_found
            && let Some(prompt) = prompt_text(&record, sidechains)
            && !prompt.trim().is_empty()
            && super::event_reader::is_prompt_text(&record, &prompt)
        {
            summary.task_details_sources[2] = record["uuid"].as_str().map(str::to_owned);
            summary.prompt = printable(&prompt);
            summary.prompt_images = images.take(&source, &record, &summary.prompt);
            summary.prompt_at = at;
            summary.turn_messages = turn_messages;
            prompt_found = true;
            if record["type"] == "attachment" {
                absorbed = record["attachment"]["source_uuid"]
                    .as_str()
                    .map(str::to_owned);
            }
        }
        if summary.last_message.is_none()
            && record["type"] == "assistant"
            && let Some(message) = content_text(&record, true)
        {
            summary.task_details_sources[1] = message_source(&record);
            summary.last_message = Some(printable(&message));
            summary.last_message_at = at;
        }
        if prompt_found
            && model_found
            && effort_found
            && absorbed.is_none()
            && summary.last_message.is_some()
            && activity.ready()
            && quiet_point
            && background.requested.is_empty()
            && background.pending.is_empty()
        {
            return false;
        }
        true
    })?;
    if let Some(event) = activity.finish()? {
        summary.task_details_sources[0] = event.calls.first().map(|call| call.id.clone());
        summary.activity = Some(event.activity());
    }
    summary.completed_background = background.completed.clone();
    summary.queued = queued_prompts(operations);
    Ok(background)
}

#[cfg(test)]
pub(super) fn read(path: &Path, len: u64) -> io::Result<Summary> {
    read_cached(path, len, None)
}

pub(super) fn read_cached(
    path: &Path,
    len: u64,
    previous: Option<&Summary>,
) -> io::Result<Summary> {
    let mut summary = read_summary(path, len)?;
    read_history(path, len, previous, false, &mut summary)?;
    Ok(summary)
}

fn read_summary(path: &Path, len: u64) -> io::Result<Summary> {
    let mut file = File::open(path)?;
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut tail_skip = 0;
    if len <= 2 * CHUNK {
        (&mut file).take(2 * CHUNK).read_to_end(&mut head)?;
    } else {
        (&mut file).take(CHUNK).read_to_end(&mut head)?;
        // The preceding byte tells whether the tail begins on a complete line.
        file.seek(SeekFrom::Start(len - CHUNK - 1))?;
        (&mut file).take(CHUNK + 1).read_to_end(&mut tail)?;
        tail_skip = if tail.first() == Some(&b'\n') {
            1
        } else {
            tail.iter()
                .position(|byte| *byte == b'\n')
                .map_or(tail.len(), |index| index + 1)
        };
    }
    let mut summary = Summary::default();
    for record in records(&head) {
        if !matches!(record["type"].as_str(), Some("user" | "assistant")) {
            continue;
        }
        if summary.cwd.is_none() {
            summary.cwd = record["cwd"]
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from);
        }
        if summary.git_branch.is_none() {
            summary.git_branch = text(&record, "gitBranch");
        }
    }
    let metadata = if len <= 2 * CHUNK {
        &head[..]
    } else {
        &tail[tail_skip..]
    };
    for record in records(metadata) {
        summary.record_metadata(&record);
    }
    if len <= 2 * CHUNK {
        read_messages(
            &mut file,
            SourceRecord {
                path: path.to_owned(),
                offset: 0,
            },
            0,
            head,
            &mut summary,
            false,
            Background::default(),
        )?;
    } else {
        read_messages(
            &mut file,
            SourceRecord {
                path: path.to_owned(),
                offset: 0,
            },
            len - CHUNK - 1,
            tail,
            &mut summary,
            false,
            Background::default(),
        )?;
    }
    Ok(summary)
}

/// Reads one tail chunk; subagent files have no queue quiet point to bound a walk.
#[cfg(test)]
pub(super) fn read_subagent(path: &Path, len: u64) -> io::Result<Summary> {
    read_subagent_cached(path, len, None)
}

pub(super) fn read_subagent_cached(
    path: &Path,
    len: u64,
    previous: Option<&Summary>,
) -> io::Result<Summary> {
    let mut summary = read_subagent_tail(path, &mut File::open(path)?, len)?;
    read_history(path, len, previous, true, &mut summary)?;
    Ok(summary)
}

pub(super) fn read_subagent_tail(
    path: &Path,
    file: &mut (impl Read + Seek),
    len: u64,
) -> io::Result<Summary> {
    let start = len.saturating_sub(CHUNK + 1);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(CHUNK + 1).read_to_end(&mut bytes)?;
    let mut origin = start;
    if start > 0 {
        let skip = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1);
        bytes.drain(..skip);
        origin += skip as u64;
    }
    let mut summary = Summary::default();
    for record in records_with_sidechains(&bytes, true) {
        summary.record_metadata(&record);
        if summary.subagent_records.len() == 3 {
            summary.subagent_records.remove(0);
        }
        summary.subagent_records.push(record);
    }
    // Start zero prevents the message scan from seeking before this bounded tail.
    read_messages(
        &mut std::io::Cursor::new(Vec::<u8>::new()),
        SourceRecord {
            path: path.to_owned(),
            offset: origin,
        },
        0,
        bytes,
        &mut summary,
        true,
        Background::default(),
    )?;
    Ok(summary)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn walk_records_offsets_point_to_transcript_line_starts() {
        for fixture in [
            include_str!("../../../tests/fixtures/claude-prompt-images.jsonl"),
            include_str!("../../../tests/fixtures/claude-tool-calls.jsonl"),
            include_str!("../../../tests/fixtures/claude-subagent-model-history.jsonl"),
        ] {
            let bytes = fixture.repeat(3 * CHUNK as usize / fixture.len() + 1);
            let mut transcript = tempfile::NamedTempFile::new().unwrap();
            transcript.write_all(bytes.as_bytes()).unwrap();
            let mut offset = 0;
            let lines: Vec<_> = bytes
                .split_inclusive('\n')
                .map(|line| {
                    let at = offset;
                    offset += line.len() as u64;
                    (at, serde_json::from_str::<Value>(line).unwrap())
                })
                .collect();
            let len = bytes.len() as u64;
            // Cover a full buffer, an empty tail, a split line, and a line boundary.
            for start in [0, len, len - CHUNK - 1, lines[lines.len() / 2].0] {
                for sidechains in [false, true] {
                    let expected: Vec<_> = lines
                        .iter()
                        .rev()
                        .filter(|(_, record)| sidechains || record["isSidechain"] != true)
                        .cloned()
                        .collect();
                    let mut actual = Vec::new();
                    walk_records(
                        transcript.as_file_mut(),
                        start,
                        bytes.as_bytes()[start as usize..].to_vec(),
                        sidechains,
                        |offset, record| {
                            actual.push((offset, record));
                            true
                        },
                    )
                    .unwrap();
                    assert_eq!(actual, expected, "start={start}, sidechains={sidechains}");
                    let mut file = BufReader::new(File::open(transcript.path()).unwrap());
                    for (offset, record) in actual {
                        if offset > 0 {
                            assert_eq!(bytes.as_bytes()[offset as usize - 1], b'\n');
                        }
                        file.seek(SeekFrom::Start(offset)).unwrap();
                        let mut line = String::new();
                        file.read_line(&mut line).unwrap();
                        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), record);
                    }
                }
            }
        }
    }

    #[test]
    fn background_branch_reaches_task_details_border() {
        let mut fixture = tempfile::NamedTempFile::new().unwrap();
        fixture
            .write_all(include_bytes!(
                "../../../tests/fixtures/claude-background-branch.jsonl"
            ))
            .unwrap();
        let live = std::env::var_os("AGENTS_BRANCH_TRANSCRIPT").map(PathBuf::from);
        let path = live.as_deref().unwrap_or(fixture.path());
        let summary = read(path, std::fs::metadata(path).unwrap().len()).unwrap();
        assert_eq!(summary.git_branch.as_deref(), Some("example.com"));
        assert!(summary.pr.is_none());
        eprintln!(
            "parsed cwd={:?}, branch={:?}, pr={:?}",
            summary.cwd, summary.git_branch, summary.pr
        );
        let mut session = super::super::session(
            "branch-regression".into(),
            summary,
            Some(path.into()),
            Utc::now(),
        );
        session.title = "Branch regression".into();
        session.status = crate::model::SessionStatus::Ready;
        let mut view = crate::ui::View::new(crate::model::Grouping::Project, false);
        view.set_sessions(vec![session], false);
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 149, 40));
        view.render(buffer.area, &mut buffer);
        let footer = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .find(|line| line.contains("Branch regression") && line.contains("─"))
            .unwrap();
        assert!(footer.contains("example.com"), "{footer}");
    }

    fn cache_records() -> Vec<Value> {
        include_str!("../../../tests/fixtures/prompt-cache/claude.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn write_cache_record(file: &mut tempfile::NamedTempFile, record: &Value) {
        writeln!(file, "{record}").unwrap();
    }

    fn cached_expiry(file: &tempfile::NamedTempFile, previous: Option<&Summary>) -> Summary {
        read_cached(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            previous,
        )
        .unwrap()
    }

    #[test]
    fn prompt_cache_uses_request_input_instead_of_stream_completion() {
        let records = cache_records();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in &records {
            write_cache_record(&mut file, record);
        }
        let first = cached_expiry(&file, None);
        let expiry = DateTime::parse_from_rfc3339("2026-10-06T17:39:12.595Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(first.cache_expires_at, Some(expiry));
        let mut streamed = records[1].clone();
        streamed["timestamp"] = Value::from("2026-10-06T16:45:29.554Z");
        write_cache_record(&mut file, &streamed);
        assert_eq!(
            cached_expiry(&file, Some(&first)).cache_expires_at,
            Some(expiry)
        );
        assert_eq!(cached_expiry(&file, None).cache_expires_at, Some(expiry));
    }

    #[test]
    fn prompt_cache_retains_ttl_across_tool_results_and_bounded_reads() {
        let records = cache_records();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in &records {
            write_cache_record(&mut file, record);
        }
        writeln!(file, "{}", " ".repeat(CHUNK as usize * 3)).unwrap();
        let mut input: Value = include_str!("../../../tests/fixtures/claude-todos.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|record| {
                record["type"] == "user"
                    && record["message"]["content"]
                        .as_array()
                        .is_some_and(|blocks| {
                            blocks.iter().any(|block| block["type"] == "tool_result")
                        })
            })
            .unwrap();
        input["timestamp"] = Value::from("2026-10-06T16:50:00Z");
        write_cache_record(&mut file, &input);
        let first = cached_expiry(&file, None);
        let mut assistant = records[1].clone();
        assistant["message"]["id"] = Value::from("msg_next_request");
        assistant["timestamp"] = Value::from("2026-10-06T16:57:00Z");
        assistant["message"]["usage"]["cache_creation"]["ephemeral_1h_input_tokens"] =
            Value::from(0);
        write_cache_record(&mut file, &assistant);
        let expiry = DateTime::parse_from_rfc3339("2026-10-06T17:50:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            cached_expiry(&file, Some(&first)).cache_expires_at,
            Some(expiry)
        );
        assert_eq!(cached_expiry(&file, None).cache_expires_at, Some(expiry));
    }

    #[test]
    fn prompt_cache_requires_known_ttl_and_request_timestamp() {
        let records = cache_records();
        for (hour, five, input, expected) in [
            (0, 0, true, None),
            (0, 23, true, Some("2026-10-06T16:44:12.595Z")),
            (12, 23, true, Some("2026-10-06T17:39:12.595Z")),
            (12, 0, false, None),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            if input {
                write_cache_record(&mut file, &records[0]);
            }
            let mut assistant = records[1].clone();
            assistant["message"]["usage"]["cache_creation"]["ephemeral_1h_input_tokens"] =
                Value::from(hour);
            assistant["message"]["usage"]["cache_creation"]["ephemeral_5m_input_tokens"] =
                Value::from(five);
            write_cache_record(&mut file, &assistant);
            assert_eq!(
                cached_expiry(&file, None).cache_expires_at,
                expected.map(|at| DateTime::parse_from_rfc3339(at)
                    .unwrap()
                    .with_timezone(&Utc))
            );
        }
    }

    #[test]
    fn successful_model_command_updates_before_another_assistant_reply() {
        let fixture = include_str!("../../../tests/fixtures/claude-model-command.jsonl");
        let rows: Vec<_> = fixture.lines().collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", rows[0]).unwrap();
        let mut previous = summary(&file);
        assert_eq!(previous.model.as_deref(), Some("claude-fable-5-1"));
        for (index, row) in rows.iter().enumerate().skip(1) {
            writeln!(file, "{row}").unwrap();
            previous = read_cached(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                Some(&previous),
            )
            .unwrap();
            if index < 3 {
                assert_eq!(previous.model.as_deref(), Some("claude-fable-5-1"));
            }
        }
        assert_eq!(previous.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(previous.model_identities["claude-opus-5-5"], "Opus 5.5");
        assert_eq!(summary(&file).model, previous.model);
        // Neither the command nor its success need remain in the bounded tail.
        writeln!(file, "{}", " ".repeat(CHUNK as usize * 3)).unwrap();
        assert_eq!(summary(&file).model, previous.model);
    }

    #[test]
    fn model_command_requires_its_own_success_and_yields_to_later_replies() {
        let fixture = include_str!("../../../tests/fixtures/claude-model-command.jsonl");
        let rows: Vec<Value> = fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for row in &rows[..3] {
            writeln!(file, "{row}").unwrap();
        }
        let mut unrelated = rows[3].clone();
        unrelated["parentUuid"] = "another-command".into();
        writeln!(file, "{unrelated}").unwrap();
        assert_eq!(summary(&file).model.as_deref(), Some("claude-fable-5-1"));
        writeln!(file, "{}", rows[3]).unwrap();
        assert_eq!(summary(&file).model.as_deref(), Some("claude-opus-5-5"));
        // A later assistant request is newer evidence of the active model.
        writeln!(file, "{}", rows[0]).unwrap();
        assert_eq!(summary(&file).model.as_deref(), Some("claude-fable-5-1"));
    }

    #[test]
    fn model_identity_survives_bounded_reads_and_cached_appends() {
        for (fixture, subagent, label) in [
            (
                include_str!("../../../tests/fixtures/claude-main-model-history.jsonl"),
                false,
                "Opus 5.5",
            ),
            (
                include_str!("../../../tests/fixtures/claude-subagent-model-history.jsonl"),
                true,
                "Fable 5.1",
            ),
        ] {
            let (identity, messages) = fixture.split_once('\n').unwrap();
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(
                file,
                "{identity}\n{}\n{messages}",
                " ".repeat(CHUNK as usize * 3)
            )
            .unwrap();
            let read = if subagent {
                read_subagent_cached
            } else {
                read_cached
            };
            let first = read(file.path(), file.as_file().metadata().unwrap().len(), None).unwrap();
            assert_eq!(
                first
                    .model_identities
                    .get(first.model.as_ref().unwrap())
                    .map(String::as_str),
                Some(label)
            );
            writeln!(file, "{messages}").unwrap();
            let appended = read(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                Some(&first),
            )
            .unwrap();
            assert_eq!(
                appended
                    .model_identities
                    .get(appended.model.as_ref().unwrap())
                    .map(String::as_str),
                Some(label)
            );
        }
    }

    #[test]
    fn model_identity_cache_keeps_names_across_model_switches() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let subagent = include_str!("../../../tests/fixtures/claude-subagent-model-history.jsonl");
        let main = include_str!("../../../tests/fixtures/claude-main-model-history.jsonl");
        let mut previous = None;
        for (records, label) in [
            (subagent, "Fable 5.1"),
            (main, "Opus 5.5"),
            (subagent, "Fable 5.1"),
        ] {
            writeln!(file, "{records}").unwrap();
            let summary = read_subagent_cached(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                previous.as_ref(),
            )
            .unwrap();
            assert_eq!(
                summary
                    .model_identities
                    .get(summary.model.as_ref().unwrap())
                    .map(String::as_str),
                Some(label)
            );
            previous = Some(summary);
        }
        let summary = previous.unwrap();
        assert_eq!(summary.model_identities["claude-fable-5-1"], "Fable 5.1");
        assert_eq!(summary.model_identities["claude-opus-5-5"], "Opus 5.5");
    }

    #[test]
    fn task_replay_uses_result_ids_and_keeps_creation_order() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!("../../../tests/fixtures/claude-todos.jsonl"))
            .unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.tasks.len(), 3);
        assert_eq!(summary.tasks[0].subject, "Task 1");
        assert_eq!(
            summary.tasks[0].status,
            crate::model::TaskStatus::InProgress
        );
        assert_eq!(summary.tasks[1].status, crate::model::TaskStatus::Completed);
        assert_eq!(summary.tasks[2].status, crate::model::TaskStatus::Pending);
    }

    #[test]
    fn task_replay_reuses_deleted_ids_without_clearing_pending_tasks() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (index, id) in (1..=7).chain([7]).enumerate() {
            let id = id.to_string();
            let tool = format!("create-{index}");
            let subject = format!("Task {index}");
            for record in [
                serde_json::json!({"type":"assistant", "message":{"content":[{"type":"tool_use", "id":tool, "name":"TaskCreate", "input":{"subject":subject}}]}}),
                serde_json::json!({"type":"user", "message":{"content":[{"type":"tool_result", "tool_use_id":tool, "content":format!("Task #{id} created successfully: {subject}")}]}, "toolUseResult":{"task":{"id":id, "subject":subject}}}),
            ] {
                writeln!(file, "{record}").unwrap();
            }
            let deleted: &[&str] = match index {
                6 => &["1", "2", "7", "3"],
                7 => &["7"],
                _ => &[],
            };
            for id in deleted {
                writeln!(
                    file,
                    "{}",
                    serde_json::json!({"type":"assistant", "message":{"content":[{"type":"tool_use", "id":format!("delete-{index}-{id}"), "name":"TaskUpdate", "input":{"taskId":id, "status":"deleted"}}]}})
                )
                .unwrap();
                writeln!(
                    file,
                    "{}",
                    serde_json::json!({"type":"user", "message":{"content":[{"type":"tool_result", "tool_use_id":format!("delete-{index}-{id}"), "content":format!("Updated task #{id} deleted")}]}})
                )
                .unwrap();
            }
        }
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        let visible = summary
            .tasks
            .iter()
            .filter(|task| task.status != TaskStatus::Deleted)
            .map(|task| (task.id.as_str(), task.status))
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            [
                ("4", TaskStatus::Pending),
                ("5", TaskStatus::Pending),
                ("6", TaskStatus::Pending)
            ]
        );
    }

    #[test]
    fn task_replay_reads_middle_records_and_preserves_compacted_tasks() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"padding", "text":"x".repeat(CHUNK as usize * 3)})
        )
        .unwrap();
        file.write_all(include_bytes!("../../../tests/fixtures/claude-todos.jsonl"))
            .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"padding", "text":"x".repeat(CHUNK as usize * 3)})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"system", "subtype":"compact_boundary"})
        )
        .unwrap();
        let first = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(first.tasks.len(), 3);
        let update = serde_json::json!({"type":"assistant", "message":{"content":[{"type":"tool_use", "id":"rename", "name":"TaskUpdate", "input":{"taskId":"3", "subject":"Renamed", "status":"deleted"}}]}}).to_string();
        let split = update.len() / 2;
        file.write_all(&update.as_bytes()[..split]).unwrap();
        let partial = read_cached(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            Some(&first),
        )
        .unwrap();
        assert_eq!(partial.task_replay.offset, first.task_replay.offset);
        file.write_all(&update.as_bytes()[split..]).unwrap();
        file.write_all(b"\n").unwrap();
        let appended = read_cached(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            Some(&partial),
        )
        .unwrap();
        assert_eq!(appended.tasks[2].subject, "Renamed");
        assert_eq!(appended.tasks[2].status, crate::model::TaskStatus::Deleted);
        file.write_all(include_bytes!("../../../tests/fixtures/claude-todos.jsonl"))
            .unwrap();
        let restarted = read_cached(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            Some(&appended),
        )
        .unwrap();
        assert_eq!(restarted.tasks.len(), 3);
        assert_eq!(restarted.tasks[2].subject, "Task 3");
    }

    const FIXTURE: &str = include_str!("../../../tests/fixtures/transcript.jsonl");

    pub(in crate::providers::claude) fn image_fixture() -> (tempfile::TempDir, Vec<Value>) {
        let records = include_str!("../../../tests/fixtures/claude-prompt-images.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (tempfile::tempdir().unwrap(), records)
    }

    /// Asserts `images` are the labelled PNGs embedded in the transcript.
    pub(in crate::providers::claude) fn assert_record_images(
        images: &[crate::images::Image],
        numbers: &[u64],
    ) {
        assert_eq!(
            images
                .iter()
                .map(|image| image.label.clone())
                .collect::<Vec<_>>(),
            numbers
                .iter()
                .map(|number| format!("[Image #{number}]"))
                .collect::<Vec<_>>()
        );
        for image in images {
            let crate::images::ImageSource::Record { record, pointer } = &image.source else {
                panic!("expected transcript image")
            };
            let mut file = BufReader::new(File::open(&record.path).unwrap());
            file.seek(SeekFrom::Start(record.offset)).unwrap();
            let mut line = String::new();
            file.read_line(&mut line).unwrap();
            let value: Value = serde_json::from_str(&line).unwrap();
            assert!(value.pointer(pointer).unwrap().is_string());
            assert!(pointer.ends_with("/source/data"));
            assert!(image.source.bytes().unwrap().starts_with(b"\x89PNG\r\n"));
        }
    }

    #[test]
    fn task_details_prompt_images_join_the_selected_prompt() {
        let (dir, records) = image_fixture();
        let path = dir.path().join("transcript.jsonl");
        // Split the two-image companion from its prompt across read chunks.
        let padding = " ".repeat(CHUNK as usize * 3);
        let mut bytes = String::new();
        for (index, record) in records.iter().enumerate() {
            bytes.push_str(&format!("{record}\n"));
            if index == 4 {
                bytes.push_str(&format!("{padding}\n"));
            }
        }
        std::fs::write(&path, &bytes).unwrap();
        let summary = read(&path, bytes.len() as u64).unwrap();
        let session = super::super::session("test".into(), summary, Some(path.clone()), Utc::now());
        assert_record_images(&session.prompt_images, &[8, 9]);
        let offset = bytes.find(&format!("{}\n", records[4])).unwrap() as u64;
        for (index, image) in session.prompt_images.iter().enumerate() {
            assert_eq!(
                image.source,
                crate::images::ImageSource::Record {
                    record: SourceRecord {
                        path: path.clone(),
                        offset
                    },
                    pointer: format!("/message/content/{}/source/data", index + 1),
                }
            );
        }
    }

    #[test]
    fn prompt_image_blocks_survive_missing_ids_and_labels() {
        let (_, records) = image_fixture();
        for (ids, labelled) in [
            (None, true),
            (Some(vec![8]), true),
            (Some(vec![8, 9]), false),
        ] {
            let mut record = records[4].clone();
            record.as_object_mut().unwrap().remove("imagePasteIds");
            if let Some(ids) = &ids {
                record["imagePasteIds"] = serde_json::json!(ids);
            }
            if !labelled {
                let text = record["message"]["content"][0]["text"].as_str().unwrap();
                record["message"]["content"][0]["text"] =
                    text.replace("[Image #8] [Image #9] ", "").into();
            }
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{record}").unwrap();
            let expected = if labelled && ids.is_some() {
                vec!["[Image #8]", "[image image/png]"]
            } else {
                vec!["[image image/png]", "[image image/png]"]
            };
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            let events = super::super::event_reader::tail(file.path(), 20).unwrap();
            assert_eq!(summary.prompt_images, events[0].images);
            assert_eq!(
                summary
                    .prompt_images
                    .iter()
                    .map(|image| image.label.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn queued_prompt_image_blocks_survive_missing_ids() {
        let mut record: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-queued-image.jsonl"
        ))
        .unwrap();
        record["attachment"]
            .as_object_mut()
            .unwrap()
            .remove("imagePasteIds");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        let events = super::super::event_reader::tail(file.path(), 20).unwrap();
        assert_eq!(summary.prompt_images, events[0].images);
        assert_eq!(summary.prompt_images.len(), 1);
        assert_eq!(summary.prompt_images[0].label, "[image image/png]");
    }

    #[test]
    fn image_only_prompts_keep_their_image_labels() {
        let (_, records) = image_fixture();
        let delivery: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-queued-image.jsonl"
        ))
        .unwrap();
        for mut record in [records[4].clone(), delivery] {
            let queued = record["type"] == "attachment";
            let (content, ids) = if queued {
                ("/attachment/prompt", "/attachment/imagePasteIds")
            } else {
                ("/message/content", "/imagePasteIds")
            };
            record
                .pointer_mut(content)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .retain(|block| block["type"] == "image");
            for labelled in [true, false] {
                let mut record = record.clone();
                let numbers = record.pointer(ids).unwrap().as_array().unwrap();
                let expected = numbers
                    .iter()
                    .map(|number| {
                        if labelled {
                            format!("[Image #{}]", number.as_u64().unwrap())
                        } else {
                            "[image image/png]".to_owned()
                        }
                    })
                    .collect::<Vec<_>>();
                if !labelled {
                    let prompt = if queued {
                        &mut record["attachment"]
                    } else {
                        &mut record
                    };
                    prompt.as_object_mut().unwrap().remove("imagePasteIds");
                }
                let mut file = tempfile::NamedTempFile::new().unwrap();
                writeln!(file, "{record}").unwrap();
                let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
                let events = super::super::event_reader::tail(file.path(), 20).unwrap();
                assert_eq!(summary.prompt, expected.join(" "));
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].body, summary.prompt);
                assert_eq!(events[0].images, summary.prompt_images);
                assert_eq!(
                    summary
                        .prompt_images
                        .iter()
                        .map(|image| image.label.clone())
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
    }

    #[test]
    fn damaged_prompt_images_keep_the_event_and_generic_label() {
        let (_, records) = image_fixture();
        for mime in [Value::Null, Value::from("invalid mime")] {
            let mut record = records[0].clone();
            record.as_object_mut().unwrap().remove("imagePasteIds");
            let source = &mut record["message"]["content"][1]["source"];
            source.as_object_mut().unwrap().remove("media_type");
            if !mime.is_null() {
                source["media_type"] = mime;
            }
            source["data"] = "".into();
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{record}").unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            let events = super::super::event_reader::tail(file.path(), 20).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, crate::event::Kind::User);
            assert_eq!(events[0].body, summary.prompt);
            assert_eq!(events[0].images, summary.prompt_images);
            assert_eq!(summary.prompt_images.len(), 1);
            assert_eq!(summary.prompt_images[0].label, "[image]");
            assert_eq!(
                summary.prompt_images[0].source,
                crate::images::ImageSource::Record {
                    record: SourceRecord {
                        path: file.path().to_owned(),
                        offset: 0
                    },
                    pointer: "/message/content/1/source/data".into(),
                }
            );
        }
    }

    #[test]
    fn subagent_prompt_images_use_absolute_tail_offsets() {
        let (_, records) = image_fixture();
        let mut record = records[4].clone();
        record["isSidechain"] = true.into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", " ".repeat(3 * CHUNK as usize)).unwrap();
        let offset = file.as_file().metadata().unwrap().len();
        writeln!(file, "{record}").unwrap();
        let summary = read_subagent_tail(
            file.path(),
            &mut File::open(file.path()).unwrap(),
            file.as_file().metadata().unwrap().len(),
        )
        .unwrap();
        assert_record_images(&summary.prompt_images, &[8, 9]);
        for (index, image) in summary.prompt_images.iter().enumerate() {
            assert_eq!(
                image.source,
                crate::images::ImageSource::Record {
                    record: SourceRecord {
                        path: file.path().to_owned(),
                        offset
                    },
                    pointer: format!("/message/content/{}/source/data", index + 1),
                }
            );
        }
    }

    #[test]
    fn image_companion_is_not_the_task_details_prompt() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!(
            "../../../tests/fixtures/claude-prompt-images.jsonl"
        ))
        .unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        let record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-prompt-images.jsonl")
                .lines()
                .nth(4)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(summary.prompt, content_text(&record, false).unwrap());
        assert_eq!(
            summary.prompt_at,
            Some("2026-10-13T19:17:18.224Z".parse().unwrap())
        );
    }

    #[test]
    fn peer_messages_are_not_a_session_prompt() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2026-10-12T06:00:00.000Z","isSidechain":false,"message":{{"role":"user","content":"Typed prompt"}}}}"#
        )
        .unwrap();
        file.write_all(include_bytes!(
            "../../../tests/fixtures/claude-peer-prompts.jsonl"
        ))
        .unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.prompt, "Typed prompt");
    }

    #[test]
    fn subagent_peer_prompts_show_sender_and_body() {
        let expected = [
            "translate-pdf → translate-pdf: user approved the pilot; second file (MathJax page) done: ~/Documents/MIT-zh/6.1910-Computation Structures/L01-notes-basics-of-information.pdf. Asking the user now whether to batch the remaining 155.",
            "general-purpose → 41 files: 23209 bytes before, 19698 bytes after. All files parse, so none are left unfixed.",
            "codex-support-research → {\"type\":\"idle_notification\",\"from\":\"codex-support-research\",\"timestamp\":\"2026-10-13T17:36:33.617Z\",\"idleReason\":\"available\",\"result\":\"done\"}",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-peer-prompts.jsonl")
            .lines()
            .zip(expected)
        {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{line}").unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            assert_eq!(summary.prompt, "");
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["isSidechain"] = true.into();
            std::fs::write(file.path(), format!("{record}\n")).unwrap();
            let summary =
                read_subagent(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            assert_eq!(summary.prompt, expected);
        }
    }

    #[test]
    fn pasted_prompt_and_queue_keep_the_event_message_body() {
        let record: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
        let raw = record["message"]["content"].as_str().unwrap();
        let expected = super::super::event_reader::pasted_content(raw).unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        let event = super::super::event_reader::tail(file.path(), 1)
            .unwrap()
            .remove(0);
        assert_eq!(event.body, expected);
        assert_eq!(summary(&file).prompt, expected);
        assert_eq!(
            queued_prompts(vec![QueueOperation::Enqueue(raw.into())]),
            [expected]
        );
    }

    #[test]
    fn xml_prompt_keeps_the_document_in_summary_and_event() {
        let xml = include_str!("../../../tests/fixtures/structured_text/real-theme.plist");
        assert_eq!(
            queued_prompts(vec![QueueOperation::Enqueue(xml.into())]),
            [xml]
        );
        let mut record: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
        record["message"]["content"] = xml.into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        assert_eq!(summary(&file).prompt, xml);
        let events = super::super::event_reader::tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, xml);
    }

    fn assert_summary(summary: &Summary) {
        let records: Vec<Value> = FIXTURE
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(summary.ai_title.as_deref(), records[2]["aiTitle"].as_str());
        assert_eq!(
            summary.prompt,
            printable(
                &super::super::event_reader::pasted_content(
                    records[0]["message"]["content"].as_str().unwrap()
                )
                .unwrap()
            )
        );
        assert_eq!(summary.cwd, records[0]["cwd"].as_str().map(PathBuf::from));
        assert_eq!(summary.git_branch.as_deref(), Some("main"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(summary.effort.as_deref(), Some("medium"));
        assert_eq!(
            summary.last_message,
            Some(printable(
                records[1]["message"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
            ))
        );
        assert_eq!(
            summary.last_prompt.as_deref(),
            records[3]["lastPrompt"].as_str()
        );
        assert_eq!(
            summary.timestamp.unwrap().to_rfc3339(),
            "2026-10-06T16:39:29.554+00:00"
        );
    }

    fn write_tool_traffic(file: &mut impl Write, bytes: usize) {
        let tool_use = serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-10-12T11:00:00Z",
            "message": {"content": [{"type": "tool_use", "id": "tool", "name": "Read", "input": {}}]}
        });
        let tool_result = serde_json::json!({
            "type": "user",
            "message": {"content": [{"type": "tool_result", "tool_use_id": "tool", "content": "x".repeat(1024)}]}
        });
        let traffic = format!("{tool_use}\n{tool_result}\n");
        for _ in 0..bytes.div_ceil(traffic.len()) {
            file.write_all(traffic.as_bytes()).unwrap();
        }
    }

    fn queue(file: &mut impl Write, operation: &str, content: &str) {
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "queue-operation", "operation": operation,
                "timestamp": "2026-10-12T10:00:00Z", "content": content,
                "reason": "absorbed_mid_turn", "commandUuid": "command", "deliveryId": "delivery"
            })
        )
        .unwrap();
    }

    fn user(file: &mut impl Write, content: &str) {
        let mut record: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
        record["message"]["content"] = content.into();
        writeln!(file, "{record}").unwrap();
    }

    fn idle_prompt(file: &mut impl Write) {
        queue(file, "enqueue", "Current prompt");
        queue(file, "dequeue", "");
        user(file, "Current prompt");
        writeln!(file, "{}", FIXTURE.lines().nth(1).unwrap()).unwrap();
        write_tool_traffic(file, 1);
    }

    fn summary(file: &tempfile::NamedTempFile) -> Summary {
        let len = file.as_file().metadata().unwrap().len();
        let mut summary = read(file.path(), len).unwrap();
        let bytes = std::fs::read(file.path()).unwrap();
        let requested = records(&bytes)
            .filter_map(|record| {
                let id = record["toolUseResult"]["backgroundTaskId"]
                    .as_str()?
                    .to_owned();
                Some((id.clone(), PathBuf::from(format!("/tmp/x/{id}.output"))))
            })
            .collect();
        let (tasks, _) = background_tasks(file.path(), len, &requested).unwrap();
        summary.background = tasks.into_values().collect();
        summary.background.sort_by(|a, b| a.output.cmp(&b.output));
        summary
    }

    fn background_start(file: &mut impl Write, task: &str, description: Option<&str>, moved: bool) {
        background_command(file, task, description, moved, "sleep 5\necho done");
    }

    fn background_command(
        file: &mut impl Write,
        task: &str,
        description: Option<&str>,
        moved: bool,
        command: &str,
    ) {
        let mut input = serde_json::json!({"command": command, "description": description});
        if !moved {
            input["run_in_background"] = true.into();
        }
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "assistant", "timestamp": "2026-10-12T12:00:00Z",
                "message": {"content": [{"type": "tool_use", "id": format!("tool-{task}"),
                    "name": "Bash", "input": input}]}
            })
        )
        .unwrap();
        let text = if moved {
            format!(
                "Command was moved to the background (ID: {task}). Output is being written to: /tmp/x/{task}.output. More details."
            )
        } else {
            format!(
                "Command running in background with ID: {task}. Output is being written to: /tmp/x/{task}.output. More details."
            )
        };
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "user", "toolUseResult": {"backgroundTaskId": task}, "message": {"content": [{"type": "tool_result",
                    "tool_use_id": format!("tool-{task}"), "content": text}]}
            })
        )
        .unwrap();
    }

    fn background_finish(file: &mut impl Write, task: &str) {
        writeln!(file, "{}", serde_json::json!({"type": "attachment", "attachment": {
            "type": "queued_command", "commandMode": "task-notification",
            "prompt": format!("<task-notification>\n<task-id>{task}</task-id>\n<status>completed</status>\n</task-notification>")
        }})).unwrap();
    }

    #[test]
    fn background_task_records_description_output_and_start_time() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        background_start(&mut file, "b1", Some("Wait five seconds"), false);
        let tasks = summary(&file).background;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "Wait five seconds");
        assert_eq!(tasks[0].output, Some(PathBuf::from("/tmp/x/b1.output")));
        assert_eq!(
            tasks[0].started_at,
            "2026-10-12T12:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        background_finish(&mut file, "b1");
        assert!(summary(&file).background.is_empty());
    }

    #[test]
    fn moved_background_task_uses_first_command_line() {
        for description in [None, Some(""), Some("  ")] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            idle_prompt(&mut file);
            background_start(&mut file, "b2", description, true);
            assert_eq!(summary(&file).background[0].description, "sleep 5");
        }
    }

    #[test]
    fn background_tasks_keep_start_order_and_ignore_queue_notifications() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        background_start(&mut file, "b1", Some("First"), false);
        background_start(&mut file, "b2", Some("Second"), false);
        let tasks = summary(&file).background;
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.description.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        queue(
            &mut file,
            "enqueue",
            "<task-notification><task-id>b2</task-id></task-notification>",
        );
        assert_eq!(summary(&file).background.len(), 2);
        background_finish(&mut file, "b1");
        let tasks = summary(&file).background;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "Second");
    }

    #[test]
    fn background_tasks_survive_later_turns_within_existing_walk() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        background_start(&mut file, "b1", Some("Earlier turn"), false);
        user(&mut file, "Next prompt");
        assert_eq!(summary(&file).background.len(), 1);
    }

    #[test]
    fn background_tasks_before_existing_quiet_point_are_found() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        background_start(&mut file, "b1", Some("Earlier task"), false);
        idle_prompt(&mut file);
        let requested = HashMap::from([("b1".to_owned(), PathBuf::from("/tmp/x/b1.output"))]);
        let (tasks, _) = background_tasks(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            &requested,
        )
        .unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn background_pairing_can_cross_existing_stopping_point() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        background_start(&mut file, "earlier", Some("Out of range"), false);
        let mut task = Vec::new();
        background_start(&mut task, "pending", Some("Pair across boundary"), false);
        let newline = task.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        file.write_all(&task[..newline]).unwrap();
        idle_prompt(&mut file);
        file.write_all(&task[newline..]).unwrap();
        let requested =
            HashMap::from([("pending".to_owned(), PathBuf::from("/tmp/x/pending.output"))]);
        let (tasks, _) = background_tasks(
            file.path(),
            file.as_file().metadata().unwrap().len(),
            &requested,
        )
        .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks["pending"].description, "Pair across boundary");
    }

    #[test]
    fn background_pairing_crosses_chunks() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        let mut task = Vec::new();
        background_start(&mut task, "b1", Some("Across chunks"), false);
        let newline = task.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        file.write_all(&task[..newline]).unwrap();
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        file.write_all(&task[newline..]).unwrap();
        assert_eq!(summary(&file).background[0].description, "Across chunks");
    }

    #[test]
    fn idle_prompt_has_no_queued_prompts() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        assert_eq!(summary(&file).prompt, "Current prompt");
        assert!(summary(&file).queued.is_empty());
    }

    #[test]
    fn pasted_prompts_show_their_content_without_tags() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        user(
            &mut file,
            "```\n\n\n<pasted_content id=\"a350\">\n```\n\n\n<\\pasted_content id=\"a832\">\nImplement workstream A.\n\nRules:\n- Do not push.\n<\\/pasted_content id=\"a832\">\n\n```\n\n发现了一个 bug。\n</pasted_content id=\"a350\">\n\n```\n嗯。",
        );
        queue(
            &mut file,
            "enqueue",
            "<pasted_content id=\"a350\">\n  queued paste\n</pasted_content id=\"a350\">",
        );
        let summary = summary(&file);
        assert_eq!(
            summary.prompt,
            "```\n\n\n```\n\n\nImplement workstream A.\n\nRules:\n- Do not push.\n\n\n```\n\n发现了一个 bug。\n\n\n```\n嗯。"
        );
        assert_eq!(summary.queued, ["queued paste\n"]);
    }

    #[test]
    fn running_prompt_keeps_queue_order_and_removes_absorbed_content() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(&mut file, "enqueue", "A");
        queue(&mut file, "enqueue", "B");
        assert_eq!(summary(&file).queued, ["A", "B"]);
        queue(&mut file, "remove", "A");
        assert_eq!(summary(&file).queued, ["B"]);
        queue(&mut file, "remove", "absent");
        assert_eq!(summary(&file).queued, ["B"]);
    }

    fn absorb(file: &mut impl Write, mode: &str, prompt: &str, uuid: &str) {
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "queue-operation", "operation": "remove",
                "timestamp": "2026-10-12T10:00:09Z", "content": prompt,
                "reason": "absorbed_mid_turn", "commandUuid": uuid, "deliveryId": "delivery"
            })
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "attachment", "timestamp": "2026-10-12T10:00:01Z",
                "attachment": {
                    "type": "queued_command", "commandMode": mode, "prompt": prompt,
                    "source_uuid": uuid, "delivery_id": "delivery", "humanTurn": true
                }
            })
        )
        .unwrap();
    }

    #[test]
    fn absorbed_prompt_replaces_the_prompt_and_restarts_the_turn() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(&mut file, "enqueue", "hey");
        absorb(&mut file, "prompt", "hey", "command-hey");
        writeln!(&mut file, "{}", FIXTURE.lines().nth(1).unwrap()).unwrap();
        let summary = summary(&file);
        assert_eq!(summary.prompt, "hey");
        assert_eq!(
            summary.prompt_at.unwrap().to_rfc3339(),
            "2026-10-12T10:00:09+00:00"
        );
        assert_eq!(summary.turn_messages, 1);
        assert!(summary.queued.is_empty());
    }

    #[test]
    fn absorbed_notification_is_not_a_prompt() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(&mut file, "enqueue", "<task-notification>done");
        absorb(
            &mut file,
            "task-notification",
            "<task-notification>done",
            "command-note",
        );
        let summary = summary(&file);
        assert_eq!(summary.prompt, "Current prompt");
        assert_eq!(summary.turn_messages, 2);
    }

    #[test]
    fn dequeue_of_current_prompt_walks_back_to_previous_quiet_point() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(&mut file, "enqueue", "A");
        queue(&mut file, "enqueue", "B");
        queue(&mut file, "dequeue", "");
        user(&mut file, "A");
        let result = summary(&file);
        assert_eq!(result.prompt, "A");
        assert_eq!(result.queued, ["B"]);
    }

    #[test]
    fn notifications_are_hidden_and_xml_keeps_its_queue_position() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(
            &mut file,
            "enqueue",
            "  <task-notification>done</task-notification>",
        );
        queue(&mut file, "enqueue", "A");
        queue(&mut file, "enqueue", "<other-session>hello</other-session>");
        assert_eq!(
            summary(&file).queued,
            ["A", "<other-session>hello</other-session>"]
        );
        queue(&mut file, "dequeue", "");
        user(&mut file, "  <task-notification>done</task-notification>");
        assert_eq!(
            summary(&file).queued,
            ["A", "<other-session>hello</other-session>"]
        );
        queue(&mut file, "dequeue", "");
        user(&mut file, "A");
        assert_eq!(
            summary(&file).queued,
            ["<other-session>hello</other-session>"]
        );
    }

    #[test]
    fn queue_reconstruction_crosses_chunk_boundaries() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        queue(&mut file, "enqueue", "A");
        write_tool_traffic(&mut file, CHUNK as usize);
        queue(&mut file, "enqueue", "B");
        write_tool_traffic(&mut file, CHUNK as usize);
        queue(&mut file, "dequeue", "");
        user(&mut file, "A");
        write_tool_traffic(&mut file, CHUNK as usize);
        assert_eq!(summary(&file).queued, ["B"]);
    }

    #[test]
    fn queue_without_quiet_point_replays_from_start_and_cleans_after_matching() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let prefix = "中".repeat(512);
        queue(&mut file, "enqueue", &format!("{prefix}A"));
        queue(&mut file, "enqueue", &format!("{prefix}B"));
        queue(&mut file, "remove", &format!("{prefix}B"));
        queue(&mut file, "enqueue", "A\n\tB\0C");
        assert_eq!(
            summary(&file).queued,
            [format!("{prefix}A"), "A\n\tBC".into()]
        );
    }

    #[test]
    fn remove_deletes_only_the_first_matching_item() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        for content in ["A", "B", "A"] {
            queue(&mut file, "enqueue", content);
        }
        queue(&mut file, "remove", "A");
        assert_eq!(summary(&file).queued, ["B", "A"]);
    }

    #[test]
    fn turn_counts_only_assistant_records_after_the_latest_prompt() {
        for large in [false, true] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(FIXTURE.as_bytes()).unwrap();
            let mut prompt: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
            prompt["message"]["content"] = "Current turn".into();
            prompt["timestamp"] = "2026-10-12T10:00:00Z".into();
            writeln!(file, "{prompt}").unwrap();
            let mut assistant: Value =
                serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
            for block in [
                serde_json::json!({"type": "thinking", "thinking": "Planning"}),
                serde_json::json!({"type": "text", "text": "Checking"}),
                serde_json::json!({"type": "tool_use", "id": "tool", "name": "Read", "input": {}}),
            ] {
                assistant["message"]["content"] = serde_json::json!([block]);
                writeln!(file, "{assistant}").unwrap();
            }
            let before = file.as_file().metadata().unwrap().len();
            write_tool_traffic(&mut file, if large { 3 * CHUNK as usize } else { 1 });
            let len = file.as_file().metadata().unwrap().len();
            let traffic = std::fs::read(file.path()).unwrap();
            let tool_calls = records(&traffic[before as usize..])
                .filter(|record| record["type"] == "assistant")
                .count();
            let summary = read(file.path(), len).unwrap();
            assert_eq!(summary.prompt, "Current turn");
            assert_eq!(summary.turn_messages, 3 + tool_calls);
            assert_eq!(
                summary.prompt_at,
                Some("2026-10-12T10:00:00Z".parse().unwrap())
            );
        }
    }

    #[test]
    fn turn_without_a_prompt_has_no_timestamp_or_messages() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.prompt_at, None);
        assert_eq!(summary.turn_messages, 0);
    }

    #[test]
    fn activity_tracks_the_latest_tool_and_record_timestamps() {
        let at = DateTime::parse_from_rfc3339("2026-10-12T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        for description in [Some("Run checks"), None, Some("  ")] {
            for completed in [false, true] {
                let mut file = tempfile::NamedTempFile::new().unwrap();
                file.write_all(FIXTURE.as_bytes()).unwrap();
                let mut assistant: Value =
                    serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
                assistant["timestamp"] = at.to_rfc3339().into();
                assistant["message"]["content"] = serde_json::json!([
                    {"type": "text", "text": "Checking"},
                    {"type": "tool_use", "id": "check", "name": "Bash", "input": {"description": description}}
                ]);
                writeln!(file, "{assistant}").unwrap();
                if completed {
                    writeln!(
                        file,
                        "{}",
                        serde_json::json!({"type": "user", "message": {"content": [
                            {"type": "tool_result", "tool_use_id": "check", "content": "done"}
                        ]}})
                    )
                    .unwrap();
                }
                let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
                assert_eq!(summary.last_message_at, Some(at));
                assert_eq!(summary.last_message.as_deref(), Some("Checking"));
                let activity = summary.activity.unwrap();
                assert_eq!(
                    activity.label,
                    if description == Some("Run checks") {
                        "Run checks"
                    } else {
                        "Bash"
                    }
                );
                assert_eq!(activity.at, at);
            }
        }
    }

    #[test]
    fn activity_skips_hidden_tools() {
        for sidechain in [false, true] {
            for same_record in [false, true] {
                for name in [
                    "TaskCreate",
                    "TaskUpdate",
                    "TaskList",
                    "TaskGet",
                    "ListAgents",
                    "TaskStop",
                ] {
                    for description in [None, Some("Update tasks")] {
                        let mut file = tempfile::NamedTempFile::new().unwrap();
                        let mut assistant: Value =
                            serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
                        assistant["isSidechain"] = sidechain.into();
                        assistant["message"]["content"] = serde_json::json!([
                            {"type": "tool_use", "id": "bash", "name": "Bash", "input": {"description": "Run checks"}}
                        ]);
                        let tool = serde_json::json!({"type": "tool_use", "id": "task", "name": name,
                            "input": {"description": description}});
                        if same_record {
                            assistant["message"]["content"]
                                .as_array_mut()
                                .unwrap()
                                .push(tool);
                        } else {
                            writeln!(file, "{assistant}").unwrap();
                            assistant["timestamp"] = "2026-10-12T12:00:00Z".into();
                            assistant["message"]["content"] = serde_json::json!([tool]);
                        }
                        writeln!(file, "{assistant}").unwrap();
                        let len = file.as_file().metadata().unwrap().len();
                        let summary = if sidechain {
                            read_subagent(file.path(), len)
                        } else {
                            read(file.path(), len)
                        }
                        .unwrap();
                        let activity = summary.activity.unwrap();
                        let events = super::super::event_reader::tail_session(
                            file.path(),
                            usize::MAX,
                            sidechain,
                        )
                        .unwrap();
                        let event = events
                            .iter()
                            .rev()
                            .find(|event| !event.calls.is_empty())
                            .unwrap();
                        assert_eq!(
                            activity.label, event.body,
                            "{name}, sidechain={sidechain}, same_record={same_record}"
                        );
                        assert_eq!(activity.at, event.started_at);
                    }
                }
            }
        }
    }

    #[test]
    fn activity_survives_chunks_and_later_prompts() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        let mut assistant: Value = serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
        assistant["message"]["content"] = serde_json::json!([
            {"type": "tool_use", "id": "earlier", "name": "Read", "input": {}},
            {"type": "tool_use", "id": "pending", "name": "Bash", "input": {"description": "中".repeat(CHUNK as usize)}}
        ]);
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(
            summary.activity.unwrap().label,
            super::super::event_reader::tail(file.path(), usize::MAX)
                .unwrap()
                .iter()
                .rev()
                .find(|event| !event.calls.is_empty())
                .unwrap()
                .body
        );
        assert_eq!(summary.last_message_at, read_fixture().last_message_at);
        writeln!(file, "{assistant}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(
            summary.activity.unwrap().label,
            super::super::event_reader::tail(file.path(), usize::MAX)
                .unwrap()
                .iter()
                .rev()
                .find(|event| !event.calls.is_empty())
                .unwrap()
                .body
        );
        writeln!(
            file,
            "{}",
            serde_json::json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "pending", "content": "done"}
            ]}})
        )
        .unwrap();
        let mut user: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
        user["message"]["content"] = "Next turn".into();
        writeln!(file, "{user}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(
            summary.activity.unwrap().label,
            super::super::event_reader::tail(file.path(), usize::MAX)
                .unwrap()
                .iter()
                .rev()
                .find(|event| !event.calls.is_empty())
                .unwrap()
                .body
        );
        assert!(summary.last_message_at.is_some());
    }

    #[test]
    fn background_metadata_survives_the_real_prompt_snapshot_attachment() {
        // Actual Haiku/low records; private attachment strings are redacted byte-for-byte.
        let fixture = include_bytes!("../../../tests/fixtures/claude-bg-metadata.jsonl");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(fixture).unwrap();
        let lines: Vec<_> = fixture
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect();
        assert_eq!(lines.last().unwrap().len(), 73654);
        let summary = read(file.path(), fixture.len() as u64).unwrap();
        assert_eq!(
            summary.last_message.as_deref(),
            Some("agents-stage5-ux-1792167241")
        );
        assert_eq!(summary.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(summary.effort.as_deref(), Some("low"));
        // The original metadata tail is entirely inside the oversized attachment.
        assert_eq!(summary.timestamp, None);
    }

    #[test]
    fn background_metadata_keeps_last_known_values_per_field() {
        let fixture = include_str!("../../../tests/fixtures/claude-bg-metadata.jsonl");
        let mut rows: Vec<Value> = fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let index = rows
            .iter()
            .position(|row| row["type"] == "assistant")
            .unwrap();
        let known = rows[index].clone();
        let mut model_only = known.clone();
        model_only.as_object_mut().unwrap().remove("effort");
        let mut without_metadata = known.clone();
        without_metadata.as_object_mut().unwrap().remove("effort");
        without_metadata
            .as_object_mut()
            .unwrap()
            .remove("requestedModel");
        without_metadata["message"]
            .as_object_mut()
            .unwrap()
            .remove("model");
        rows.splice(index..=index, [known, model_only, without_metadata]);
        for oversized_attachment in [false, true] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let end = rows.len() - usize::from(!oversized_attachment);
            for row in &rows[..end] {
                writeln!(file, "{row}").unwrap();
            }
            let len = file.as_file().metadata().unwrap().len();
            let summary = read(file.path(), len).unwrap();
            assert_eq!(summary.model.as_deref(), Some("claude-haiku-5-5"));
            assert_eq!(summary.effort.as_deref(), Some("low"));
            if !oversized_attachment {
                let summary = read_subagent(file.path(), len).unwrap();
                assert_eq!(summary.model.as_deref(), Some("claude-haiku-5-5"));
                assert_eq!(summary.effort.as_deref(), Some("low"));
            }
        }
    }

    #[test]
    fn messages_survive_multiple_chunks_of_tool_traffic() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        let expected = read_fixture();
        assert_eq!(summary.prompt, expected.prompt);
        assert_eq!(summary.last_message, expected.last_message);
        assert_eq!(summary.model, expected.model);
        assert_eq!(summary.effort, expected.effort);
        assert_eq!(
            summary.timestamp.unwrap().to_rfc3339(),
            "2026-10-12T11:00:00+00:00"
        );
    }

    fn read_fixture() -> Summary {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        read(file.path(), file.as_file().metadata().unwrap().len()).unwrap()
    }

    #[test]
    fn prompt_is_the_last_real_user_message() {
        for large in [false, true] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(FIXTURE.as_bytes()).unwrap();
            if large {
                write_tool_traffic(&mut file, 3 * CHUNK as usize);
            }
            let mut user: Value = serde_json::from_str(FIXTURE.lines().next().unwrap()).unwrap();
            user["message"]["content"] = "second prompt".into();
            writeln!(file, "{user}").unwrap();
            for content in ["", "  ", "  <system-reminder>skip</system-reminder>"] {
                user["message"]["content"] = content.into();
                writeln!(file, "{user}").unwrap();
            }
            user["message"]["content"] = "sidechain prompt".into();
            user["isSidechain"] = true.into();
            writeln!(file, "{user}").unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            assert_eq!(summary.prompt, "second prompt");
            assert_eq!(summary.last_message, read_fixture().last_message);
        }
    }

    #[test]
    fn messages_straddling_chunk_boundaries_are_whole() {
        for kind in ["user", "assistant"] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(FIXTURE.as_bytes()).unwrap();
            write_tool_traffic(&mut file, 2 * CHUNK as usize);
            let message = format!("boundary {kind} {}", "中".repeat(CHUNK as usize));
            let mut record: Value = serde_json::from_str(
                FIXTURE
                    .lines()
                    .nth(usize::from(kind == "assistant"))
                    .unwrap(),
            )
            .unwrap();
            record["message"]["content"] = if kind == "user" {
                message.clone().into()
            } else {
                serde_json::json!([{"type": "text", "text": message}])
            };
            writeln!(file, "{record}").unwrap();
            write_tool_traffic(&mut file, CHUNK as usize / 2);
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            if kind == "user" {
                assert_eq!(summary.prompt, printable(&message));
            } else {
                assert_eq!(summary.last_message, Some(printable(&message)));
            }
        }
    }

    fn pr_fixture(file: &mut impl Write, fixture: &str, cwd: &Path, output: Option<&Path>) {
        for line in fixture.lines() {
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["cwd"] = cwd.to_str().unwrap().into();
            if let Some(output) = output
                && let Some(content) = record["content"].as_str()
                && let Some((before, rest)) = content.split_once("<output-file>")
                && let Some((_, after)) = rest.split_once("</output-file>")
            {
                record["content"] = format!(
                    "{before}<output-file>{}</output-file>{after}",
                    output.display()
                )
                .into();
            }
            writeln!(file, "{record}").unwrap();
        }
    }

    const PR_FOREGROUND: &str =
        include_str!("../../../tests/fixtures/claude-pr-create/foreground.jsonl");
    const PR_BACKGROUND: &str =
        include_str!("../../../tests/fixtures/claude-pr-create/background.jsonl");

    #[test]
    fn pull_request_foreground_and_background_output() {
        for (fixture, repository, output, expected) in [
            (PR_FOREGROUND, "example-user/panel", None, Some(1)),
            (
                PR_BACKGROUND,
                "blockchainian/claude",
                Some("background.output"),
                Some(50),
            ),
            (
                PR_BACKGROUND,
                "blockchainian/claude",
                Some("failed.output"),
                None,
            ),
        ] {
            let checkout = crate::providers::pull_request::tests::checkout(&format!(
                "https://github.com/{repository}.git"
            ));
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let output = output.map(|name| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/claude-pr-create")
                    .join(name)
            });
            pr_fixture(&mut file, fixture, checkout.path(), output.as_deref());
            assert_eq!(summary(&file).pr.map(|pr| pr.number), expected);
        }
    }

    #[test]
    fn pull_request_replay_retains_matching_origin_and_last_match() {
        let checkout =
            crate::providers::pull_request::tests::checkout("git@github.com:example-user/panel.git");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let mut header: Value =
            serde_json::from_str(PR_FOREGROUND.lines().next().unwrap()).unwrap();
        header["message"]["content"] = serde_json::json!([]);
        pr_fixture(&mut file, &header.to_string(), checkout.path(), None);
        // Keep creation outside both bounded chunks.
        write_tool_traffic(&mut file, CHUNK as usize * 2);
        pr_fixture(&mut file, PR_FOREGROUND, checkout.path(), None);
        write_tool_traffic(&mut file, CHUNK as usize * 2);
        let mut previous = summary(&file);
        assert_eq!(previous.pr.as_ref().map(|pr| pr.number), Some(1));
        assert!(
            std::process::Command::new("git")
                .current_dir(checkout.path())
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/example-org/wallet"
                ])
                .status()
                .unwrap()
                .success()
        );
        for (url, expected) in [
            ("https://github.com/example-org/wallet/pull/850", 1),
            ("https://github.com/EXAMPLE-USER/PANEL/pull/2", 2),
        ] {
            pr_fixture(
                &mut file,
                &PR_FOREGROUND.replace("https://github.com/example-user/panel/pull/1", url),
                checkout.path(),
                None,
            );
            let updated = read_cached(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                Some(&previous),
            )
            .unwrap();
            assert_eq!(updated.pr.as_ref().unwrap().number, expected);
            previous = updated;
        }
    }

    #[test]
    fn pull_request_background_call_survives_incremental_refreshes() {
        let checkout = crate::providers::pull_request::tests::checkout(
            "https://github.com/blockchainian/claude",
        );
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-pr-create/background.output");
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let records: Vec<_> = PR_BACKGROUND.lines().collect();
        let mut previous = Summary::default();
        for (index, record) in records.iter().enumerate() {
            if index == 2 {
                let unrelated = record.replace("toolu_01CnLyE4YFzWEDPdsYbGa4Ub", "unrelated");
                pr_fixture(&mut file, &unrelated, checkout.path(), Some(&output));
                previous = read_cached(
                    file.path(),
                    file.as_file().metadata().unwrap().len(),
                    Some(&previous),
                )
                .unwrap();
                assert!(previous.pr.is_none());
            }
            pr_fixture(&mut file, record, checkout.path(), Some(&output));
            previous = read_cached(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                Some(&previous),
            )
            .unwrap();
            assert_eq!(
                previous.pr.as_ref().map(|pr| pr.number),
                (index == 2).then_some(50)
            );
        }
    }

    #[test]
    fn pull_request_only_assistant_bash_create_calls() {
        let checkout =
            crate::providers::pull_request::tests::checkout("https://github.com/example-user/panel");
        for (field, replacement) in [
            ("type", "api-request-blob"),
            ("name", "Write"),
            ("command", "gh pr view"),
        ] {
            let mut records: Vec<Value> = PR_FOREGROUND
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if field == "type" {
                records[0][field] = replacement.into();
            } else if field == "name" {
                records[0]["message"]["content"][0][field] = replacement.into();
                records[0]["message"]["content"][0]["input"]["content"] = "gh pr create".into();
            } else {
                records[0]["message"]["content"][0]["input"][field] = replacement.into();
            }
            let fixture = records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            let mut file = tempfile::NamedTempFile::new().unwrap();
            pr_fixture(&mut file, &fixture, checkout.path(), None);
            assert!(summary(&file).pr.is_none());
        }
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", serde_json::json!({"type":"pr-link", "prNumber":1,"prUrl":"https://github.com/example-user/panel/pull/1"})).unwrap();
        assert!(summary(&file).pr.is_none());
    }

    #[test]
    fn truncated_last_line_preserves_preceding_summary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        let assistant = FIXTURE.lines().nth(1).unwrap();
        file.write_all(&assistant.as_bytes()[..assistant.len() / 2])
            .unwrap();
        assert_summary(&read(file.path(), file.as_file().metadata().unwrap().len()).unwrap());
    }

    #[test]
    fn real_transcript_summary_and_large_head_tail() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        assert_summary(&read(file.path(), file.as_file().metadata().unwrap().len()).unwrap());
        let mut large = tempfile::NamedTempFile::new().unwrap();
        let lines: Vec<_> = FIXTURE.lines().collect();
        writeln!(large, "{}", lines[0]).unwrap();
        // Repeated real title records fill the middle without changing the summary.
        for _ in 0..2000 {
            writeln!(large, "{}", lines[2]).unwrap();
        }
        for line in &lines[1..] {
            writeln!(large, "{line}").unwrap();
        }
        let len = large.as_file().metadata().unwrap().len();
        assert!(len > 2 * CHUNK);
        assert_summary(&read(large.path(), len).unwrap());
    }

    #[test]
    fn cwd_and_branch_come_from_the_tail_when_a_huge_record_fills_the_head() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let lines: Vec<_> = FIXTURE.lines().collect();
        let huge = format!(
            r#"{{"type":"file-history-snapshot","snapshot":"{}"}}"#,
            "x".repeat(CHUNK as usize + 1)
        );
        writeln!(file, "{huge}").unwrap();
        for _ in 0..2000 {
            writeln!(file, "{}", lines[2]).unwrap();
        }
        for line in &lines[1..] {
            writeln!(file, "{line}").unwrap();
        }
        let len = file.as_file().metadata().unwrap().len();
        let summary = read(file.path(), len).unwrap();
        let records = FIXTURE
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let with_cwd = records
            .iter()
            .find(|record| record["cwd"].is_string())
            .unwrap();
        assert_eq!(
            summary.cwd.as_deref(),
            with_cwd["cwd"].as_str().map(Path::new)
        );
        assert_eq!(
            summary.git_branch.as_deref(),
            with_cwd["gitBranch"].as_str()
        );
    }

    #[test]
    fn printable_preserves_unicode_and_allowed_whitespace() {
        assert_eq!(printable("a\0b\n\t\u{7}c"), "ab\n\tc");
        assert_eq!(printable(&"中".repeat(600)), "中".repeat(600));
    }

    #[test]
    fn sidechains_are_skipped_and_latest_text_and_titles_win() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        let mut assistant: Value = serde_json::from_str(FIXTURE.lines().nth(1).unwrap()).unwrap();
        assistant["requestedModel"] = "claude-opus".into();
        let text_block = assistant["message"]["content"][0].clone();
        assistant["message"]["content"]
            .as_array_mut()
            .unwrap()
            .push(text_block);
        assistant["message"]["content"][1]["text"] = "latest\u{0} text".into();
        writeln!(file, "{assistant}").unwrap();
        assistant["isSidechain"] = true.into();
        assistant["message"]["content"][1]["text"] = "subagent text".into();
        writeln!(file, "{assistant}").unwrap();
        let mut title: Value = serde_json::from_str(FIXTURE.lines().nth(2).unwrap()).unwrap();
        title["type"] = "custom-title".into();
        title["customTitle"] = "User title".into();
        writeln!(file, "{title}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.last_message.as_deref(), Some("latest text"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus"));
        assert_eq!(summary.custom_title.as_deref(), Some("User title"));
        assert_eq!(
            summary.ai_title.as_deref(),
            Some("Codex manager 停止时线程处理")
        );
    }
}

#[cfg(test)]
mod question_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn ask_user_question_matches_latest_call_by_id() {
        let pending = include_str!("../../../tests/fixtures/claude-questions-pending.jsonl");
        let answered = include_str!("../../../tests/fixtures/claude-questions-answered.jsonl");
        let call: Value = serde_json::from_str(pending).unwrap();
        let expected = question_texts(&call["message"]["content"][0]["input"]);
        let answer: Value = serde_json::from_str(answered.lines().last().unwrap()).unwrap();
        let mut other_answer = answer.clone();
        other_answer["message"]["content"][0]["tool_use_id"] = "other-call".into();
        let mut older_call = call.clone();
        older_call["message"]["content"][0]["id"] = "older-call".into();
        for (records, questions) in [
            (vec![call.clone(), other_answer], expected.clone()),
            (vec![older_call, call.clone(), answer.clone()], vec![]),
            (
                vec![call.clone(), answer.clone(), {
                    let mut later_call = call.clone();
                    later_call["message"]["content"][0]["id"] = "later-call".into();
                    later_call
                }],
                expected,
            ),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for record in records {
                writeln!(file, "{record}").unwrap();
            }
            let len = file.as_file().metadata().unwrap().len();
            assert_eq!(read(file.path(), len).unwrap().pending_questions, questions);
        }
    }
}
