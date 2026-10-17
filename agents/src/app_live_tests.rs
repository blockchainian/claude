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

pub(crate) fn send(app: &mut App, id: SessionKey, input: SessionInput) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        Some(&id),
        "created session must be keyboard selectable"
    );
    let action = match input.clone() {
        SessionInput::PromptWithImages { .. } => {
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
    let event = app
        .receiver
        .recv_timeout(Duration::from_secs(120))
        .expect("provider input deadline");
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
