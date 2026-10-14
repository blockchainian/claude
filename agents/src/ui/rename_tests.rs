use super::*;
use crate::providers::{Provider, codex::CodexProvider};

fn child_view(
    child_provider: ProviderId,
    parent_provider: ProviderId,
) -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut provider = CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let sessions = provider.refresh(2).sessions;
    let mut parent = sessions
        .iter()
        .find(|session| session.id.id == "thread-12")
        .unwrap()
        .clone();
    let mut child = sessions
        .iter()
        .find(|session| session.id.id == "thread-13")
        .unwrap()
        .clone();
    parent.id.provider = parent_provider;
    child.id.provider = child_provider;
    child.parent = Some(parent.id.clone());
    parent.status = SessionStatus::Ready;
    child.status = SessionStatus::Ready;
    let id = child.id.clone();
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(HashMap::from([(
        child_provider,
        Capabilities {
            resume: true,
            rename: true,
            fork: true,
            delete: true,
            ..Capabilities::default()
        },
    )]));
    view.set_sessions(vec![parent, child], false);
    view.selected = view
        .rows
        .iter()
        .position(|session| session.id == id)
        .unwrap();
    assert_eq!(view.selected_id(), Some(&id));
    (dir, view)
}

#[test]
fn manager_child_rename_opens_dialog_and_emits_action() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let id = view.selected_id().unwrap().clone();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.rename_target.as_ref(), Some(&id));
    view.rename.set_text("  Manager child renamed  ");
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::Rename {
            id,
            name: "Manager child renamed".into()
        })
    );
}

#[test]
fn codex_child_session_keys_stay_silent() {
    for (child, parent) in [
        (ProviderId::Codex, ProviderId::Claude),
        (ProviderId::Codex, ProviderId::Codex),
    ] {
        let (_dir, mut view) = child_view(child, parent);
        let mut codes = vec![
            KeyCode::Char('f'),
            KeyCode::Char('x'),
            KeyCode::Char('a'),
            KeyCode::Backspace,
            KeyCode::Char('h'),
            KeyCode::Char('p'),
        ];
        if parent != ProviderId::Claude || child != ProviderId::Codex {
            codes.push(KeyCode::Char('r'));
        }
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            None
        );
        for code in codes {
            assert_eq!(
                view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
                None
            );
            assert!(view.rename_target.is_none());
            assert!(view.confirm_dialog.is_none());
            assert!(view.pinned.is_empty());
            assert!(view.hidden.is_empty());
        }
    }
}
