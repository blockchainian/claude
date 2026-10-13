use crate::log::{Event, Filter, Kind, participant_text};
use chrono::{DateTime, Utc};
use std::{io, path::Path};

pub(super) fn tool_hidden(name: &str) -> bool {
    matches!(name, "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet")
}

fn input_text<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input[key].as_str().filter(|text| !text.trim().is_empty())
}

fn default_detail(input: &serde_json::Value) -> Option<(&str, &str)> {
    ["description", "file_path", "prompt", "query", "message"]
        .into_iter()
        .find_map(|key| input_text(input, key).map(|text| (key, text)))
}

fn tool_detail(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => input_text(input, "description")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                crate::shell_summary::summarize_shell_commands(
                    input_text(input, "command").unwrap_or(""),
                )
            }),
        "WebFetch" => input_text(input, "url")
            .map(|url| {
                let address = url.split_once("://").map_or(url, |(_, address)| address);
                address.split(['?', '#']).next().unwrap_or("").to_owned()
            })
            .unwrap_or_default(),
        "SendMessage" => {
            let message = input_text(input, "summary")
                .or_else(|| input_text(input, "message").and_then(|text| text.lines().next()));
            participant_text(input_text(input, "to"), message.unwrap_or(""))
        }
        "Agent" | "Task" => participant_text(
            input_text(input, "name"),
            default_detail(input).map(|(_, text)| text).unwrap_or(""),
        ),
        "SubagentHandback" => participant_text(
            input_text(input, "name"),
            input_text(input, "message")
                .and_then(|text| text.lines().next())
                .unwrap_or(""),
        ),
        _ => default_detail(input)
            .map(|(_, text)| text)
            .unwrap_or("")
            .to_owned(),
    }
}

fn tool_label(name: &str, detail: &str) -> String {
    if detail.is_empty() {
        name.to_owned()
    } else if name == "Bash" {
        detail.to_owned()
    } else {
        format!("{name} {detail}")
    }
}

pub(super) fn tool_activity(block: &serde_json::Value) -> Option<String> {
    let name = block["name"].as_str()?;
    if tool_hidden(name) {
        return None;
    }
    let input = &block["input"];
    if matches!(
        name,
        "Bash" | "WebFetch" | "SendMessage" | "SubagentHandback"
    ) || (matches!(name, "Agent" | "Task") && input_text(input, "name").is_some())
    {
        Some(tool_label(name, &tool_detail(name, input)))
    } else {
        Some(input_text(input, "description").unwrap_or(name).to_owned())
    }
}

#[cfg(test)]
pub fn tail(path: &Path, count: usize) -> io::Result<Vec<Event>> {
    tail_filtered(path, count, &Filter::default(), false)
}

pub fn tail_filtered(
    path: &Path,
    count: usize,
    filter: &Filter,
    sidechains: bool,
) -> io::Result<Vec<Event>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut log = Log {
        filter: filter.clone(),
        ..Log::default()
    };
    super::transcript::walk_records(&mut file, len, Vec::new(), sidechains, |record| {
        log.record(record, count)
    })?;
    let mut events: Vec<_> = log.entries.into_iter().map(|entry| entry.event).collect();
    events.reverse();
    let mut previous_end = None;
    for event in &mut events {
        if event.kind != Kind::Model || event.previous_end.is_none() {
            event.previous_end = previous_end;
        }
        previous_end = match event.kind {
            Kind::Tool | Kind::Error | Kind::Denied => event.ended_at,
            _ => Some(event.started_at),
        };
    }
    events.retain(|event| filter.matches(&event.kind));
    let skip = events.len().saturating_sub(count);
    Ok(events.into_iter().skip(skip).collect())
}

#[derive(Default)]
struct Log {
    filter: Filter,
    matching: usize,
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
    denial: Option<String>,
}

impl Log {
    fn ready(&self, count: usize) -> bool {
        self.matching > count
            && self.batch.is_none_or(|index| {
                self.entries[..index]
                    .iter()
                    .filter(|entry| self.filter.matches(&entry.event.kind))
                    .count()
                    > count
            })
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
                let entry_count = self.entries.len();
                if let Some(blocks) = content.as_array() {
                    for block in blocks {
                        if block["type"] == "tool_result"
                            && let Some(id) = block["tool_use_id"].as_str()
                        {
                            let result = self.results.entry(id.to_owned()).or_insert(ResultAt {
                                at,
                                sequence: self.sequence,
                                error: false,
                                denial: None,
                            });
                            result.error |= block["is_error"] == true;
                            if block["is_error"] == true {
                                if record["toolDenialKind"] == "automode-blocked" {
                                    result.denial = Some(
                                        block["content"]
                                            .as_str()
                                            .and_then(|text| text.split_once("Reason: ["))
                                            .and_then(|(_, text)| text.split_once(']'))
                                            .map(|(reason, _)| reason)
                                            .unwrap_or("Dangerous")
                                            .to_owned(),
                                    );
                                } else if record["permissionDecision"]["reasonType"]
                                    == "safetyCheck"
                                {
                                    result.denial = Some("Safety Check".to_owned());
                                }
                            }
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
                    && let Some(text) = text.filter(|text| !text.trim().is_empty())
                {
                    if let Some((kind, body)) = wrapped_message(&text) {
                        self.push(kind, body, at);
                    } else if !text.trim_start().starts_with('<') {
                        self.push(Kind::User, text, at);
                    }
                }
                // A hidden turn starter is still the immediately preceding event.
                if self.entries.len() == entry_count
                    && !content.as_array().is_some_and(|blocks| {
                        blocks.iter().any(|block| block["type"] == "tool_result")
                    })
                    && let Some(entry) = self.entries.last_mut()
                    && entry.event.kind == Kind::Model
                    && entry.event.previous_end.is_none()
                {
                    entry.event.previous_end = Some(at);
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
                    let body = wrapped_message(body)
                        .map(|(_, body)| body)
                        .unwrap_or_else(|| body.to_owned());
                    self.push(kind, body, at);
                }
            }
            _ => {}
        }
        !self.ready(count)
    }

    fn push(&mut self, kind: Kind, body: String, started_at: DateTime<Utc>) {
        let ended_at = matches!(kind, Kind::Tool).then_some(started_at);
        self.matching += usize::from(self.filter.matches(&kind));
        self.entries.push(Entry {
            event: Event {
                kind,
                body,
                path: None,
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
        if tool_hidden(name) {
            return;
        }
        let input = &block["input"];
        let result = block["id"].as_str().and_then(|id| self.results.get(id));
        let detail = default_detail(input);
        let path = detail
            .filter(|(key, _)| *key == "file_path")
            .filter(|_| name != "Bash" && self.batch.is_none())
            .filter(|_| result.is_none_or(|result| result.denial.is_none()))
            .map(|(_, path)| path.to_owned());
        let detail = tool_detail(name, input);
        let call = if let Some(reason) = result.and_then(|result| result.denial.as_deref()) {
            format!("{name}: {reason}")
        } else if path.is_some() {
            name.to_owned()
        } else {
            tool_label(name, &detail)
        };
        let mut entry = if let Some(index) = self.batch.take() {
            let entry = self.entries.remove(index);
            self.matching -= usize::from(self.filter.matches(&entry.event.kind));
            entry
        } else {
            Entry {
                event: Event {
                    kind: Kind::Tool,
                    body: String::new(),
                    path: None,
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
        let body = match entry.event.path.take() {
            Some(path) => format!("{body} {path}"),
            None => body.to_owned(),
        };
        entry.event.path = path;
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
        if let Some(result) = result {
            if result.denial.is_some() {
                entry.event.kind = Kind::Denied;
            } else if result.error && entry.event.kind != Kind::Denied {
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
        self.matching += usize::from(self.filter.matches(&entry.event.kind));
        self.batch = Some(self.entries.len());
        self.entries.push(entry);
    }
}

/// Extracts the visible text of supported harness messages.
fn wrapped_message(text: &str) -> Option<(Kind, String)> {
    if let Some(body) = pasted_content(text) {
        return Some((Kind::User, user_text(&body)));
    }
    let (opening, rest) = text.trim().strip_prefix('<')?.split_once('>')?;
    let tag = opening.split_whitespace().next()?;
    let kind = match tag {
        "agent-message" | "teammate-message" => Kind::Agent,
        "cross-session-message" => Kind::Session,
        _ => return None,
    };
    let (inner, closing) = rest.rsplit_once("</")?;
    let closing = closing.strip_suffix('>')?;
    if closing != tag {
        return None;
    }
    let inner = inner.trim();
    let report;
    let body = if kind == Kind::Agent && inner.starts_with("[Subagent hand-back]") {
        if let Some(line) = inner
            .lines()
            .position(|line| line.trim_end().ends_with("The report follows:"))
        {
            report = inner.lines().skip(line + 1).collect::<Vec<_>>().join("\n");
            report.as_str()
        } else {
            inner
        }
    } else {
        inner
    };
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let attribute = |key: &str| {
        opening
            .split_once(&format!(" {key}=\""))?
            .1
            .split_once('"')
            .map(|(value, _)| value)
    };
    let name = match tag {
        "cross-session-message" => Some(attribute("from-name")?),
        "teammate-message" => attribute("teammate_id"),
        _ => attribute("from-name").or_else(|| attribute("from")),
    };
    Some((kind, participant_text(name, &body)))
}

fn pasted_content(mut text: &str) -> Option<String> {
    let mut body = String::new();
    let mut found = false;
    while let Some(start) = text.find("<pasted_content") {
        body.push_str(&text[..start]);
        let Some((opening, rest)) = text[start + 1..].split_once('>') else {
            body.push_str(&text[start..]);
            text = "";
            break;
        };
        let closing = format!("</{opening}>");
        let end = (opening.split_whitespace().next() == Some("pasted_content"))
            .then(|| {
                [closing.as_str(), "</pasted_content>"]
                    .into_iter()
                    .filter_map(|tag| rest.find(tag).map(|index| (index, tag.len())))
                    .min_by_key(|(index, _)| *index)
            })
            .flatten();
        if let Some((end, closing_len)) = end {
            let inner = rest[..end]
                .split_inclusive('\n')
                .map(|line| line.strip_prefix("  ").unwrap_or(line))
                .collect::<String>();
            body.push_str(&inner);
            text = &rest[end + closing_len..];
            found = true;
        } else {
            body.push_str(&text[start..start + opening.len() + 2]);
            text = rest;
        }
    }
    body.push_str(text);
    found.then_some(body)
}

fn user_text(text: &str) -> String {
    text.lines()
        .filter(|line| {
            !line.trim().strip_prefix("```").is_some_and(|suffix| {
                suffix.is_empty()
                    || suffix
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '+' | '#' | '.'))
            })
        })
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Write;

    #[test]
    fn real_tool_details_match_log_and_latest_activity() {
        let expected = [
            "mkdir, cd, ls, wc, head",
            "找那个会话",
            "SendMessage (hide-idle-teammates) 问 line_truncation.rs 是不是它在改",
            "SendMessage (team-lead) Report: /tmp/research",
            "SubagentHandback Report: /tmp/research",
            "WebFetch code.claude.com/docs/en/hooks",
            "cd, ls, sed, wc",
            "cd, cat, wc, sed",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
            .lines()
            .zip(expected)
        {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{line}").unwrap();
            let events = tail_filtered(file.path(), 8, &Filter::default(), true).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, expected);
            let len = file.as_file().metadata().unwrap().len();
            let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
            assert_eq!(summary.activity.unwrap().label, expected);
        }
    }

    #[test]
    fn real_agent_names_in_log_and_activity() {
        let expected = [
            "Agent (reviewer) Review files",
            "SubagentHandback Checks passed.",
            "(worker-id) Checks passed.",
            "(team-lead) Review files.",
            "(peer-session) Checks passed.",
        ];
        for (line, expected) in include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .zip(expected)
        {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), line).unwrap();
            let events = tail(file.path(), 8).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].body, expected);
            let record: Value = serde_json::from_str(line).unwrap();
            if record["type"] == "assistant" {
                let len = file.as_file().metadata().unwrap().len();
                let summary = super::super::transcript::read_subagent(file.path(), len).unwrap();
                assert_eq!(summary.activity.unwrap().label, expected);
            }
        }
    }

    #[test]
    fn task_names_and_named_handbacks_share_log_and_activity_format() {
        let records: Vec<Value> = include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (mut record, tool_name, expected) in [
            (records[0].clone(), "Task", "Task (reviewer) Review files"),
            (
                records[1].clone(),
                "SubagentHandback",
                "SubagentHandback (reviewer) Checks passed.",
            ),
        ] {
            let block = &mut record["message"]["content"][0];
            block["name"] = tool_name.into();
            block["input"]["name"] = "reviewer".into();
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), record.to_string()).unwrap();
            assert_eq!(tail(file.path(), 1).unwrap()[0].body, expected);
            let summary = super::super::transcript::read_subagent(
                file.path(),
                file.as_file().metadata().unwrap().len(),
            )
            .unwrap();
            assert_eq!(summary.activity.unwrap().label, expected);
        }
    }

    #[test]
    fn agent_names_without_details_or_names_keep_clean_spacing() {
        let record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-agent-names.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        for name in ["Agent", "Task", "SendMessage", "SubagentHandback"] {
            for (participant, expected) in [
                ("reviewer", format!("{name} (reviewer)")),
                (" ", name.to_owned()),
            ] {
                let mut record = record.clone();
                let block = &mut record["message"]["content"][0];
                block["name"] = name.into();
                block["input"] =
                    json!({if name == "SendMessage" { "to" } else { "name" }: participant});
                let file = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(file.path(), record.to_string()).unwrap();
                assert_eq!(tail(file.path(), 1).unwrap()[0].body, expected);
                let summary = super::super::transcript::read_subagent(
                    file.path(),
                    file.as_file().metadata().unwrap().len(),
                )
                .unwrap();
                assert_eq!(summary.activity.unwrap().label, expected);
            }
        }
    }

    #[test]
    fn bash_blank_description_uses_command_in_both_surfaces() {
        let mut record: Value = serde_json::from_str(
            include_str!("../../../tests/fixtures/claude-tool-details.jsonl")
                .lines()
                .nth(6)
                .unwrap(),
        )
        .unwrap();
        record["message"]["content"][0]["input"]["description"] = "  ".into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        let events = tail_filtered(file.path(), 8, &Filter::default(), true).unwrap();
        assert_eq!(events[0].body, "cd, ls, sed, wc");
        let summary = super::super::transcript::read_subagent(
            file.path(),
            file.as_file().metadata().unwrap().len(),
        )
        .unwrap();
        assert_eq!(summary.activity.unwrap().label, "cd, ls, sed, wc");
    }

    #[test]
    fn subagent_transcript_reads_sidechain_records_only_in_subagent_mode() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!("../../../tests/fixtures/subagent.jsonl"))
            .unwrap();
        assert!(tail(file.path(), 8).unwrap().is_empty());
        let events = tail_filtered(file.path(), 8, &Filter::default(), true).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Tool);
        assert!(events[0].body.starts_with("SubagentHandback"));
        assert!(events[0].ended_at.is_some());
        let mut filter = Filter::default();
        filter.toggle(&[Kind::Model]);
        assert!(
            tail_filtered(file.path(), 8, &filter, true)
                .unwrap()
                .is_empty()
        );
    }

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
    fn model_turn() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (time, mut r) in [
            ("10:00:00", record("user", 0, json!("inspect"))),
            ("10:00:02", tool(2, "first", "a", "Read", json!({}))),
            ("10:00:05", result(5, "a", false)),
            (
                "10:00:11",
                record("assistant", 11, json!([{"type":"text", "text":"done"}])),
            ),
            (
                "10:01:00",
                record(
                    "user",
                    0,
                    json!("<task-notification>finished</task-notification>"),
                ),
            ),
            (
                "10:01:57",
                record("assistant", 57, json!([{"type":"text", "text":"checked"}])),
            ),
        ] {
            r["timestamp"] = format!("2026-10-06T{time}Z").into();
            writeln!(file, "{r}").unwrap();
        }
        file
    }

    fn denial_records(fixture: &str) -> Vec<Value> {
        fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn events_from(records: Vec<Value>) -> Vec<Event> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        tail(file.path(), 20).unwrap()
    }

    #[test]
    fn automode_blocked_is_denied_with_classifier_reason() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Bash: Create Unsafe Agents");
        assert_eq!(events[0].ended_at, Some(end));
    }

    #[test]
    fn automode_blocked_without_brackets_is_denied_as_dangerous() {
        let records = denial_records(include_str!(
            "../../../tests/fixtures/automode_blocked_dangerous.jsonl"
        ));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let events = events_from(records);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Edit: Dangerous");
        assert_eq!(events[0].ended_at, Some(end));
    }

    #[test]
    fn safety_check_is_denied_and_ends_before_the_next_reply() {
        let mut records =
            denial_records(include_str!("../../../tests/fixtures/safety_check.jsonl"));
        let end = records[1]["timestamp"].as_str().unwrap().parse().unwrap();
        let mut reply = record("assistant", 1, json!([{"type":"text", "text":"checked"}]));
        reply["timestamp"] = "2026-10-12T15:13:30Z".into();
        records.push(reply);
        let events = events_from(records);
        assert_eq!(events[0].kind, Kind::Denied);
        assert_eq!(events[0].body, "Bash: Safety Check");
        assert_eq!(events[0].ended_at, Some(end));
        assert_eq!(events[1].previous_end, Some(end));
    }

    #[test]
    fn automode_unavailable_and_other_errors_stay_errors() {
        for (denial, reason) in [
            ("automode-unavailable", "classifier"),
            ("permission-rule", "user"),
            ("hook", "hook"),
        ] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[1]["toolDenialKind"] = denial.into();
            records[1]["permissionDecision"]["reasonType"] = reason.into();
            let events = events_from(records);
            assert_eq!(events[0].kind, Kind::Error);
            assert_eq!(events[0].body, "echo");
        }
    }

    #[test]
    fn automode_reason_without_complete_brackets_uses_dangerous() {
        for text in ["denied", "Reason: [unfinished"] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[1]["message"]["content"][0]["content"] = text.into();
            let events = events_from(records);
            assert_eq!(events[0].kind, Kind::Denied);
            assert_eq!(events[0].body, "Bash: Dangerous");
        }
    }

    #[test]
    fn denied_parallel_batch_takes_precedence_over_errors_in_either_order() {
        for first in [true, false] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            let mut other = records[0].clone();
            other["message"]["content"][0]["id"] = "other".into();
            other["message"]["content"][0]["name"] = "Read".into();
            let mut result = records[1].clone();
            result.as_object_mut().unwrap().remove("toolDenialKind");
            result.as_object_mut().unwrap().remove("permissionDecision");
            result["message"]["content"][0]["tool_use_id"] = "other".into();
            records.insert(if first { 1 } else { 0 }, other);
            records.push(result);
            let events = events_from(records);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, Kind::Denied);
            assert!(events[0].body.contains("Bash: Create Unsafe Agents"));
            assert!(events[0].body.contains("Read"));
        }
    }

    #[test]
    fn model_reply_duration_starts_after_tools_and_hidden_turn_starters() {
        let file = model_turn();
        let events = tail(file.path(), 20).unwrap();
        let models: Vec<_> = events
            .iter()
            .filter(|event| event.kind == Kind::Model)
            .collect();
        let tool = events
            .iter()
            .find(|event| event.kind == Kind::Tool)
            .unwrap();
        assert_eq!(
            (tool.ended_at.unwrap() - tool.previous_end.unwrap()).num_seconds(),
            5
        );
        assert_eq!(models.len(), 2);
        for (event, seconds) in models.into_iter().zip([6, 57]) {
            assert_eq!(
                (event.started_at - event.previous_end.unwrap()).num_seconds(),
                seconds
            );
        }
    }

    #[test]
    fn model_reply_tail_finds_tool_end_across_chunks() {
        let mut file = model_turn();
        let before = tail(file.path(), 1).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].kind, Kind::Model);
        assert_eq!(
            (before[0].started_at - before[0].previous_end.unwrap()).num_seconds(),
            57
        );
        // The preceding tool batch crosses a transcript chunk boundary.
        writeln!(
            file,
            "{}",
            tool(
                58,
                "large",
                "b",
                "Read",
                json!({"prompt":"x".repeat(140_000)})
            )
        )
        .unwrap();
        writeln!(file, "{}", result(59, "b", false)).unwrap();
        writeln!(
            file,
            "{}",
            record("assistant", 59, json!([{"type":"text", "text":"again"}]))
        )
        .unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].previous_end, Some(at(59)));
    }

    #[test]
    fn consecutive_model_replies_reference_the_previous_text() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            record("user", 0, json!("inspect")),
            record("assistant", 11, json!([{"type":"text", "text":"first"}])),
            record("assistant", 20, json!([{"type":"text", "text":"second"}])),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events[0].previous_end, Some(at(11)));
        assert_eq!(
            (events[0].started_at - events[0].previous_end.unwrap()).num_seconds(),
            9
        );
    }

    #[test]
    fn model_reply_without_reference_keeps_none() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{}",
            record("assistant", 11, json!([{"type":"text", "text":"first"}]))
        )
        .unwrap();
        assert_eq!(tail(file.path(), 1).unwrap()[0].previous_end, None);
    }

    #[test]
    fn model_reply_directly_after_user_takes_the_gap() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (time, mut r) in [
            ("10:01:00", record("user", 0, json!("inspect"))),
            (
                "10:01:57",
                record("assistant", 57, json!([{"type":"text", "text":"done"}])),
            ),
        ] {
            r["timestamp"] = format!("2026-10-06T{time}Z").into();
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(
            (events[0].started_at - events[0].previous_end.unwrap()).num_seconds(),
            57
        );
    }

    #[test]
    fn model_reply_uses_tool_end_after_any_user_content() {
        for content in [
            json!("typed prompt"),
            json!("<task-notification>finished</task-notification>"),
            json!("<agent-message>finished</agent-message>"),
            json!("<cross-session-message from-name=\"peer\">finished</cross-session-message>"),
            json!("<pasted_content>finished</pasted_content>"),
            json!([{"type":"image", "source":{"type":"base64", "data":""}}]),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            for r in [
                record("user", 0, content),
                tool(2, "between", "a", "Read", json!({})),
                result(5, "a", false),
                record("assistant", 11, json!([{"type":"text", "text":"done"}])),
            ] {
                writeln!(file, "{r}").unwrap();
            }
            assert_eq!(tail(file.path(), 1).unwrap()[0].previous_end, Some(at(5)));
        }
    }
    fn fixture() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        // Reuse the real prompt and assistant response before the tool records.
        for line in include_str!("../../../tests/fixtures/transcript.jsonl")
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
    fn wrapped_fixture() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for (second, content) in [
            (
                1,
                "<pasted_content id=\"paste-1\">\n  first line\n  second line\n</pasted_content id=\"paste-1\">",
            ),
            (
                3,
                "<agent-message from=\"a113c3cb9c1482c7a\">\n[Subagent hand-back] The text below is the final report of a subagent. The report follows:\n  Report: /private/tmp/claude-501/mit-dl/agent6.md\n  - Source: css.csail.mit.edu/6.5660/2026/\n</agent-message>",
            ),
            (
                5,
                "<cross-session-message from=\"uds:/tmp/cc-socks/71065.sock\" from-name=\"translate-pdf\" from-mode=\"prompting\">\ntranslate-pdf pilots ready for review.\n</cross-session-message>",
            ),
        ] {
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(second).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            writeln!(file, "{}", record("user", second + 1, json!(content))).unwrap();
        }
        for content in [
            "<task-notification>finished</task-notification>",
            "  <system-reminder>hidden</system-reminder>",
            "<unknown>hidden</unknown>",
        ] {
            writeln!(file, "{}", record("user", 7, json!(content))).unwrap();
        }
        file
    }

    #[test]
    fn wrapped_messages_extract_bodies_and_keep_queue_origin() {
        let file = wrapped_fixture();
        let events = tail(file.path(), 20).unwrap();
        assert_eq!(events.len(), 6);
        for (pair, kind, body) in [
            (&events[0..2], Kind::User, "first line second line"),
            (
                &events[2..4],
                Kind::Agent,
                "(a113c3cb9c1482c7a) Report: /private/tmp/claude-501/mit-dl/agent6.md - Source: css.csail.mit.edu/6.5660/2026/",
            ),
            (
                &events[4..6],
                Kind::Session,
                "(translate-pdf) translate-pdf pilots ready for review.",
            ),
        ] {
            assert_eq!(pair[0].kind, Kind::QueuedSystem);
            assert_eq!(pair[1].kind, kind);
            for event in pair {
                assert_eq!(event.body, body);
                assert_eq!(event.ended_at, None);
            }
        }
    }

    #[test]
    fn fenced_pasted_user_text_keeps_the_following_comment() {
        let content = "```\n\n\n<pasted_content id=\"7771\">\n  17:58:53 User    实话实说吧啊，…\n        6s Tool    mcp__plugin_codex_codex-manager__send …\n</pasted_content id=\"7771\">\n\n```\n\n那个五点五十八五十三秒和五点五十九零四秒。 哎，这个 model 的这个回复是…";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!(content))).unwrap();
        writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
        let events = tail(file.path(), 2).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, Kind::User);
        assert_eq!(events[1].kind, Kind::QueuedUser);
        for event in events {
            assert!(event.body.starts_with("17:58:53 User 实话实说吧啊"));
            assert!(event.body.contains("那个五点五十八五十三秒"));
            assert!(!event.body.contains("```"));
            assert!(!event.body.contains("<pasted_content"));
        }
    }

    #[test]
    fn pasted_blocks_anywhere_preserve_surrounding_user_and_queue_text() {
        for content in [
            "before\n```text\n<pasted_content id=\"a\">\n  first\n  second\n</pasted_content id=\"a\">\n```\nbetween <pasted_content id=\"b\">  third</pasted_content id=\"b\"> after",
            "<pasted_content id=\"a\">\n  before first second\n</pasted_content id=\"a\"> between <pasted_content id=\"b\">  third</pasted_content id=\"b\"> after",
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", record("user", 1, json!(content))).unwrap();
            writeln!(file, "{}", json!({"type":"queue-operation", "timestamp":at(2).to_rfc3339(), "operation":"enqueue", "content":content})).unwrap();
            let events = tail(file.path(), 2).unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].kind, Kind::User);
            for event in events {
                assert_eq!(event.body, "before first second between third after");
            }
        }
    }

    #[test]
    fn agent_report_without_harness_keeps_the_whole_inner_text() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", record("user", 1, json!([{"type":"text", "text":"<agent-message from=\"agent\">\n  Report without harness\n  The report follows:\n  details\n</agent-message>"}]))).unwrap();
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Agent);
        assert_eq!(
            events[0].body,
            "(agent) Report without harness The report follows: details"
        );
    }

    #[test]
    fn task_list_tools_produce_no_events() {
        for name in ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet"] {
            assert!(events_from(vec![tool(1, "call", "task", name, json!({}))]).is_empty());
        }
    }

    #[test]
    fn task_list_tools_preserve_the_bash_batch() {
        let bash = tool(
            1,
            "batch",
            "bash",
            "Bash",
            json!({"description":"inspect files"}),
        );
        let expected = events_from(vec![bash.clone(), result(3, "bash", false)]);
        assert_eq!(expected.len(), 1);
        for task_first in [false, true] {
            let task = tool(1, "batch", "task", "TaskCreate", json!({}));
            let mut mixed = bash.clone();
            let blocks = mixed["message"]["content"].as_array_mut().unwrap();
            blocks.insert(
                usize::from(!task_first),
                task["message"]["content"][0].clone(),
            );
            assert_eq!(events_from(vec![mixed, result(3, "bash", false)]), expected);
        }
    }

    #[test]
    fn task_list_tool_results_cannot_make_error_or_denied_rows() {
        for name in ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet"] {
            let mut records = denial_records(include_str!(
                "../../../tests/fixtures/automode_blocked.jsonl"
            ));
            records[0]["message"]["content"][0]["name"] = name.into();
            assert!(events_from(records).is_empty());
            assert!(
                events_from(vec![
                    tool(1, "call", "task", name, json!({})),
                    result(2, "task", true)
                ])
                .is_empty()
            );
        }
    }

    #[test]
    fn task_stop_produces_a_tool_row() {
        let events = events_from(vec![tool(1, "call", "task", "TaskStop", json!({}))]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Tool);
        assert_eq!(events[0].body, "TaskStop");
    }

    #[test]
    fn file_path_is_stored_separately_from_the_tool_name() {
        for name in ["Read", "Edit", "Write"] {
            let events = events_from(vec![tool(
                1,
                "call",
                "file",
                name,
                json!({"file_path":"~/Users/project/src/main.rs"}),
            )]);
            assert_eq!(events[0].body, name);
            assert_eq!(
                events[0].path.as_deref(),
                Some("~/Users/project/src/main.rs")
            );
        }
    }

    #[test]
    fn bash_description_replaces_only_the_exact_tool_name() {
        for (name, input, expected) in [
            (
                "Bash",
                json!({"description":"看 dot 仓库和引用位置"}),
                "看 dot 仓库和引用位置",
            ),
            ("Bash", json!({}), "Bash"),
            ("Bash", json!({"description":" \n "}), "Bash"),
            (
                "Read",
                json!({"description":"inspect files"}),
                "Read inspect files",
            ),
            (
                "bash",
                json!({"description":"inspect files"}),
                "bash inspect files",
            ),
        ] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            writeln!(file, "{}", tool(1, "call", "a", name, input)).unwrap();
            let events = tail(file.path(), 1).unwrap();
            assert_eq!(events[0].kind, Kind::Tool);
            assert_eq!(events[0].body, expected);
        }
    }

    #[test]
    fn parallel_calls_apply_bash_description_per_call() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            tool(
                1,
                "batch",
                "a",
                "Bash",
                json!({"description":"inspect files"}),
            ),
            tool(2, "batch", "b", "Bash", json!({})),
            tool(
                3,
                "batch",
                "c",
                "Read",
                json!({"description":"read config"}),
            ),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].body, "inspect files，Bash，Read read config");
    }

    #[test]
    fn parallel_calls_without_details_have_no_separator_after_names() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for r in [
            tool(
                1,
                "batch",
                "a",
                "mcp__plugin_codex_codex-manager__detach",
                json!({}),
            ),
            tool(2, "batch", "b", "TaskStop", json!({})),
        ] {
            writeln!(file, "{r}").unwrap();
        }
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].body,
            "mcp__plugin_codex_codex-manager__detach，TaskStop"
        );
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
        assert_eq!(events[3].body, "build *");
        assert_eq!(events[3].started_at, at(36));
        assert_eq!(events[3].ended_at, Some(at(38)));
        assert_eq!(events[4].body, "next task");
        assert_eq!(events[4].started_at, at(40));
        assert_eq!(events[5].body, "finished");
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
        super::super::transcript::walk_records(&mut measured, len, Vec::new(), false, |record| {
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
            "<system-reminder>finished</system-reminder>",
            "<unknown>finished</unknown>",
            "<agent-message-extra>finished</agent-message-extra>",
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

#[cfg(test)]
mod log_filters {
    use super::*;

    #[test]
    fn filtered_tail_keeps_batch_kinds_and_event_times() {
        for payload in [
            include_str!("../../../tests/fixtures/automode_blocked.jsonl"),
            include_str!("../../../tests/fixtures/safety_check.jsonl"),
            include_str!("../../../tests/fixtures/transcript.jsonl"),
        ] {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), payload).unwrap();
            let all = tail(file.path(), usize::MAX).unwrap();
            for kinds in [
                &[Kind::Model, Kind::Agent][..],
                &[Kind::Tool],
                &[Kind::Error, Kind::Denied],
                &[Kind::User, Kind::QueuedUser],
                &[],
            ] {
                let mut filter = Filter::default();
                filter.toggle(kinds);
                let matching: Vec<_> = all
                    .iter()
                    .filter(|event| filter.matches(&event.kind))
                    .cloned()
                    .collect();
                for count in [0, 1, 2, 8] {
                    let expected = &matching[matching.len().saturating_sub(count)..];
                    assert_eq!(
                        tail_filtered(file.path(), count, &filter, false).unwrap(),
                        expected
                    );
                }
            }
        }
    }
}
