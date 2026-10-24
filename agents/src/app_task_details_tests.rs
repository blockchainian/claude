use super::*;
use crate::model::{Activity, BackgroundTask, PullRequest};
use chrono::{Duration as Age, Utc};
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

fn cell_text(buffer: &Buffer, x: u16, y: u16) -> String {
    // Remove terminal metadata while keeping the buffer's cell coordinates.
    let mut text = String::new();
    let mut chars = buffer[(x, y)].symbol().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            text.push(ch);
        } else {
            match chars.next() {
                Some(']') => {
                    while let Some(ch) = chars.next() {
                        if ch == '\x1b' && chars.next() == Some('\\') {
                            break;
                        }
                    }
                }
                Some('[') => {
                    for ch in chars.by_ref() {
                        if ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                other => panic!("unexpected terminal escape: {other:?}"),
            }
        }
    }
    text
}

fn row_text(buffer: &Buffer, y: u16, start: u16, end: u16) -> String {
    (start..end).map(|x| cell_text(buffer, x, y)).collect()
}

fn setup(activity: bool) -> (tempfile::TempDir, App) {
    let (directory, mut app) = fixture();
    let now = Utc::now();
    let session = &mut app.sessions[0];
    session.cwd = crate::test_project();
    session.prompt = "Check rendering".into();
    session.prompt_at = Some(now - Age::seconds(65));
    session.last_record_at = Some(now);
    session.turn_messages = 3;
    session.last_message = Some("Done".into());
    session.last_message_at = Some(now - Age::minutes(2));
    session.activity = activity.then(|| Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
        label: "Read files".into(),
        at: now - Age::minutes(1),
    });
    session.git_branch = Some("main".into());
    session.pr = Some(PullRequest {
        number: 13,
        url: "https://github.com/blockchainian/claude/pull/13".into(),
    });
    session.background = vec![BackgroundTask {
        description: "Check files".into(),
        output: Some(directory.path().join("missing-output")),
        started_at: now - Age::hours(1),
    }];
    app.view.set_sessions(app.sessions.clone(), false);
    select_archive(&mut app);
    (directory, app)
}

#[test]
fn app_task_details_groups_and_project_links_follow_panel_rules() {
    for activity in [false, true] {
        let (_directory, mut app) = setup(activity);
        for width in [120, 149, 150, 200, 240] {
            let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
            app.draw(&mut terminal).unwrap();
            let buffer = terminal.backend().buffer();
            let status_y = position(buffer, "❯").1 - 1;
            let separators: Vec<_> = (0..width)
                .filter(|&x| cell_text(buffer, x, 2) == "│")
                .collect();
            let left = separators[0] + 2;
            if separators[0] - 2 >= 56 {
                let updated_end = (1..separators[0])
                    .rfind(|&x| cell_text(buffer, x, 3) != " ")
                    .unwrap()
                    + 1;
                assert_eq!(updated_end, separators[0] - 1);
            }
            let right = if width >= 150 {
                separators[1] - 1
            } else {
                width - 1
            };
            for (label, value) in [
                (
                    if activity { "Last activity" } else { "Update" },
                    if activity { "1m ago" } else { "2m ago" },
                ),
                ("Prompt", "1m 5s"),
            ] {
                let (x, y) = position(buffer, label);
                assert_eq!(x, left + (right - left - label.len() as u16) / 2);
                let actual = row_text(buffer, y, left, right);
                assert!(actual.trim_end().ends_with(value), "{width}: {actual:?}");
                assert_eq!(
                    actual.trim_end().len(),
                    usize::from(right - left - u16::from(width < 150))
                );
                assert!(!actual.contains("messages"));
            }
            let pr_x = right - 3 - u16::from(width < 150);
            let folder_x = left;
            let folder = row_text(buffer, status_y, folder_x, pr_x);
            let folder = folder.split_whitespace().next().unwrap();
            assert!(
                "project".starts_with(folder.trim_end_matches('…')),
                "{width}: {folder}"
            );
            assert_eq!(row_text(buffer, status_y, pr_x, pr_x + 3), "#13");
            assert!(
                buffer[(folder_x, status_y)]
                    .modifier
                    .contains(Modifier::DIM)
            );
            assert!(buffer[(pr_x, status_y)].symbol().contains("\x1b]8;;"));
            let rendered = (2..status_y)
                .map(|y| row_text(buffer, y, left, right))
                .collect::<Vec<_>>();
            assert!(
                rendered
                    .iter()
                    .all(|row| !row.contains("Project") && !row.contains("Model"))
            );
            assert!(
                rendered
                    .iter()
                    .all(|row| !row.contains(crate::test_project().to_str().unwrap()))
            );

            let (_, background_y) = position(buffer, "Check files");
            let time_right = right - u16::from(width < 150);
            assert_eq!(
                row_text(buffer, background_y, time_right - 5, time_right),
                "1h 0m"
            );
            assert_eq!(cell_text(buffer, time_right, background_y), "╶");
            for x in pr_x..pr_x + 3 {
                assert_eq!(click_link(&mut app, MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left), column: x, row: status_y, modifiers: KeyModifiers::NONE,
                }), Some(Action::open_path(format!("vscode://blockchainian.git/view-pr?repo={}&url=https%3A%2F%2Fgithub.com%2Fblockchainian%2Fclaude%2Fpull%2F13", crate::test_project().to_str().unwrap().replace('/', "%2F")).into())));
            }
            assert_eq!(
                click_link(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: folder_x,
                        row: status_y,
                        modifiers: KeyModifiers::NONE,
                    }
                ),
                Some(Action::open_path(crate::test_project()))
            );
        }
    }
}

#[test]
fn app_task_details_without_pr_has_only_folder_link_and_truncated_branch() {
    let (_directory, mut app) = setup(false);
    app.sessions[0].git_branch = Some("分支".repeat(40));
    app.sessions[0].pr = None;
    app.view.set_sessions(app.sessions.clone(), false);
    for width in [60, 80, 90, 149, 150] {
        let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
        app.draw(&mut terminal).unwrap();
        let buffer = terminal.backend().buffer();
        let y = position(buffer, "❯").1 - 1;
        let separators: Vec<_> = (0..width)
            .filter(|&x| cell_text(buffer, x, 2) == "│")
            .collect();
        let left = if width < 100 { 1 } else { separators[0] + 2 };
        let end = if width >= 150 {
            separators[1] - 1
        } else {
            width - 1
        };
        let rule = row_text(buffer, y, left, end);
        assert!(!rule.contains("#13"));
        assert!(rule.contains('分') && rule.contains('…'), "{width}: {rule}");
        for x in end - 5..end {
            assert_eq!(
                click_link(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: x,
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    }
                ),
                None
            );
        }
    }
}

fn click_link(app: &mut App, event: MouseEvent) -> Option<Action> {
    assert_eq!(app.view.handle_mouse(event), None);
    app.view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..event
    })
}
