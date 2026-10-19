//! Keeps pending question drafts with their session.

use super::*;
use question_view::{Outcome, QuestionView, Target};

#[derive(Default)]
pub(super) struct Questions {
    views: HashMap<SessionKey, QuestionView>,
    closed: HashMap<SessionKey, crate::ask::Ask>,
    pub(super) sending: HashSet<SessionKey>,
    /// The last activated position, for suppressing the second release of a double click.
    last_click: Option<(SessionKey, u16, u16, Instant)>,
}

impl Questions {
    fn sync(&mut self, rows: &[Session]) {
        let answerable = |ask: &crate::ask::Ask| {
            ask.session.provider != ProviderId::Claude
                || matches!(
                    ask.kind,
                    crate::ask::AskKind::Questions { blocking: true, .. }
                )
        };
        let background = |row: &Session| {
            row.is_live_background()
                || (row.id.provider == ProviderId::Claude && row.background_job.is_some())
        };
        let pending = |id: &SessionKey| {
            rows.iter()
                .find(|row| {
                    &row.id == id && background(row) && row.status == SessionStatus::NeedsInput
                })
                .and_then(|row| row.ask.as_ref())
                .filter(|ask| answerable(ask))
        };
        self.views
            .retain(|id, view| pending(id).is_some_and(|ask| ask == view.ask()));
        self.closed.retain(|id, closed| pending(id) == Some(closed));
        self.sending.retain(|id| pending(id).is_some());
        for row in rows {
            if row.status == SessionStatus::NeedsInput
                && background(row)
                && let Some(ask) = &row.ask
                && answerable(ask)
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
            || self.selector.is_some()
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
        let outcome = view.key(key);
        Some(self.question_outcome(id, outcome))
    }

    fn question_outcome(&mut self, id: SessionKey, outcome: Option<Outcome>) -> Option<Action> {
        let ask = self.questions.views[&id].ask().clone();
        match outcome? {
            Outcome::Dismiss => {
                self.questions.closed.insert(id, ask);
                None
            }
            Outcome::Submit(answer) => Some(Action::AnswerQuestions { ask, answer }),
        }
    }

    /// Wheel steps options or question tabs; a click chooses an option or shows a tab.
    pub(super) fn question_mouse(&mut self, mouse: MouseEvent) -> Option<Option<Action>> {
        let prompt = self.viewport.borrow().prompt;
        if !prompt.contains((mouse.column, mouse.row).into()) {
            return None;
        }
        let id = self.selected_id()?.clone();
        self.questions.visible(&id)?;
        if self.questions.sending.contains(&id) {
            return Some(None);
        }
        self.mouse_press = None;
        self.text_selection.borrow_mut().take();
        let area = prompt.inner(Margin::new(super::render::HORIZONTAL_PADDING, 0));
        let interval = self.double_click_interval;
        let view = self.questions.views.get_mut(&id).expect("visible question");
        let target = view.target(area, mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => view.scroll(
                mouse.kind == MouseEventKind::ScrollDown,
                matches!(target, Some(Target::Tabs(_))),
            ),
            MouseEventKind::Up(MouseButton::Left) => match target {
                Some(Target::Tabs(Some(page))) => view.show_page(page),
                Some(Target::Option(row)) => {
                    let now = Instant::now();
                    let double = self.questions.last_click.take().is_some_and(
                        |(session, column, row, at)| {
                            (session, column, row) == (id.clone(), mouse.column, mouse.row)
                                && now.duration_since(at) < interval
                        },
                    );
                    if !double {
                        self.questions.last_click =
                            Some((id.clone(), mouse.column, mouse.row, now));
                        let outcome = view.choose(row);
                        return Some(self.question_outcome(id, outcome));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Some(None)
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
            cache_expires_at: None,
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
            archived: false,
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
            awaits_prompt: false,
        });
        row.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        row.transcript = Some("probe.jsonl".into());
        let record: serde_json::Value =
            include_str!("../../tests/fixtures/claude-questions/transcript.jsonl")
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
    fn title_editor_cursor_takes_precedence_over_a_pending_question() {
        let mut view = fixture();
        assert!(view.active_question().is_some());
        view.rename_target = view.selected_id().cloned();
        view.rename.set_text("你好");
        view.input_box.focused = false;
        let area = Rect::new(0, 0, 160, 40);
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        let input = view.viewport.borrow().title_input;
        assert_eq!(view.cursor_pos(area), Some((input.x + 4, input.y)));
        assert!(
            view.center_footer_hints()
                .to_string()
                .contains("enter rename")
        );
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
    fn ask_user_question_is_answerable_before_process_metadata_arrives() {
        let mut view = fixture();
        let mut row = view.rows[0].clone();
        row.process = None;
        view.set_sessions(vec![row], false);
        assert!(view.active_question().is_some());
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::AnswerQuestions { .. })
        ));
    }

    #[test]
    fn transcript_questions_stay_read_only_after_refresh() {
        let mut view = fixture();
        assert!(view.active_question().is_some());
        let mut rows = view.rows.clone();
        let crate::ask::AskKind::Questions { blocking, .. } =
            &mut rows[0].ask.as_mut().unwrap().kind;
        *blocking = false;
        view.set_sessions(rows, false);

        assert!(view.active_question().is_none());
        assert!(!screen(&view, 100, 40).contains("Enter to confirm"));
        assert!(!matches!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::AnswerQuestions { .. })
        ));
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
        view.input_complete(
            &ask.session,
            &input,
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Claude AskUserQuestion disconnected",
            )),
        );
        view.expire_messages(Instant::now() + Duration::from_secs(2));
        assert!(screen(&view, 100, 40).contains("Claude AskUserQuestion disconnected"));
        assert!(matches!(
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
            Some(Action::AnswerQuestions { .. })
        ));
        view.input_sending(&ask.session, &input);
        assert!(!screen(&view, 100, 40).contains("Claude AskUserQuestion disconnected"));
    }

    #[test]
    fn answer_submission_keeps_screen_and_blocks_duplicates() {
        let mut view = fixture();
        let Some(Action::AnswerQuestions { ask, answer }) =
            view.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE))
        else {
            panic!("answer action missing");
        };
        let before = screen(&view, 100, 40);
        let input = crate::providers::SessionInput::Answer {
            ask: ask.clone(),
            answer,
            transcript: "probe.jsonl".into(),
        };
        view.input_sending(&ask.session, &input);
        assert_eq!(screen(&view, 100, 40), before);
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        view.handle_paste("duplicate answer");
        assert_eq!(screen(&view, 100, 40), before);
        view.input_complete(&ask.session, &input, Ok(()));
        assert!(view.active_question().is_none());
    }

    fn click(view: &mut View, column: u16, row: u16) -> Option<Action> {
        let mouse = |kind| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left)));
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left)))
    }

    fn row_of(screen: &str, text: &str) -> u16 {
        screen
            .lines()
            .position(|line| line.contains(text))
            .unwrap_or_else(|| panic!("{text} missing: {screen}")) as u16
    }

    #[test]
    fn ask_flow_single_click_answers() {
        let mut view = fixture();
        let before = screen(&view, 100, 40);
        let green = row_of(&before, "2. Green");
        let Some(Action::AnswerQuestions { answer, .. }) = click(&mut view, 10, green) else {
            panic!("single click did not answer")
        };
        assert_eq!(answer.answers["Probe single choice?"].answers, ["Green"]);

        let mut view = fixture();
        let other = row_of(&screen(&view, 100, 40), "Type something.");
        assert_eq!(click(&mut view, 10, other), None);
        view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let Some(Action::AnswerQuestions { answer, .. }) =
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("typed answer missing")
        };
        assert_eq!(
            answer.answers["Probe single choice?"].other.as_deref(),
            Some("x")
        );
    }

    fn multi_fixture() -> View {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        let crate::ask::AskKind::Questions { questions, .. } =
            &mut rows[0].ask.as_mut().unwrap().kind;
        questions[0].multi = true;
        view.set_sessions(rows, false);
        view
    }

    fn selected_answers(view: &mut View) -> Vec<String> {
        view.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        let Some(Action::AnswerQuestions { answer, .. }) =
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("review did not submit")
        };
        answer.answers["Probe single choice?"].answers.clone()
    }

    #[test]
    fn ask_flow_multi_single_click_toggles() {
        let mut view = multi_fixture();
        let green = row_of(&screen(&view, 100, 40), "2. [ ] Green");
        assert_eq!(click(&mut view, 10, green), None);
        assert_eq!(selected_answers(&mut view), ["Green"]);
        view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        view.questions.last_click = None;
        screen(&view, 100, 40);
        assert_eq!(click(&mut view, 10, green), None);
        assert!(selected_answers(&mut view).is_empty());
    }

    #[test]
    fn ask_flow_multi_double_click_toggles_only_once() {
        let mut view = multi_fixture();
        let green = row_of(&screen(&view, 100, 40), "2. [ ] Green");
        assert_eq!(click(&mut view, 10, green), None);
        assert!(screen(&view, 100, 40).contains("2. [x] Green"));
        assert_eq!(click(&mut view, 10, green), None);
        assert_eq!(selected_answers(&mut view), ["Green"]);
    }

    #[test]
    fn ask_flow_wheel_steps_options_without_wrapping() {
        let mut view = fixture();
        let row = row_of(&screen(&view, 100, 40), "1. Red");
        let wheel = |view: &mut View, kind| {
            view.handle_mouse(MouseEvent {
                kind,
                column: 10,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        wheel(&mut view, MouseEventKind::ScrollUp);
        assert!(screen(&view, 100, 40).contains("› 1. Red"));
        for _ in 0..6 {
            wheel(&mut view, MouseEventKind::ScrollDown);
        }
        assert!(screen(&view, 100, 40).contains("› 4. Type something."));
    }

    #[test]
    fn ask_flow_prompt_keeps_its_rows_across_tabs() {
        let mut view = fixture();
        let row = view.rows[0].clone();
        let mut ask = row.ask.clone().unwrap();
        let crate::ask::AskKind::Questions { questions, .. } = &mut ask.kind;
        let mut second = questions[0].clone();
        second.id = "Second?".into();
        second.header = "Second".into();
        second.multi = true;
        second.options.push(crate::ask::QuestionOption {
            label: "Extra".into(),
            description: "Extra choice".into(),
            preview: None,
        });
        let mut third = questions[0].clone();
        third.id = "Third?".into();
        third.header = "Third".into();
        third.options.truncate(2);
        questions.extend([second, third]);
        let mut row = row;
        row.ask = Some(ask);
        view.set_sessions(vec![row], false);
        let mut prompts = Vec::new();
        for expected in ["Probe single choice?", "Extra", "Third", "2. Cancel"] {
            let text = screen(&view, 160, 50);
            assert!(text.contains(expected), "{expected}: {text}");
            prompts.push(view.viewport.borrow().prompt);
            view.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        assert!(
            prompts.windows(2).all(|pair| pair[0] == pair[1]),
            "{prompts:?}"
        );
    }

    #[test]
    fn ask_flow_uses_question_footer_instead_of_open_hint() {
        let view = fixture();
        let text = view.center_footer_hints().to_string();
        assert!(!text.contains("open"), "{text}");
        assert!(text.contains("confirm"), "{text}");
    }
}
