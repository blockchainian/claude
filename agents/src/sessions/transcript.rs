use crate::model::{Activity, PullRequest};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const CHUNK: u64 = 64 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
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
    pub model: Option<String>,
    pub effort: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

impl Summary {
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

fn content_text(record: &Value, last: bool) -> Option<String> {
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
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(|record| record["isSidechain"] != true)
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

fn read_messages(
    file: &mut File,
    mut start: u64,
    mut bytes: Vec<u8>,
    summary: &mut Summary,
) -> io::Result<()> {
    let mut prompt_found = false;
    let mut turn_messages = 0;
    let mut operations = Vec::new();
    let mut next_user = None;
    let mut dequeued_user = None;
    let mut quiet_point = false;
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
        for record in records(&bytes[skip..]).rev() {
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
            if !prompt_found && record["type"] == "assistant" {
                turn_messages += 1;
            }
            if !prompt_found
                && record["type"] == "user"
                && let Some(prompt) = content_text(&record, false)
                && !prompt.trim().is_empty()
                && !prompt.trim_start().starts_with('<')
            {
                summary.prompt = preview(&prompt);
                summary.prompt_at = at;
                summary.turn_messages = turn_messages;
                prompt_found = true;
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
                && summary.last_message.is_some()
                && summary.activity.is_some()
                && quiet_point
            {
                summary.queued = queued_prompts(operations);
                return Ok(());
            }
        }
        if start == 0 {
            summary.queued = queued_prompts(operations);
            return Ok(());
        }
        bytes.truncate(skip);
        let previous = start.saturating_sub(CHUNK);
        file.seek(SeekFrom::Start(previous))?;
        let mut block = Vec::new();
        (&mut *file)
            .take(start - previous)
            .read_to_end(&mut block)?;
        block.extend_from_slice(&bytes);
        bytes = block;
        start = previous;
    }
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
        summary.record_pr(&record);
        if let Some(timestamp) = record["timestamp"]
            .as_str()
            .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        {
            summary.timestamp = Some(timestamp.with_timezone(&Utc));
        }
        // A head chunk swallowed by one huge record still leaves these in the tail.
        if summary.cwd.is_none() {
            summary.cwd = record["cwd"]
                .as_str()
                .filter(|cwd| !cwd.is_empty())
                .map(PathBuf::from);
        }
        if matches!(record["type"].as_str(), Some("user" | "assistant")) {
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
        match record["type"].as_str() {
            Some("ai-title") => {
                if let Some(title) = text(&record, "aiTitle") {
                    summary.ai_title = Some(title);
                }
            }
            Some("custom-title") => {
                if let Some(title) = text(&record, "customTitle") {
                    summary.custom_title = Some(title);
                }
            }
            Some("last-prompt") => {
                if let Some(prompt) = text(&record, "lastPrompt") {
                    summary.last_prompt = Some(prompt);
                }
            }
            Some("assistant") => {
                if let Some(model) =
                    text(&record, "requestedModel").or_else(|| text(&record["message"], "model"))
                {
                    summary.model = Some(model);
                }
                if let Some(effort) = text(&record, "effort") {
                    summary.effort = Some(effort);
                }
            }
            _ => {}
        }
    }
    if len <= 2 * CHUNK {
        read_messages(&mut file, 0, head, &mut summary)?;
    } else {
        read_messages(&mut file, len - CHUNK - 1, tail, &mut summary)?;
    }
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
            last["prUrl"] = "https://github.com/example-user/example-app/pull/833".into();
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
