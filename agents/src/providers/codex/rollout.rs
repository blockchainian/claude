use crate::log::{Event, Filter, Kind, participant_text};
use crate::model::Activity;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK: u64 = 64 * 1024;

#[derive(Default)]
pub(super) struct Summary {
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub activity: Option<Activity>,
    pub updated_at: Option<DateTime<Utc>>,
    pub turn_ended: bool,
}

/// Walk complete JSON records newest first, retaining records spanning chunks.
fn walk(path: &Path, mut visit: impl FnMut(Value) -> bool) -> io::Result<()> {
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
        for line in bytes[first..].split(|b| *b == b'\n').rev() {
            if let Ok(record) = serde_json::from_slice(line)
                && !visit(record)
            {
                return Ok(());
            }
        }
        remainder = bytes[..first].to_vec();
    }
    Ok(())
}

fn timestamp(record: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

fn preview(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(512)
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

fn assistant_text(record: &Value) -> Option<String> {
    let payload = &record["payload"];
    match (record["type"].as_str()?, payload["type"].as_str()) {
        ("response_item", Some("message")) if payload["role"] == "assistant" => {
            content_text(&payload["content"])
        }
        ("event_msg", Some("task_complete")) => payload["last_agent_message"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
        ("event_msg", Some("agent_message")) => payload["message"].as_str().map(str::to_owned),
        ("event_msg", Some("item_completed")) if payload["item"]["type"] == "AgentMessage" => {
            content_text(&payload["item"]["content"])
        }
        _ => None,
    }
}

fn tool_body(payload: &Value) -> Option<String> {
    let name = payload["name"].as_str()?;
    if name == "exec"
        && payload["type"] == "custom_tool_call"
        && let Some(input) = payload["input"].as_str()
    {
        return Some(super::exec_summary::summarize(input));
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
    Some(match detail.filter(|text| !text.trim().is_empty()) {
        Some(detail) => format!("{name} {}", preview(&detail)),
        None => name.to_owned(),
    })
}

pub(super) fn summary(path: &Path) -> io::Result<Summary> {
    let mut summary = Summary::default();
    let mut boundary = false;
    walk(path, |record| {
        let Some(at) = timestamp(&record) else {
            return true;
        };
        summary.updated_at.get_or_insert(at);
        let payload = &record["payload"];
        if !boundary && record["type"] == "event_msg" {
            match payload["type"].as_str() {
                Some("task_complete" | "turn_aborted") => {
                    summary.turn_ended = true;
                    boundary = true;
                }
                Some("task_started") => boundary = true,
                _ => {}
            }
        }
        if summary.last_message.is_none()
            && let Some(text) = assistant_text(&record)
        {
            summary.last_message = Some(preview(&text));
            summary.last_message_at = Some(at);
        }
        if summary.activity.is_none() && record["type"] == "response_item" {
            let label = match payload["type"].as_str() {
                Some("function_call" | "custom_tool_call") => tool_body(payload),
                Some("reasoning") => Some("Thinking".to_owned()),
                _ => None,
            };
            if let Some(label) = label {
                summary.activity = Some(Activity { label, at });
            }
        }
        !(boundary && summary.last_message.is_some() && summary.activity.is_some())
    })?;
    Ok(summary)
}

pub(super) fn tail_filtered(path: &Path, count: usize, filter: &Filter) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let mut results = HashMap::new();
    let mut seen = HashSet::new();
    let mut matching = 0;
    walk(path, |record| {
        let Some(at) = timestamp(&record) else {
            return true;
        };
        let payload = &record["payload"];
        let (kind, body, ended_at) = match (record["type"].as_str(), payload["type"].as_str()) {
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                if let Some(id) = payload["call_id"].as_str() {
                    results.insert(id.to_owned(), at);
                }
                return true;
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let Some(body) = tool_body(payload) else {
                    return true;
                };
                let ended = payload["call_id"]
                    .as_str()
                    .and_then(|id| results.remove(id));
                (Kind::Tool, body, ended)
            }
            (Some("response_item"), Some("message")) => {
                let kind = match payload["role"].as_str() {
                    Some("user") => Kind::User,
                    Some("assistant") => Kind::Model,
                    _ => return true,
                };
                let Some(body) = content_text(&payload["content"]) else {
                    return true;
                };
                if let Some(id) = payload["id"].as_str()
                    && !seen.insert(id.to_owned())
                {
                    return true;
                }
                (kind, body, None)
            }
            (Some("event_msg"), Some("item_completed"))
                if payload["item"]["type"] == "AgentMessage" =>
            {
                let item = &payload["item"];
                if let Some(id) = item["id"].as_str()
                    && !seen.insert(id.to_owned())
                {
                    return true;
                }
                let Some(body) = content_text(&item["content"]) else {
                    return true;
                };
                (Kind::Model, body, None)
            }
            (Some("event_msg"), Some("turn_aborted")) => {
                (Kind::Error, "Turn aborted".to_owned(), Some(at))
            }
            _ => return true,
        };
        matching += usize::from(filter.matches(&kind));
        entries.push(Event {
            kind,
            body,
            path: None,
            started_at: at,
            ended_at,
            previous_end: None,
        });
        matching <= count
    })?;
    entries.reverse();
    let mut previous_end = None;
    for entry in &mut entries {
        entry.previous_end = previous_end;
        previous_end = Some(entry.ended_at.unwrap_or(entry.started_at));
    }
    entries.retain(|entry| filter.matches(&entry.kind));
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
    #[test]
    fn real_agent_names_in_log_and_activity() {
        for (line, expected) in include_str!("../../../tests/fixtures/codex-agent-names.jsonl")
            .lines()
            .zip([
                "spawn_agent (real_scan) Review files.",
                "send_message (/root) Review files.",
                "followup_task (ux_probe) Review files.",
            ])
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail_filtered(file.path(), 8, &Filter::default()).unwrap();
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
            let expected = format!("{name} (reviewer)");
            assert_eq!(
                tail_filtered(file.path(), 1, &Filter::default()).unwrap()[0].body,
                expected
            );
            assert_eq!(
                summary(file.path()).unwrap().activity.unwrap().label,
                expected
            );
        }
    }

    #[test]
    fn exec_summary_keeps_missing_input_and_existing_fallback() {
        assert_eq!(
            tool_body(&serde_json::json!({"type":"custom_tool_call", "name":"exec"})).as_deref(),
            Some("exec")
        );
        assert_eq!(
            tool_body(
                &serde_json::json!({"type":"custom_tool_call", "name":"exec", "input":"tools.exec_command({"})
            )
            .as_deref(),
            Some("exec tools.exec_command({")
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
        let events = tail_filtered(&fixture(), 100, &Filter::default()).unwrap();
        assert_eq!(
            events.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            vec![&Kind::User, &Kind::Tool, &Kind::Model]
        );
        assert_eq!(events[1].body, "javascript");
        assert!(events[1].ended_at.is_some());
        assert_eq!(events[2].body, "Checks passed.");
        assert!(events[2].previous_end.is_some());
    }
    #[test]
    fn filtered_tail_counts_matching_events() {
        let mut filter = Filter::default();
        filter.toggle(&[Kind::User]);
        let events = tail_filtered(&fixture(), 1, &filter).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, "Check the dashboard.");
        assert!(
            tail_filtered(Path::new("missing"), 0, &filter)
                .unwrap()
                .is_empty()
        );
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
        assert_eq!(result.last_message.unwrap(), "中".repeat(512));
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
        let events = tail_filtered(file.path(), 100, &Filter::default()).unwrap();
        assert_eq!(events.len(), 3);
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
        let events = tail_filtered(file.path(), 1, &Filter::default()).unwrap();
        assert_eq!(events[0].kind, Kind::Model);
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
        assert_eq!(result.activity.unwrap().label, "exec_command cargo check");
        writeln!(file,r#"{{"timestamp":"2026-10-13T18:00:02Z","type":"event_msg","payload":{{"type":"turn_aborted","reason":"interrupted"}}}}"#).unwrap();
        assert!(summary(file.path()).unwrap().turn_ended);
    }
}
