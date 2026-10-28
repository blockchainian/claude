use super::*;

/// Selects the session with the keyboard, as a user does.
pub(crate) fn select(app: &mut App, id: &SessionKey) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    if app.view.selected_id() == Some(id) {
        return;
    }
    for _ in 0..5 {
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        for _ in 0..app.sessions.len() + 4 {
            if app.view.selected_id() == Some(id) {
                break;
            }
            app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        }
        if app.view.selected_id() == Some(id) {
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
