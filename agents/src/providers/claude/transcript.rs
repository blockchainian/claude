use crate::log::question_texts;
use crate::model::{Activity, BackgroundTask, PullRequest, SessionTask, TaskStatus};
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
    pub detail_sources: [Option<String>; 3],
    pub started_at: Option<DateTime<Utc>>,
    pub subagent_records: Vec<Value>,
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub pr: Option<PullRequest>,
    pub prompt: String,
    pub prompt_images: Vec<crate::log::Image>,
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
                match block["name"].as_str() {
                    Some("TaskCreate") => {
                        if let Some(id) = block["id"].as_str() {
                            self.creates.insert(id.to_owned());
                        }
                    }
                    Some("TaskUpdate") => {
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
                // A fresh successful create can restart numbering within a transcript.
                if tasks.iter().any(|task| task.id == id) {
                    tasks.clear();
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
            if let Some((id, name)) = model_identity(&record) {
                summary.model_identities.insert(id, name);
            }
        }
    }
    Ok(())
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
        self.record_pr(record);
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

    fn record_pr(&mut self, record: &Value) {
        if record["type"] == "pr-link"
            && let Some(number) = record["prNumber"].as_u64()
            && let Some(url) = text(record, "prUrl")
        {
            self.pr = Some(PullRequest { number, url });
        }
    }
}

pub(super) fn printable(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
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

#[cfg(test)]
thread_local! {
    static PARSED_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_parsed_bytes() -> u64 {
    PARSED_BYTES.with(|count| count.replace(0))
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
        .filter_map(|line| {
            #[cfg(test)]
            PARSED_BYTES.with(|count| count.set(count.get() + line.len() as u64));
            serde_json::from_slice::<Value>(line).ok()
        })
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
/// attachment instead of a user record.
fn prompt_text(record: &Value) -> Option<String> {
    match record["type"].as_str()? {
        "user" => {
            if let Some((name, body)) = super::peer::origin_message(record) {
                return Some(crate::log::participant_text(name, body));
            }
            if record["isMeta"] == true {
                return None;
            }
            let text = content_text(record, false)?;
            Some(match super::peer::wrapped_message(&text) {
                Some((_, (name, body))) => crate::log::participant_text(name, body),
                None => typed_text(text),
            })
        }
        "attachment" => {
            let prompt = super::images::queued_prompt(record)?;
            prompt
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    Some(
                        prompt
                            .as_array()?
                            .iter()
                            .filter(|block| block["type"] == "text")
                            .filter_map(|block| block["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                })
                .map(typed_text)
        }
        _ => None,
    }
}

/// Text the user typed, with pasted blocks shown as their content.
fn typed_text(text: String) -> String {
    super::log::pasted_content(&text).unwrap_or(text)
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
        .map(typed_text)
        .filter(|content| !content.trim_start().starts_with('<'))
        .map(|content| {
            printable(&content)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
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
                && block["name"] == "Bash"
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
                        output,
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

/// Walk complete transcript records newest first, retaining a split first line.
/// The callback returns false once it has all the records it needs.
pub(super) fn walk_records(
    file: &mut (impl Read + Seek),
    mut start: u64,
    mut bytes: Vec<u8>,
    sidechains: bool,
    mut visit: impl FnMut(Value) -> bool,
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
        for record in records_with_sidechains(&bytes[skip..], sidechains).rev() {
            if !visit(record) {
                return Ok(());
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
    let mut images = super::images::Companions::default();
    let mut image_error = None;
    walk_records(file, start, bytes, sidechains, |record| {
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
        images.record(&record);
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
                    && block["name"] == "AskUserQuestion"
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
            && let Some(prompt) = prompt_text(&record)
            && !prompt.trim().is_empty()
            && !prompt.trim_start().starts_with('<')
        {
            summary.detail_sources[2] = record["uuid"].as_str().map(str::to_owned);
            summary.prompt = printable(&prompt);
            match images.take(&record, &summary.prompt) {
                Ok(images) => summary.prompt_images = images,
                Err(error) => {
                    image_error = Some(error);
                    return false;
                }
            }
            summary.prompt_at = at;
            summary.turn_messages = turn_messages;
            prompt_found = true;
            if record["type"] == "attachment" {
                absorbed = record["attachment"]["source_uuid"]
                    .as_str()
                    .map(str::to_owned);
            }
        }
        // The latest visible tool call, finished or not; the panel shows it while it
        // is newer than the last reply.
        if summary.activity.is_none()
            && record["type"] == "assistant"
            && let Some(at) = at
            && let Some(blocks) = record["message"]["content"].as_array()
            && let Some((kind, label)) = blocks
                .iter()
                .rev()
                .filter(|block| block["type"] == "tool_use")
                .find_map(|block| {
                    super::log::tool_activity(block).map(|label| {
                        summary.detail_sources[0] = block["id"].as_str().map(str::to_owned);
                        (
                            crate::log::tool_kind(block["name"].as_str().unwrap()),
                            label,
                        )
                    })
                })
        {
            summary.activity = Some(Activity {
                kind,
                label: label.to_owned(),
                at,
            });
        }
        if summary.last_message.is_none()
            && record["type"] == "assistant"
            && let Some(message) = content_text(&record, true)
        {
            summary.detail_sources[1] = message_source(&record);
            summary.last_message = Some(printable(&message));
            summary.last_message_at = at;
        }
        if prompt_found
            && model_found
            && effort_found
            && absorbed.is_none()
            && summary.last_message.is_some()
            && summary.activity.is_some()
            && quiet_point
            && background.requested.is_empty()
            && background.pending.is_empty()
        {
            return false;
        }
        true
    })?;
    if let Some(error) = image_error {
        return Err(error);
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
        summary.record_pr(&record);
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
            0,
            head,
            &mut summary,
            false,
            Background::default(),
        )?;
    } else {
        read_messages(
            &mut file,
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
    let mut summary = read_subagent_tail(&mut File::open(path)?, len)?;
    read_history(path, len, previous, true, &mut summary)?;
    Ok(summary)
}

pub(super) fn read_subagent_tail(file: &mut (impl Read + Seek), len: u64) -> io::Result<Summary> {
    let start = len.saturating_sub(CHUNK + 1);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(CHUNK + 1).read_to_end(&mut bytes)?;
    if start > 0 {
        let skip = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1);
        bytes.drain(..skip);
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
            let session = super::super::session("test".into(), first.clone(), None, Utc::now());
            assert_eq!(session.model_label(), label);
            writeln!(file, "{messages}").unwrap();
            let appended = read(
                file.path(),
                file.as_file().metadata().unwrap().len(),
                Some(&first),
            )
            .unwrap();
            let session = super::super::session("test".into(), appended, None, Utc::now());
            assert_eq!(session.model_label(), label);
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
            let session = super::super::session("test".into(), summary.clone(), None, Utc::now());
            assert_eq!(session.model_label(), label);
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
        let dir = tempfile::tempdir().unwrap();
        let records: Vec<Value> =
            include_str!("../../../tests/fixtures/claude-prompt-images.jsonl")
                .lines()
                .map(|line| {
                    let line =
                        line.replace("/redacted/claude-session", dir.path().to_str().unwrap());
                    serde_json::from_str(&line).unwrap()
                })
                .collect();
        std::fs::create_dir(dir.path().join("images")).unwrap();
        for number in [6, 8, 9] {
            std::fs::write(
                dir.path().join(format!("images/{number}.png")),
                b"\x89PNG\r\n\x1a\n",
            )
            .unwrap();
        }
        (dir, records)
    }

    #[test]
    fn details_prompt_images_join_the_selected_prompt() {
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
        for missing in [false, true] {
            if missing {
                std::fs::remove_file(dir.path().join("images/8.png")).unwrap();
            }
            let summary = read(&path, bytes.len() as u64).unwrap();
            let session =
                super::super::session("test".into(), summary, Some(path.clone()), Utc::now());
            let numbers = if missing { vec![9] } else { vec![8, 9] };
            assert_eq!(
                session.prompt_images,
                numbers
                    .into_iter()
                    .map(|number| crate::log::Image {
                        label: format!("[Image #{number}]"),
                        path: dir.path().join(format!("images/{number}.png")),
                    })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn image_companion_is_not_the_details_prompt() {
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
    fn peer_prompts_show_sender_and_body() {
        let expected = [
            "(translate-pdf) translate-pdf: user approved the pilot; second file (MathJax page) done: ~/Documents/MIT-zh/6.1910-Computation Structures/L01-notes-basics-of-information.pdf. Asking the user now whether to batch the remaining 155.",
            "(general-purpose) 41 files: 23209 bytes before, 19698 bytes after. All files parse, so none are left unfixed.",
            "(codex-support-research) {\"type\":\"idle_notification\",\"from\":\"codex-support-research\",\"timestamp\":\"2026-10-13T17:36:33.617Z\",\"idleReason\":\"available\",\"result\":\"done\"}",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-peer-prompts.jsonl")
            .lines()
            .zip(expected)
        {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{line}").unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            assert_eq!(summary.prompt, expected);
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["isSidechain"] = true.into();
            std::fs::write(file.path(), format!("{record}\n")).unwrap();
            let summary =
                read_subagent(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            assert_eq!(summary.prompt, expected);
        }
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
                &super::super::log::pasted_content(
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

    const PR: &str = include_str!("../../../tests/fixtures/pr-link.jsonl");

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
        assert_eq!(tasks[0].output, PathBuf::from("/tmp/x/b1.output"));
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
        assert_eq!(summary.queued, ["queued paste"]);
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
    fn system_items_are_hidden_but_consume_their_dequeue() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        idle_prompt(&mut file);
        queue(
            &mut file,
            "enqueue",
            "  <task-notification>done</task-notification>",
        );
        queue(&mut file, "enqueue", "A");
        queue(&mut file, "enqueue", "<other-session>hello</other-session>");
        assert_eq!(summary(&file).queued, ["A"]);
        queue(&mut file, "dequeue", "");
        user(&mut file, "  <task-notification>done</task-notification>");
        assert_eq!(summary(&file).queued, ["A"]);
        queue(&mut file, "dequeue", "");
        user(&mut file, "A");
        assert!(summary(&file).queued.is_empty());
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
        assert_eq!(summary(&file).queued, [format!("{prefix}A"), "A BC".into()]);
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
                        let bash_at: DateTime<Utc> =
                            assistant["timestamp"].as_str().unwrap().parse().unwrap();
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
                        if name == "TaskStop" {
                            assert_eq!(activity.label, description.unwrap_or(name));
                            assert_eq!(
                                activity.at,
                                assistant["timestamp"]
                                    .as_str()
                                    .unwrap()
                                    .parse::<DateTime<Utc>>()
                                    .unwrap()
                            );
                        } else {
                            assert_eq!(
                                activity.label, "Run checks",
                                "{name}, sidechain={sidechain}, same_record={same_record}"
                            );
                            assert_eq!(activity.at, bash_at);
                        }
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
        assert_eq!(summary.activity.unwrap().label, "Read");
        assert_eq!(summary.last_message_at, read_fixture().last_message_at);
        writeln!(file, "{assistant}").unwrap();
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        assert_eq!(summary.activity.unwrap().label, "中".repeat(CHUNK as usize));
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
        assert_eq!(summary.activity.unwrap().label, "中".repeat(CHUNK as usize));
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

    #[test]
    fn pull_request_absent() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        assert!(
            read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap()
                .pr
                .is_none()
        );
    }

    #[test]
    fn pull_request_last_wins_in_head_and_tail() {
        let expected: Value = serde_json::from_str(PR).unwrap();
        for large in [false, true] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(FIXTURE.as_bytes()).unwrap();
            file.write_all(PR.as_bytes()).unwrap();
            let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
            let pr = summary.pr.unwrap();
            assert_eq!(pr.number, expected["prNumber"].as_u64().unwrap());
            assert_eq!(pr.url, expected["prUrl"].as_str().unwrap());
            if large {
                for _ in 0..2000 {
                    writeln!(file, "{}", FIXTURE.lines().nth(2).unwrap()).unwrap();
                }
                assert!(
                    read(file.path(), file.as_file().metadata().unwrap().len())
                        .unwrap()
                        .pr
                        .is_some()
                );
            }
            let mut last = expected.clone();
            last["prNumber"] = 833.into();
            last["prUrl"] = "https://github.com/0xbabedead/chadwallet/pull/833".into();
            writeln!(file, "{last}").unwrap();
            let pr = read(file.path(), file.as_file().metadata().unwrap().len())
                .unwrap()
                .pr
                .unwrap();
            assert_eq!(pr.number, 833);
            assert_eq!(pr.url, last["prUrl"].as_str().unwrap());
        }
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
