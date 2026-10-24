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
    let mut provider = CodexProvider::new(dir.path().into());
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
    assert_eq!(view.active_panel, Panel::TaskDetails);
    view.set_active_panel(Panel::TaskDetails);
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
        None
    );
    assert_eq!(view.active_panel, Panel::TaskList);
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
        (KeyCode::Char('l'), KeyModifiers::CONTROL, None),
        (KeyCode::Char('f'), KeyModifiers::NONE, Some("Fork")),
        (KeyCode::Char('r'), KeyModifiers::NONE, Some("Rename")),
        (KeyCode::Char('x'), KeyModifiers::NONE, Some("Stop")),
        (KeyCode::Char('a'), KeyModifiers::NONE, Some("Archive")),
        (KeyCode::Backspace, KeyModifiers::NONE, Some("Delete")),
    ] {
        for notice in [None, Some("Keep this notice".to_owned())] {
            let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Codex);
            let id = view.selected_id().unwrap().clone();
            assert!(!view.selected_row().unwrap().can_resume());
            if let Some(notice) = &notice {
                view.show_error(notice.clone());
            }
            assert_eq!(
                view.handle_key(KeyEvent::new(code, modifiers)),
                label.map(Action::Unsupported),
                "{code:?}"
            );
            assert_eq!(view.selected_id(), Some(&id));
            assert_eq!(
                view.feedback.clone().map(|(feedback, _)| feedback),
                notice.map(footer::Feedback::Error)
            );
            assert!(view.rename_target.is_none());
            assert!(view.confirm_dialog.is_none());
            assert!(view.pinned.active.is_empty());
        }
    }
}

#[test]
fn manager_thread_keys_match_ordinary_codex_threads() {
    for (code, modifiers) in [
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::CONTROL),
        (KeyCode::Char('o'), KeyModifiers::NONE),
        (KeyCode::Char('f'), KeyModifiers::NONE),
        (KeyCode::Char('n'), KeyModifiers::NONE),
        (KeyCode::Char('x'), KeyModifiers::NONE),
    ] {
        let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
        let session = view.selected_row().unwrap().clone();
        let expected = match code {
            KeyCode::Enter | KeyCode::Char('l' | 'o') => None,
            KeyCode::Char('f') => Some(Action::Fork(session.id)),
            KeyCode::Char('n') => Some(Action::NewDraft { cwd: session.cwd }),
            KeyCode::Char('x') => Some(Action::Stop(session.id)),
            _ => unreachable!(),
        };
        assert_eq!(view.handle_key(KeyEvent::new(code, modifiers)), expected);
        if matches!(code, KeyCode::Enter | KeyCode::Char('o')) {
            assert_eq!(view.active_panel, Panel::Prompt);
        } else if code == KeyCode::Char('l') {
            assert_eq!(view.active_panel, Panel::TaskList);
        }
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
fn manager_thread_pins_itself() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let child = view.selected_row().unwrap().clone();
    for pinned in [true, false] {
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(Action::TogglePin {
                tab: 0,
                id: child.id.clone(),
                pinned
            })
        );
        view.set_pinned(&crate::state::TaskPins {
            active: (if pinned {
                std::slice::from_ref(&child.id)
            } else {
                &[]
            })
            .to_vec(),
            inactive: vec![],
        });
        assert_eq!(view.is_pinned(view.selected), pinned);
    }
    assert_eq!(view.center_footer_hints().to_string(), "? help");
}

#[test]
fn title_requires_double_click_to_rename() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    let area = Rect::new(0, 0, 160, 40);
    view.render(area, &mut Buffer::empty(area));
    let title = view.viewport.borrow().task_details_title;
    assert!(title.width > 0);
    let click = |view: &mut View| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            view.handle_mouse(MouseEvent {
                kind,
                column: title.x,
                row: title.y,
                modifiers: KeyModifiers::NONE,
            });
        }
    };
    click(&mut view);
    assert!(view.rename_target.is_none());
    click(&mut view);
    assert_eq!(view.rename_target.as_ref(), view.selected_id());
    let original = view.selected_row().unwrap().display_title().to_owned();
    assert_eq!(view.rename.text(), original);
    view.handle_paste(" changed");
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(view.rename_target.is_none());
    assert_eq!(view.selected_row().unwrap().display_title(), original);
}

#[test]
fn draft_title_edit_supports_line_keys_repository_return_and_cancel() {
    let directory = tempfile::tempdir().unwrap();
    let mut view = View::default();
    let id = view.add_draft(DraftSetup {
        provider: ProviderId::Claude,
        cwd: directory.path().into(),
        model: None,
        effort: None,
        branch: String::new(),
    });
    view.set_active_panel(Panel::TaskDetails);
    assert_eq!(view.name_target, Some(id));
    assert!(view.selector.is_none());
    assert!(view.rename.is_empty());
    view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(view.name_target, Some(id));
    view.handle_paste("甲乙");
    for code in [
        KeyCode::Home,
        KeyCode::Right,
        KeyCode::Delete,
        KeyCode::Char('丙'),
        KeyCode::End,
    ] {
        view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert_eq!(view.rename.text(), "甲丙");
    view.set_active_panel(Panel::TaskDetails);
    view.open_field(draft_controls::Field::Directory);
    assert!(view.title_editor().is_none());
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(view.title_editor().unwrap().text(), "甲丙");
    view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(view.drafts[0].name.as_deref(), Some("甲丙"));
    assert!(view.name_target.is_none());
    assert!(view.selector.is_some());
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!view.prompt_focused());
    view.set_active_panel(Panel::Prompt);
    view.handle_paste("prompt body");
    assert_eq!(view.input_box.editor.text(), "prompt body");
    view.add_draft(DraftSetup {
        provider: ProviderId::Claude,
        cwd: directory.path().into(),
        model: None,
        effort: None,
        branch: String::new(),
    });
    view.set_active_panel(Panel::TaskDetails);
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(view.name_target.is_none());
    assert_eq!(view.drafts.len(), 1);
    assert_eq!(view.drafts[0].input.editor.text(), "prompt body");
}

#[test]
fn failed_draft_keeps_its_title_editor() {
    let directory = tempfile::tempdir().unwrap();
    let mut view = View::default();
    let id = view.add_draft(DraftSetup {
        provider: ProviderId::Claude,
        cwd: directory.path().into(),
        model: None,
        effort: None,
        branch: String::new(),
    });
    view.draft_created(id, Err(std::io::Error::other("failed")));
    assert!(view.title_editor().is_some());
    assert_eq!(view.drafts[0].error.as_deref(), Some("failed"));
}

#[test]
fn keyboard_title_rename_keeps_task_list_shortcuts_after_cancel() {
    let (_directory, mut view) = child_view(ProviderId::Codex, ProviderId::Claude);
    assert!(view.active_panel == Panel::TaskList);
    view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    view.set_active_panel(Panel::TaskDetails);
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    view.set_active_panel(Panel::TaskList);
    view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(view.search.focused());
}

#[path = "title_save_tests.rs"]
mod title_save_tests;

#[test]
fn codex_spawn_child_pins_itself() {
    let (_dir, mut view) = child_view(ProviderId::Codex, ProviderId::Codex);
    let id = view.selected_id().unwrap().clone();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        Some(Action::TogglePin {
            tab: 0,
            id,
            pinned: true
        })
    );
}
