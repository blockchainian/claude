use crate::model::BackgroundTask;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Terminal {
    pub process_id: String,
    pub command: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Terminals {
    pub data: Vec<Terminal>,
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub(super) struct Background {
    first_seen: HashMap<(String, String), DateTime<Utc>>,
    rollouts: HashMap<PathBuf, Starts>,
}

impl Background {
    pub(super) fn tasks(
        &mut self,
        thread: &str,
        path: Option<&Path>,
        terminals: Vec<Terminal>,
        now: DateTime<Utc>,
    ) -> io::Result<Vec<BackgroundTask>> {
        self.first_seen.retain(|(owner, id), _| {
            owner != thread || terminals.iter().any(|terminal| &terminal.process_id == id)
        });
        if terminals.is_empty() {
            return Ok(vec![]);
        }
        let starts = if let Some(path) = path {
            let starts = self.rollouts.entry(path.to_owned()).or_default();
            match starts.read(path) {
                Ok(()) => Some(&starts.times),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        Ok(terminals
            .into_iter()
            .map(|terminal| {
                let seen = self
                    .first_seen
                    .entry((thread.to_owned(), terminal.process_id.clone()))
                    .or_insert(now);
                BackgroundTask {
                    description: terminal.command,
                    output: None,
                    started_at: starts
                        .and_then(|times| times.get(&terminal.process_id))
                        .copied()
                        .unwrap_or(*seen),
                }
            })
            .collect())
    }
}

#[derive(Default)]
struct Starts {
    offset: u64,
    identity: Option<(u64, u64)>,
    boundary: crate::jsonl::Boundary,
    times: HashMap<String, DateTime<Utc>>,
}

impl Starts {
    fn read(&mut self, path: &Path) -> io::Result<()> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        if self.identity != Some(identity)
            || metadata.len() < self.offset
            || !self.boundary.matches(&mut file)?
        {
            *self = Self::default();
        }
        self.identity = Some(identity);
        let mut error = None;
        let offset = crate::jsonl::read_lines(&mut file, self.offset, |line| {
            if error.is_some() {
                return;
            }
            let Ok(record) = serde_json::from_slice::<Value>(line) else {
                return;
            };
            if let Err(invalid) = self.record(&record) {
                error = Some(invalid);
            }
        })?;
        if let Some(error) = error {
            return Err(error);
        }
        self.offset = offset;
        self.boundary.capture(&mut file, offset)
    }

    fn record(&mut self, record: &Value) -> io::Result<()> {
        let payload = &record["payload"];
        if record["type"] != "response_item" || payload["type"] != "custom_tool_call_output" {
            return Ok(());
        }
        let mut output = Vec::new();
        if let Some(text) = payload["output"].as_str() {
            output.push(text);
        }
        if let Some(blocks) = payload["output"].as_array() {
            output.extend(blocks.iter().filter_map(|block| block["text"].as_str()));
        }
        for text in output {
            let Ok(result) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            let Some(id) = result.get("session_id") else {
                continue;
            };
            // Only the first yielded execution supplies the start, never a completed poll.
            if result.get("exit_code").is_some() {
                continue;
            }
            let id = id
                .as_u64()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid Codex background session id",
                    )
                })?
                .to_string();
            let at = record["timestamp"]
                .as_str()
                .and_then(|text| text.parse::<DateTime<Utc>>().ok())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing Codex background output timestamp",
                    )
                })?;
            self.times.entry(id).or_insert(at);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ROLLOUT: &str = include_str!("../../../tests/fixtures/codex-background/rollout.jsonl");
    fn terminals() -> Vec<Terminal> {
        serde_json::from_str::<Terminals>(include_str!(
            "../../../tests/fixtures/codex-background/terminals.json"
        ))
        .unwrap()
        .data
    }

    #[test]
    fn background_terminal_uses_recorded_start_and_command() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), ROLLOUT).unwrap();
        let now = "2026-10-19T06:00:00Z".parse().unwrap();
        let tasks = Background::default()
            .tasks("thread", Some(file.path()), terminals(), now)
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description, "sleep 120 ; echo done");
        assert_eq!(tasks[0].output, None);
        assert_eq!(
            tasks[0].started_at,
            "2026-10-19T05:08:55.993Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn background_terminal_first_seen_is_stable_and_scoped_to_thread() {
        let now = "2026-10-19T06:00:00Z".parse().unwrap();
        let later = now + chrono::Duration::seconds(10);
        let mut background = Background::default();
        for seen in [now, later] {
            let tasks = background.tasks("first", None, terminals(), seen).unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].started_at, now);
        }
        assert_eq!(
            background
                .tasks("second", None, terminals(), later)
                .unwrap()[0]
                .started_at,
            later
        );
        assert!(
            background
                .tasks("first", None, vec![], later)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            background.tasks("first", None, terminals(), later).unwrap()[0].started_at,
            later
        );
    }

    #[test]
    fn background_terminal_picks_up_start_when_rollout_arrives() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let now = "2026-10-19T06:00:00Z".parse().unwrap();
        let mut background = Background::default();
        assert_eq!(
            background
                .tasks("thread", Some(file.path()), terminals(), now)
                .unwrap()[0]
                .started_at,
            now
        );
        std::fs::write(file.path(), ROLLOUT).unwrap();
        assert_eq!(
            background
                .tasks("thread", Some(file.path()), terminals(), now)
                .unwrap()[0]
                .started_at,
            "2026-10-19T05:08:55.993Z".parse::<DateTime<Utc>>().unwrap()
        );
    }
}
