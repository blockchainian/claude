use super::drafts::Step;
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
    crate::providers::claude::write_fixture_cache(&directory.path().join("config"));
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
        branch: crate::repository::repository_identity(cwd)
            .map(|_| "main".to_owned())
            .unwrap_or_default(),
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
/// Starts after repository selection, names the task and focuses its prompt.
fn named_draft(view: &mut View, cwd: &std::path::Path, name: &str) -> crate::model::DraftId {
    let id = view.add_draft(setup(cwd));
    view.open_name(id);
    view.handle_paste(name);
    assert_eq!(key(view, KeyCode::Enter), None);
    assert_eq!(view.selected_draft().unwrap().step, Step::Prompt);
    assert_eq!(view.input_label(), name);
    id
}
/// Submits the first prompt; outside a repository the branch step is skipped.
fn create(view: &mut View, name: &str) -> Option<Action> {
    assert_eq!(view.input_label(), name);
    key(view, KeyCode::Enter)
}
fn render(view: &View) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
    view.render_at(buffer.area, &mut buffer, Utc::now());
    buffer
}

#[test]
fn acknowledged_job_refresh_drops_its_draft_and_selects_real_session() {
    let (directory, mut view) = fixture();
    let mut arrived = view.rows[0].clone();
    arrived.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    arrived.updated_at = Utc::now();
    view.add_draft(setup(directory.path()));
    let selected = view.add_draft(setup(directory.path()));
    assert_eq!(view.drafts.len(), 1);
    view.draft_created(selected, Ok("e772d00b".into()));
    let mut older = arrived.clone();
    older.id.id = "d6e5280c-6bcd-42c4-8bb9-6ff96d1b0744".into();
    older.background_job = None;
    older.updated_at -= chrono::Duration::days(1);
    let expected = arrived.id.clone();
    view.set_sessions(vec![arrived, older], false);
    assert_eq!(view.selected_id(), Some(&expected));
    assert!(view.drafts.is_empty());
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
fn pending_job_survives_leaving_until_its_session_arrives() {
    let (directory, mut view) = fixture();
    let id = named_draft(&mut view, directory.path(), "probe");
    view.handle_paste("typed prompt");
    create(&mut view, "probe");
    view.draft_created(id, Ok("e772d00b".into()));
    assert_eq!(view.selected_draft, None);
    key(&mut view, KeyCode::Esc);
    view.input_box.focused = false;
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_draft, None);
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
fn rendered_draft_model_click_opens_and_double_click_applies_provider_choice() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    let area = Rect::new(0, 0, 180, 52);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let model = view
        .viewport
        .borrow()
        .field_targets
        .iter()
        .find(|(_, field)| *field == Field::Model)
        .unwrap()
        .0;
    assert_eq!(buffer[(model.x, model.y)].symbol(), "U");
    assert_eq!(click(&mut view, model.x, model.y), None);
    assert_eq!(view.selector.as_ref().unwrap().field(), Field::Model);
    view.render_at(area, &mut buffer, Utc::now());
    let (rect, _) = view
        .viewport
        .borrow()
        .picker_rows
        .iter()
        .find(|(rect, _)| {
            (rect.x..rect.right())
                .map(|x| buffer[(x, rect.y)].symbol())
                .collect::<String>()
                .trim_end()
                == "Haiku 5.5"
        })
        .copied()
        .unwrap();
    assert_eq!(click(&mut view, rect.x + 2, rect.y), None);
    assert_eq!(click(&mut view, rect.x + 2, rect.y), None);
    assert!(view.selector.is_none());
    assert_eq!(
        view.selected_draft().unwrap().setup.model.as_deref(),
        Some("claude-haiku-5-5")
    );
}

#[test]
fn log_border_keeps_model_and_effort_picker_controls_in_wide_and_narrow_layouts() {
    let (_directory, mut view) = fixture();
    view.rows[view.selected].background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    view.rows[view.selected].effort = Some("high".into());
    let model_id = view.rows[view.selected].model.clone().unwrap();
    view.input_choices
        .entry(ProviderId::Claude)
        .or_default()
        .display_names
        .insert(model_id, "Opus 5.5".into());
    let model_name = "Opus 5.5";
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
        assert_eq!(model.x, log.x + u16::from(width < 150));
        assert_eq!(
            model.y,
            if width >= 150 {
                log.bottom()
            } else {
                log.y - 1
            }
        );
        assert_eq!(effort.y, model.y);
        assert_eq!(effort.x, model.right() + 3);
        assert!(model.right() < effort.x);
        assert!(
            buffer[(effort.x, model.y)]
                .modifier
                .contains(ratatui::style::Modifier::DIM)
        );
        assert_eq!(text(effort.x, effort.y, effort.width), "high");
        for (rect, field) in [(model, Field::Model), (effort, Field::Effort)] {
            assert_eq!(click(&mut view, rect.x, rect.y), None);
            assert_eq!(view.selector.as_ref().unwrap().field(), field);
            key(&mut view, KeyCode::Esc);
        }
    }
}

#[test]
fn model_group_heading_and_log_border_show_the_catalog_name() {
    let (_directory, mut view) = fixture();
    view.rows[view.selected].model = Some("claude-opus-5-5".into());
    view.grouping = crate::model::Grouping::Model;
    let buffer = render(&view);
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.lines()
            .any(|line| line.split('│').next().unwrap().trim() == "Opus 5.5"),
        "{text}"
    );
    assert!(text.contains("╴Opus 5.5 "), "{text}");
    assert!(!text.contains("claude-opus-5-5"), "{text}");
}
#[test]
fn log_model_targets_cover_only_visible_border_value_cells() {
    let (_directory, mut view) = fixture();
    let row = &mut view.rows[view.selected];
    row.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    row.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    row.model = Some("claude-opus-5-5".into());
    for effort in [Some("medium"), None] {
        view.rows[view.selected].effort = effort.map(str::to_owned);
        // Missing effort remains visible even when the picker has no choices.
        if effort.is_none() {
            for model in &mut view
                .input_choices
                .get_mut(&ProviderId::Claude)
                .unwrap()
                .models
            {
                model.efforts.clear();
            }
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
            let total = rule.width.saturating_sub(4);
            let effort_width = effort.unwrap_or("Default").len() as u16;
            let available = total.saturating_sub(effort_width.min(total / 2) + 3);
            let show_effort = available > 0;
            // The catalog's name for `claude-opus-5-5`.
            let name = "Opus 5.5";
            let name_width = available.min(name.len() as u16);
            let start = rule.x + 2;
            let expected_name = if name_width == 0 {
                String::new()
            } else if name_width < name.len() as u16 {
                format!("{}…", &name[..usize::from(name_width - 1)])
            } else {
                name.into()
            };
            let drawn = (start..start + name_width)
                .map(|x| buffer[(x, rule.y)].symbol())
                .collect::<String>();
            assert_eq!(drawn, expected_name, "width={width}");
            let targets = view.viewport.borrow().field_targets.clone();
            let y = rule.y;
            let effort_start = start + name_width + 3;
            let effort_width = rule
                .right()
                .saturating_sub(2 + effort_start)
                .min(effort_width);
            for x in area.x..area.right() {
                let expected = if name_width > 0 && (start..start + name_width).contains(&x) {
                    Some(Field::Model)
                } else if show_effort && (effort_start..effort_start + effort_width).contains(&x) {
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
                    if field == Field::Model {
                        // A model name's own spaces are part of the value.
                        let expected = expected_name.chars().nth(usize::from(x - start));
                        assert_eq!(symbol.chars().next(), expected);
                    } else {
                        assert!(!symbol.trim().is_empty());
                    }
                    assert!(
                        buffer[(x, y)]
                            .modifier
                            .contains(ratatui::style::Modifier::DIM)
                    );
                    assert_eq!(click(&mut view, x, y), None);
                    if field == Field::Effort && effort.is_none() {
                        assert!(view.selector.is_none());
                    } else {
                        assert_eq!(view.selector.as_ref().unwrap().field(), field);
                    }
                    key(&mut view, KeyCode::Esc);
                }
            }
            assert!(targets.iter().all(|(rect, _)| !rect.is_empty()));
        }
    }
}

#[test]
fn draft_first_message_accepts_images_and_reports_background_failure() {
    for text in ["describe this", ""] {
        let (directory, mut view) = fixture();
        let id = named_draft(&mut view, directory.path(), "probe");
        view.handle_paste(text);
        view.attach_image(
            crate::images::ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        let paths = view.input_box.editor.image_paths();
        let action = create(&mut view, "probe");
        assert!(
            matches!(action, Some(Action::CreateTask { draft_id, .. }) if draft_id == id),
            "{action:?}"
        );
        let Some(Action::CreateTask { request, .. }) = action else {
            unreachable!()
        };
        assert_eq!(request.prompt, text);
        assert_eq!(request.images, paths);
        assert!(view.drafts[0].working);
        view.draft_created(id, Err(std::io::Error::other("delivery failed")));
        assert_eq!(view.selected_draft, None);
        assert!(view.drafts.is_empty());
        assert_eq!(view.notice.as_ref().unwrap().0, "delivery failed");
    }
}

#[test]
fn draft_is_not_a_list_row_and_escape_or_moving_away_discards_it() {
    let (directory, mut view) = fixture();
    let session = view.rows[0].id.clone();
    named_draft(&mut view, directory.path(), "probe");
    view.handle_paste("typed prompt");
    let buffer = render(&view);
    let list = view.center_layout(buffer.area).list;
    let list = (list.top()..list.bottom())
        .map(|y| {
            (list.left()..list.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!list.contains("New task"), "{list}");
    key(&mut view, KeyCode::Esc);
    assert!(view.drafts.is_empty());
    assert_eq!(view.selected_draft, None);
    assert_eq!(view.selected_id(), Some(&session));
    named_draft(&mut view, directory.path(), "probe");
    view.handle_paste("typed prompt");
    view.input_box.focused = false;
    key(&mut view, KeyCode::Down);
    assert!(view.drafts.is_empty());
    assert_eq!(view.selected_id(), Some(&session));
    assert!(!view.input_box.editor.text().contains("typed prompt"));
}

#[test]
fn chosen_branch_rides_with_the_first_prompt() {
    let (directory, mut view) = fixture();
    let id = named_draft(&mut view, directory.path(), "probe");
    view.drafts[0].branch_choice = Some(crate::tasks::BranchChoice::New("feature/probe".into()));
    view.handle_paste("start the work");
    let action = create(&mut view, "probe");
    let Some(Action::CreateTask {
        draft_id, branch, ..
    }) = action
    else {
        panic!("{action:?}");
    };
    assert_eq!(draft_id, id);
    assert_eq!(
        branch,
        Some(crate::tasks::BranchChoice::New("feature/probe".into()))
    );
}

#[test]
fn details_browser_opens_folders_and_applies_repositories_without_a_click_delay() {
    let (directory, mut view) = fixture();
    let parent = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("code");
    for (name, branch) in [("a", "main"), ("b", "work")] {
        let repository = parent.join(name);
        std::fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-b", branch]);
    }
    let nested = parent.join("z/nested");
    std::fs::create_dir_all(&nested).unwrap();
    git(&nested, &["init", "-b", "main"]);
    view.startup_cwd = parent.join("a");
    let id = view.add_draft(setup(&parent.join("a")));
    assert!(!view.input_box.focused);
    view.open_field(Field::Directory);
    let area = Rect::new(0, 0, 160, 40);
    let row_with = |view: &mut View, text: &str| {
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, Utc::now());
        let details = view.center_layout(area).details;
        (details.top()..details.bottom())
            .find(|y| {
                (details.left()..details.right())
                    .map(|x| buffer[(x, *y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    == text
            })
            .map(|y| (details.x, y))
            .unwrap_or_else(|| panic!("missing {text:?}"))
    };
    let (x, y) = row_with(&mut view, "z");
    assert_eq!(click(&mut view, x, y), None);
    assert!(view.pending_click_in(Instant::now()).is_none());
    row_with(&mut view, "nested");
    let (x, y) = row_with(&mut view, "..");
    assert_eq!(click(&mut view, x, y), None);
    assert!(view.pending_click_in(Instant::now()).is_none());
    let (x, y) = row_with(&mut view, "b");
    let action = click(&mut view, x, y);
    assert!(view.pending_click_in(Instant::now()).is_none());
    let Some(Action::ChangeDraftDirectory { draft_id, cwd }) = action else {
        panic!("{action:?}");
    };
    assert_eq!((draft_id, &cwd), (id, &parent.join("b")));
    view.draft_directory_changed(id, cwd, "work".into());
    assert_eq!(view.selected_draft().unwrap().step, Step::Name);
    assert!(!view.input_box.focused);
    assert!(view.input_box.editor.is_empty());
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let folder = view
        .viewport
        .borrow()
        .field_targets
        .iter()
        .find(|(_, field)| *field == Field::Directory)
        .unwrap()
        .0;
    let directory = (folder.x..folder.right())
        .map(|x| buffer[(x, folder.y)].symbol())
        .collect::<String>();
    assert_eq!(directory, "b");
    assert!(view.viewport.borrow().details_links.is_empty());
    assert_eq!(click(&mut view, folder.x, folder.y), None);
    let selector = view.selector.as_ref().unwrap();
    assert_eq!(selector.field(), Field::Directory);
    assert_eq!(selector.selected, 2);
}

#[test]
fn new_task_takes_its_name_on_the_rule_then_its_prompt() {
    let (directory, mut view) = fixture();
    let id = view.add_draft(setup(directory.path()));
    view.open_name(id);
    assert_eq!(view.selected_draft().unwrap().step, Step::Name);
    assert!(!view.input_box.focused);
    let buffer = render(&view);
    let input = view.viewport.borrow().title_input;
    let details = view.viewport.borrow().details;
    assert_eq!(input.y, details.bottom());
    // Wide characters own the cell after them.
    let row = |buffer: &Buffer, rect: Rect| {
        (rect.x..rect.right())
            .filter(|&x| x == rect.x || buffer[(x - 1, rect.y)].symbol().width() < 2)
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    };
    assert_eq!(row(&buffer, input), "New task");
    assert!(
        buffer[(input.x, input.y)]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    assert_eq!(view.cursor_pos(buffer.area), Some((input.x, input.y)));
    // An empty name is a no-op; typing replaces the placeholder.
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.selected_draft().unwrap().step, Step::Name);
    view.handle_paste("改 Claude Code 的颜色");
    let buffer = render(&view);
    let input = view.viewport.borrow().title_input;
    let (visible, caret) = view.rename.window(usize::from(input.width));
    assert_eq!(row(&buffer, input), visible.trim_end());
    assert_eq!(
        view.cursor_pos(buffer.area),
        Some((input.x + caret, input.y))
    );
    assert!(
        !buffer[(input.x, input.y)]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.selected_draft().unwrap().step, Step::Prompt);
    assert!(view.selector.is_none());
    assert!(view.input_box.focused);
    assert_eq!(view.input_label(), "改 Claude Code 的颜色");
    assert!(view.input_box.editor.is_empty());
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.selected_draft().unwrap().step, Step::Prompt);
    view.handle_paste("make it blue");
    let action = key(&mut view, KeyCode::Enter);
    let Some(Action::CreateTask {
        draft_id, request, ..
    }) = action
    else {
        panic!("{action:?}");
    };
    assert_eq!(draft_id, id);
    assert_eq!(request.name.as_deref(), Some("改 Claude Code 的颜色"));
    assert_eq!(request.prompt, "make it blue");
    assert_eq!(view.input_box.state, prompt_box::SendState::Ready);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    let screen = render(&view)
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(!screen.contains("Creating"), "{screen}");
    assert!(!screen.contains("waiting for session"), "{screen}");
    assert!(!screen.contains("New prompt ❯"), "{screen}");
    view.input_box.history.borrow().iter().for_each(|entry| {
        assert_ne!(entry.text(), "改 Claude Code 的颜色");
    });
}

#[test]
fn name_step_escape_cancels_the_entire_draft() {
    let (directory, mut view) = fixture();
    let id = view.add_draft(setup(directory.path()));
    view.open_name(id);
    view.handle_paste("half a name");
    key(&mut view, KeyCode::Esc);
    assert!(view.selected_draft().is_none());
    assert!(view.selector.is_none());
    assert!(view.input_box.editor.is_empty());
    assert!(view.drafts.is_empty());
}

#[test]
fn prompt_submission_clears_input_keeps_task_label_and_branch_escape_cancels() {
    let (directory, mut view) = fixture();
    git(directory.path(), &["init", "-b", "main"]);
    git(
        directory.path(),
        &[
            "-c",
            "user.name=Probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "probe",
        ],
    );
    let id = named_draft(&mut view, directory.path(), "named task");
    view.handle_paste("make it blue");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.selected_draft().unwrap().step, Step::Branch);
    assert!(view.input_box.editor.is_empty());
    assert_eq!(view.input_label(), "named task");
    assert!(!view.selector.as_ref().unwrap().input_active());
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.selector.as_ref().unwrap().input_active());
    view.handle_paste("discarded branch");
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected_draft().unwrap().id, id);
    assert!(!view.selector.as_ref().unwrap().input_active());
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(
        view.selector
            .as_mut()
            .unwrap()
            .active_editor()
            .unwrap()
            .is_empty()
    );
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert!(view.drafts.is_empty());
    assert!(view.selected_draft.is_none());
    assert!(view.input_box.editor.is_empty());
}

#[test]
fn submitted_draft_returns_to_prior_task_and_selects_arriving_job_after_refresh() {
    let (directory, mut view) = fixture();
    let original = view.selected_id().unwrap().clone();
    view.focus_input();
    view.handle_paste("prior input");
    let id = named_draft(&mut view, directory.path(), "background task");
    view.drafts[0].branch_choice =
        Some(crate::tasks::BranchChoice::New("feature/background".into()));
    view.handle_paste("background prompt");
    assert!(matches!(
        create(&mut view, "background task"),
        Some(Action::CreateTask { .. })
    ));
    assert_eq!(view.selected_draft, None);
    assert_eq!(view.selected_id(), Some(&original));
    assert!(view.selector.is_none());
    assert_eq!(view.input_box.editor.text(), "prior input");
    view.handle_paste(" continued");
    assert_eq!(view.input_box.editor.text(), "prior input continued");
    view.set_sessions(view.rows.clone(), false);
    assert_eq!(view.drafts.len(), 1);
    view.draft_worktree_created(
        id,
        Ok((
            directory.path().join("worktree"),
            "feature/background".into(),
        )),
    );
    view.draft_created(id, Ok("e772d00b".into()));
    let mut arrived = view.rows[0].clone();
    arrived.id.id = "d6e5280c-6bcd-42c4-8bb9-6ff96d1b0744".into();
    arrived.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    let expected = arrived.id.clone();
    let mut sessions = view.rows.clone();
    sessions.push(arrived);
    view.set_sessions(sessions, false);
    assert_eq!(view.selected_id(), Some(&expected));
    assert!(view.drafts.is_empty());
}

#[test]
fn submitted_drafts_without_a_prior_selection_stay_hidden_and_fail_with_notice() {
    for worktree in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut view = View::default();
        let id = named_draft(&mut view, directory.path(), "background task");
        view.handle_paste("background prompt");
        assert!(matches!(
            create(&mut view, "background task"),
            Some(Action::CreateTask { .. })
        ));
        assert_eq!(view.selected_draft, None);
        assert_eq!(view.selected_id(), None);
        assert_eq!(view.input_label(), "No task");
        assert!(view.selector.is_none());
        let next = named_draft(&mut view, directory.path(), "next task name");
        view.handle_paste("next task");
        assert_eq!(view.drafts.len(), 2);
        let error = std::io::Error::other("background creation failed");
        if worktree {
            view.draft_worktree_created(id, Err(error));
        } else {
            view.draft_created(id, Err(error));
        }
        assert_eq!(view.selected_draft, Some(next));
        assert_eq!(view.input_box.editor.text(), "next task");
        assert_eq!(view.drafts.len(), 1);
        let text = render(&view)
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("background creation failed"), "{text}");
    }
}
