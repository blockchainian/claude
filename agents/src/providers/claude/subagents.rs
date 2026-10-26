use super::transcript::Summary;
use crate::model::SessionStatus;
use serde::Deserialize;
use std::path::Path;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metadata {
    description: Option<String>,
    name: Option<String>,
    task_kind: Option<String>,
    team_name: Option<String>,
    #[serde(default)]
    finished: bool,
}

impl Metadata {
    pub(super) fn read(path: &Path) -> Option<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).ok(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Self::default()),
            Err(_) => None,
        }
    }

    pub(super) fn title(&self) -> String {
        self.description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(self.name.as_deref())
            .unwrap_or_default()
            .to_owned()
    }

    pub(super) fn description(&self) -> Option<&str> {
        self.description
            .as_deref()
            .filter(|description| !description.trim().is_empty())
    }

    pub(super) fn name(&self) -> Option<String> {
        self.name.clone()
    }

    pub(super) fn status(&self, summary: &Summary, config: &Path, alive: bool) -> SessionStatus {
        if self.finished {
            return SessionStatus::Inactive;
        }
        let Some(last) = summary.subagent_records.last() else {
            return SessionStatus::Inactive;
        };
        let blocks = last["message"]["content"].as_array();
        if last["type"] == "user"
            && blocks.is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block["type"] == "tool_result"
                        && (summary.subagent_records.iter().rev().skip(1).any(|record| {
                            record["type"] == "assistant"
                                && record["message"]["content"].as_array().is_some_and(|uses| {
                                    uses.iter().any(|tool| {
                                        tool["type"] == "tool_use"
                                            && tool["name"] == "SubagentHandback"
                                            && tool["id"].as_str().is_some()
                                            && tool["id"] == block["tool_use_id"]
                                    })
                                })
                        }) || (last["toolUseResult"]["success"] == true
                            && last["toolUseResult"]["message"]
                                == "Report delivered to your caller."))
                })
            })
        {
            return SessionStatus::Inactive;
        }
        let text = super::transcript::content_text(last, false);
        if last["type"] == "user"
            && text
                .as_deref()
                .is_some_and(|text| text.trim().starts_with("[Request interrupted by user"))
        {
            return SessionStatus::Inactive;
        }
        if last["type"] == "assistant" {
            if last["message"]["stop_reason"] == "stop_sequence"
                && text
                    .as_deref()
                    .is_some_and(|text| text.starts_with("API Error:"))
            {
                return SessionStatus::Inactive;
            }
            if last["message"]["stop_reason"] == "end_turn"
                && blocks.is_some_and(|blocks| {
                    !blocks.is_empty() && blocks.iter().all(|block| block["type"] == "text")
                })
            {
                return if self.task_kind.as_deref() == Some("in_process_teammate")
                    && self.is_member(config)
                {
                    SessionStatus::Ready
                } else {
                    SessionStatus::Inactive
                };
            }
        }
        if alive {
            SessionStatus::Working
        } else {
            SessionStatus::Inactive
        }
    }

    fn is_member(&self, config: &Path) -> bool {
        let (Some(team), Some(name)) = (&self.team_name, &self.name) else {
            return false;
        };
        // Team names identify one directory beneath the configured teams root.
        if Path::new(team).components().count() != 1
            || !matches!(
                Path::new(team).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return false;
        }
        let Ok(bytes) = std::fs::read(config.join("teams").join(team).join("config.json")) else {
            return false;
        };
        let Ok(team) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        team["members"].as_array().is_some_and(|members| {
            members
                .iter()
                .any(|member| member["name"].as_str() == Some(name))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Provider;
    use crate::providers::claude::{ClaudeProvider, registry, tests::transcript};
    use serde_json::{Value, json};
    use std::{
        fs,
        io::{Read, Seek, SeekFrom},
        path::PathBuf,
    };

    const FIXTURE: &str = include_str!("../../../tests/fixtures/subagent.jsonl");

    fn records() -> Vec<Value> {
        FIXTURE
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn pending() -> Vec<Value> {
        let mut rows = records();
        rows.pop();
        rows[0]["message"]["content"][0]["name"] = "Bash".into();
        rows[0]["message"]["content"][0]["input"] =
            json!({"command":"pwd", "description":"Check directory"});
        rows
    }

    fn text_end(stop: Value, text: &str) -> Vec<Value> {
        let mut rows = pending();
        rows[0]["message"]["content"] = json!([{"type":"text", "text":text}]);
        rows[0]["message"]["stop_reason"] = stop;
        rows
    }

    fn status(rows: Vec<Value>, teammate: bool, listed: bool, alive: bool) -> SessionStatus {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-x.jsonl");
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let summary = crate::providers::claude::transcript::read_subagent(
            &path,
            fs::metadata(&path).unwrap().len(),
        )
        .unwrap();
        let meta: Metadata = serde_json::from_value(json!({"taskKind":if teammate {"in_process_teammate"} else {"background"}, "teamName":"test", "name":"worker"})).unwrap();
        if listed {
            fs::create_dir_all(dir.path().join("teams/test")).unwrap();
            fs::write(
                dir.path().join("teams/test/config.json"),
                r#"{"members":[{"name":"worker"}]}"#,
            )
            .unwrap();
        }
        meta.status(&summary, dir.path(), alive)
    }

    #[test]
    fn subagent_input_requires_running_background_parent() {
        let dir = tempfile::tempdir().unwrap();
        let mut entry = registry::tests::fixture(dir.path());
        let id = entry["sessionId"].as_str().unwrap().to_owned();
        let parent = transcript(dir.path(), &id, 0);
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        let child = agents.join("agent-input.jsonl");
        fs::write(&child, pending()[0].to_string()).unwrap();
        fs::write(
            child.with_extension("meta.json"),
            r#"{"description":"Check directory","taskKind":"background"}"#,
        )
        .unwrap();
        entry["kind"] = "bg".into();
        entry["jobId"] = id[..8].into();
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-input")
            .unwrap();
        assert_eq!(child.status, SessionStatus::Working);
        assert_eq!(child.input_disabled_reason(), None);
        assert!(!child.can_resume());

        entry["kind"] = "interactive".into();
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-input")
            .unwrap();
        assert_eq!(child.input_disabled_reason(), Some(""));

        fs::write(
            agents.join("agent-input.meta.json"),
            r#"{"description":"Check directory","finished":true}"#,
        )
        .unwrap();
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-input")
            .unwrap();
        assert_eq!(child.input_disabled_reason(), Some(""));
    }

    #[test]
    fn finished_subagents_remain_in_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let entry = registry::tests::fixture(dir.path());
        let id = entry["sessionId"].as_str().unwrap();
        let parent = transcript(dir.path(), id, 0);
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("agent-done.jsonl"), FIXTURE).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-done")
            .unwrap();
        assert_eq!(child.parent.as_ref().map(|key| key.id.as_str()), Some(id));
        assert_eq!(child.status, SessionStatus::Inactive);
    }

    #[test]
    fn pending_subagents_with_dead_parent_remain_in_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let parent = transcript(dir.path(), id, 0);
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(agents.join("agent-pending.jsonl"), pending()[0].to_string()).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        let parent = snapshot.sessions.iter().find(|s| s.id.id == id).unwrap();
        assert_eq!(parent.status, SessionStatus::Inactive);
        assert!(parent.process.is_none());
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-pending")
            .unwrap();
        assert_eq!(child.parent.as_ref(), Some(&parent.id));
        assert_eq!(child.status, SessionStatus::Inactive);
    }

    #[test]
    fn handback_result_finishes() {
        assert_eq!(
            status(records(), false, false, true),
            SessionStatus::Inactive
        );
    }

    #[test]
    fn finished_wait_with_prompt_snapshot_is_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let entry = registry::tests::fixture(dir.path());
        let parent = transcript(dir.path(), entry["sessionId"].as_str().unwrap(), 0);
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        let path = agents.join("agent-a1f849f784f4f1c99.jsonl");
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/claude-finished-wait.jsonl"),
        )
        .unwrap();
        let mut metadata: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-finished-wait.meta.json"
        ))
        .unwrap();
        metadata["finished"] = false.into();
        fs::write(path.with_extension("meta.json"), metadata.to_string()).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-a1f849f784f4f1c99")
            .unwrap();
        assert_eq!(child.status, SessionStatus::Working);
        fs::write(
            path.with_extension("meta.json"),
            include_str!("../../../tests/fixtures/claude-finished-wait.meta.json"),
        )
        .unwrap();
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-a1f849f784f4f1c99")
            .unwrap();
        assert_eq!(child.title, "wait");
        assert_eq!(child.status, SessionStatus::Inactive);

        // Completion metadata also works without a terminal transcript record.
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/claude-finished-wait.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            store
                .refresh(1)
                .sessions
                .iter()
                .find(|s| s.id.id == "agent-a1f849f784f4f1c99")
                .unwrap()
                .status,
            SessionStatus::Inactive
        );
    }
    #[test]
    fn handback_pair_does_not_need_result_metadata() {
        let mut rows = records();
        rows[1].as_object_mut().unwrap().remove("toolUseResult");
        assert_eq!(status(rows, false, false, true), SessionStatus::Inactive);
    }

    #[test]
    fn handback_result_without_tool_use_finishes() {
        let mut rows = records();
        rows.remove(0);
        assert_eq!(status(rows, false, false, true), SessionStatus::Inactive);
    }

    #[test]
    fn other_tool_result_keeps_working() {
        let mut rows = records();
        rows[0]["message"]["content"][0]["name"] = "Bash".into();
        rows[1].as_object_mut().unwrap().remove("toolUseResult");
        assert_eq!(status(rows, false, false, true), SessionStatus::Working);
    }

    #[test]
    fn resumed_user_message_keeps_working() {
        let mut rows = records();
        let mut user = rows[1].clone();
        user["message"]["content"] = json!("Continue checking");
        user.as_object_mut().unwrap().remove("toolUseResult");
        rows.push(user);
        assert_eq!(status(rows, false, false, true), SessionStatus::Working);
    }

    #[test]
    fn mixed_content_end_turn_keeps_working() {
        let mut rows = text_end(json!("end_turn"), "Checking");
        rows[0]["message"]["content"]
            .as_array_mut()
            .unwrap()
            .push(pending()[0]["message"]["content"][0].clone());
        assert_eq!(status(rows, false, false, true), SessionStatus::Working);
    }

    #[test]
    fn background_end_turn_finishes() {
        assert_eq!(
            status(text_end(json!("end_turn"), "Done"), false, false, true),
            SessionStatus::Inactive
        );
    }
    #[test]
    fn listed_teammate_is_ready() {
        assert_eq!(
            status(text_end(json!("end_turn"), "Done"), true, true, true),
            SessionStatus::Ready
        );
    }
    #[test]
    fn unlisted_teammate_finishes() {
        assert_eq!(
            status(text_end(json!("end_turn"), "Done"), true, false, true),
            SessionStatus::Inactive
        );
    }
    #[test]
    fn pending_tool_with_live_parent_works() {
        assert_eq!(
            status(pending(), false, false, true),
            SessionStatus::Working
        );
    }
    #[test]
    fn pending_tool_with_dead_parent_finishes() {
        assert_eq!(
            status(pending(), false, false, false),
            SessionStatus::Inactive
        );
    }
    #[test]
    fn api_error_finishes() {
        assert_eq!(
            status(
                text_end(
                    json!("stop_sequence"),
                    "API Error: Output blocked by content filtering policy"
                ),
                false,
                false,
                true
            ),
            SessionStatus::Inactive
        );
    }
    #[test]
    fn interruption_finishes() {
        let mut rows = records();
        rows[1]["message"]["content"] = json!("[Request interrupted by user]");
        rows[1].as_object_mut().unwrap().remove("toolUseResult");
        assert_eq!(status(rows, false, false, true), SessionStatus::Inactive);
    }

    #[test]
    fn interruption_during_tool_use_finishes() {
        let mut rows = records();
        rows[1]["message"]["content"] =
            json!([{"type":"text","text":"[Request interrupted by user for tool use]"}]);
        rows[1].as_object_mut().unwrap().remove("toolUseResult");
        assert_eq!(status(rows, false, false, true), SessionStatus::Inactive);
    }

    #[test]
    fn discovers_fields_and_retains_cache_after_parent_exits() {
        let dir = tempfile::tempdir().unwrap();
        let entry = registry::tests::fixture(dir.path());
        let id = entry["sessionId"].as_str().unwrap();
        let parent = transcript(dir.path(), id, 0);
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        let path = agents.join("agent-x.jsonl");
        let mut rows = pending();
        let mut reply = rows[0].clone();
        reply["message"]["content"] = json!([{"type":"text", "text":"Still checking"}]);
        rows.insert(0, reply);
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        fs::write(
            agents.join("agent-x.meta.json"),
            r#"{"description":"Check files","name":"worker"}"#,
        )
        .unwrap();
        fs::write(agents.join("agent-done.jsonl"), FIXTURE).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        let merged =
            crate::sessions::Hub::new(vec![Box::new(ClaudeProvider::new(dir.path().into()))])
                .refresh();
        assert_eq!(merged.sessions, snapshot.sessions);
        assert_eq!(merged.has_more, snapshot.has_more);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-x")
            .unwrap();
        assert_eq!(snapshot.sessions.len(), 3);
        assert_eq!(child.parent.as_ref().map(|key| key.id.as_str()), Some(id));
        assert_eq!(child.id.id, "agent-x");
        assert_eq!(child.transcript.as_ref(), Some(&path));
        assert_eq!(child.process, None);
        assert_eq!(child.title, "Check files");
        assert_eq!(child.prompt, "Check files");
        assert_eq!(child.name.as_deref(), Some("worker"));
        assert_eq!(child.cwd, PathBuf::from(rows[0]["cwd"].as_str().unwrap()));
        assert_eq!(child.git_branch.as_deref(), Some("HEAD"));
        assert_eq!(child.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(child.last_message.as_deref(), Some("Still checking"));
        assert_eq!(child.activity.as_ref().unwrap().label, "Check directory");
        assert_eq!(child.status, crate::model::SessionStatus::Working);
        assert_eq!(
            child.started_at,
            Some(rows[0]["timestamp"].as_str().unwrap().parse().unwrap())
        );
        let cached = store.summaries[&path]
            .summary
            .last_message
            .as_ref()
            .unwrap()
            .as_ptr();
        assert_eq!(store.refresh(1).sessions, snapshot.sessions);
        assert_eq!(
            store.summaries[&path]
                .summary
                .last_message
                .as_ref()
                .unwrap()
                .as_ptr(),
            cached
        );
        let mut reply = rows[0].clone();
        reply["message"]["content"][0]["text"] = json!("Checking again");
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "\n{reply}"
        )
        .unwrap();
        let changed = store.refresh(1);
        assert_eq!(
            changed
                .sessions
                .iter()
                .find(|s| s.id.id == "agent-x")
                .unwrap()
                .last_message
                .as_deref(),
            Some("Checking again")
        );
        fs::write(agents.join("agent-x.meta.json"), r#"{"name":"renamed"}"#).unwrap();
        let changed = store.refresh(1);
        let child = changed
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-x")
            .unwrap();
        assert_eq!(child.title, "renamed");
        assert_eq!(child.prompt, "renamed");
        fs::remove_file(dir.path().join("sessions/live.json")).unwrap();
        let snapshot = store.refresh(1);
        assert_eq!(snapshot.sessions.len(), 3);
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|s| s.status == SessionStatus::Inactive)
        );
        assert!(store.summaries.contains_key(&path));
    }

    #[test]
    fn discovered_subagent_start_time_survives_transcript_updates() {
        let dir = tempfile::tempdir().unwrap();
        let entry = registry::tests::fixture(dir.path());
        let parent = transcript(dir.path(), entry["sessionId"].as_str().unwrap(), 0);
        fs::write(dir.path().join("sessions/live.json"), entry.to_string()).unwrap();
        let agents = parent.with_extension("").join("subagents");
        fs::create_dir_all(&agents).unwrap();
        let path = agents.join("agent-x.jsonl");
        fs::write(&path, pending()[0].to_string()).unwrap();
        let created: chrono::DateTime<chrono::Utc> =
            fs::metadata(&path).unwrap().created().unwrap().into();
        let mut store = ClaudeProvider::new(dir.path().into());
        for elapsed in [60, 120] {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(
                    std::time::SystemTime::from(created) + std::time::Duration::from_secs(elapsed),
                )
                .unwrap();
            let snapshot = store.refresh(1);
            let child = snapshot
                .sessions
                .iter()
                .find(|s| s.id.id == "agent-x")
                .unwrap();
            assert_eq!(
                child.started_at,
                Some(pending()[0]["timestamp"].as_str().unwrap().parse().unwrap())
            );
            assert_eq!(child.activity.as_ref().unwrap().label, "Check directory");
        }
        fs::write(agents.join("agent-x.meta.json"), r#"{"description":"Review files","name":"reviewer","taskKind":"in_process_teammate","teamName":"review"}"#).unwrap();
        fs::create_dir_all(dir.path().join("teams/review")).unwrap();
        fs::write(
            dir.path().join("teams/review/config.json"),
            r#"{"members":[{"name":"reviewer"}]}"#,
        )
        .unwrap();
        let mut rows = pending();
        let mut reply = text_end(json!("end_turn"), "Review complete").remove(0);
        reply["timestamp"] = json!("2026-10-06T09:36:06Z");
        rows.push(reply);
        fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-x")
            .unwrap();
        assert_eq!(child.status, SessionStatus::Ready);
        assert_eq!(child.title, "Review files");
        assert_eq!(
            child.started_at,
            Some(rows[0]["timestamp"].as_str().unwrap().parse().unwrap())
        );
        assert_eq!(child.last_message.as_deref(), Some("Review complete"));
        assert!(child.latest_activity().is_none());
        fs::write(&path, FIXTURE).unwrap();
        let snapshot = store.refresh(1);
        let child = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "agent-x")
            .unwrap();
        assert_eq!(
            child.parent.as_ref(),
            snapshot
                .sessions
                .iter()
                .find(|s| s.parent.is_none())
                .map(|s| &s.id)
        );
        assert_eq!(child.status, SessionStatus::Inactive);
    }

    #[test]
    fn reads_at_most_one_tail_chunk_of_24_mb() {
        struct Counted {
            inner: std::io::Cursor<Vec<u8>>,
            read: usize,
        }
        impl Read for Counted {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                let count = self.inner.read(bytes)?;
                self.read += count;
                Ok(count)
            }
        }
        impl Seek for Counted {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                self.inner.seek(pos)
            }
        }
        let mut bytes = vec![b'!'; 24 * 1024 * 1024];
        bytes.push(b'\n');
        bytes.extend_from_slice(FIXTURE.as_bytes());
        let len = bytes.len() as u64;
        let mut reader = Counted {
            inner: std::io::Cursor::new(bytes),
            read: 0,
        };
        let summary = crate::providers::claude::transcript::read_subagent_tail(
            Path::new("subagent.jsonl"),
            &mut reader,
            len,
        )
        .unwrap();
        assert!(reader.read <= 64 * 1024 + 1);
        assert_eq!(summary.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(summary.subagent_records.len(), 2);
    }
}
