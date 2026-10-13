use crate::model::{Activity, BackgroundTask, PullRequest};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const CHUNK: u64 = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
    pub subagent_records: Vec<Value>,
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub pr: Option<PullRequest>,
    pub prompt: String,
    pub queued: Vec<String>,
    pub prompt_at: Option<DateTime<Utc>>,
    pub turn_messages: usize,
    pub ai_title: Option<String>,
    pub custom_title: Option<String>,
    pub last_prompt: Option<String>,
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub background: Vec<BackgroundTask>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
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
            Some("assistant") => {
                if let Some(model) =
                    text(record, "requestedModel").or_else(|| text(&record["message"], "model"))
                {
                    self.model = Some(model);
                }
                if let Some(effort) = text(record, "effort") {
                    self.effort = Some(effort);
                }
            }
            _ => {}
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

pub(super) fn preview(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(512)
        .collect()
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)?
        .as_str()
        .map(preview)
        .filter(|text| !text.trim().is_empty())
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
/// attachment instead of a user record.
fn prompt_text(record: &Value) -> Option<String> {
    match record["type"].as_str()? {
        "user" => content_text(record, false),
        "attachment" => {
            let attachment = &record["attachment"];
            (attachment["type"] == "queued_command" && attachment["commandMode"] == "prompt")
                .then(|| attachment["prompt"].as_str().map(str::to_owned))
                .flatten()
        }
        _ => None,
    }
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
        .filter(|content| !content.trim_start().starts_with('<'))
        .map(|content| {
            preview(&content)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn background_result(content: &str) -> Option<(String, PathBuf)> {
    let task = if let Some(rest) = content.strip_prefix("Command running in background with ID: ") {
        rest.split_once('.')?.0
    } else {
        content
            .strip_prefix("Command was moved to the background (ID: ")?
            .split_once(')')?
            .0
    };
    let output = content.split_once("Output is being written to: ")?.1;
    let output = output.split_once(". ").map_or(output, |(path, _)| path);
    let output = output.strip_suffix('.').unwrap_or(output);
    (!task.is_empty() && !output.is_empty()).then(|| (task.to_owned(), PathBuf::from(output)))
}

#[derive(Default)]
struct Background {
    completed: HashSet<String>,
    pending: HashMap<String, (String, PathBuf)>,
    tasks: Vec<BackgroundTask>,
}

impl Background {
    fn record(&mut self, record: &Value, at: Option<DateTime<Utc>>, pairing_only: bool) {
        if record["type"] == "attachment" {
            let attachment = &record["attachment"];
            if attachment["type"] == "queued_command"
                && attachment["commandMode"] == "task-notification"
                && let Some(prompt) = attachment["prompt"].as_str()
                && let Some((_, rest)) = prompt.split_once("<task-id>")
                && let Some((task, _)) = rest.split_once("</task-id>")
            {
                self.completed.insert(task.to_owned());
            }
        }
        let Some(blocks) = record["message"]["content"].as_array() else {
            return;
        };
        for block in blocks.iter().rev() {
            if !pairing_only
                && record["type"] == "user"
                && block["type"] == "tool_result"
                && let Some(id) = block["tool_use_id"].as_str()
                && let Some(content) = block["content"].as_str()
                && let Some((task, output)) = background_result(content)
                && !self.completed.contains(&task)
            {
                self.pending.insert(id.to_owned(), (task, output));
            }
            if record["type"] == "assistant"
                && block["type"] == "tool_use"
                && block["name"] == "Bash"
                && let Some(id) = block["id"].as_str()
                && let Some((_, output)) = self.pending.remove(id)
                && let Some(started_at) = at
                && let Some(description) = block["input"]["description"]
                    .as_str()
                    .filter(|description| !description.trim().is_empty())
                    .or_else(|| block["input"]["command"].as_str()?.lines().next())
            {
                self.tasks.push(BackgroundTask {
                    waits_for_codex: block["input"]["command"]
                        .as_str()
                        .and_then(|command| shell_words::split(command).ok())
                        .is_some_and(|argv| {
                            argv.get(1).is_some_and(|path| {
                                Path::new(path).ends_with("codex-manager/manager.mjs")
                            }) && argv.get(2).is_some_and(|argument| argument == "await")
                        }),
                    description: description.to_owned(),
                    output,
                    started_at,
                });
            }
        }
    }

    fn finish(mut self) -> Vec<BackgroundTask> {
        self.tasks.reverse();
        self.tasks
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
) -> io::Result<()> {
    let mut prompt_found = false;
    let mut turn_messages = 0;
    // An absorbed queued prompt waits for its `remove` record, which carries
    // the moment the running turn actually picked it up.
    let mut absorbed = None;
    let mut operations = Vec::new();
    let mut next_user = None;
    let mut dequeued_user = None;
    let mut quiet_point = false;
    let mut background = Background::default();
    let mut pairing_only = false;
    walk_records(file, start, bytes, sidechains, |record| {
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
        background.record(&record, at, pairing_only);
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
            summary.prompt = preview(&prompt);
            summary.prompt_at = at;
            summary.turn_messages = turn_messages;
            prompt_found = true;
            if record["type"] == "attachment" {
                absorbed = record["attachment"]["source_uuid"]
                    .as_str()
                    .map(str::to_owned);
            }
        }
        // The latest tool call, finished or not; the panel shows it while it
        // is newer than the last reply.
        if summary.activity.is_none()
            && record["type"] == "assistant"
            && let Some(at) = at
            && let Some(blocks) = record["message"]["content"].as_array()
            && let Some(label) = blocks
                .iter()
                .rev()
                .filter(|block| block["type"] == "tool_use")
                .find_map(|block| {
                    block["input"]["description"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty())
                        .or_else(|| block["name"].as_str())
                })
        {
            summary.activity = Some(Activity {
                label: label.to_owned(),
                at,
            });
        }
        if summary.last_message.is_none()
            && record["type"] == "assistant"
            && let Some(message) = content_text(&record, true)
        {
            summary.last_message = Some(preview(&message));
            summary.last_message_at = at;
        }
        if prompt_found
            && absorbed.is_none()
            && summary.last_message.is_some()
            && summary.activity.is_some()
            && quiet_point
        {
            // Tasks before this existing stopping point are intentionally invisible.
            // Only an already-seen result may extend the walk to its tool use.
            pairing_only = true;
            if background.pending.is_empty() {
                return false;
            }
        }
        true
    })?;
    summary.background = background.finish();
    summary.queued = queued_prompts(operations);
    Ok(())
}

pub(super) fn read(path: &Path, len: u64) -> io::Result<Summary> {
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
        read_messages(&mut file, 0, head, &mut summary, false)?;
    } else {
        read_messages(&mut file, len - CHUNK - 1, tail, &mut summary, false)?;
    }
    Ok(summary)
}

/// Reads one tail chunk; subagent files have no queue quiet point to bound a walk.
pub(super) fn read_subagent(path: &Path, len: u64) -> io::Result<Summary> {
    read_subagent_tail(&mut File::open(path)?, len)
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
    )?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = include_str!("../../tests/fixtures/transcript.jsonl");

    fn assert_summary(summary: &Summary) {
        let records: Vec<Value> = FIXTURE
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(summary.ai_title.as_deref(), records[2]["aiTitle"].as_str());
        assert_eq!(
            summary.prompt,
            preview(records[0]["message"]["content"].as_str().unwrap())
        );
        assert_eq!(summary.cwd, records[0]["cwd"].as_str().map(PathBuf::from));
        assert_eq!(summary.git_branch.as_deref(), Some("main"));
        assert_eq!(summary.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(summary.effort.as_deref(), Some("medium"));
        assert_eq!(
            summary.last_message,
            Some(preview(
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

    const PR: &str = include_str!("../../tests/fixtures/pr-link.jsonl");

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
        read(file.path(), file.as_file().metadata().unwrap().len()).unwrap()
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
                "type": "user", "message": {"content": [{"type": "tool_result",
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
    fn background_task_detects_codex_wait_from_shell_arguments() {
        for (command, expected) in [
            (
                r#"node "/Users/x/.claude/plugins/cache/blockchainian/codex/0.20.2/codex-manager/manager.mjs" await --thread 01a1336e-ac44-7562-b8b3-5e6d9d536968"#,
                true,
            ),
            (r#"node "/some path/codex-manager/manager.mjs" await"#, true),
            ("node 'relative/codex-manager/manager.mjs' await", true),
            ("sleep 5", false),
            ("node /tmp/other/manager.mjs await", false),
            ("node /tmp/codex-manager/manager.mjs send", false),
            ("node /tmp/not-codex-manager/manager.mjs await", false),
            ("node /tmp/codex-manager/manager.mjs", false),
            (r#"node "/tmp/codex-manager/manager.mjs await"#, false),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            idle_prompt(&mut file);
            background_command(&mut file, "b1", Some("Wait for Codex"), false, command);
            let tasks = summary(&file).background;
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].waits_for_codex, expected, "{command}");
            background_finish(&mut file, "b1");
            assert!(summary(&file).background.is_empty());
        }
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
    fn background_tasks_before_existing_quiet_point_are_not_scanned() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        background_start(&mut file, "b1", Some("Earlier task"), false);
        idle_prompt(&mut file);
        assert!(summary(&file).background.is_empty());
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
        let tasks = summary(&file).background;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "Pair across boundary");
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
    fn queue_without_quiet_point_replays_from_start_and_previews_after_matching() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let prefix = "中".repeat(512);
        queue(&mut file, "enqueue", &format!("{prefix}A"));
        queue(&mut file, "enqueue", &format!("{prefix}B"));
        queue(&mut file, "remove", &format!("{prefix}B"));
        queue(&mut file, "enqueue", "A\n\tB\0C");
        assert_eq!(summary(&file).queued, [prefix, "A BC".into()]);
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
    fn messages_survive_multiple_chunks_of_tool_traffic() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        write_tool_traffic(&mut file, 3 * CHUNK as usize);
        let summary = read(file.path(), file.as_file().metadata().unwrap().len()).unwrap();
        let expected = read_fixture();
        assert_eq!(summary.prompt, expected.prompt);
        assert_eq!(summary.last_message, expected.last_message);
        // Older replies must not restore metadata outside the original tail.
        assert_eq!(summary.model, None);
        assert_eq!(summary.effort, None);
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
                assert_eq!(summary.prompt, preview(&message));
            } else {
                assert_eq!(summary.last_message, Some(preview(&message)));
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
    fn previews_preserve_unicode_and_allowed_whitespace() {
        assert_eq!(preview("a\0b\n\t\u{7}c"), "ab\n\tc");
        assert_eq!(preview(&"中".repeat(600)).chars().count(), 512);
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
