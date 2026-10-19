use super::draft_controls::{Origin, SelectorKind};
use super::*;
use crate::providers::Provider;
use crate::state::DraftBranch;
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

struct FixtureDirectory {
    directory: tempfile::TempDir,
    _parent: tempfile::TempDir,
}

impl FixtureDirectory {
    fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
}

fn fixture() -> (FixtureDirectory, View) {
    // Repository selectors browse the cwd's parent; keep that scan inside this fixture.
    let parent = tempfile::tempdir().unwrap();
    let directory = FixtureDirectory {
        directory: tempfile::tempdir_in(parent.path()).unwrap(),
        _parent: parent,
    };
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
    view.set_active_panel(Panel::Details);
    view.open_name(id);
    view.set_active_panel(Panel::Details);
    view.handle_paste(name);
    assert_eq!(key(view, KeyCode::Enter), None);
    assert!(view.selected_draft().unwrap().name.is_some());
    assert_eq!(view.input_label(), name);
    let draft = view.drafts.iter_mut().find(|draft| draft.id == id).unwrap();
    draft.repository_confirmed = true;
    view.close_selector();
    view.set_active_panel(Panel::Prompt);
    id
}
/// Submits the current prompt through the field rule.
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
    view.set_active_panel(Panel::Details);
    let selected = view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
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
    assert_eq!(view.selected_draft, Some(id));
    assert_eq!(
        view.selected_draft().unwrap().job.as_deref(),
        Some("e772d00b")
    );
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.selected_draft, Some(id));
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
fn rendered_draft_model_click_opens_and_single_click_applies_provider_choice() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
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
    assert_eq!(buffer[(model.x, model.y)].symbol(), "O");
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
        // The narrow layout starts them at the midline, under the task's folder.
        let gap = view.center_layout(area).gap;
        assert_eq!(model.x, if width >= 150 { log.x } else { gap.x + 3 });
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
        for width in [20, 40, 60, 80, 120, 149, 150, 200] {
            let frame = Rect::new(0, 0, width, 60);
            let mut buffer = Buffer::empty(frame);
            view.render_at(frame, &mut buffer, Utc::now());
            let area = view.viewport.borrow().log;
            let gap = view.center_layout(frame).gap;
            let rule_left = if width >= 150 {
                area.x - 2
            } else if gap.is_empty() {
                0
            } else {
                gap.x + 1
            };
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
            // A missing effort is not drawn, so the model gets the whole rule.
            let effort_width = effort.map_or(0, str::len) as u16;
            let available = if effort.is_some() {
                total.saturating_sub(effort_width.min(total / 2) + 3)
            } else {
                total
            };
            let show_effort = effort.is_some() && available > 0;
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
fn wizard_name_opens_repository_before_prompt() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste("wizard task");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Repository { .. })
    ));
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Wizard);
    assert!(!view.prompt_focused());
}

fn git_draft() -> (FixtureDirectory, View) {
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
    git(directory.path(), &["branch", "feature/x"]);
    named_draft(&mut view, directory.path(), "probe");
    (directory, view)
}

fn click_field(view: &mut View, field: Field) {
    let rect = view
        .viewport
        .borrow()
        .field_targets
        .iter()
        .find(|(_, target)| *target == field)
        .unwrap()
        .0;
    assert!(
        view.viewport
            .borrow()
            .overlays
            .iter()
            .all(|overlay| !overlay.area.intersects(rect))
    );
    assert_eq!(click(view, rect.x, rect.y), None);
}

fn assert_field_click_cancels(field: Field) {
    for width in [120, 200] {
        for named in [false, true] {
            let (_directory, mut view) = git_draft();
            if named {
                view.handle_paste("half prompt");
            } else {
                let id = view.selected_draft.unwrap();
                view.drafts
                    .iter_mut()
                    .find(|draft| draft.id == id)
                    .unwrap()
                    .name = None;
                view.open_name(id);
                view.set_active_panel(Panel::Details);
            }
            let draft = view.selected_draft().unwrap();
            let choices = (
                draft.setup.cwd.clone(),
                draft.setup.model.clone(),
                draft.setup.effort.clone(),
                draft.branch.clone(),
            );
            let prompt = view.input_box.editor.text().to_owned();
            let name_target = view.name_target;
            let name = view.rename.text().to_owned();
            view.set_active_panel(Panel::Details);
            let now = Utc::now();
            let area = Rect::new(0, 0, width, 50);
            let mut before = Buffer::empty(area);
            view.render_at(area, &mut before, now);
            click_field(&mut view, field);
            assert_eq!(view.selector.as_ref().unwrap().field(), field);
            let mut after = Buffer::empty(area);
            view.render_at(area, &mut after, now);
            click_field(&mut view, field);
            assert!(
                view.selector.is_none(),
                "{field:?}, width={width}, named={named}"
            );
            assert_eq!(view.name_target, name_target);
            assert_eq!(view.rename.text(), name);
            assert_eq!(view.input_box.editor.text(), prompt);
            assert!(!view.prompt_focused());
            let draft = view.selected_draft().unwrap();
            assert_eq!(
                (
                    draft.setup.cwd.clone(),
                    draft.setup.model.clone(),
                    draft.setup.effort.clone(),
                    draft.branch.clone()
                ),
                choices
            );
            after.reset();
            view.render_at(area, &mut after, now);
            assert_eq!(after, before);
        }
    }
}

#[test]
fn model_field_click_cancels_selector() {
    assert_field_click_cancels(Field::Model);
}

#[test]
fn effort_field_click_cancels_selector() {
    assert_field_click_cancels(Field::Effort);
}

#[test]
fn repository_field_click_cancels_selector() {
    assert_field_click_cancels(Field::Directory);
}

#[test]
fn branch_field_click_cancels_selector() {
    assert_field_click_cancels(Field::Branch);
}

#[test]
fn clicking_another_field_switches_selector() {
    let (_directory, mut view) = git_draft();
    let fields = [Field::Model, Field::Effort, Field::Directory, Field::Branch];
    for from in fields {
        for to in fields {
            if from == to {
                continue;
            }
            view.close_selector();
            render(&view);
            click_field(&mut view, from);
            render(&view);
            click_field(&mut view, to);
            assert_eq!(view.selector.as_ref().unwrap().field(), to);
        }
    }
}

#[test]
fn branch_and_repository_selection_orders_create_in_selected_repository() {
    for order in ["branch", "repository", "wizard"] {
        for branch in ["feature/x", "main", "feature/new"] {
            let (directory, mut view) = git_draft();
            view.discard_selected_draft();
            view.add_draft(setup(directory.path()));
            view.set_active_panel(Panel::Details);
            view.handle_paste("branch task");
            assert_eq!(key(&mut view, KeyCode::Enter), None);
            assert!(!view.selected_draft().unwrap().repository_confirmed);
            if order != "branch" {
                if order == "repository" {
                    view.set_active_panel(Panel::Details);
                    key(&mut view, KeyCode::Esc);
                    assert!(view.selector.is_none());
                    assert!(view.confirm_dialog.is_none());
                    render(&view);
                    click_field(&mut view, Field::Directory);
                }
                let selected = view.selector.as_ref().unwrap().selected;
                let Some(Action::ChangeDraftDirectory { draft_id, cwd }) =
                    view.apply_choice(selected)
                else {
                    panic!("repository choice");
                };
                view.draft_directory_changed(draft_id, cwd, "main".into());
            }
            if order != "wizard" {
                render(&view);
                click_field(&mut view, Field::Branch);
            }
            let index = match &view.selector.as_ref().unwrap().kind {
                SelectorKind::Branch { branches, .. } => {
                    assert_eq!(branches, &["feature/x", "main"]);
                    branches
                        .iter()
                        .position(|name| name == branch)
                        .unwrap_or(branches.len())
                }
                _ => panic!("branch selector"),
            };
            render(&view);
            let row = view
                .viewport
                .borrow()
                .picker_rows
                .iter()
                .find(|(_, choice)| *choice == index)
                .unwrap()
                .0;
            assert_eq!(click(&mut view, row.x, row.y), None);
            if branch == "feature/new" {
                view.set_active_panel(Panel::Details);
                view.handle_paste(branch);
                assert_eq!(key(&mut view, KeyCode::Enter), None);
            }
            assert!(view.selector.is_none(), "order={order}, branch={branch}");
            view.set_active_panel(Panel::Prompt);
            assert!(view.prompt_focused());
            assert!(!view.selected_draft().unwrap().working);
            view.handle_paste("use the chosen branch");
            let action = key(&mut view, KeyCode::Enter);
            assert!(
                view.selector.is_none(),
                "{order}: repository reopened for {branch}"
            );
            let Some(Action::CreateTask {
                request,
                branch: choice,
                ..
            }) = action
            else {
                panic!("{order}: task not created for {branch}");
            };
            assert_eq!(
                std::fs::canonicalize(request.cwd).unwrap(),
                std::fs::canonicalize(directory.path()).unwrap()
            );
            assert_eq!(request.prompt, "use the chosen branch");
            let expected = match branch {
                "main" => None,
                "feature/new" => Some(crate::tasks::BranchChoice::New(branch.into())),
                _ => Some(crate::tasks::BranchChoice::Existing(branch.into())),
            };
            assert_eq!(choice, expected);
            assert!(view.selected_draft().unwrap().repository_confirmed);
        }
    }
}

#[test]
fn prompt_enter_opens_missing_branch_and_wizard_pick_keeps_prompt() {
    let (_directory, mut view) = git_draft();
    view.handle_paste("half prompt");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Wizard);
    view.set_active_panel(Panel::Details);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(
        view.selected_draft().unwrap().branch,
        DraftBranch::Existing("main".into())
    );
    assert!(view.selector.is_none());
    view.set_active_panel(Panel::Prompt);
    assert!(view.prompt_focused());
    assert_eq!(view.input_box.editor.text(), "half prompt");
    assert!(matches!(
        key(&mut view, KeyCode::Enter),
        Some(Action::CreateTask { branch: None, .. })
    ));
}

#[test]
fn border_branch_and_model_picks_never_create_with_everything_filled() {
    let (_directory, mut view) = git_draft();
    view.handle_paste("half prompt");
    view.open_field(Field::Branch);
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Border);
    assert_eq!(view.apply_choice(0), None);
    assert_eq!(
        view.selected_draft().unwrap().branch,
        DraftBranch::Existing("feature/x".into())
    );
    view.open_field(Field::Model);
    assert_eq!(view.apply_choice(0), None);
    assert_eq!(view.active_panel, Panel::Details);
    assert_eq!(view.input_box.editor.text(), "half prompt");
    assert!(!view.selected_draft().unwrap().working);
}

#[test]
fn border_repository_keeps_prompt_and_same_repository_keeps_branch() {
    let (directory, mut view) = git_draft();
    view.open_field(Field::Branch);
    assert_eq!(view.apply_choice(0), None);
    view.set_active_panel(Panel::Prompt);
    view.handle_paste("half prompt");
    view.open_field(Field::Directory);
    let selected = view.selector.as_ref().unwrap().selected;
    let Some(Action::ChangeDraftDirectory { draft_id, cwd }) = view.apply_choice(selected) else {
        panic!("repository pick");
    };
    assert!(view.name_target.is_none());
    assert_eq!(view.active_panel, Panel::Details);
    assert_eq!(view.input_box.editor.text(), "half prompt");
    let subdirectory = cwd.join("subdir");
    std::fs::create_dir(&subdirectory).unwrap();
    view.draft_directory_changed(draft_id, subdirectory, "main".into());
    assert_eq!(
        view.selected_draft().unwrap().branch,
        DraftBranch::Existing("feature/x".into())
    );
    view.set_active_panel(Panel::Prompt);
    let action = key(&mut view, KeyCode::Enter);
    assert!(
        matches!(action, Some(Action::CreateTask { branch: Some(crate::tasks::BranchChoice::Existing(ref name)), .. }) if name == "feature/x")
    );
    drop(directory);
}

#[test]
fn changing_repository_resets_branch_and_next_prompt_enter_opens_branch_list() {
    let (directory, mut view) = git_draft();
    view.open_field(Field::Branch);
    view.apply_choice(0);
    view.set_active_panel(Panel::Prompt);
    view.handle_paste("half prompt");
    let other = directory.path().join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-b", "other-main"]);
    let id = view.selected_draft.unwrap();
    view.draft_directory_changed(id, other, "other-main".into());
    assert_eq!(view.selected_draft().unwrap().branch, DraftBranch::Unchosen);
    assert_eq!(view.selected_draft().unwrap().setup.branch, "other-main");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Branch { .. })
    ));
    assert_eq!(view.input_box.editor.text(), "half prompt");
}

#[test]
fn wizard_selector_escape_only_closes_one_layer_and_then_confirms_delete() {
    let (_directory, mut view) = git_draft();
    view.handle_paste("half prompt");
    for field in [Field::Directory, Field::Branch] {
        view.open_field(field);
        view.selector.as_mut().unwrap().origin = Origin::Wizard;
        view.set_active_panel(Panel::Details);
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert!(view.selector.is_none());
        view.set_active_panel(Panel::Prompt);
        assert!(view.prompt_focused());
        assert_eq!(view.input_box.editor.text(), "half prompt");
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert!(view.confirm_dialog.is_some());
        key(&mut view, KeyCode::Esc);
    }
}

#[test]
fn title_edit_prefills_and_escape_or_right_click_inside_keeps_original_name() {
    let (directory, mut view) = fixture();
    let id = named_draft(&mut view, directory.path(), "original");
    for right_click in [false, true] {
        view.open_name(id);
        view.set_active_panel(Panel::Details);
        assert_eq!(view.rename.text(), "original");
        view.handle_paste(" changed");
        if right_click {
            render(&view);
            let input = view.viewport.borrow().title_input;
            for kind in [
                MouseEventKind::Down(MouseButton::Right),
                MouseEventKind::Up(MouseButton::Right),
            ] {
                view.handle_mouse(MouseEvent {
                    kind,
                    column: input.x,
                    row: input.y,
                    modifiers: KeyModifiers::NONE,
                });
            }
        } else {
            key(&mut view, KeyCode::Esc);
        }
        assert_eq!(
            view.selected_draft().unwrap().name.as_deref(),
            Some("original")
        );
        assert!(view.name_target.is_none());
        assert_eq!(view.active_panel, Panel::Prompt);
    }
}

#[test]
fn saving_existing_name_is_a_side_edit_and_does_not_create() {
    let (_directory, mut view) = git_draft();
    view.handle_paste("half prompt");
    let id = view.selected_draft.unwrap();
    view.open_name(id);
    view.set_active_panel(Panel::Details);
    view.rename.set_text("renamed");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(
        view.selected_draft().unwrap().name.as_deref(),
        Some("renamed")
    );
    assert!(view.selector.is_none());
    view.set_active_panel(Panel::Prompt);
    assert!(view.prompt_focused());
    assert_eq!(view.input_box.editor.text(), "half prompt");
}

#[test]
fn non_git_prompt_enter_requires_repository_even_if_marked_confirmed() {
    let (directory, mut view) = fixture();
    named_draft(&mut view, directory.path(), "choose a repository");
    view.handle_paste("kept prompt");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Repository { .. })
    ));
    assert!(!view.drafts[0].repository_confirmed);
    assert!(!view.drafts[0].working);
    assert_eq!(view.input_box.editor.text(), "kept prompt");
}

#[test]
fn frozen_draft_ignores_second_submit_and_controls() {
    let (directory, mut view) = git_draft();
    view.discard_selected_draft();
    named_draft(&mut view, directory.path(), "plain");
    view.drafts[0].branch = DraftBranch::Existing("main".into());
    view.handle_paste("plain prompt");
    assert!(matches!(
        key(&mut view, KeyCode::Enter),
        Some(Action::CreateTask { .. })
    ));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    view.open_field(Field::Model);
    assert!(view.selector.is_none());
    assert!(view.selected_draft().unwrap().working);
}

#[test]
fn unnamed_repository_side_edit_keeps_typed_name_and_non_git_branch_is_noop() {
    let (directory, mut view) = fixture();
    let id = view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste("half name");
    view.open_field(Field::Branch);
    assert!(view.selector.is_none());
    view.open_field(Field::Directory);
    view.cancel_selector();
    assert_eq!(view.name_target, Some(id));
    assert_eq!(view.rename.text(), "half name");
}

#[test]
fn draft_first_message_accepts_images_and_keeps_input_after_background_failure() {
    for text in ["describe this", ""] {
        let (directory, mut view) = git_draft();
        view.discard_selected_draft();
        let id = named_draft(&mut view, directory.path(), "probe");
        view.drafts[0].branch = DraftBranch::Existing("main".into());
        view.handle_paste(text);
        view.attach_image(
            crate::images::ImageAttachment::from_image(
                image::load_from_memory(&crate::images::fixture_png()).unwrap(),
            )
            .unwrap(),
        );
        let paths = view.input_box.editor.image_paths();
        let action = create(&mut view, "probe");
        if text.is_empty() {
            assert_eq!(action, None, "images alone do not submit a draft");
            continue;
        }
        let Some(Action::CreateTask { request, .. }) = action else {
            panic!("{action:?}");
        };
        assert_eq!(request.prompt, text);
        assert_eq!(request.images, paths);
        view.draft_created(id, Err(std::io::Error::other("delivery failed")));
        assert_eq!(view.selected_draft, Some(id));
        assert!(!view.selected_draft().unwrap().working);
        assert_eq!(
            view.selected_draft().unwrap().error.as_deref(),
            Some("delivery failed")
        );
        assert_eq!(view.input_box.editor.prompt_text(), text);
        assert_eq!(view.input_box.editor.image_paths(), paths);
    }
}

#[test]
fn wizard_repository_pick_confirms_then_result_opens_branch_without_creating() {
    let (directory, mut view) = git_draft();
    let id = view.selected_draft.unwrap();
    view.drafts
        .iter_mut()
        .find(|draft| draft.id == id)
        .unwrap()
        .repository_confirmed = false;
    assert_eq!(view.advance_draft(false), None);
    let selector = view.selector.as_ref().unwrap();
    assert_eq!(selector.origin, Origin::Wizard);
    let selected = selector.selected;
    let Some(Action::ChangeDraftDirectory { draft_id, cwd }) = view.apply_choice(selected) else {
        panic!("repository not preselected");
    };
    assert_eq!(
        std::fs::canonicalize(cwd).unwrap(),
        std::fs::canonicalize(directory.path()).unwrap()
    );
    assert!(view.selected_draft().unwrap().repository_confirmed);
    assert!(view.selected_draft().unwrap().continue_after_directory);
    view.draft_directory_changed(draft_id, directory.path().into(), "main".into());
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Wizard);
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Branch { .. })
    ));
    assert!(!view.selected_draft().unwrap().working);
}

#[test]
fn checked_out_branch_is_an_explicit_choice_without_a_worktree_request() {
    let (_directory, mut view) = git_draft();
    view.open_field(Field::Branch);
    let index = match &view.selector.as_ref().unwrap().kind {
        SelectorKind::Branch { branches, .. } => {
            branches.iter().position(|name| name == "main").unwrap()
        }
        _ => unreachable!(),
    };
    assert_eq!(view.apply_choice(index), None);
    assert_eq!(
        view.selected_draft().unwrap().branch,
        DraftBranch::Existing("main".into())
    );
    view.set_active_panel(Panel::Prompt);
    view.handle_paste("run here");
    assert!(matches!(
        key(&mut view, KeyCode::Enter),
        Some(Action::CreateTask { branch: None, .. })
    ));
}

#[test]
fn wizard_same_root_repository_pick_keeps_side_branch_from_a_subdirectory() {
    let (directory, mut view) = git_draft();
    view.open_field(Field::Branch);
    view.apply_choice(0);
    let subdirectory = directory.path().join("subdirectory");
    std::fs::create_dir(&subdirectory).unwrap();
    let draft = &mut view.drafts[0];
    draft.setup.cwd = subdirectory;
    draft.repository_confirmed = false;
    view.set_active_panel(Panel::Prompt);
    view.handle_paste("kept prompt");
    key(&mut view, KeyCode::Enter);
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Wizard);
    let selected = view.selector.as_ref().unwrap().selected;
    let Some(Action::ChangeDraftDirectory { draft_id, cwd }) = view.apply_choice(selected) else {
        panic!("repository choice");
    };
    view.draft_directory_changed(draft_id, cwd, "main".into());
    assert!(view.selector.is_none());
    assert_eq!(
        view.drafts[0].branch,
        DraftBranch::Existing("feature/x".into())
    );
    assert!(!view.drafts[0].working);
    view.set_active_panel(Panel::Prompt);
    assert!(matches!(
        key(&mut view, KeyCode::Enter),
        Some(Action::CreateTask { .. })
    ));
}

#[test]
fn frozen_repository_results_and_double_click_title_use_draft_identity() {
    let (directory, mut view) = git_draft();
    view.discard_selected_draft();
    let id = named_draft(&mut view, directory.path(), "original");
    render(&view);
    let title = view.viewport.borrow().details_title;
    click(&mut view, title.x, title.y);
    click(&mut view, title.x, title.y);
    assert_eq!(view.name_target, Some(id));
    assert_eq!(view.rename.text(), "original");
    key(&mut view, KeyCode::Esc);
    view.set_active_panel(Panel::Prompt);
    view.drafts[0].branch = DraftBranch::Existing("main".into());
    view.handle_paste("prompt");
    assert!(matches!(
        key(&mut view, KeyCode::Enter),
        Some(Action::CreateTask { .. })
    ));
    let cwd = view.drafts[0].setup.cwd.clone();
    view.draft_directory_changed(id, cwd.join("ignored"), "ignored".into());
    assert_eq!(view.drafts[0].setup.cwd, cwd);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
}

#[test]
fn right_click_outside_cancels_delete_confirmation_and_preserves_panel() {
    let (directory, mut view) = fixture();
    named_draft(&mut view, directory.path(), "keep me");
    view.handle_paste("keep prompt");
    key(&mut view, KeyCode::Esc);
    assert!(view.confirm_dialog.is_some());
    render(&view);
    let panel = view.active_panel;
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right),
    ] {
        view.handle_mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
    }
    assert!(view.confirm_dialog.is_none());
    assert_eq!(view.active_panel, panel);
    assert_eq!(view.input_box.editor.text(), "keep prompt");
}

#[test]
fn non_git_repository_result_is_rejected_without_changing_fields() {
    let (_directory, mut view) = git_draft();
    let id = view.drafts[0].id;
    let cwd = view.drafts[0].setup.cwd.clone();
    let outside = tempfile::tempdir().unwrap();
    view.draft_directory_changed(id, outside.path().into(), String::new());
    assert_eq!(view.drafts[0].setup.cwd, cwd);
    assert!(view.drafts[0].error.is_some());
    assert!(!view.drafts[0].continue_after_directory);
}

#[test]
fn loading_a_directory_that_lost_git_requires_repository_selection() {
    let (_directory, view) = git_draft();
    let mut stored = view.stored_drafts();
    let plain = tempfile::tempdir().unwrap();
    stored[0].cwd = plain.path().into();
    stored[0].repository_confirmed = true;
    stored[0].branch = DraftBranch::Existing("main".into());
    let mut restored = View::default();
    restored.set_drafts(&stored);
    assert!(!restored.drafts[0].repository_confirmed);
    assert_eq!(restored.drafts[0].branch, DraftBranch::Unchosen);
    assert!(restored.drafts[0].error.is_some());
}

#[test]
fn repository_pick_before_naming_skips_repository_wizard_after_name() {
    let (directory, mut view) = git_draft();
    view.discard_selected_draft();
    let id = view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste("name in progress");
    view.open_field(Field::Directory);
    let selected = view.selector.as_ref().unwrap().selected;
    let Some(Action::ChangeDraftDirectory { cwd, .. }) = view.apply_choice(selected) else {
        panic!("repository choice");
    };
    view.draft_directory_changed(id, cwd, "main".into());
    assert_eq!(view.rename.text(), "name in progress");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Branch { .. })
    ));
    assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Wizard);
}

#[test]
fn control_enter_uses_the_draft_rule_instead_of_sending_to_a_provider_session() {
    let (directory, mut view) = fixture();
    named_draft(&mut view, directory.path(), "Unconfirmed");
    view.handle_paste("kept prompt");
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        None
    );
    assert!(matches!(
        view.selector.as_ref().map(|s| &s.kind),
        Some(SelectorKind::Repository { .. })
    ));
    assert_eq!(view.input_box.editor.text(), "kept prompt");
}

#[test]
fn draft_name_saves_when_mouse_activates_another_panel() {
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        for (x, y) in [(0, 0), (2, 5), (80, 5), (158, 38)] {
            let (directory, mut view) = fixture();
            let id = view.add_draft(setup(directory.path()));
            view.set_active_panel(Panel::Details);
            view.handle_paste("  Saved on blur  ");
            render(&view);
            assert!(!view.viewport.borrow().title_input.contains((x, y).into()));
            let action = view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Down(button),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            });
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Up(button),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            });
            if button == MouseButton::Middle && view.active_panel == Panel::Details {
                assert_eq!(action, None);
                assert_eq!(view.name_target, Some(id));
                assert!(view.selector.is_none());
                assert_eq!(view.rename.text(), "  Saved on blur  ");
                assert!(
                    view.drafts
                        .iter()
                        .find(|d| d.id == id)
                        .unwrap()
                        .name
                        .is_none()
                );
                continue;
            }
            assert!(view.name_target.is_none());
            assert!(view.selector.is_none());
            if button == MouseButton::Right && view.active_panel == Panel::Details {
                assert!(view.drafts.iter().all(|draft| draft.id != id));
            } else {
                assert_eq!(
                    view.drafts
                        .iter()
                        .find(|d| d.id == id)
                        .unwrap()
                        .name
                        .as_deref(),
                    Some("Saved on blur")
                );
                assert!(matches!(
                    view.take_drafts_action(),
                    Some(Action::PersistDrafts)
                ));
            }
        }
    }
}

#[test]
fn draft_name_click_inside_and_empty_blur_do_not_accept_name() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste("   ");
    render(&view);
    click(&mut view, 0, 0);
    assert!(view.drafts[0].name.is_none());
    view.handle_paste("Still editing");
    render(&view);
    let input = view.viewport.borrow().title_input;
    click(&mut view, input.x, input.y);
    assert!(view.drafts[0].name.is_none());
    assert!(view.name_target.is_some());
}

#[test]
fn named_draft_json_lives_until_the_real_session_arrives() {
    for blur in [false, true] {
        let (directory, mut view) = fixture();
        let mut arrived = view.rows[0].clone();
        let id = view.add_draft(setup(directory.path()));
        view.set_active_panel(Panel::Details);
        assert!(view.stored_drafts().is_empty());
        view.handle_paste("Persistent name");
        render(&view);
        if blur {
            click(&mut view, 0, 0);
        } else {
            key(&mut view, KeyCode::Enter);
        }
        let save = |view: &mut View| {
            assert!(matches!(
                view.take_drafts_action(),
                Some(Action::PersistDrafts)
            ));
            let preferences = crate::state::Preferences {
                drafts: view.stored_drafts(),
                ..Default::default()
            };
            preferences.save_drafts(directory.path()).unwrap();
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(directory.path().join("agents.json")).unwrap(),
            )
            .unwrap()["state"]["drafts"]
                .clone()
        };
        assert_eq!(save(&mut view)[0]["name"], "Persistent name");
        view.submit_draft(id);
        assert_eq!(save(&mut view)[0]["name"], "Persistent name");
        view.draft_created(id, Err(std::io::Error::other("creation failed")));
        assert_eq!(save(&mut view)[0]["error"], "creation failed");
        view.submit_draft(id);
        view.draft_created(id, Ok("e772d00b".into()));
        assert_eq!(save(&mut view)[0]["job"], "e772d00b");
        arrived.background_job = Some(crate::model::BackgroundJob {
            id: "e772d00b".into(),
            needs: None,
            awaits_prompt: false,
        });
        view.set_sessions(vec![arrived], false);
        assert_eq!(save(&mut view), serde_json::json!([]));
    }
}

#[test]
fn draft_effort_choices_are_unique_and_select_the_effective_value() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    for effort in [None, Some("high")] {
        view.drafts[0].setup.effort = effort.map(str::to_owned);
        view.open_field(Field::Effort);
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Effort { values, labels, .. } = &selector.kind else {
            panic!("expected effort choices");
        };
        assert_eq!(values, &["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(labels, values);
        assert_eq!(values[selector.selected], effort.unwrap_or("medium"));
    }
}

#[test]
fn draft_model_choices_are_unique_and_select_the_effective_value() {
    let (directory, mut view) = fixture();
    let config = directory.path().join("config");
    std::fs::write(
        config.join("settings.json"),
        r#"{"model":"claude-fable-5-1"}"#,
    )
    .unwrap();
    let provider = crate::providers::claude::ClaudeProvider::new(config);
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.set_draft_defaults(
        directory.path(),
        provider.task_defaults(directory.path()).unwrap().unwrap(),
    );
    for model in [None, Some("claude-sonnet-5-5")] {
        view.drafts[0].setup.model = model.map(str::to_owned);
        view.open_field(Field::Model);
        let selector = view.selector.as_ref().unwrap();
        let SelectorKind::Model { values, labels, .. } = &selector.kind else {
            panic!("expected model choices");
        };
        let choices = &view.input_choices[&ProviderId::Claude].models;
        assert_eq!(values.len(), choices.len());
        let mut unique = values.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), choices.len());
        for (value, label) in values.iter().zip(labels) {
            assert_eq!(
                label,
                &choices
                    .iter()
                    .find(|model| model.id == *value)
                    .unwrap()
                    .label
            );
        }
        assert_eq!(
            values[selector.selected],
            model.unwrap_or("claude-fable-5-1")
        );
    }
}

#[test]
fn selecting_draft_default_values_clears_overrides() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.drafts[0].setup.model = Some("claude-sonnet-5-5".into());
    view.drafts[0].setup.effort = Some("high".into());
    for (field, default) in [(Field::Model, "claude-opus-5-5"), (Field::Effort, "medium")] {
        view.open_field(field);
        let selector = view.selector.as_ref().unwrap();
        let values = match &selector.kind {
            SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. } => values,
            _ => panic!("expected model or effort choices"),
        };
        let index = values.iter().position(|value| value == default).unwrap();
        assert_eq!(view.apply_choice(index), None);
    }
    assert!(view.drafts[0].setup.model.is_none());
    assert!(view.drafts[0].setup.effort.is_none());
}

#[test]
fn session_choices_select_reported_values_and_dispatch_only_changes() {
    let (_directory, mut view) = fixture();
    let session = &mut view.rows[view.selected];
    session.model = Some("claude-sonnet-5-5".into());
    session.effort = Some("high".into());
    session.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    session.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    let id = session.id.clone();
    for (field, expected) in [(Field::Model, "claude-sonnet-5-5"), (Field::Effort, "high")] {
        view.open_field(field);
        let selector = view.selector.as_ref().unwrap();
        let values = match &selector.kind {
            SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. } => values,
            _ => panic!("expected model or effort choices"),
        };
        assert_eq!(values[selector.selected], expected);
        let selected = selector.selected;
        let other = usize::from(selected == 0);
        let value = values[other].clone();
        assert_eq!(view.apply_choice(selected), None);
        view.open_field(field);
        let action = if field == Field::Model {
            Action::SetModel {
                id: id.clone(),
                model: value,
            }
        } else {
            Action::SetEffort {
                id: id.clone(),
                effort: value,
            }
        };
        assert_eq!(view.apply_choice(other), Some(action));
    }
}

#[test]
fn draft_defaults_are_visible_clickable_and_can_be_selected_without_an_override() {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    let area = Rect::new(0, 0, 180, 52);
    for (field, expected) in [(Field::Model, "Opus 5.5"), (Field::Effort, "medium")] {
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, Utc::now());
        let rect = view
            .viewport
            .borrow()
            .field_targets
            .iter()
            .find(|(_, target)| *target == field)
            .unwrap()
            .0;
        let text = (rect.x..rect.right())
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect::<String>();
        assert_eq!(text, expected);
        assert_eq!(click(&mut view, rect.x, rect.y), None);
        assert_eq!(view.selector.as_ref().unwrap().field(), field);
        let selected = view.selector.as_ref().unwrap().selected;
        view.render_at(area, &mut buffer, Utc::now());
        let rect = view
            .viewport
            .borrow()
            .picker_rows
            .iter()
            .find(|(_, index)| *index == selected)
            .unwrap()
            .0;
        let text = (rect.x..rect.right())
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect::<String>();
        assert_eq!(text.trim(), expected);
        key(&mut view, KeyCode::Enter);
        let draft = view.selected_draft().unwrap();
        assert!(draft.setup.model.is_none());
        assert!(draft.setup.effort.is_none());
    }
}

#[test]
fn draft_configured_defaults_survive_clearing_overrides() {
    let (directory, mut view) = fixture();
    let mut setup = setup(directory.path());
    setup.model = Some("claude-fable-5-1".into());
    setup.effort = Some("xhigh".into());
    view.add_draft(setup);
    view.set_active_panel(Panel::Details);
    for field in [Field::Model, Field::Effort] {
        view.open_field(field);
        view.set_active_panel(Panel::Details);
        key(&mut view, KeyCode::Enter);
    }
    let buffer = render(&view);
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Fable 5.1"), "{text}");
    assert!(text.contains("xhigh"), "{text}");
    assert!(view.selected_draft().unwrap().setup.model.is_none());
    assert!(view.selected_draft().unwrap().setup.effort.is_none());
}

#[test]
fn sessions_without_model_or_effort_have_no_border_controls() {
    let (_directory, mut view) = fixture();
    view.rows[view.selected].model = None;
    view.rows[view.selected].effort = None;
    render(&view);
    assert!(
        view.viewport
            .borrow()
            .field_targets
            .iter()
            .all(|(_, field)| !matches!(field, Field::Model | Field::Effort))
    );
}

#[test]
fn named_drafts_keep_default_controls_without_overrides() {
    let (directory, mut view) = fixture();
    named_draft(&mut view, directory.path(), "default controls");
    render(&view);
    for field in [Field::Model, Field::Effort] {
        assert!(
            view.viewport
                .borrow()
                .field_targets
                .iter()
                .any(|(_, target)| *target == field)
        );
    }
}

#[test]
fn draft_name_click_branch_opens_branch() {
    assert_name_field_click(&[Field::Branch], false);
}

#[test]
fn draft_name_click_model_or_effort_opens_clicked_field() {
    assert_name_field_click(&[Field::Model, Field::Effort], false);
}

#[test]
fn draft_name_click_directory_is_side_edit() {
    assert_name_field_click(&[Field::Directory], false);
}

#[test]
fn draft_rename_click_fields_opens_clicked_field() {
    assert_name_field_click(
        &[Field::Branch, Field::Model, Field::Effort, Field::Directory],
        true,
    );
}

fn assert_name_field_click(fields: &[Field], named: bool) {
    for &field in fields {
        let (directory, mut view) = git_draft();
        let id = if named {
            view.selected_draft.unwrap()
        } else {
            view.discard_selected_draft();
            view.add_draft(setup(directory.path()))
        };
        view.open_name(id);
        view.set_active_panel(Panel::Details);
        view.rename.set_text("Clicked name");
        render(&view);
        click_field(&mut view, field);
        assert_eq!(
            view.selected_draft().unwrap().name.as_deref(),
            Some("Clicked name")
        );
        assert_eq!(view.selector.as_ref().unwrap().field(), field);
        assert_eq!(view.selector.as_ref().unwrap().origin, Origin::Border);
        if field == Field::Directory {
            let selected = view.selector.as_ref().unwrap().selected;
            let Some(Action::ChangeDraftDirectory { draft_id, cwd }) = view.apply_choice(selected)
            else {
                panic!("directory choice")
            };
            view.draft_directory_changed(draft_id, cwd, "main".into());
            assert!(view.selector.is_none());
        }
    }
}

#[test]
fn draft_name_click_prompt_focuses_prompt() {
    assert_name_prompt_click("Typed name");
}

#[test]
fn draft_empty_name_click_prompt_routes_keys_to_prompt() {
    assert_name_prompt_click("");
}

fn assert_name_prompt_click(name: &str) {
    let (directory, mut view) = fixture();
    view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste(name);
    render(&view);
    let prompt = view.viewport.borrow().prompt;
    click(&mut view, prompt.x + 2, prompt.y);
    assert!(view.prompt_focused());
    assert!(view.name_target.is_none());
    assert!(view.selector.is_none());
    key(&mut view, KeyCode::Char('p'));
    assert_eq!(view.input_box.editor.text(), "p");
    assert_eq!(
        view.selected_draft().unwrap().name.as_deref(),
        (!name.is_empty()).then_some(name)
    );
    if name.is_empty() {
        key(&mut view, KeyCode::Enter);
        assert!(view.name_target.is_some());
        assert_eq!(view.active_panel, Panel::Details);
    }
}

#[test]
fn draft_name_click_other_task_keeps_click() {
    let (directory, mut view) = fixture();
    view.grouping = Grouping::Status;
    let session = view.rows[0].id.clone();
    let id = view.add_draft(setup(directory.path()));
    view.set_active_panel(Panel::Details);
    view.handle_paste("Leave draft");
    render(&view);
    let viewport = view.viewport.borrow();
    let entry = view.center_rows(&view.selectable_indices(), view.grouping).iter().position(|entry| matches!(entry, super::navigation::CenterRow::Task(index) if view.rows[*index].id == session)).unwrap();
    let x = viewport.list_rows.x + 2;
    let y = viewport.list_rows.y + (entry - viewport.first_entry) as u16;
    drop(viewport);
    click(&mut view, x, y);
    assert_eq!(view.selected_id(), Some(&session));
    assert_eq!(
        view.drafts
            .iter()
            .find(|draft| draft.id == id)
            .unwrap()
            .name
            .as_deref(),
        Some("Leave draft")
    );
    assert!(view.selector.is_none());
}

#[test]
fn draft_name_right_click_outside_cancels_rename() {
    let (_directory, mut view) = git_draft();
    let id = view.selected_draft.unwrap();
    view.open_name(id);
    view.set_active_panel(Panel::Details);
    view.rename.set_text("Discard rename");
    render(&view);
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right),
    ] {
        view.handle_mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
    }
    assert_eq!(
        view.selected_draft().unwrap().name.as_deref(),
        Some("probe")
    );
    assert!(view.name_target.is_none());
    assert!(view.selector.is_none());
    assert!(view.confirm_dialog.is_none());
}

fn type_pending_branch(view: &mut View) {
    render(view);
    click_field(view, Field::Branch);
    let create = view.selector.as_ref().unwrap().len() - 1;
    view.apply_choice(create);
    view.handle_paste("feature/pending");
    assert_eq!(
        view.selector
            .as_ref()
            .unwrap()
            .branch_editor()
            .unwrap()
            .text(),
        "feature/pending"
    );
}

fn assert_pending_branch(view: &View) {
    assert_eq!(view.selected_draft().unwrap().branch, DraftBranch::Unchosen);
    assert_eq!(
        view.selector
            .as_ref()
            .unwrap()
            .branch_editor()
            .unwrap()
            .text(),
        "feature/pending"
    );
}

#[test]
fn draft_branch_edit_survives_field_switch() {
    for field in [Field::Model, Field::Effort, Field::Directory] {
        let (_directory, mut view) = git_draft();
        type_pending_branch(&mut view);
        render(&view);
        click_field(&mut view, field);
        assert_eq!(view.selector.as_ref().unwrap().field(), field);
        render(&view);
        click_field(&mut view, Field::Branch);
        assert_pending_branch(&view);
    }
}

#[test]
fn draft_branch_edit_field_toggle_closes_once_and_keeps_text() {
    let (_directory, mut view) = git_draft();
    type_pending_branch(&mut view);
    render(&view);
    click_field(&mut view, Field::Branch);
    assert!(view.selector.is_none());
    render(&view);
    click_field(&mut view, Field::Branch);
    assert_pending_branch(&view);
}

#[test]
fn draft_branch_edit_prompt_click_closes_and_enter_restores_text() {
    let (_directory, mut view) = git_draft();
    type_pending_branch(&mut view);
    render(&view);
    let prompt = view.viewport.borrow().prompt;
    click(&mut view, prompt.x + 2, prompt.y);
    assert!(view.selector.is_none());
    assert!(view.prompt_focused());
    view.handle_paste("Prompt kept");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_pending_branch(&view);
    assert_eq!(view.input_box.editor.text(), "Prompt kept");
}

#[test]
fn draft_branch_edit_survives_selecting_another_task() {
    let (_directory, mut view) = git_draft();
    let id = view.selected_draft.unwrap();
    type_pending_branch(&mut view);
    let session = view
        .rows
        .iter()
        .position(|row| row.status != SessionStatus::New)
        .unwrap();
    view.select(session);
    assert!(view.selector.is_none());
    view.select_draft(id);
    render(&view);
    click_field(&mut view, Field::Branch);
    assert_pending_branch(&view);
}

#[test]
fn resumable_session_settings_dispatch_only_a_changed_value() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for status in [
            SessionStatus::Inactive,
            SessionStatus::Ready,
            SessionStatus::Working,
        ] {
            let (directory, mut view) = fixture();
            let model = if provider == ProviderId::Codex {
                std::fs::write(
                    directory.path().join("models_cache.json"),
                    include_str!("../../tests/fixtures/codex-models-cache.json"),
                )
                .unwrap();
                let provider = crate::providers::codex::CodexProvider::new(
                    directory.path().into(),
                    directory.path().join("manager"),
                );
                view.input_choices
                    .insert(ProviderId::Codex, provider.input_choices());
                "gpt-6.1-sol"
            } else {
                "claude-sonnet-5-5"
            };
            let session = &mut view.rows[view.selected];
            session.id.provider = provider;
            session.status = status;
            session.process = None;
            session.model = Some(model.into());
            session.effort = Some("high".into());
            let id = session.id.clone();
            if status == SessionStatus::Inactive {
                key(&mut view, KeyCode::Tab);
            }
            for field in [Field::Model, Field::Effort] {
                let area = Rect::new(0, 0, 180, 52);
                let mut buffer = Buffer::empty(area);
                view.render_at(area, &mut buffer, Utc::now());
                let rect = view
                    .viewport
                    .borrow()
                    .field_targets
                    .iter()
                    .find(|(_, target)| *target == field)
                    .unwrap()
                    .0;
                assert_eq!(click(&mut view, rect.x, rect.y), None);
                let selector = view
                    .selector
                    .as_ref()
                    .expect("resumable session selector opens");
                let current = selector.selected;
                let (SelectorKind::Model { values, .. } | SelectorKind::Effort { values, .. }) =
                    &selector.kind
                else {
                    unreachable!()
                };
                assert_eq!(
                    values[current],
                    if field == Field::Model { model } else { "high" }
                );
                let other = usize::from(current == 0);
                let value = values[other].clone();
                // Choosing the current value closes the selector without touching the session.
                assert_eq!(view.apply_choice(current), None);
                assert!(view.selector.is_none());
                assert_eq!(click(&mut view, rect.x, rect.y), None);
                let expected = if field == Field::Model {
                    Action::SetModel {
                        id: id.clone(),
                        model: value,
                    }
                } else {
                    Action::SetEffort {
                        id: id.clone(),
                        effort: value,
                    }
                };
                assert_eq!(view.apply_choice(other), Some(expected));
            }
        }
    }
}
