//! Resolves unpersisted questions from a blocked job and its attached screen.
use super::attach;
use crate::{
    ask::Ask,
    model::{Session, SessionStatus},
    providers::SessionKey,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs, io,
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

#[derive(Default)]
pub(super) struct PendingAsks {
    entries: HashMap<SessionKey, Entry>,
}

struct Entry {
    block: Value,
    epoch: DateTime<Utc>,
    receiver: Option<Receiver<io::Result<Ask>>>,
    result: Option<Result<Ask, String>>,
}

fn pending_block(bytes: &[u8]) -> io::Result<Option<Value>> {
    let state: Value = serde_json::from_slice(bytes)?;
    if state["tempo"] != "blocked"
        || !state["needs"]
            .as_str()
            .is_some_and(|needs| needs.starts_with("answer:"))
    {
        return Ok(None);
    }
    let block = &state["block"];
    if !block["questions"]
        .as_array()
        .is_some_and(|questions| !questions.is_empty())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude blocked question has no questions",
        ));
    }
    Ok(Some(block.clone()))
}

impl PendingAsks {
    pub(super) fn refresh(&mut self, config: &Path, sessions: &mut [Session]) {
        self.entries.retain(|id, _| {
            sessions.iter().any(|s| {
                &s.id == id && s.status == SessionStatus::NeedsInput && s.is_live_background()
            })
        });
        for session in sessions {
            if session.status != SessionStatus::NeedsInput || !session.is_live_background() {
                continue;
            }
            // A persisted pending call already has complete provider fields.
            if session.ask.is_some() {
                continue;
            }
            let job = session
                .background_job
                .as_ref()
                .expect("live background job");
            let path = config.join("jobs").join(&job.id).join("state.json");
            let block = match fs::read(&path).and_then(|bytes| pending_block(&bytes)) {
                Ok(Some(block)) => block,
                Ok(None) => {
                    self.entries.remove(&session.id);
                    continue;
                }
                Err(error) => {
                    eprintln!("agents: Claude pending question: {error}");
                    continue;
                }
            };
            let epoch = session.updated_at;
            let changed = self
                .entries
                .get(&session.id)
                .is_none_or(|entry| entry.block != block || entry.epoch != epoch);
            if changed {
                let (sender, receiver) = mpsc::channel();
                let (job_id, cwd, id, source) = (
                    job.id.clone(),
                    session.cwd.clone(),
                    session.id.clone(),
                    block.clone(),
                );
                let since = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(DateTime::<Utc>::from)
                    .unwrap_or(epoch);
                let result = thread::Builder::new()
                    .name("agents-question-screen".into())
                    .spawn(move || {
                        let _ = sender.send(attach::pending(&job_id, &cwd, id, since, &source));
                    });
                let entry = Entry {
                    block,
                    epoch,
                    receiver: result.as_ref().ok().map(|_| receiver),
                    result: result.err().map(|error| Err(error.to_string())),
                };
                self.entries.insert(session.id.clone(), entry);
            }
            let entry = self
                .entries
                .get_mut(&session.id)
                .expect("pending screen entry");
            if let Some(receiver) = &entry.receiver {
                match receiver.try_recv() {
                    Ok(result) => {
                        entry.result = Some(result.map_err(|error| error.to_string()));
                        entry.receiver = None;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        entry.result = Some(Err("Claude question inspection stopped".into()));
                        entry.receiver = None;
                    }
                }
            }
            match &entry.result {
                Some(Ok(ask)) => {
                    session.pending_questions = match &ask.kind {
                        crate::ask::AskKind::Questions { questions, .. } => questions
                            .iter()
                            .map(|question| question.text.clone())
                            .collect(),
                    };
                    session.ask = Some(ask.clone());
                }
                Some(Err(error)) => {
                    if let Some(job) = &mut session.background_job {
                        job.needs = Some(error.clone());
                    }
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ask_detection_uses_recorded_job_block_without_inventing_fields() {
        let fixture =
            include_bytes!("../../../tests/fixtures/claude-attach/ask/pending-state.json");
        let block = pending_block(fixture).unwrap().unwrap();
        assert_eq!(block["questions"][0]["question"], "Probe escape?");
        assert!(block["questions"][0].get("multiSelect").is_none());
        assert!(block["questions"][0].get("header").is_none());
        let mut state: Value = serde_json::from_slice(fixture).unwrap();
        state["tempo"] = "working".into();
        assert!(
            pending_block(&serde_json::to_vec(&state).unwrap())
                .unwrap()
                .is_none()
        );
    }
}
