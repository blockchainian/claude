use super::*;
use crate::providers::Provider;
use std::process::Command;

fn git(directory: &std::path::Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn fixture() -> (tempfile::TempDir, View) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("config/projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider =
        crate::providers::claude::ClaudeProvider::new(directory.path().join("config"));
    let mut sessions = provider.refresh(1).sessions;
    assert_eq!(sessions.len(), 1);
    sessions[0].cwd = directory.path().into();
    sessions[0].status = SessionStatus::Ready;
    let mut view = View::default();
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        provider.capabilities(),
    )]));
    view.set_input_choices(HashMap::from([(
        ProviderId::Claude,
        provider.input_choices(),
    )]));
    view.set_sessions(sessions, false);
    (directory, view)
}
fn setup(cwd: &std::path::Path) -> DraftSetup {
    DraftSetup {
        provider: ProviderId::Claude,
        cwd: cwd.into(),
        model: None,
        effort: None,
        branch: "main".into(),
        trunk: "main".into(),
    }
}
fn key(view: &mut View, code: KeyCode) -> Option<Action> {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn click(view: &mut View, x: u16, y: u16) -> Option<Action> {
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };
    assert!(view.handle_mouse(event).is_none());
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..event
    })
}
fn render(view: &View) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    buffer
}

#[test]
fn draft_shows_under_active_tab() {
    let (directory, mut view) = fixture();
    let id = view.add_draft(setup(directory.path()));
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    let buffer = render(&view);
    assert!(
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("New task")
    );
    let entries = view.center_rows(&view.selectable_indices(), Grouping::Project);
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, navigation::CenterRow::Draft(draft) if *draft == id))
    );
    key(&mut view, KeyCode::Esc);
    key(&mut view, KeyCode::Tab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    let entries = view.center_rows(&view.selectable_indices(), Grouping::Project);
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, navigation::CenterRow::Draft(_)))
    );
}

#[test]
fn acknowledged_job_refresh_drops_only_its_draft_and_selects_real_session() {
    let (directory, mut view) = fixture();
    let mut arrived = view.rows[0].clone();
    arrived.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
    });
    arrived.updated_at = Utc::now();
    let other = view.add_draft(setup(directory.path()));
    let selected = view.add_draft(setup(directory.path()));
    view.draft_created(selected, Ok("e772d00b".into()));
    let mut older = arrived.clone();
    older.id.id = "d6e5280c-6bcd-42c4-8bb9-6ff96d1b0744".into();
    older.background_job = None;
    older.updated_at -= chrono::Duration::days(1);
    let expected = arrived.id.clone();
    view.set_sessions(vec![arrived, older], false);
    assert_eq!(view.selected_id(), Some(&expected));
    assert_eq!(view.drafts.len(), 1);
    assert_eq!(view.drafts[0].id, other);
}

#[test]
fn linked_worktree_draft_stays_in_primary_project_group() {
    let (directory, mut view) = fixture();
    let repository = directory.path().join("repository");
    std::fs::create_dir_all(repository.join("src")).unwrap();
    git(&repository, &["init", "-b", "main"]);
    std::fs::write(
        repository.join("src/README.md"),
        include_bytes!("../../tests/fixtures/claude-peer-message.txt"),
    )
    .unwrap();
    git(&repository, &["add", "--", "src/README.md"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=UI probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "-m",
            "fixture",
        ],
    );
    let linked = directory.path().join("linked");
    git(
        &repository,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    view.rows[0].cwd = repository.join("src");
    view.set_sessions(view.rows.clone(), false);
    let id = view.add_draft(setup(&repository.join("src")));
    view.draft_worktree_complete(id, Ok((linked.join("src"), "feature".into())));
    let entries = view.center_rows(&view.selectable_indices(), Grouping::Project);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry, navigation::CenterRow::Group(_)))
            .count(),
        1
    );
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, navigation::CenterRow::DraftGroup(_)))
    );
    assert_eq!(
        view.selected_draft().unwrap().heading(),
        std::fs::canonicalize(repository).unwrap().join("src")
    );
}

#[test]
fn project_heading_defers_single_open_and_double_creates_in_actual_directory() {
    let (directory, mut view) = fixture();
    render(&view);
    let (x, y) = {
        let viewport = view.viewport.borrow();
        (viewport.list_links[0].0.x, viewport.list_links[0].0.y)
    };
    assert_eq!(click(&mut view, x, y), None);
    assert_eq!(
        view.flush_pending_click(Instant::now() + Duration::from_secs(1)),
        Some(Action::OpenPath(directory.path().into()))
    );
    assert_eq!(click(&mut view, x, y), None);
    assert_eq!(
        click(&mut view, x, y),
        Some(Action::NewDraft {
            cwd: directory.path().into()
        })
    );
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        view.grouping = grouping;
        assert_eq!(
            key(&mut view, KeyCode::Char('n')),
            Some(Action::NewDraft {
                cwd: directory.path().into()
            })
        );
    }
}

#[test]
fn draft_navigation_click_tab_and_pending_job_preserve_input_without_keys() {
    let (directory, mut view) = fixture();
    let session = view.rows[0].id.clone();
    let id = view.add_draft(setup(directory.path()));
    key(&mut view, KeyCode::Esc);
    render(&view);
    key(&mut view, KeyCode::Home);
    assert_eq!(view.selected_id(), Some(&session));
    key(&mut view, KeyCode::End);
    assert_eq!(view.selected_draft, Some(id));
    view.focus_input();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    view.handle_paste(text);
    key(&mut view, KeyCode::Esc);
    key(&mut view, KeyCode::Home);
    assert_eq!(view.selected_id(), Some(&session));
    let buffer = render(&view);
    let position = buffer
        .content
        .chunks(160)
        .enumerate()
        .find_map(|(y, row)| {
            let text = row.iter().map(|cell| cell.symbol()).collect::<String>();
            text.find("New task").map(|x| (x as u16, y as u16))
        })
        .unwrap();
    click(&mut view, position.0, position.1);
    assert_eq!(view.selected_draft, Some(id));
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_draft, None);
    view.status_filter = 0;
    view.select_draft(id);
    assert_eq!(view.input_box.editor.text(), text);
    view.draft_created(id, Ok("e772d00b".into()));
    assert!(view.input_disabled().is_some());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(view.drafts.len(), 1);
}

#[test]
fn prompt_model_label_does_not_become_a_metadata_picker_target() {
    let (_directory, mut view) = fixture();
    view.rows[0].prompt = format!(
        "Model: {}",
        include_str!("../../tests/fixtures/claude-peer-message.txt")
            .lines()
            .next()
            .unwrap()
    );
    render(&view);
    assert_eq!(
        view.viewport
            .borrow()
            .field_targets
            .iter()
            .filter(|(_, field)| *field == Field::Model)
            .count(),
        1
    );
}

#[test]
fn rendered_draft_model_double_click_opens_and_applies_provider_choice() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    key(&mut view, KeyCode::Esc);
    let area = Rect::new(0, 0, 180, 52);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let (x, y) = (0..area.height)
        .find_map(|y| {
            let text = (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            text.find("Model:")
                .map(|x| ((x + "Model:".len() + 1) as u16, y))
        })
        .unwrap();
    assert_eq!(click(&mut view, x, y), None);
    assert_eq!(click(&mut view, x, y), None);
    assert!(view.picker.is_some());
    view.render_at(area, &mut buffer, Utc::now());
    let index = view
        .picker
        .as_ref()
        .unwrap()
        .values
        .iter()
        .position(|value| value == "haiku")
        .unwrap();
    let (rect, _) = view
        .viewport
        .borrow()
        .picker_rows
        .iter()
        .find(|(_, row)| *row == index)
        .copied()
        .unwrap();
    assert_eq!(click(&mut view, rect.x + 2, rect.y), None);
    assert_eq!(
        view.selected_draft().unwrap().setup.model.as_deref(),
        Some("haiku")
    );
}

#[test]
fn log_border_keeps_model_and_effort_picker_controls_in_wide_and_narrow_layouts() {
    let (_directory, mut view) = fixture();
    view.rows[view.selected].background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
    });
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    view.rows[view.selected].effort = Some("high".into());
    view.rows[view.selected].model_label = Some("Opus 5.5".into());
    let model_name = view.rows[view.selected].model_label().to_owned();
    for width in [149, 150, 200] {
        let area = Rect::new(0, 0, width, 60);
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, Utc::now());
        let targets = view.viewport.borrow().field_targets.clone();
        let model = targets
            .iter()
            .find(|(_, field)| *field == Field::Model)
            .unwrap()
            .0;
        let effort = targets
            .iter()
            .find(|(_, field)| *field == Field::Effort)
            .unwrap()
            .0;
        let text = |x, y, width| {
            (x..x + width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let log = view.viewport.borrow().log;
        assert_eq!(text(model.x, model.y, model.width), model_name);
        assert_eq!(model.x, log.x + if width >= 150 { 1 } else { 2 });
        assert_eq!(
            model.y,
            if width >= 150 {
                log.bottom()
            } else {
                log.y - 1
            }
        );
        assert_eq!(effort.y, model.y);
        assert_eq!(effort.right(), log.right() - 2);
        assert!(model.right() < effort.x);
        assert!(
            buffer[(effort.x, model.y)]
                .modifier
                .contains(ratatui::style::Modifier::DIM)
        );
        assert_eq!(text(effort.x, effort.y, effort.width), "high");
        for (rect, field) in [(model, Field::Model), (effort, Field::Effort)] {
            assert_eq!(click(&mut view, rect.x, rect.y), None);
            assert_eq!(click(&mut view, rect.x, rect.y), None);
            assert_eq!(view.picker.as_ref().unwrap().field, field);
            key(&mut view, KeyCode::Esc);
        }
    }
}

#[test]
fn live_effort_picker_remains_available_when_transcript_omits_effort() {
    let (_directory, mut view) = fixture();
    let id = view.selected_id().unwrap().clone();
    let session = &mut view.rows[view.selected];
    session.status = SessionStatus::Ready;
    session.model = None;
    session.effort = None;
    session.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
    });
    session.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    render(&view);
    let rect = view
        .viewport
        .borrow()
        .field_targets
        .iter()
        .find(|(_, field)| *field == Field::Effort)
        .map(|(rect, _)| *rect)
        .expect("missing optional effort metadata must not hide the control");
    assert_eq!(click(&mut view, rect.x, rect.y), None);
    assert_eq!(click(&mut view, rect.x, rect.y), None);
    let index = view
        .picker
        .as_ref()
        .unwrap()
        .values
        .iter()
        .position(|value| value == "high")
        .unwrap();
    assert_eq!(
        view.apply_choice(index),
        Some(Action::SetEffort {
            id,
            effort: "high".into()
        })
    );
}

#[test]
fn log_model_targets_cover_only_visible_border_value_cells() {
    let (_directory, mut view) = fixture();
    let row = &mut view.rows[view.selected];
    row.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
    });
    row.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    row.model = Some("claude-opus-5-5".into());
    for effort in [Some("medium"), None] {
        view.rows[view.selected].effort = effort.map(str::to_owned);
        // Providers without effort choices show only the model.
        if effort.is_none() {
            view.input_choices
                .get_mut(&ProviderId::Claude)
                .unwrap()
                .efforts
                .clear();
        }
        for width in [20, 40, 60, 80, 120, 149, 150, 200] {
            let frame = Rect::new(0, 0, width, 60);
            let mut buffer = Buffer::empty(frame);
            view.render_at(frame, &mut buffer, Utc::now());
            let area = view.viewport.borrow().log;
            let rule_left = if width >= 150 { area.x - 2 } else { 0 };
            let rule = Rect::new(
                rule_left,
                if width >= 150 {
                    area.bottom()
                } else {
                    area.y - 1
                },
                width - rule_left,
                1,
            );
            let available = (rule.width / 2).saturating_sub(5);
            let show_effort = effort.is_some() && available > 0;
            let name_width = available.min("claude-opus-5-5".len() as u16);
            let start = rule.x + 3;
            let expected_name = if name_width == 0 {
                String::new()
            } else if name_width < "claude-opus-5-5".len() as u16 {
                format!("{}…", &"claude-opus-5-5"[..usize::from(name_width - 1)])
            } else {
                "claude-opus-5-5".into()
            };
            let drawn = (start..start + name_width)
                .map(|x| buffer[(x, rule.y)].symbol())
                .collect::<String>();
            assert_eq!(drawn, expected_name, "width={width}");
            let targets = view.viewport.borrow().field_targets.clone();
            let y = rule.y;
            let effort_width = available.min(6);
            for x in area.x..area.right() {
                let expected = if name_width > 0 && (start..start + name_width).contains(&x) {
                    Some(Field::Model)
                } else if show_effort
                    && (area.right() - 2 - effort_width..area.right() - 2).contains(&x)
                {
                    Some(Field::Effort)
                } else {
                    None
                };
                let actual = targets
                    .iter()
                    .find(|(rect, _)| rect.contains((x, y).into()));
                assert_eq!(
                    actual.map(|(_, field)| *field),
                    expected,
                    "width={width}, x={x}, effort={effort:?}"
                );
                if let Some(field) = expected {
                    let symbol = buffer[(x, y)].symbol();
                    assert!(!symbol.trim().is_empty());
                    assert!(
                        buffer[(x, y)]
                            .modifier
                            .contains(ratatui::style::Modifier::DIM)
                    );
                    assert_eq!(click(&mut view, x, y), None);
                    assert_eq!(click(&mut view, x, y), None);
                    assert_eq!(view.picker.as_ref().unwrap().field, field);
                    key(&mut view, KeyCode::Esc);
                }
            }
            assert!(targets.iter().all(|(rect, _)| !rect.is_empty()));
        }
    }
}
