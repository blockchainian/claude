//! When one transcript's turns ran: each turn spans from the prompt that started it to the record
//! that ended it, so a long tool run counts as work. Only the turns since a given time are read,
//! from the transcript's end backward, then whatever it gains.

use crate::providers::ProviderId;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::fs::File;
use std::io;
use std::path::Path;

/// The turns of one transcript since some time.
#[derive(Debug, Default)]
pub struct Turns {
    /// Finished turns as start and end, in the order they ended.
    finished: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    /// When the turn still running at the last read record started.
    running: Option<DateTime<Utc>>,
    /// The earliest time the turns cover.
    since: Option<DateTime<Utc>>,
    /// Bytes of complete records read so far.
    read: u64,
}

/// What a record says about the turns.
#[derive(Debug, PartialEq)]
enum Mark {
    Start(DateTime<Utc>),
    /// A turn ended at `end`; `start` when the record tells it.
    End {
        start: Option<DateTime<Utc>>,
        end: DateTime<Utc>,
    },
}

impl Turns {
    /// Reads the turns of `path` since `since`: from its end back to the last turn that ended
    /// before then the first time or after it shrank, otherwise only what it gained.
    pub fn update(
        &mut self,
        path: &Path,
        provider: ProviderId,
        since: DateTime<Utc>,
    ) -> io::Result<()> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < self.read || self.since.is_none_or(|covered| since < covered) {
            *self = Self {
                since: Some(since),
                ..Self::default()
            };
            let end = crate::jsonl::complete_end(&mut file)?;
            crate::jsonl::read_lines_back(&mut file, end, |at, line| {
                let ended_before = matches!(
                    mark(line, provider),
                    Some(Mark::End { end, .. }) if end < since
                );
                if ended_before {
                    self.read = at + line.len() as u64 + 1;
                }
                !ended_before
            })?;
        }
        self.read = crate::jsonl::read_lines(&mut file, self.read, |line| {
            if let Some(mark) = mark(line, provider) {
                self.apply(mark);
            }
        })?;
        self.finished.retain(|(_, end)| *end >= since);
        self.since = Some(since);
        Ok(())
    }

    fn apply(&mut self, mark: Mark) {
        match mark {
            Mark::Start(at) => {
                self.running.get_or_insert(at);
            }
            Mark::End { start, end } => {
                let start = start.or(self.running.take()).unwrap_or(end);
                self.running = None;
                self.finished.push((start.min(end), end));
            }
        }
    }

    /// Start and end of each turn, the running one ending at `now` while the session `works`.
    pub fn spans(
        &self,
        now: DateTime<Utc>,
        works: bool,
    ) -> impl Iterator<Item = (DateTime<Utc>, DateTime<Utc>)> + '_ {
        let running = self
            .running
            .filter(|_| works)
            .map(|start| (start.min(now), now));
        self.finished.iter().copied().chain(running)
    }
}

fn mark(line: &[u8], provider: ProviderId) -> Option<Mark> {
    match provider {
        ProviderId::Claude => claude_mark(serde_json::from_slice(line).ok()?),
        ProviderId::Codex => codex_mark(serde_json::from_slice(line).ok()?),
    }
}

/// A Claude prompt starts a turn; its turn_duration record, or the user interrupting it, ends it.
fn claude_mark(record: ClaudeRecord) -> Option<Mark> {
    let at = parse_time(record.timestamp.as_deref()?)?;
    match record.kind.as_deref()? {
        "system" if record.subtype.as_deref() == Some("turn_duration") => Some(Mark::End {
            start: Some(at - Duration::milliseconds(record.duration_ms?)),
            end: at,
        }),
        "user" if !record.is_meta && !record.is_sidechain => {
            let message = record.message?;
            let text = prompt_text(&message.content)?;
            if text.starts_with("[Request interrupted by user") {
                Some(Mark::End {
                    start: None,
                    end: at,
                })
            } else if text.starts_with("<command-name>") || text.starts_with("<local-command") {
                // A command the CLI ran itself starts no turn.
                None
            } else {
                Some(Mark::Start(at))
            }
        }
        _ => None,
    }
}

/// The text a user record carries, which tool results do not.
fn prompt_text(content: &serde_json::Value) -> Option<&str> {
    match content {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .find(|block| block["type"] == "text")
            .and_then(|block| block["text"].as_str()),
        _ => None,
    }
}

/// A Codex task_started starts a turn and its task_complete or turn_aborted ends it; review turns
/// nested in another turn are skipped.
fn codex_mark(record: CodexRecord) -> Option<Mark> {
    if record.kind != "event_msg" {
        return None;
    }
    let payload = record.payload?;
    if payload.turn_id.is_none() || payload.turn_id != payload.root_turn_id {
        return None;
    }
    let at = parse_time(&record.timestamp)?;
    match payload.kind.as_deref()? {
        "task_started" => Some(Mark::Start(at)),
        "task_complete" | "turn_aborted" => Some(Mark::End {
            start: payload
                .started_at
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
            end: at,
        }),
        _ => None,
    }
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[derive(Deserialize)]
struct ClaudeRecord {
    #[serde(rename = "type")]
    kind: Option<String>,
    subtype: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<i64>,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    message: Option<ClaudeMessage>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    content: serde_json::Value,
}

#[derive(Deserialize)]
struct CodexRecord {
    timestamp: String,
    #[serde(rename = "type")]
    kind: String,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    turn_id: Option<String>,
    root_turn_id: Option<String>,
    started_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn at(text: &str) -> DateTime<Utc> {
        parse_time(text).unwrap()
    }

    // Shapes of real Claude Code 2.1 records, content replaced: a prompt, a tool result, the
    // turn's duration record, then a prompt the user interrupted.
    const CLAUDE_PROMPT: &str = r#"{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user","content":"check the files"},"uuid":"a","timestamp":"2026-10-14T10:25:31.227Z","sessionId":"s"}"#;
    const CLAUDE_TOOL_RESULT: &str = r#"{"parentUuid":"a","isSidechain":false,"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":"ok"}]},"uuid":"b","timestamp":"2026-10-14T10:26:28.100Z","sessionId":"s"}"#;
    const CLAUDE_TURN_DURATION: &str = r#"{"parentUuid":"b","isSidechain":false,"type":"system","subtype":"turn_duration","durationMs":100166,"messageCount":57,"timestamp":"2026-10-14T10:27:11.393Z","uuid":"c","isMeta":false,"sessionId":"s"}"#;
    const CLAUDE_LATER_PROMPT: &str = r#"{"parentUuid":"c","isSidechain":false,"type":"user","message":{"role":"user","content":[{"type":"text","text":"and the tests"}]},"uuid":"d","timestamp":"2026-10-14T11:00:00.000Z","sessionId":"s"}"#;
    const CLAUDE_INTERRUPT: &str = r#"{"parentUuid":"d","isSidechain":false,"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]},"uuid":"e","timestamp":"2026-10-14T11:00:30.000Z","sessionId":"s"}"#;

    fn transcript(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        file
    }

    fn spans(turns: &Turns, now: &str, works: bool) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
        turns.spans(at(now), works).collect()
    }

    #[test]
    fn claude_turns_span_from_their_duration_and_interrupts_end_them() {
        let file = transcript(&[
            CLAUDE_PROMPT,
            CLAUDE_TOOL_RESULT,
            CLAUDE_TURN_DURATION,
            CLAUDE_LATER_PROMPT,
            CLAUDE_INTERRUPT,
        ]);
        let mut turns = Turns::default();
        turns
            .update(file.path(), ProviderId::Claude, at("2026-10-14T10:00:00Z"))
            .unwrap();
        assert_eq!(
            spans(&turns, "2026-10-14T12:00:00Z", true),
            [
                (
                    at("2026-10-14T10:25:31.227Z"),
                    at("2026-10-14T10:27:11.393Z")
                ),
                (at("2026-10-14T11:00:00Z"), at("2026-10-14T11:00:30Z")),
            ]
        );
    }

    #[test]
    fn a_running_turn_lasts_until_now_only_while_the_session_works() {
        let file = transcript(&[CLAUDE_TURN_DURATION, CLAUDE_LATER_PROMPT]);
        let mut turns = Turns::default();
        turns
            .update(file.path(), ProviderId::Claude, at("2026-10-14T10:50:00Z"))
            .unwrap();
        assert_eq!(
            spans(&turns, "2026-10-14T11:05:00Z", true),
            [(at("2026-10-14T11:00:00Z"), at("2026-10-14T11:05:00Z"))]
        );
        assert!(spans(&turns, "2026-10-14T11:05:00Z", false).is_empty());
    }

    #[test]
    fn reading_starts_at_the_last_turn_that_ended_before_the_window_and_follows_appends() {
        let mut file = transcript(&[CLAUDE_PROMPT, CLAUDE_TURN_DURATION, CLAUDE_LATER_PROMPT]);
        let mut turns = Turns::default();
        turns
            .update(file.path(), ProviderId::Claude, at("2026-10-14T10:50:00Z"))
            .unwrap();
        // The turn before the window is skipped; the prompt after it still starts the running turn.
        assert!(turns.finished.is_empty());
        assert_eq!(turns.running, Some(at("2026-10-14T11:00:00Z")));
        writeln!(file, "{CLAUDE_INTERRUPT}").unwrap();
        turns
            .update(file.path(), ProviderId::Claude, at("2026-10-14T10:50:00Z"))
            .unwrap();
        assert_eq!(
            turns.finished,
            [(at("2026-10-14T11:00:00Z"), at("2026-10-14T11:00:30Z"))]
        );
    }

    // Shapes of real codex-cli 0.162 rollout records, content replaced: a turn the user started,
    // a review turn nested in it, the turn ending, and a turn the user aborted.
    const CODEX_STARTED: &str = r#"{"timestamp":"2026-10-15T18:14:21.872Z","ordinal":1,"type":"event_msg","payload":{"type":"task_started","turn_attribution":{"turn_id":"t1","turn_trigger":"user","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"t1"},"turn_id":"t1","root_turn_id":"t1","started_at":1792088061,"model_context_window":475000,"collaboration_mode_kind":"default"}}"#;
    const CODEX_REVIEW_COMPLETE: &str = r#"{"timestamp":"2026-10-15T18:14:30.000Z","ordinal":5,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t1","turn_id":"r1","last_agent_message":"ok"}}"#;
    const CODEX_COMPLETE: &str = r#"{"timestamp":"2026-10-15T18:15:00.000Z","ordinal":9,"type":"event_msg","payload":{"type":"task_complete","root_turn_id":"t1","turn_id":"t1","last_agent_message":"done"}}"#;
    const CODEX_ABORTED: &str = r#"{"timestamp":"2026-10-15T18:20:49.116Z","ordinal":14,"type":"event_msg","payload":{"type":"turn_aborted","root_turn_id":"t2","turn_id":"t2","reason":"interrupted","started_at":1792088421,"completed_at":1792088449,"duration_ms":27256}}"#;

    #[test]
    fn codex_turns_skip_nested_reviews_and_take_aborted_starts() {
        let file = transcript(&[
            CODEX_STARTED,
            CODEX_REVIEW_COMPLETE,
            CODEX_COMPLETE,
            CODEX_ABORTED,
        ]);
        let mut turns = Turns::default();
        turns
            .update(file.path(), ProviderId::Codex, at("2026-10-15T18:00:00Z"))
            .unwrap();
        assert_eq!(
            spans(&turns, "2026-10-15T19:00:00Z", true),
            [
                (at("2026-10-15T18:14:21.872Z"), at("2026-10-15T18:15:00Z")),
                (
                    DateTime::from_timestamp(1792088421, 0).unwrap(),
                    at("2026-10-15T18:20:49.116Z")
                ),
            ]
        );
    }
}
