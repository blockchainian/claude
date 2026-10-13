use chrono::{DateTime, Utc};
use std::{io, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    User,
    Model,
    Tool,
    Error,
    QueuedUser,
    QueuedSystem,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub body: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub previous_end: Option<DateTime<Utc>>,
}
pub fn tail(path: &Path, count: usize) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut log = Log::default();
    super::transcript::walk_records(&mut file, len, Vec::new(), |record| {
        log.record(record, count)
    })?;
    let mut events: Vec<_> = log.entries.into_iter().map(|entry| entry.event).collect();
    events.reverse();
    let mut previous_end = None;
    for event in &mut events {
        event.previous_end = previous_end;
        previous_end = match event.kind {
            Kind::Tool | Kind::Error => event.ended_at,
            _ => Some(event.started_at),
        };
    }
    let skip = events.len().saturating_sub(count);
    Ok(events.into_iter().skip(skip).collect())
}

#[derive(Default)]
struct Log {
    entries: Vec<Entry>,
    results: std::collections::HashMap<String, ResultAt>,
    assistant: Option<String>,
    batch: Option<usize>,
    sequence: usize,
}
struct Entry {
    event: Event,
    missing: bool,
    background: bool,
    last_result: Option<usize>,
}
struct ResultAt {
    at: DateTime<Utc>,
    sequence: usize,
    error: bool,
}

impl Log {
    fn ready(&self, count: usize) -> bool {
        self.entries.len() > count && self.batch.is_none_or(|index| index > count)
    }

    fn record(&mut self, record: serde_json::Value, count: usize) -> bool {
        self.sequence += 1;
        if record["type"] == "assistant" {
            let key = record["message"]["id"]
                .as_str()
                .or_else(|| record["requestId"].as_str())
                .or_else(|| record["uuid"].as_str())
                .map(str::to_owned);
            if key.is_none() || key != self.assistant {
                self.batch = None;
                self.assistant = key;
                if self.ready(count) {
                    return false;
                }
            }
        }
        let Some(at) = record["timestamp"]
            .as_str()
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|at| at.with_timezone(&Utc))
        else {
            return !self.ready(count);
        };
        let content = &record["message"]["content"];
        match record["type"].as_str() {
            Some("user") => {
                if let Some(blocks) = content.as_array() {
                    for block in blocks {
                        if block["type"] == "tool_result"
                            && let Some(id) = block["tool_use_id"].as_str()
                        {
                            let result = self.results.entry(id.to_owned()).or_insert(ResultAt {
                                at,
                                sequence: self.sequence,
                                error: false,
                            });
                            result.error |= block["is_error"] == true;
                        }
                    }
                }
                let text = content.as_str().map(str::to_owned).or_else(|| {
                    Some(
                        content
                            .as_array()?
                            .iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                });
                if record["isMeta"] != true
                    && let Some(text) = text.filter(|text| {
                        !text.trim().is_empty() && !text.trim_start().starts_with('<')
                    })
                {
                    self.push(Kind::User, text, at);
                }
            }
            Some("assistant") => {
                if let Some(blocks) = content.as_array() {
                    for block in blocks.iter().rev() {
                        match block["type"].as_str() {
                            Some("text") => {
                                if let Some(text) = block["text"].as_str() {
                                    self.push(Kind::Model, text.to_owned(), at);
                                }
                            }
                            Some("tool_use") => self.tool(block, at),
                            _ => {}
                        }
                    }
                }
            }
            Some("queue-operation") if record["operation"] == "enqueue" => {
                if let Some(text) = record["content"].as_str() {
                    let (kind, body) = if let Some(notification) = text
                        .trim()
                        .strip_prefix("<task-notification>")
                        .and_then(|text| text.strip_suffix("</task-notification>"))
                    {
                        let summary = notification
                            .split_once("<summary>")
                            .and_then(|(_, text)| text.split_once("</summary>"))
                            .map(|(summary, _)| summary.trim())
                            .unwrap_or_else(|| {
                                notification
                                    .lines()
                                    .map(str::trim)
                                    .find(|line| !line.is_empty())
                                    .unwrap_or("")
                            });
                        (Kind::Tool, summary)
                    } else if text.starts_with('<') {
                        (Kind::QueuedSystem, text)
                    } else {
                        (Kind::QueuedUser, text)
                    };
                    self.push(kind, body.to_owned(), at);
                }
            }
            _ => {}
        }
        !self.ready(count)
    }

    fn push(&mut self, kind: Kind, body: String, started_at: DateTime<Utc>) {
        let ended_at = matches!(kind, Kind::Tool).then_some(started_at);
        self.entries.push(Entry {
            event: Event {
                kind,
                body,
                started_at,
                ended_at,
                previous_end: None,
            },
            missing: false,
            background: false,
            last_result: None,
        });
    }

    fn tool(&mut self, block: &serde_json::Value, at: DateTime<Utc>) {
        let Some(name) = block["name"].as_str() else {
            return;
        };
        let input = &block["input"];
        let detail = ["description", "file_path", "prompt", "query", "message"]
            .iter()
            .find_map(|key| input[key].as_str().filter(|text| !text.trim().is_empty()))
            .unwrap_or("");
        let call = format!("{name} {detail}");
        let mut entry = if let Some(index) = self.batch.take() {
            self.entries.remove(index)
        } else {
            Entry {
                event: Event {
                    kind: Kind::Tool,
                    body: String::new(),
                    started_at: at,
                    ended_at: None,
                    previous_end: None,
                },
                missing: false,
                background: false,
                last_result: None,
            }
        };
        let body = entry
            .event
            .body
            .strip_suffix(" *")
            .filter(|_| entry.background)
            .unwrap_or(&entry.event.body);
        entry.event.body = if body.is_empty() {
            call
        } else {
            format!("{call}，{body}")
        };
        entry.background |= input["run_in_background"] == true;
        if entry.background {
            entry.event.body.push_str(" *");
        }
        entry.event.started_at = at;
        let result = block["id"].as_str().and_then(|id| self.results.get(id));
        if let Some(result) = result {
            if result.error {
                entry.event.kind = Kind::Error;
            }
            if !entry.missing
                && entry
                    .last_result
                    .is_none_or(|sequence| result.sequence < sequence)
            {
                entry.event.ended_at = Some(result.at);
                entry.last_result = Some(result.sequence);
            }
        } else {
            entry.missing = true;
            entry.event.ended_at = None;
        }
        self.batch = Some(self.entries.len());
        self.entries.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;

    fn at(second: u32) -> DateTime<Utc> {
        format!("2026-10-06T16:39:{second:02}Z").parse().unwrap()
    }
    fn record(kind: &str, second: u32, content: Value) -> Value {
        json!({"type":kind, "timestamp":at(second).to_rfc3339(),
            "uuid":format!("record-{second}"), "message":{"content":content}})
    }
    fn tool(second: u32, message: &str, id: &str, name: &str, input: Value) -> Value {
        let mut r = record(
            "assistant",
            second,
            json!([{"type":"tool_use", "id":id, "name":name, "input":input}]),
        );
        r["message"]["id"] = message.into();
        r
    }
    fn result(second: u32, id: &str, error: bool) -> Value {
        record(
            "user",
            second,
            json!([{"type":"tool_result", "tool_use_id":id, "is_error":error, "content":"done"}]),
        )
    }
    fn fixture() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Reuse the real prompt and assistant response before the tool records.
        for line in include_str!("../../tests/fixtures/transcript.jsonl")
            .lines()
            .take(2)
        {
            writeln!(file, "{line}").unwrap();
        }
        let mut sidechain = record("user", 43, json!("sidechain"));
        sidechain["isSidechain"] = true.into();
        let mut meta = record("user", 43, json!("meta"));
        meta["isMeta"] = true.into();
        for r in [
            tool(
                30,
                "batch",
                "a",
                "Read",
                json!({"description":"", "file_path":"src/main.rs"}),
            ),
            result(31, "a", false),
            tool(32, "batch", "b", "Grep", json!({"query":"needle"})),
            result(35, "b", true),
            tool(
                36,
                "background",
                "c",
                "Bash",
                json!({"description":"build", "run_in_background":true}),
            ),
            result(38, "c", false),
            json!({"type":"queue-operation", "timestamp":at(40).to_rfc3339(), "operation":"enqueue", "content":"next task"}),
            json!({"type":"queue-operation", "timestamp":at(42).to_rfc3339(), "operation":"enqueue", "content":"<agent-message>finished</agent-message>"}),
            json!({"type":"queue-operation", "timestamp":at(43).to_rfc3339(), "operation":"enqueue", "content":"<task-notification>\n<task-id>bcfsuvg3w</task-id>\n<status>completed</status>\n<summary>  Background command \"等 Codex 做日志面板\" completed (exit code 0) \n</summary>\n</task-notification>"}),
            record(
                "user",
                41,
                json!("<task-notification>finished</task-notification>"),
            ),
            sidechain,
            meta,
            json!({"type":"attachment", "attachment":{"type":"noise"}}),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        file
    }
    #[test]
    fn groups_interleaved_parallel_calls_and_tracks_predecessors() {
        let file = fixture();
        let events = tail(file.path(), 8).unwrap();
        assert_eq!(events.len(), 7);
        assert_eq!(
            events.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            [
                &Kind::User,
                &Kind::Model,
                &Kind::Error,
                &Kind::Tool,
                &Kind::QueuedUser,
                &Kind::QueuedSystem,
                &Kind::Tool
            ]
        );
        assert!(events[0].body.contains("Stop hook error"));
        assert!(events[1].body.starts_with("对，必须"));
        assert_eq!(
            events[0].started_at,
            "2026-10-06T16:39:12.595Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            events[1].started_at,
            "2026-10-06T16:39:29.554Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(events[2].body, "Read src/main.rs，Grep needle");
        assert_eq!(events[2].started_at, at(30));
        assert_eq!(events[2].ended_at, Some(at(35)));
        assert_eq!(events[3].body, "Bash build *");
        assert_eq!(events[3].started_at, at(36));
        assert_eq!(events[3].ended_at, Some(at(38)));
        assert_eq!(events[4].body, "next task");
        assert_eq!(events[4].started_at, at(40));
        assert_eq!(events[5].body, "<agent-message>finished</agent-message>");
        assert_eq!(events[5].started_at, at(42));
        assert_eq!(
            events[6].body,
            "Background command \"等 Codex 做日志面板\" completed (exit code 0)"
        );
        assert_eq!(events[6].started_at, at(43));
        assert_eq!(events[6].ended_at, Some(at(43)));
        assert_eq!(events[0].previous_end, None);
        for pair in events.windows(2) {
            let end = if matches!(pair[0].kind, Kind::Tool | Kind::Error) {
                pair[0].ended_at
            } else {
                Some(pair[0].started_at)
            };
            assert_eq!(pair[1].previous_end, end);
        }
        assert_eq!(tail(file.path(), 2).unwrap(), events[5..]);
        assert!(tail(file.path(), 0).unwrap().is_empty());
    }
    #[test]
    fn missing_result_keeps_the_entire_batch_running_and_request_id_groups() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for mut r in [
            tool(1, "unused", "a", "Read", json!({"file_path":"a"})),
            result(2, "a", true),
            tool(3, "unused", "b", "Read", json!({"file_path":"b"})),
        ] {
            r["message"].as_object_mut().unwrap().remove("id");
            r["requestId"] = "request".into();
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Error);
        assert_eq!(events[0].body, "Read a，Read b");
        assert_eq!(events[0].ended_at, None);
        assert_eq!(events[0].started_at, at(1));
    }
    #[test]
    fn chunk_boundary_keeps_the_whole_record_and_parallel_group() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!("before"))).unwrap();
        writeln!(
            file,
            "{}",
            tool(
                2,
                "large",
                "a",
                "Read",
                json!({"prompt":"x".repeat(70_000)})
            )
        )
        .unwrap();
        writeln!(file, "{}", result(3, "a", false)).unwrap();
        writeln!(
            file,
            "{}",
            tool(4, "large", "b", "Read", json!({"file_path":"b"}))
        )
        .unwrap();
        writeln!(file, "{}", result(5, "b", false)).unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].started_at, at(2));
        assert_eq!(events[0].previous_end, Some(at(1)));
        assert_eq!(events[0].ended_at, Some(at(5)));
        assert_eq!(
            events[0].body,
            format!("Read {}，Read b", "x".repeat(70_000))
        );
    }
    #[test]
    fn stops_in_the_tail_of_a_large_file() {
        let base = fixture();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&vec![b'x'; 200_000]).unwrap();
        writeln!(file).unwrap();
        file.write_all(&std::fs::read(base.path()).unwrap())
            .unwrap();
        assert_eq!(tail(file.path(), 2).unwrap(), tail(base.path(), 2).unwrap());
    }
    #[test]
    fn shared_walk_reads_only_the_last_chunk_when_events_are_complete() {
        use std::io::{Read, Seek, SeekFrom};
        struct MeasuredFile {
            file: std::fs::File,
            bytes_read: usize,
            earliest: u64,
        }
        impl Read for MeasuredFile {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.earliest = self.earliest.min(self.file.stream_position()?);
                let read = self.file.read(buffer)?;
                self.bytes_read += read;
                Ok(read)
            }
        }
        impl Seek for MeasuredFile {
            fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
                self.file.seek(from)
            }
        }
        let base = fixture();
        let mut fixture = tempfile::NamedTempFile::new().unwrap();
        fixture.write_all(&vec![b'x'; 200_000]).unwrap();
        writeln!(fixture).unwrap();
        fixture
            .write_all(&std::fs::read(base.path()).unwrap())
            .unwrap();
        let file = std::fs::File::open(fixture.path()).unwrap();
        let len = file.metadata().unwrap().len();
        let mut measured = MeasuredFile {
            file,
            bytes_read: 0,
            earliest: len,
        };
        let mut log = Log::default();
        super::super::transcript::walk_records(&mut measured, len, Vec::new(), |record| {
            log.record(record, 2)
        })
        .unwrap();
        assert_eq!(measured.bytes_read, 64 * 1024);
        assert_eq!(measured.earliest, len - 64 * 1024);
        assert_eq!(
            log.entries[0].event.body,
            "Background command \"等 Codex 做日志面板\" completed (exit code 0)"
        );
    }

    #[test]
    fn queued_task_notifications_end_at_the_record_time_for_every_status() {
        for status in ["completed", "failed", "killed", "stopped"] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            let content = format!(
                "<task-notification>\n<status>{status}</status>\n<summary> \njob {status}\n </summary>\n</task-notification>"
            );
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, format!("job {status}"));
            assert_eq!(events[0].started_at, at(2));
            assert_eq!(events[0].ended_at, Some(at(2)));
        }
    }

    #[test]
    fn queued_task_notifications_without_summary_use_the_first_nonempty_line() {
        for (content, body) in [
            (
                "<task-notification>\n \n  finished  \nmore\n</task-notification>",
                "finished",
            ),
            (
                "<task-notification>finished</task-notification>",
                "finished",
            ),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, body);
            assert_eq!(events[0].started_at, at(2));
            assert_eq!(events[0].ended_at, Some(at(2)));
        }
    }

    #[test]
    fn other_queued_tags_keep_their_content_and_kind() {
        for content in [
            "<agent-message>finished</agent-message>",
            "<cross-session-message>finished</cross-session-message>",
            "<pasted_content>finished</pasted_content>",
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::QueuedSystem);
            assert_eq!(events[0].body, content);
            assert_eq!(events[0].ended_at, None);
        }
    }

    #[test]
    fn text_blocks_keep_order_skip_thinking_and_keep_unterminated_last_record() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            record(
                "user",
                1,
                json!([{"type":"text","text":"first"},{"type":"text","text":"second"}])
            )
        )
        .unwrap();
        write!(file,"{}",record("assistant",2,json!([{"type":"thinking","thinking":"private"},{"type":"text","text":"one"},{"type":"text","text":"two"}]))).unwrap();
        let events = tail(file.path(), 8).unwrap();
        assert_eq!(
            events.iter().map(|e| e.body.as_str()).collect::<Vec<_>>(),
            ["first\nsecond", "one", "two"]
        );
        assert_eq!(events[1].previous_end, Some(at(1)));
        assert_eq!(events[2].previous_end, Some(at(2)));
    }
    #[test]
    fn a_valid_json_suffix_at_the_chunk_edge_is_not_a_complete_record() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!("before"))).unwrap();
        let suffix = format!(
            "{}\n{}\n",
            record("user", 2, json!("ghost")),
            record("user", 3, json!("after"))
        );
        // The initial window starts on valid JSON inside an invalid, longer line.
        file.write_all(&vec![b'x'; 70_000]).unwrap();
        file.write_all(suffix.as_bytes()).unwrap();
        file.write_all(&vec![b'\n'; 64 * 1024 - suffix.len()])
            .unwrap();
        let events = tail(file.path(), 2).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.body.as_str())
                .collect::<Vec<_>>(),
            ["before", "after"]
        );
        assert_eq!(events[1].previous_end, Some(at(1)));
    }
}
