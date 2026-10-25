//! Turn membership indexed by transcript byte positions for the event list.

use crate::providers::ProviderId;
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// What a record says about turn boundaries.
#[derive(Debug, PartialEq)]
enum Mark {
    Start,
    End,
}

/// Turn membership indexed by transcript byte positions, including partially loaded turns.
pub(crate) struct EventTurns {
    spans: Vec<(u64, u64, crate::event::Turn)>,
}

impl EventTurns {
    pub(crate) fn new<'a>(
        provider: ProviderId,
        records: impl IntoIterator<Item = (u64, &'a serde_json::Value)>,
    ) -> Self {
        let mut spans = Vec::new();
        let mut running: Option<(u64, crate::event::Turn)> = None;
        let mut ended = false;
        for (offset, record) in records {
            let signal = match provider {
                ProviderId::Claude => ClaudeRecord::deserialize(record).ok().and_then(claude_mark),
                ProviderId::Codex => CodexRecord::deserialize(record).ok().and_then(codex_mark),
            };
            if let Some(Mark::Start) = signal {
                if running.is_some_and(|(_, turn)| {
                    provider == ProviderId::Codex
                        || (record["type"] != "attachment" && !turn.started)
                }) {
                    let (start, turn) = running.take().unwrap();
                    if start < offset {
                        spans.push((start, offset - 1, turn));
                    }
                }
                running.get_or_insert((
                    offset,
                    crate::event::Turn {
                        id: offset,
                        started: true,
                        complete: false,
                    },
                ));
                ended = false;
            } else if running.is_none() && !ended {
                running = Some((
                    offset,
                    crate::event::Turn {
                        id: offset,
                        started: false,
                        complete: false,
                    },
                ));
            }
            if matches!(signal, Some(Mark::End)) {
                if let Some((start, mut turn)) = running.take() {
                    turn.complete = true;
                    spans.push((start, offset, turn));
                }
                ended = true;
            }
        }
        if let Some((start, turn)) = running {
            spans.push((start, u64::MAX, turn));
        }
        Self { spans }
    }

    pub(crate) fn at(&self, offset: u64) -> Option<crate::event::Turn> {
        let index = self.spans.partition_point(|(start, _, _)| *start <= offset);
        index.checked_sub(1).and_then(|index| {
            let (_, end, turn) = self.spans[index];
            (offset <= end).then_some(turn)
        })
    }
}

/// A Claude prompt starts a turn; its turn_duration record, or the user interrupting it, ends it.
fn claude_mark(record: ClaudeRecord) -> Option<Mark> {
    parse_time(record.timestamp.as_deref()?)?;
    if record.is_sidechain || record.is_compact_summary {
        return None;
    }
    match record.kind.as_deref()? {
        "attachment"
            if record
                .attachment
                .as_ref()
                .is_some_and(|a| a["type"] == "queued_command") =>
        {
            Some(Mark::Start)
        }
        "system" if record.subtype.as_deref() == Some("turn_duration") => {
            record.duration_ms?;
            Some(Mark::End)
        }
        "user" if !record.is_meta || record.turn_origin.is_some() => {
            let message = record.message?;
            let text = prompt_text(&message.content)?;
            if text.starts_with("[Request interrupted by user") {
                Some(Mark::End)
            } else if text.starts_with("<command-name>") || text.starts_with("<local-command") {
                // A command the CLI ran itself starts no turn.
                None
            } else {
                Some(Mark::Start)
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
    parse_time(&record.timestamp)?;
    match payload.kind.as_deref()? {
        "task_started" => Some(Mark::Start),
        "task_complete" | "turn_aborted" => Some(Mark::End),
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
    #[serde(rename = "isCompactSummary", default)]
    is_compact_summary: bool,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    message: Option<ClaudeMessage>,
    attachment: Option<serde_json::Value>,
    #[serde(rename = "turnOrigin")]
    turn_origin: Option<String>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_command_is_a_turn_start_signal() {
        let record = include_str!("../tests/fixtures/claude-attach/queued-command.json");
        assert!(matches!(
            claude_mark(serde_json::from_str(record).unwrap()),
            Some(Mark::Start)
        ));
    }

    #[test]
    fn turn_groups_follow_real_claude_boundaries_and_partial_pages() {
        let text = include_str!("../tests/fixtures/turn-groups/claude-steer.jsonl");
        let mut offset = 0;
        let records: Vec<(u64, serde_json::Value)> = text
            .lines()
            .map(|line| {
                let position = offset;
                offset += line.len() as u64 + 1;
                (position, serde_json::from_str(line).unwrap())
            })
            .collect();
        let turns = EventTurns::new(
            ProviderId::Claude,
            records.iter().map(|(at, record)| (*at, record)),
        );
        let start = records
            .iter()
            .find(|(_, r)| r["type"] == "user" && r["turnOrigin"] == "human")
            .unwrap()
            .0;
        let turn = turns.at(start).unwrap();
        assert!(turn.started && turn.complete);
        for (at, record) in &records {
            if record["type"] == "attachment" && record["attachment"]["type"] == "queued_command" {
                assert_eq!(turns.at(*at), Some(turn));
            }
        }
        let partial = EventTurns::new(
            ProviderId::Claude,
            records
                .iter()
                .filter(|(at, _)| *at > start)
                .map(|(at, record)| (*at, record)),
        );
        assert!(
            !partial
                .at(records.iter().find(|(at, _)| *at > start).unwrap().0)
                .unwrap()
                .started
        );
    }

    #[test]
    fn turn_groups_codex_starts_remain_distinct_when_ends_are_not_loaded() {
        let records: Vec<serde_json::Value> =
            include_str!("../tests/fixtures/codex-latest-activity.jsonl")
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .filter(|record: &serde_json::Value| {
                    matches!(
                        CodexRecord::deserialize(record).ok().and_then(codex_mark),
                        Some(Mark::Start)
                    )
                })
                .collect();
        assert!(records.len() >= 2);
        let turns = EventTurns::new(
            ProviderId::Codex,
            records
                .iter()
                .enumerate()
                .map(|(offset, record)| (offset as u64, record)),
        );
        for offset in 0..records.len() as u64 {
            assert_eq!(turns.at(offset).unwrap().id, offset);
        }
    }

    fn claude_boundary_records() -> Vec<serde_json::Value> {
        include_str!("../tests/fixtures/turn-groups/claude-boundaries.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn turn_groups_real_meta_wakeups_start_turns() {
        let records = claude_boundary_records();
        for origin in ["peer", "scheduled"] {
            let record = records
                .iter()
                .find(|record| record["isMeta"] == true && record["turnOrigin"] == origin)
                .unwrap();
            assert!(
                matches!(
                    claude_mark(serde_json::from_value(record.clone()).unwrap()),
                    Some(Mark::Start)
                ),
                "{origin}"
            );
        }
    }

    #[test]
    fn turn_groups_real_compaction_records_do_not_start_turns() {
        let records = claude_boundary_records();
        let summary = records
            .iter()
            .find(|record| record["isCompactSummary"] == true)
            .unwrap();
        assert_eq!(
            claude_mark(serde_json::from_value(summary.clone()).unwrap()),
            None
        );
        for record in records.iter().filter(|record| {
            record["message"]["content"].as_str().is_some_and(|text| {
                text.starts_with("<local-command") || text.starts_with("<command-name>")
            })
        }) {
            assert_eq!(
                claude_mark(serde_json::from_value(record.clone()).unwrap()),
                None
            );
        }
    }

    #[test]
    fn turn_groups_real_tool_results_and_sidechains_do_not_start_turns() {
        let records = claude_boundary_records();
        let tool = records
            .iter()
            .find(|record| {
                record["message"]["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
            })
            .unwrap();
        let sidechain = records
            .iter()
            .find(|record| record["isSidechain"] == true)
            .unwrap();
        for record in [tool, sidechain] {
            assert_eq!(
                claude_mark(serde_json::from_value(record.clone()).unwrap()),
                None
            );
        }
    }

    #[test]
    fn turn_groups_real_interrupt_closes_before_the_next_human_prompt() {
        let records = claude_boundary_records();
        let sequence: Vec<_> = records
            .iter()
            .filter(|record| {
                record["sessionId"] == "d9cac3b7-7847-4953-b998-03aeb47c7d98"
                    && record["type"] == "user"
                    && record["isMeta"] != true
            })
            .collect();
        assert_eq!(sequence.len(), 3);
        assert!(matches!(
            claude_mark(serde_json::from_value(sequence[1].clone()).unwrap()),
            Some(Mark::End)
        ));
        let turns = EventTurns::new(
            ProviderId::Claude,
            sequence
                .iter()
                .enumerate()
                .map(|(index, record)| (index as u64, *record)),
        );
        let first = turns.at(0).unwrap();
        assert!(first.started && first.complete);
        assert_eq!(turns.at(1), Some(first));
        let next = turns.at(2).unwrap();
        assert!(next.started && !next.complete);
        assert_ne!(next.id, first.id);
    }
}
