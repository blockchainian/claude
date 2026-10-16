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
    session.cwd = "/tmp/project".into();
    session.prompt = "Check rendering".into();
    session.prompt_at = Some(now - Age::seconds(65));
    session.last_record_at = Some(now);
    session.turn_messages = 3;
    session.last_message = Some("Done".into());
    session.last_message_at = Some(now - Age::minutes(2));
    session.activity = activity.then(|| Activity {
        kind: crate::log::Kind::Tool,
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
        output: directory.path().join("missing-output"),
        started_at: now - Age::hours(1),
    }];
    app.view.set_sessions(app.sessions.clone(), false);
    select_archive(&mut app);
    (directory, app)
}

#[test]
fn app_details_groups_end_at_matching_panel_margins_and_pr_click_moves() {
    for activity in [false, true] {
        let (_directory, mut app) = setup(activity);
        for width in [120, 149, 150, 200] {
            let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
            app.draw(&mut terminal).unwrap();
            let buffer = terminal.backend().buffer();
            let (left, project_y) = position(buffer, "Project");
            let separators: Vec<_> = (0..width)
                .filter(|&x| cell_text(buffer, x, 3) == "│")
                .collect();
            assert_eq!(left, separators[0] + 2);
            if width != 150 {
                let updated_end = (1..separators[0])
                    .rfind(|&x| cell_text(buffer, x, 4) != " ")
                    .unwrap()
                    + 1;
                assert_eq!(updated_end, separators[0] - 1);
            }
            let right = if width >= 150 {
                (left..width)
                    .find(|&x| cell_text(buffer, x, project_y) == "│")
                    .unwrap()
                    - 1
            } else {
                width - 1
            };
            for (label, group) in [
                (
                    if activity {
                        "Latest activity"
                    } else {
                        "Last message"
                    },
                    if activity { "1m ago" } else { "2m ago" },
                ),
                ("Prompt · 3 messages", "1m 5s"),
                ("Project · main", "#13"),
            ] {
                let (x, y) = position(buffer, label);
                assert_eq!(x, left);
                let actual = row_text(buffer, y, left, right);
                assert!(actual.starts_with(label), "{actual:?}");
                assert!(actual.ends_with(group), "{width}: {actual:?}");
                let group_start = right - group.chars().count() as u16;
                assert!(group_start > left + label.len() as u16);
                assert!(
                    row_text(buffer, y, left + label.len() as u16, group_start)
                        .chars()
                        .all(|c| c == ' ')
                );
                for offset in label
                    .match_indices('·')
                    .map(|(i, _)| label[..i].chars().count() as u16)
                {
                    assert_eq!(
                        row_text(buffer, y, left + offset - 1, left + offset + 2),
                        " · "
                    );
                    assert!(buffer[(left + offset, y)].modifier.contains(Modifier::DIM));
                }
            }
            let (_, background_y) = position(buffer, "Check files");
            assert_eq!(row_text(buffer, background_y, right - 5, right), "1h 0m");
            for x in right - 3..right {
                assert_eq!(click_link(&mut app, MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left), column: x, row: project_y, modifiers: KeyModifiers::NONE,
                }), Some(Action::OpenPath("vscode://blockchainian.git/view-pr?repo=%2Ftmp%2Fproject&url=https%3A%2F%2Fgithub.com%2Fblockchainian%2Fclaude%2Fpull%2F13".into())));
            }
            assert_eq!(
                click_link(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: left + 3,
                        row: project_y,
                        modifiers: KeyModifiers::NONE,
                    }
                ),
                None
            );
        }
    }
}

#[test]
fn app_details_narrow_groups_preserve_labels_and_do_not_leave_pr_hitboxes() {
    let (_directory, mut app) = setup(false);
    app.sessions[0].git_branch = Some("分支".repeat(40));
    app.view.set_sessions(app.sessions.clone(), false);
    for width in [60, 80, 90, 150] {
        let mut terminal = Terminal::new(TestBackend::new(width, 60)).unwrap();
        app.draw(&mut terminal).unwrap();
        let buffer = terminal.backend().buffer();
        let (left, y) = position(buffer, "Project");
        assert_eq!(row_text(buffer, y, left, left + 8), "Project ");
        let end = (left..width)
            .find(|&x| cell_text(buffer, x, y) == "│")
            .unwrap_or(width);
        assert!(row_text(buffer, y, left, end).contains('…'));
        for x in left..end {
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
