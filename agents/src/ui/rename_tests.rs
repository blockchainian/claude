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
            archive: true,
            stop: true,
            resume: true,
            rename: true,
            fork: true,
            delete: true,
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
fn manager_child_session_keys_operate() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let id = view.selected_id().unwrap().clone();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        Some(Action::OpenTerminal(id.clone()))
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
        Some(Action::Fork(id.clone()))
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        Some(Action::Stop(id))
    );
}

#[test]
fn codex_spawn_child_session_keys_emit_unsupported() {
    for (code, modifiers, label) in [
        (KeyCode::Char('l'), KeyModifiers::CONTROL, "Terminal"),
        (KeyCode::Char('f'), KeyModifiers::NONE, "Fork"),
        (KeyCode::Char('r'), KeyModifiers::NONE, "Rename"),
        (KeyCode::Char('x'), KeyModifiers::NONE, "Stop"),
        (KeyCode::Char('a'), KeyModifiers::NONE, "Archive"),
        (KeyCode::Backspace, KeyModifiers::NONE, "Delete"),
        (KeyCode::Char('h'), KeyModifiers::NONE, "Hide"),
        (KeyCode::Char('p'), KeyModifiers::NONE, "Pin"),
    ] {
        for notice in [None, Some("Keep this notice".to_owned())] {
            let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Codex);
            let id = view.selected_id().unwrap().clone();
            assert!(!view.selected_row().unwrap().can_resume());
            view.set_notice(notice.clone());
            assert_eq!(
                view.handle_key(KeyEvent::new(code, modifiers)),
                Some(Action::Unsupported(label)),
                "{code:?}"
            );
            assert_eq!(view.selected_id(), Some(&id));
            assert_eq!(view.notice.clone().map(|(text, _)| text), notice);
            assert!(view.rename_target.is_none());
            assert!(view.confirm_dialog.is_none());
            assert!(view.pinned.is_empty());
            assert!(view.hidden.is_empty());
        }
    }
}

#[test]
fn manager_thread_keys_match_ordinary_codex_threads() {
    for (code, modifiers) in [
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Right, KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::CONTROL),
        (KeyCode::Char('o'), KeyModifiers::NONE),
        (KeyCode::Char('f'), KeyModifiers::NONE),
        (KeyCode::Char('n'), KeyModifiers::NONE),
        (KeyCode::Char('x'), KeyModifiers::NONE),
    ] {
        let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
        let session = view.selected_row().unwrap().clone();
        let expected = match code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                Action::OpenTerminal(session.id)
            }
            KeyCode::Char('o') => Action::OpenTerminal(session.id),
            KeyCode::Char('f') => Action::Fork(session.id),
            KeyCode::Char('n') => Action::NewDraft { cwd: session.cwd },
            KeyCode::Char('x') => Action::Stop(session.id),
            _ => unreachable!(),
        };
        assert_eq!(
            view.handle_key(KeyEvent::new(code, modifiers)),
            Some(expected)
        );
    }
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let id = view.selected_id().unwrap().clone();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        Some(Action::Archive(id.clone()))
    );
    assert!(view.confirm_dialog.is_none());
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        None
    );
    assert!(view.confirm_dialog.is_some());
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::Delete(id))
    );
}

#[test]
fn manager_thread_hides_itself_and_pins_its_parent_group() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let child = view.selected_row().unwrap().clone();
    let parent = child.parent.clone().unwrap();
    for pinned in [true, false] {
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(Action::TogglePin {
                id: parent.clone(),
                pinned
            })
        );
        view.set_pinned(if pinned {
            std::slice::from_ref(&parent)
        } else {
            &[]
        });
        assert_eq!(view.is_pinned(view.selected), pinned);
    }
    assert!(
        view.center_footer_hints()
            .to_string()
            .contains("enter Terminal")
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
        Some(Action::Hide(child.id.clone()))
    );
    assert!(view.hidden.contains(&child.id));
    assert!(!view.hidden.contains(&parent));
}
