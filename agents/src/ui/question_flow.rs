//! Keeps pending question drafts with their session.

use super::*;
use question_view::{Outcome, QuestionView};

#[derive(Default)]
pub(super) struct Questions {
    views: HashMap<SessionKey, QuestionView>,
    closed: HashMap<SessionKey, crate::ask::Ask>,
    pub(super) sending: HashSet<SessionKey>,
}

impl Questions {
    fn sync(&mut self, rows: &[Session]) {
        let pending = |id: &SessionKey| {
            rows.iter()
                .find(|row| {
                    &row.id == id
                        && row.is_live_background()
                        && row.status == SessionStatus::NeedsInput
                })
                .and_then(|row| row.ask.as_ref())
        };
        self.views
            .retain(|id, view| pending(id).is_some_and(|ask| ask == view.ask()));
        self.closed.retain(|id, closed| pending(id) == Some(closed));
        self.sending.retain(|id| pending(id).is_some());
        for row in rows {
            if row.status == SessionStatus::NeedsInput
                && row.is_live_background()
                && let Some(ask) = &row.ask
                && !self.views.contains_key(&row.id)
            {
                self.views
                    .insert(row.id.clone(), QuestionView::new(ask.clone()));
            }
        }
    }

    fn visible(&self, id: &SessionKey) -> Option<&QuestionView> {
        self.views
            .get(id)
            .filter(|view| self.closed.get(id) != Some(view.ask()))
    }

    pub(super) fn begin(&mut self, ask: &crate::ask::Ask) {
        self.sending.insert(ask.session.clone());
        if let Some(view) = self.views.get_mut(&ask.session)
            && view.ask() == ask
        {
            view.set_error(None);
        }
    }

    pub(super) fn complete(&mut self, ask: &crate::ask::Ask, result: &std::io::Result<()>) {
        self.sending.remove(&ask.session);
        if let Some(view) = self.views.get_mut(&ask.session)
            && view.ask() == ask
        {
            view.set_error(result.as_ref().err().map(ToString::to_string));
        }
        if result.is_ok() {
            self.closed.insert(ask.session.clone(), ask.clone());
        }
    }
}

impl View {
    pub(super) fn sync_questions(&mut self) {
        self.questions.sync(&self.rows);
    }

    pub(super) fn active_question(&self) -> Option<&QuestionView> {
        if self.selected_draft.is_some() {
            return None;
        }
        self.questions.visible(self.selected_id()?)
    }

    pub(super) fn question_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        if self.help
            || self.confirm_dialog.is_some()
            || self.picker.is_some()
            || self.branch_editor.is_some()
            || self.searching
            || self.rename_target.is_some()
            || self.log_search.editing
            || self.log_detail_search.editing
        {
            return None;
        }
        let id = self.selected_id()?.clone();
        self.questions.visible(&id)?;
        if self.questions.sending.contains(&id) {
            return Some(None);
        }
        let view = self.questions.views.get_mut(&id).expect("visible question");
        Some(match view.key(key) {
            Some(Outcome::Dismiss) => {
                self.questions.closed.insert(id, view.ask().clone());
                None
            }
            Some(Outcome::Submit(answer)) => Some(Action::AnswerQuestions {
                ask: view.ask().clone(),
                answer,
            }),
            None => None,
        })
    }

    pub(super) fn question_paste(&mut self, text: &str) -> bool {
        let Some(id) = self.selected_id().cloned() else {
            return false;
        };
        if self.questions.visible(&id).is_none() {
            return false;
        }
        if !self.questions.sending.contains(&id) {
            self.questions
                .views
                .get_mut(&id)
                .expect("visible question")
                .paste(text);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn session(id: &str, title: &str, status: SessionStatus, age: i64) -> Session {
        Session {
            id: SessionKey {
                provider: ProviderId::Claude,
                id: id.into(),
            },
            parent: None,
            cwd: "/tmp/project".into(),
            name: None,
            title: title.into(),
            prompt: title.into(),
            detail_sources: Default::default(),
            prompt_images: Vec::new(),
            queued: Vec::new(),
            prompt_at: None,
            turn_messages: 0,
            last_record_at: None,
            model_label: None,
            model: None,
            effort: None,
            git_branch: None,
            pr: None,
            status,
            process: None,
            updated_at: Utc::now() - chrono::Duration::seconds(age),
            started_at: None,
            last_message: None,
            last_message_at: None,
            activity: None,
            ask: None,
            pending_questions: Vec::new(),
            background_job: None,
            background: Vec::new(),
            tasks: Vec::new(),
            last_prompt: None,
            transcript: None,
        }
    }
    fn screen(view: &View, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        buf.content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fixture() -> View {
        let mut row = session("ask-flow", "Question task", SessionStatus::NeedsInput, 0);
        row.background_job = Some(crate::model::BackgroundJob {
            id: "probe".into(),
            needs: None,
        });
        row.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        row.transcript = Some("probe.jsonl".into());
        let record: serde_json::Value =
            include_str!("../../tests/fixtures/claude-attach/ask/transcript.jsonl")
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .find(|record| {
                    record["message"]["content"]
                        .as_array()
                        .is_some_and(|parts| {
                            parts
                                .iter()
                                .any(|part| part["input"]["questions"][0]["header"] == "Single")
                        })
                })
                .unwrap();
        let input = record["message"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|part| part["name"] == "AskUserQuestion")
            .unwrap();
        let question = &input["input"]["questions"][0];
        row.ask = Some(crate::ask::Ask {
            id: input["id"].as_str().unwrap().into(),
            session: row.id.clone(),
            since: row.updated_at,
            kind: crate::ask::AskKind::Questions {
                blocking: true,
                questions: vec![crate::ask::Question {
                    id: question["question"].as_str().unwrap().into(),
                    text: question["question"].as_str().unwrap().into(),
                    header: question["header"].as_str().unwrap().into(),
                    multi: false,
                    other: true,
                    secret: false,
                    options: serde_json::from_value(question["options"].clone()).unwrap(),
                }],
            },
        });
        let mut view = View::default();
        view.set_sessions(vec![row], false);
        view
    }

    #[test]
    fn ask_flow_replaces_prompt_and_routes_answer() {
        let mut view = fixture();
        let screen = screen(&view, 100, 40);
        assert!(screen.contains("Probe single choice?"), "{screen}");
        assert!(screen.contains("Red choice"), "{screen}");
        let action = view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        let Some(Action::AnswerQuestions { ask, answer }) = action else {
            panic!("answer action missing: {action:?}")
        };
        assert_eq!(answer.answers["Probe single choice?"].answers, ["Green"]);
        assert_eq!(ask.session.id, "ask-flow");
    }

    #[test]
    fn ask_flow_escape_stays_closed_after_refresh() {
        let mut view = fixture();
        assert!(
            view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none()
        );
        view.set_sessions(view.rows.clone(), false);
        assert!(!screen(&view, 100, 40).contains("Esc to close"));
    }

    #[test]
    fn ask_flow_failure_keeps_draft_and_shows_notice() {
        let mut view = fixture();
        let mut capture = vt100::Parser::new(50, 200, 0);
        capture.process(include_bytes!(
            "../../tests/fixtures/claude-attach/ask/unicode/refused.raw"
        ));
        assert!(capture.screen().contents().contains("Review your answers"));
        let reason =
            include_str!("../../tests/fixtures/claude-attach/ask/unicode/refused.reason.txt")
                .trim();
        let ask = view.selected_row().unwrap().ask.clone().unwrap();
        let input = crate::providers::SessionInput::Answer {
            ask: ask.clone(),
            answer: Default::default(),
            transcript: "probe.jsonl".into(),
        };
        view.input_sending(&ask.session, &input);
        assert!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
                .is_none()
        );
        view.input_complete(&ask.session, &input, Err(std::io::Error::other(reason)));
        view.expire_messages(Instant::now() + Duration::from_secs(2));
        assert!(screen(&view, 100, 40).contains("Claude attach timed out"));
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::AnswerQuestions { .. })
        ));
        view.input_sending(&ask.session, &input);
        assert!(!screen(&view, 100, 40).contains("Claude attach timed out"));
    }

    #[test]
    fn ask_flow_uses_question_footer_instead_of_open_hint() {
        let view = fixture();
        let text = view.center_footer_hints().to_string();
        assert!(!text.contains("open"), "{text}");
        assert!(text.contains("confirm"), "{text}");
    }
}
