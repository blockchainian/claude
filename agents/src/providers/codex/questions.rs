use crate::{
    ask::{Answer, Ask, AskKind, Question},
    model::Session,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn apply(
    sessions: &mut [Session],
    pending: Vec<(usize, String, DateTime<Utc>, Vec<Question>)>,
) {
    for (index, call, since, items) in pending {
        let session = &mut sessions[index];
        session.ask = Some(ask(session, &call, since, items));
        session.status = crate::model::SessionStatus::NeedsInput;
    }
}

fn ask(session: &Session, call: &str, since: DateTime<Utc>, mut questions: Vec<Question>) -> Ask {
    for question in &mut questions {
        question.other = true;
        question.secret = false;
    }
    Ask {
        id: call.into(),
        session: session.id.clone(),
        since,
        kind: AskKind::Questions {
            questions,
            blocking: true,
        },
    }
}

pub(super) fn encode(ask: &Ask, answer: &Answer) -> io::Result<Value> {
    let AskKind::Questions { questions, .. } = &ask.kind;
    if answer
        .answers
        .keys()
        .any(|id| !questions.iter().any(|q| &q.id == id))
    {
        return Err(io::Error::other("Unknown Codex question id"));
    }
    let mut encoded = BTreeMap::new();
    for question in questions {
        let Some(value) = answer.answers.get(&question.id) else {
            continue;
        };
        if (!question.multi && value.answers.len() + usize::from(value.other.is_some()) > 1)
            || value
                .answers
                .iter()
                .any(|label| !question.options.iter().any(|option| &option.label == label))
        {
            return Err(io::Error::other("Invalid Codex answer selection"));
        }
        let mut answers = value.answers.clone();
        if let Some(other) = &value.other {
            if !question.other || other.trim().is_empty() {
                return Err(io::Error::other("Invalid Codex free text answer"));
            }
            answers.push(other.clone());
        }
        encoded.insert(&question.id, json!({"answers": answers}));
    }
    Ok(json!({"answers": encoded}))
}

pub(super) fn confirm(
    path: &Path,
    call: &str,
    expected: &Value,
    mut offset: u64,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut file = std::fs::File::open(path)?;
    loop {
        let mut found = None;
        offset = crate::jsonl::read_lines(&mut file, offset, |line| {
            let Ok(record) = serde_json::from_slice::<Value>(line) else {
                return;
            };
            let payload = &record["payload"];
            if record["type"] == "response_item"
                && payload["type"] == "function_call_output"
                && payload["call_id"] == call
            {
                found = Some(
                    payload["output"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("Missing Codex answer output"))
                        .and_then(|text| {
                            serde_json::from_str::<Value>(text).map_err(io::Error::other)
                        }),
                );
            }
        })?;
        if let Some(result) = found {
            if result? != *expected {
                return Err(io::Error::other("Codex recorded a different answer"));
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Codex answer missing from rollout",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Provider, SessionInput, codex::CodexProvider};
    use rusqlite::Connection;

    fn session() -> Session {
        let dir = tempfile::tempdir().unwrap();
        Connection::open(dir.path().join("state_5.sqlite"))
            .unwrap()
            .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        CodexProvider::new(dir.path().into())
            .snapshot(1)
            .unwrap()
            .sessions
            .remove(0)
    }

    fn captured_ask() -> Ask {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            include_str!("../../../tests/fixtures/codex-input/daemon-pending.jsonl"),
        )
        .unwrap();
        let summary = super::super::rollout::summary(&path).unwrap();
        let (call, since) = summary.pending_question_call.unwrap();
        ask(&session(), &call, since, summary.pending_questions)
    }

    #[test]
    fn codex_answers_have_a_send_job() {
        let dir = tempfile::tempdir().unwrap();
        let provider = CodexProvider::new(dir.path().into());
        let session = session();
        let mut ask = captured_ask();
        ask.session = session.id.clone();
        assert!(
            provider
                .send(
                    &session,
                    SessionInput::Answer {
                        ask,
                        answer: Answer::default(),
                        transcript: session.transcript.clone().unwrap(),
                    }
                )
                .is_some()
        );
    }

    #[test]
    fn rollout_question_uses_call_identity_and_default_flags() {
        let ask = captured_ask();
        assert_eq!(ask.id, "call_3d3e7995122343ac8f2f117588d23898");
        let AskKind::Questions { questions, .. } = ask.kind;
        assert_eq!(questions[0].id, "color");
        assert!(questions[0].other);
        assert!(!questions[0].secret);
    }

    #[test]
    fn answers_use_question_ids_and_omit_other_field() {
        let ask = captured_ask();
        let selected: Answer =
            serde_json::from_value(json!({"answers":{"color":{"answers":["Blue"]}}})).unwrap();
        assert_eq!(
            encode(&ask, &selected).unwrap(),
            json!({"answers":{"color":{"answers":["Blue"]}}})
        );
        let other: Answer =
            serde_json::from_value(json!({"answers":{"color":{"answers":[],"other":"Green"}}}))
                .unwrap();
        assert_eq!(
            encode(&ask, &other).unwrap(),
            json!({"answers":{"color":{"answers":["Green"]}}})
        );
        for invalid in [
            json!({"answers":{"wrong":{"answers":["Blue"]}}}),
            json!({"answers":{"color":{"answers":["Green"]}}}),
            json!({"answers":{"color":{"answers":["Blue"],"other":"Green"}}}),
        ] {
            assert!(encode(&ask, &serde_json::from_value(invalid).unwrap()).is_err());
        }
    }

    #[test]
    fn unanswered_questions_keep_the_shared_submit_behavior() {
        let ask = captured_ask();
        let answer: Answer =
            serde_json::from_value(json!({"answers":{"color":{"answers":[]}}})).unwrap();
        assert_eq!(
            encode(&ask, &answer).unwrap(),
            json!({"answers":{"color":{"answers":[]}}})
        );
    }

    #[test]
    fn success_requires_the_matching_rollout_output_and_answer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            include_str!("../../../tests/fixtures/codex-input/daemon-answered.jsonl"),
        )
        .unwrap();
        let ask = captured_ask();
        confirm(
            &path,
            &ask.id,
            &json!({"answers":{"color":{"answers":["Blue"]}}}),
            0,
        )
        .unwrap();
        assert!(
            confirm(
                &path,
                &ask.id,
                &json!({"answers":{"color":{"answers":["Red"]}}}),
                0
            )
            .is_err()
        );
    }

    #[test]
    fn codex_replayed_question_is_visible_in_dashboard() {
        let mut row = session();
        row.status = crate::model::SessionStatus::NeedsInput;
        row.ask = Some(captured_ask());
        row.ask.as_mut().unwrap().session = row.id.clone();
        let mut view = crate::ui::View::new(crate::model::Grouping::Project, false);
        view.set_sessions(vec![row], false);
        let area = ratatui::layout::Rect::new(0, 0, 100, 24);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        view.render_at(area, &mut buffer, "2026-10-20T03:00:00Z".parse().unwrap());
        let rendered = buffer
            .content()
            .chunks(100)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert!(rendered.contains("Pick a color?"), "{rendered}");
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/codex_question.txt");
        if std::env::var_os("UPDATE_CODEX_QUESTION_SNAPSHOT").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
        view.prepare_prompt();
        assert!(matches!(
            view.handle_key(crossterm::event::KeyCode::Char('2').into()),
            Some(crate::model::Action::AnswerQuestions { .. })
        ));
    }

    #[test]
    fn offline_questions_offer_an_answer_from_the_rollout() {
        let dir = tempfile::tempdir().unwrap();
        let provider = CodexProvider::new(dir.path().into());
        let mut sessions = vec![session()];
        let ask = captured_ask();
        let AskKind::Questions { questions, .. } = ask.kind;
        apply(&mut sessions, vec![(0, ask.id, ask.since, questions)]);
        assert!(sessions[0].ask.is_some());
        assert!(
            provider
                .send(&sessions[0], SessionInput::Interrupt)
                .is_some()
        );
    }
}
