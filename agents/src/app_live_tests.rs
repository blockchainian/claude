//! Live-provider drivers enter through the same action and completion paths as the dashboard.
use super::*;

pub(crate) fn app(provider: Box<dyn crate::providers::Provider>, preferences: PathBuf) -> App {
    let mut app = App::new(Hub::new(vec![provider]), preferences);
    app.refresh();
    app
}

pub(crate) fn refresh(app: &mut App) {
    app.refresh();
}

pub(crate) fn session(app: &App, id: &str) -> Session {
    app.sessions
        .iter()
        .find(|session| session.id.id == id)
        .expect("real provider snapshot must contain the created session")
        .clone()
}

pub(crate) fn action(app: &mut App, action: Action) {
    assert!(!app.handle_action(Some(action)));
}

/// Deletes the session and waits for the worker to finish.
pub(crate) fn delete(app: &mut App, id: SessionKey) {
    action(app, Action::Delete(id.clone()));
    let event = app
        .receiver
        .recv_timeout(Duration::from_secs(120))
        .expect("provider delete deadline");
    assert!(
        matches!(&event, Input::Deleted { id: deleted, .. } if deleted == &id),
        "expected real provider deletion"
    );
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
}

/// Selects the session with the keyboard, as a user does.
pub(crate) fn select(app: &mut App, id: &SessionKey) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    if app.view.selected_id() == Some(id) {
        return;
    }
    for _ in 0..5 {
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        for _ in 0..app.sessions.len() + 4 {
            if app.view.selected_id() == Some(&id) {
                break;
            }
            app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        }
        if app.view.selected_id() == Some(&id) {
            break;
        }
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    }
    assert_eq!(
        app.view.selected_id(),
        Some(id),
        "created session must be keyboard selectable"
    );
}

/// Opens the selected session's terminal in the log detail panel and waits until its screen
/// satisfies `ready`.
pub(crate) fn open_terminal(app: &mut App, id: SessionKey, ready: impl Fn(&str) -> bool) {
    select(app, &id);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
    app.draw(&mut terminal).unwrap();
    app.open_terminal(id);
    wait_terminal(app, ready);
}

/// Pastes `text` into the open terminal as the user's paste does.
pub(crate) fn paste_terminal(app: &mut App, text: &str) {
    app.paste_terminal(text);
}

pub(crate) fn close_terminal(app: &mut App) {
    app.close_terminal();
}

/// Waits until the open terminal's screen satisfies `ready`.
pub(crate) fn wait_terminal(app: &mut App, ready: impl Fn(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let screen = app
            .terminal
            .as_ref()
            .expect("terminal must be open")
            .screen
            .lock()
            .unwrap()
            .screen()
            .contents();
        if ready(&screen) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal never got ready: {screen}"
        );
        let _ = app.receiver.recv_timeout(Duration::from_millis(100));
    }
}

pub(crate) fn send(app: &mut App, id: SessionKey, input: SessionInput) {
    select(app, &id);
    let action = match input.clone() {
        SessionInput::PromptWithImages { .. } | SessionInput::Answer { .. } => {
            app.send_input(id.clone(), input.clone());
            None
        }
        SessionInput::Prompt(text) => Some(Action::SendPrompt {
            id: id.clone(),
            text,
        }),
        SessionInput::Interrupt => Some(Action::Stop(id.clone())),
        SessionInput::SetModel(model) => Some(Action::SetModel {
            id: id.clone(),
            model,
        }),
        SessionInput::SetEffort(effort) => Some(Action::SetEffort {
            id: id.clone(),
            effort,
        }),
    };
    if let Some(action) = action {
        self::action(app, action);
    }
    assert!(
        app.pending_inputs.contains(&id),
        "App did not launch provider input"
    );
    // An open terminal reports its output while the input runs.
    let event = loop {
        let event = app
            .receiver
            .recv_timeout(Duration::from_secs(120))
            .expect("provider input deadline");
        if !matches!(event, Input::TerminalOutput) {
            break event;
        }
    };
    let Input::Sent { result, .. } = &event else {
        panic!("expected real provider completion")
    };
    let error = result.as_ref().err().map(ToString::to_string);
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
    assert!(error.is_none(), "provider input failed: {error:?}");
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    if matches!(input, SessionInput::Interrupt) {
        assert!(!screen.contains("Failed to stop"), "{screen}");
    } else {
        assert!(
            !screen.contains(" · sent"),
            "completed provider send must keep the regular prompt: {screen}"
        );
    }
}

pub(crate) fn create(app: &mut App, request: crate::tasks::NewTask) -> String {
    action(
        app,
        Action::NewDraft {
            cwd: request.cwd.clone(),
        },
    );
    action(
        app,
        Action::CreateTask {
            draft_id: 1,
            provider: ProviderId::Claude,
            request,
            branch: None,
        },
    );
    let event = app
        .receiver
        .recv_timeout(Duration::from_secs(60))
        .expect("task creation deadline");
    let Input::Created {
        provider, result, ..
    } = &event
    else {
        panic!("expected real create completion")
    };
    let job = app
        .hub
        .provider(*provider)
        .created_job(result.as_ref().expect("real Claude creation"))
        .expect("Claude job id");
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    job
}

pub(crate) fn answer_questions(app: &mut App, session: Session, values: Vec<Vec<String>>) {
    answer(app, session, values);
}

fn answer(app: &mut App, session: Session, values: Vec<Vec<String>>) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    // Wide characters draw with a blank continuation cell, so compare text without spaces.
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    let ask = session
        .ask
        .clone()
        .expect("live provider detected question");
    let crate::ask::AskKind::Questions { questions, .. } = &ask.kind;
    let id = session.id.clone();
    app.sessions = vec![session.clone()];
    app.view.set_sessions(vec![session], false);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
    app.draw(&mut terminal).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        compact(&screen).contains(&compact(&questions[0].text)),
        "question view absent: {screen}"
    );
    let key = |app: &mut App, code| {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    };
    for (question, answers) in questions.iter().zip(values) {
        // Single-select choices move on by themselves; other pages move on to the next tab.
        let moves_on = !question.multi && !answers.is_empty();
        for answer in answers {
            if let Some(note) = answer.strip_prefix("user_note: ") {
                app.handle_event(Event::Key(KeyEvent::new(
                    KeyCode::Char(char::from_digit(question.options.len() as u32 + 1, 10).unwrap()),
                    KeyModifiers::NONE,
                )));
                app.handle_event(Event::Paste(note.to_owned()));
                app.handle_event(Event::Key(KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                )));
            } else {
                let n = question
                    .options
                    .iter()
                    .position(|o| o.label == answer)
                    .unwrap()
                    + 1;
                app.handle_event(Event::Key(KeyEvent::new(
                    KeyCode::Char(char::from_digit(n as u32, 10).unwrap()),
                    KeyModifiers::NONE,
                )));
            }
        }
        if !moves_on {
            key(app, KeyCode::Right);
        }
    }
    if questions.len() > 1 || questions.iter().any(|question| question.multi) {
        key(app, KeyCode::Char('1'));
    }
    assert!(
        app.pending_inputs.contains(&id),
        "dashboard did not submit answer"
    );
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    // Terminal output and its close can arrive before the answer completes.
    let event = loop {
        let event = app
            .receiver
            .recv_timeout(Duration::from_secs(120))
            .expect("live answer deadline");
        if matches!(event, Input::Sent { .. }) {
            break event;
        }
        app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    };
    let Input::Sent { result, .. } = &event else {
        unreachable!()
    };
    assert!(result.is_ok(), "live dashboard answer failed: {result:?}");
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
}
