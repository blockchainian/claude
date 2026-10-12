use super::*;
use chrono::{Duration, Utc};

fn session(id: &str, title: &str, status: SessionStatus, age: i64) -> Session {
    Session {
        id: id.into(),
        cwd: "/tmp/project".into(),
        name: None,
        title: title.into(),
        prompt: title.into(),
        queued: Vec::new(),
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        model: None,
        effort: None,
        git_branch: None,
        pr: None,
        status,
        process: None,
        updated_at: Utc::now() - Duration::seconds(age),
        last_message: None,
        last_message_at: None,
        activity: None,
        background: Vec::new(),
        last_prompt: None,
        transcript: None,
    }
}
fn screen(view: &View, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf.content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn key(view: &mut View, code: KeyCode) -> Option<Action> {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn fixture() -> View {
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("42", "Current task", SessionStatus::Ready, -3600),
            session(
                "43",
                "A longer task title that uses the available column",
                SessionStatus::Ready,
                330,
            ),
            session("44", "Needs an answer", SessionStatus::NeedsInput, 7230),
            session("45", "Unloaded task", SessionStatus::Inactive, 172830),
            session("46", "Working task", SessionStatus::Working, 86430),
            session("47", "Failed task", SessionStatus::NeedsInput, 3630),
        ],
        false,
    );
    view.set_pinned(&["42".into(), "44".into()]);
    view.selected = view.rows.iter().position(|s| s.id == "42").unwrap();
    view
}
fn simple_fixture() -> View {
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("42", "Current task", SessionStatus::Ready, 120),
            session("43", "Needs an answer", SessionStatus::NeedsInput, 120),
            session("44", "Unloaded task", SessionStatus::Inactive, 120),
        ],
        false,
    );
    view
}
#[test]
fn live_center_columns_without_voice_and_with_needs_input_error_delta() {
    assert_eq!(
        screen(&fixture(), 160, 22),
        include_str!("../../tests/ui_snapshots/live_center_columns.txt")
    );
}
#[test]
fn live_center_help_two_columns_with_pin_and_narrow_tabs_delta() {
    let mut view = simple_fixture();
    key(&mut view, KeyCode::Char('?'));
    assert_eq!(
        screen(&view, 60, 24),
        include_str!("../../tests/ui_snapshots/live_center_help_two_columns.txt")
    );
    // At 56 columns the filled tab strip hides Inactive; the shortcut columns still fit.
    let narrow = screen(&view, 56, 24);
    let wide = screen(&view, 60, 24);
    assert_eq!(
        narrow.lines().skip(3).collect::<Vec<_>>(),
        wide.lines().skip(3).collect::<Vec<_>>()
    );
}
#[test]
fn live_center_status_filter() {
    let mut view = simple_fixture();
    key(&mut view, KeyCode::BackTab);
    assert_eq!(
        screen(&view, 40, 12),
        include_str!("../../tests/ui_snapshots/live_center_status_filter.txt")
    );
}
#[test]
fn live_center_search_active() {
    let mut view = View::default();
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    key(&mut view, KeyCode::Char('/'));
    for c in "find".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_search_active.txt")
    );
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((15, 3)));
}
#[test]
fn live_center_show_more() {
    let mut view = View::default();
    view.set_sessions(
        vec![session("42", "Recent task", SessionStatus::Inactive, 120)],
        true,
    );
    key(&mut view, KeyCode::Down);
    assert_eq!(
        screen(&view, 80, 12),
        include_str!("../../tests/ui_snapshots/live_center_show_more.txt")
    );
    assert_eq!(key(&mut view, KeyCode::Enter), Some(Action::ShowMore));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(screen(&view, 80, 12).contains("Loading more…"));
}
#[test]
fn agents_overview_model_grouping() {
    let mut view = View::new(Grouping::Model, false);
    let rows = [
        ("Older task", Some("model-a"), 4),
        ("Other model", Some("model-b"), 3),
        ("Recent task", Some("model-a"), 2),
        ("Legacy task", None, 1),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (title, model, age))| {
        let mut row = session(&i.to_string(), title, SessionStatus::Ready, age);
        row.model = model.map(str::to_owned);
        row
    })
    .collect();
    view.set_sessions(rows, false);
    assert_eq!(
        screen(&view, 100, 24),
        include_str!("../../tests/ui_snapshots/agents_overview_model_grouping.txt")
    );
}
#[test]
fn agents_overview_recent_sessions_twelve_rows_delta() {
    let mut view = View::default();
    view.set_sessions(
        (1..=12)
            .map(|i| {
                let mut row = session(
                    &i.to_string(),
                    &format!("Task {i}"),
                    SessionStatus::Inactive,
                    (13 - i) * 60,
                );
                row.cwd = "/".into();
                row
            })
            .collect(),
        true,
    );
    let rendered = screen(&view, 80, 24);
    assert!(rendered.contains("All 12"));
    assert!(rendered.contains("/  12"));
    let expected = include_str!("../../tests/ui_snapshots/agents_overview_recent_sessions.txt");
    // The reference loads ten rows. Twelve rows change only counts and append Task 2 / Task 1.
    for (actual, expected) in rendered
        .lines()
        .skip(5)
        .take(10)
        .zip(expected.lines().skip(5).take(10))
    {
        assert_eq!(
            actual.split("Inactive").next(),
            expected.split("Inactive").next()
        );
    }
    assert!(rendered.contains("Task 2 "));
    assert!(rendered.contains("Task 1 "));
    assert!(rendered.contains("Show more"));
}
#[test]
fn search_text_wins_and_refresh_preserves_selection_and_state() {
    let mut view = fixture();
    key(&mut view, KeyCode::Char('/'));
    for c in "task".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    key(&mut view, KeyCode::Char('j'));
    assert_eq!(view.search, "taskj");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(view.search, "task");
    let id = view.selected_id().unwrap().to_owned();
    view.set_sessions(view.rows.clone(), false);
    assert_eq!(view.selected_id(), Some(id.as_str()));
    assert!(view.searching);
    assert_eq!(view.search, "task");
    key(&mut view, KeyCode::Esc);
    assert!(!view.searching);
    assert_eq!(key(&mut view, KeyCode::Esc), Some(Action::Quit));
}
#[test]
fn actions_and_hide_move_to_next_visible_row() {
    let mut view = fixture();
    assert_eq!(
        key(&mut view, KeyCode::Char('o')),
        Some(Action::Resume("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('f')),
        Some(Action::Fork("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Right),
        Some(Action::Open("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('x')),
        Some(Action::Unsupported("Stop"))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin {
            id: "42".into(),
            pinned: false
        })
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('h')),
        Some(Action::Hide("42".into()))
    );
    assert_eq!(view.selected_id(), Some("44"));
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(screen(&view, 88, 16).contains("Rename › Needs an answer"));
    key(&mut view, KeyCode::Esc);

    assert_eq!(
        key(&mut view, KeyCode::Char('g')),
        Some(Action::PersistGrouping(Grouping::Status))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('n')),
        Some(Action::New(Some("/tmp/project".into())))
    );
}
#[test]
fn paging_tracks_headings_and_never_wraps() {
    let mut view = View::default();
    view.set_sessions(
        (0..30)
            .map(|i| {
                let mut row = session(
                    &i.to_string(),
                    &format!("Task {i}"),
                    SessionStatus::Ready,
                    i,
                );
                if i >= 10 {
                    row.cwd = "/tmp/second-project".into();
                }
                row
            })
            .collect(),
        true,
    );
    screen(&view, 110, 18);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), Some("10"));
    key(&mut view, KeyCode::PageUp);
    assert_eq!(view.selected_id(), Some("0"));
    for _ in 0..40 {
        key(&mut view, KeyCode::PageDown);
        screen(&view, 110, 18);
    }
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.selected_id(), Some("0"));
}
#[test]
fn styles_and_unicode_clipping() {
    use super::style::selection_style_for;
    assert_eq!(
        selection_style_for(false, true, true),
        Style::default()
            .bold()
            .fg(Color::Indexed(18))
            .bg(Color::Indexed(111))
    );
    assert_eq!(
        selection_style_for(false, true, false),
        Style::default()
            .bold()
            .fg(Color::Indexed(24))
            .bg(Color::Indexed(233))
    );
    assert_eq!(
        selection_style_for(true, true, false),
        Style::default().bold().reversed()
    );
    assert_eq!(
        selection_style_for(false, false, false),
        Style::default().bold().reversed()
    );
    assert_eq!(
        super::line_truncation::truncate_line_with_ellipsis_if_overflow(
            Line::from("你好 world"),
            4
        )
        .to_string(),
        "你…"
    );
    let view = fixture();
    for width in 0..100 {
        for height in 0..7 {
            let area = Rect::new(5, 8, width, height);
            view.render(area, &mut Buffer::empty(area));
        }
    }
}

#[test]
fn details_include_attention_and_clip_messages() {
    let mut row = session("details", "Details task", SessionStatus::NeedsInput, 120);
    row.model = Some("claude-opus".into());
    row.effort = Some("high".into());
    row.git_branch = Some("main".into());
    row.process = Some(crate::model::Process {
        pid: 123,
        status: "waiting".into(),
        kind: None,
        version: Some("2.0".into()),
        started_at: None,
    });
    row.prompt = "prompt ".repeat(40);
    row.last_message = Some("message ".repeat(60));
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let rendered = screen(&view, 160, 50);
    for label in [
        "Model: claude-opus",
        "Reasoning: high",
        "Branch",
        "main",
        "Needs attention",
        "Open task to review.",
        "Waiting for your response.",
        "Last message",
    ] {
        assert!(rendered.contains(label), "{label}");
    }
    let position = |label: &str| rendered.lines().position(|line| line.contains(label));
    // "Project" names the grouping in the header and the path heads the task
    // list, so find the details line that ends with the path.
    let project = rendered
        .lines()
        .position(|line| line.trim_end().ends_with("/tmp/project"));
    assert!(position("Prompt") < position("Last message"));
    assert!(position("Needs attention") < position("Last message"));
    assert!(position("Last message") < project);
    assert!(project < position("Branch"));
    assert!(position("Branch") < position("Model: claude-opus"));
    let area = Rect::new(0, 0, 38, 25);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    assert_eq!(buf[(0, 24)].symbol(), "…");
    let title_cell = &buf[(0, 2)];
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));
    assert_eq!(title_cell.fg, Color::Reset);
}

#[test]
fn empty_states_notices_and_help_only_handle_close_keys() {
    let mut view = View::default();
    assert!(screen(&view, 80, 12).contains("No tasks yet"));
    view.set_loading(true);
    assert!(screen(&view, 80, 12).contains("Loading tasks…"));
    view.set_sessions(vec![session("a", "Alpha", SessionStatus::Ready, 1)], false);
    key(&mut view, KeyCode::Char('/'));
    key(&mut view, KeyCode::Char('z'));
    assert!(screen(&view, 80, 12).contains("No matching tasks"));
    key(&mut view, KeyCode::Esc);
    view.set_notice(Some("Notice".into()));
    assert!(screen(&view, 80, 12).ends_with("  Notice"));
    key(&mut view, KeyCode::Left);
    assert!(view.notice.is_none());
    key(&mut view, KeyCode::Char('?'));
    assert_eq!(key(&mut view, KeyCode::Char('n')), None);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        None
    );
    assert!(view.help);
    key(&mut view, KeyCode::Esc);
    assert!(!view.help);
}

#[test]
fn marker_pos_follows_the_selected_row_and_clears_on_help() {
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("a", "First", SessionStatus::Ready, 10),
            session("b", "Second", SessionStatus::Ready, 20),
        ],
        false,
    );
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let (x, y) = view.marker_pos().unwrap();
    assert_eq!(buf[(x, y)].symbol(), "›");
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.render(area, &mut buf);
    let (x2, y2) = view.marker_pos().unwrap();
    assert_eq!((x2, y2), (x, y + 1));
    assert_eq!(buf[(x2, y2)].symbol(), "›");
    view.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    view.render(area, &mut buf);
    assert_eq!(view.marker_pos(), None);
}

#[test]
fn details_name_a_missing_prompt() {
    let mut row = session("blank", "Blank task", SessionStatus::Ready, 10);
    row.prompt = String::new();
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    assert!(screen(&view, 160, 24).contains("No prompt available."));
}

#[test]
fn rename_edits_title_and_commits_trimmed_text() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(screen(&view, 88, 16).contains("Rename › Current task"));
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((23, 3)));
    for c in " updated  ".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Rename {
            id: "42".into(),
            name: "Current task updated".into()
        })
    );
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn rename_locks_navigation_cancels_and_ignores_empty_enter() {
    let mut view = fixture();
    let hints = view.center_footer_hints().to_string();
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(
        view.center_footer_hints().to_string(),
        "esc back  enter rename"
    );
    for code in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Tab,
        KeyCode::Right,
        KeyCode::F(3),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.selected_id(), Some("42"));
    }
    for _ in 0..12 {
        key(&mut view, KeyCode::Backspace);
    }
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    for c in " e\u{301}".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    key(&mut view, KeyCode::Backspace);
    assert!(screen(&view, 88, 16).contains("Rename ›"));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.center_footer_hints().to_string(), hints);
    assert_eq!(view.rows[view.selected].display_title(), "Current task");
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn rename_survives_status_refresh_but_drops_removed_or_hidden_target() {
    let mut view = fixture();
    view.status_filter = 3;
    key(&mut view, KeyCode::Char('r'));
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "42").unwrap().status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some("42"));
    assert!(screen(&view, 88, 16).contains("Rename › Current task"));
    view.hide("42");
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
    let mut view = fixture();
    key(&mut view, KeyCode::Char('r'));
    view.set_sessions(vec![], true);
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn live_center_rename_active() {
    let mut view = View::default();
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_rename_active.txt")
    );
    let area = Rect::new(0, 0, 88, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert_eq!(buf[(2, 3)].fg, Color::Cyan);
    assert!(
        buf[(2, 3)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(
        buf[(80, 3)].style(),
        Style::default()
            .fg(Color::Reset)
            .bg(Color::Reset)
            .underline_color(Color::Reset)
            .patch(super::style::active_tab_style())
    );
}

#[test]
fn delete_dialog_defaults_to_cancel_and_requires_enter() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(!view.delete_dialog.as_ref().unwrap().confirm);
    assert_eq!(
        view.center_footer_hints().to_string(),
        "↑/↓ move  enter select  esc back"
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete("42".into()))
    );
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    key(&mut view, KeyCode::Esc);
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Char('2')), None);
    assert!(view.delete_dialog.as_ref().unwrap().confirm);
    for code in [
        KeyCode::Char('o'),
        KeyCode::Char('n'),
        KeyCode::Right,
        KeyCode::Char(' '),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert!(view.delete_dialog.is_some());
    }
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete("42".into()))
    );
}

#[test]
fn delete_dialog_locks_list_and_supports_movement_keys() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    for code in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::End,
        KeyCode::Home,
        KeyCode::Char('o'),
        KeyCode::Char('n'),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.selected_id(), Some("42"));
    }
    for c in ['n', 'p', 'j', 'k'] {
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(view.selected_id(), Some("42"));
    }
}

#[test]
fn delete_live_rows_open_the_dialog_and_ignore_show_more() {
    let mut view = fixture();
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: 123,
        status: "idle".into(),
        kind: None,
        version: None,
        started_at: None,
    });
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.delete_dialog.is_some());
    assert!(screen(&view, 120, 24).contains("This stops any running work in this task"));
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete("42".into()))
    );
    view.has_more = true;
    view.selected = usize::MAX;
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.delete_dialog.is_none());
}

#[test]
fn live_center_delete_dialog() {
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(
        vec![session("44", "Unloaded task", SessionStatus::Inactive, 120)],
        false,
    );
    key(&mut view, KeyCode::Backspace);
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_delete_dialog.txt")
    );
    let area = Rect::new(0, 0, 88, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert!(
        buf[(6, 6)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(buf[(6, 7)].modifier.contains(ratatui::style::Modifier::DIM));
    // A painted cell reports unset colours as Reset, so compare against a painted default cell.
    let mut expected = ratatui::buffer::Cell::default();
    expected.set_style(super::style::selection_style());
    assert_eq!(buf[(6, 10)].style(), expected.style());
    for width in 0..100 {
        for height in 0..16 {
            let area = Rect::new(5, 8, width, height);
            view.render(area, &mut Buffer::empty(area));
        }
    }
}

#[test]
fn delete_dialog_number_shortcuts_follow_codex() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    assert!(view.delete_dialog.is_some());
    assert_eq!(key(&mut view, KeyCode::Char('1')), None);
    assert!(view.delete_dialog.is_none(), "1 accepts Cancel at once");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Char('2')), None);
    assert!(
        view.delete_dialog.is_some(),
        "2 only selects the delete item"
    );
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete("42".into()))
    );
}

#[test]
fn details_branch_pull_request_snapshot() {
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("main".into());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row.clone()], false);
    let area = Rect::new(0, 0, 40, 18);
    let render = |view: &View| {
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        buf.content
            .chunks(40)
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let symbol = cell.symbol();
                        let symbol = symbol
                            .rsplit("\x1b\\")
                            .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
                            .unwrap_or("");
                        symbol.split("\x1b]8").next().unwrap().to_owned()
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch.txt")
    );
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    view.set_sessions(vec![row.clone()], false);
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch_pr.txt")
    );
    row.git_branch = None;
    view.set_sessions(vec![row], false);
    assert!(!render(&view).contains("#833"));
}

#[test]
fn details_activity_follows_the_clock() {
    let now = Utc::now();
    let mut row = session("activity", "Activity task", SessionStatus::Working, 7);
    row.prompt_at = Some(now - Duration::seconds(4200));
    row.turn_messages = 42;
    row.last_record_at = Some(now - Duration::seconds(3595));
    row.activity = Some(crate::model::Activity {
        label: "Run checks".into(),
        at: now - Duration::seconds(7),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::hours(1));
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row.clone()], false);
    let render = |view: &View| {
        let area = Rect::new(0, 0, 40, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        buf.content
            .chunks(40)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned()
    };
    let rendered = render(&view);
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_activity_working.txt")
    );
    assert!(rendered.contains("Latest activity · 7s ago"));
    assert!(!rendered.contains("Last message"));
    row.activity.as_mut().unwrap().label =
        "Run checks on every changed surface in the project".into();
    view.set_sessions(vec![row.clone()], false);
    let clipped = render(&view);
    assert!(clipped.contains("Run checks on every changed surface in…"));
    assert!(!clipped.contains("surface in the project"));
    // The slot follows the clock, not the session status: a newer tool call
    // still shows once the session is idle, and a newer reply replaces it.
    row.activity.as_mut().unwrap().label = "Run checks".into();
    row.status = SessionStatus::Ready;
    view.set_sessions(vec![row.clone()], false);
    assert!(render(&view).contains("Latest activity · 7s ago"));
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_activity_ready.txt")
    );
    row.status = SessionStatus::Working;
    row.last_message_at = Some(now - Duration::seconds(3));
    view.set_sessions(vec![row.clone()], false);
    let replied = render(&view);
    assert!(!replied.contains("Latest activity"));
    assert_eq!(
        replied,
        include_str!("../../tests/ui_snapshots/details_activity_replied.txt")
    );
}

#[test]
fn prompt_header_counts_singular_zero_and_clamps_elapsed_time() {
    let now: chrono::DateTime<Utc> = "2026-10-12T12:00:00Z".parse().unwrap();
    for (status, prompt_at, last_record_at, count, expected) in [
        (
            SessionStatus::Working,
            Some(now - Duration::seconds(45)),
            None,
            1,
            "Prompt · 1 message · 45s",
        ),
        (
            SessionStatus::Ready,
            Some(now),
            Some(now - Duration::seconds(1)),
            0,
            "Prompt · 0 messages · 0s",
        ),
        (
            SessionStatus::Working,
            Some(now + Duration::seconds(1)),
            None,
            0,
            "Prompt · 0 messages · 0s",
        ),
        (SessionStatus::Ready, None, Some(now), 0, "Prompt"),
    ] {
        let mut row = session("turn", "Turn task", status, 0);
        row.prompt_at = prompt_at;
        row.last_record_at = last_record_at;
        row.turn_messages = count;
        let mut view = View::default();
        view.set_sessions(vec![row], false);
        let area = Rect::new(0, 0, 40, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        let header = buf
            .content
            .chunks(40)
            .find(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .starts_with("Prompt")
            })
            .unwrap();
        assert_eq!(
            header
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end(),
            expected
        );
        for cell in &header[..expected.chars().count()] {
            assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
        }
    }
}

#[test]
fn details_prompt_shows_queued_entries_in_order() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt".into();
    row.queued = vec!["First queued prompt".into(), "Second queued prompt".into()];
    assert_eq!(
        queued_details(row, 15),
        include_str!("../../tests/ui_snapshots/details_queued.txt")
    );
}

#[test]
fn details_prompt_truncates_long_queued_entry() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt ".repeat(12);
    row.queued = vec!["Queued prompt that extends beyond the panel width".into()];
    assert_eq!(
        queued_details(row, 15),
        include_str!("../../tests/ui_snapshots/details_queued_truncated.txt")
    );
}

fn queued_details(row: Session, height: u16) -> String {
    let mut view = View::default();
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, height);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    buf.content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn details_background_boxes_tail_and_clip_output() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "discard\nfirst\nlast\n{}", "x".repeat(80)).unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.last_message = Some("Checking the project".into());
    row.background = vec![
        crate::model::BackgroundTask {
            description: "Run checks".into(),
            output: output.path().into(),
            started_at: Utc::now(),
        },
        crate::model::BackgroundTask {
            description: "Waiting for output".into(),
            output: output.path().with_extension("missing"),
            started_at: Utc::now(),
        },
    ];
    let rendered = queued_details(row.clone(), 22);
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_background.txt")
    );
    row.background[0].description = "A very long description ".repeat(5);
    assert_eq!(
        queued_details(row.clone(), 22),
        include_str!("../../tests/ui_snapshots/details_background_long_title.txt")
    );
    let clipped = queued_details(row, 17);
    assert!(clipped.contains("Background\n╭"));
    assert!(clipped.contains("Last message"));
    assert!(!clipped.contains("Project"));
    assert!(clipped.ends_with("\n…"));
}

#[test]
fn background_box_dims_border_but_not_title_or_output() {
    use ratatui::style::Modifier;
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: Utc::now(),
    });
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let top = (0..20)
        .find(|y| buf[(0, *y)].symbol() == "╭")
        .expect("box top border");
    assert!(buf[(0, top)].modifier.contains(Modifier::DIM));
    assert!(buf[(30, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(2, top)].symbol(), "R");
    assert!(!buf[(2, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(2, top + 1)].symbol(), "t");
    assert!(!buf[(2, top + 1)].modifier.contains(Modifier::DIM));
}

#[test]
fn background_task_whose_output_ended_is_hidden() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick\n\n[exited with code 0]").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: Utc::now(),
    });
    let rendered = queued_details(row, 19);
    assert!(!rendered.contains("\nBackground\n"));
    assert!(!rendered.contains("Run checks"));
}

#[test]
fn background_redraw_follows_selection() {
    let mut first = session("first", "First", SessionStatus::Ready, 0);
    first.background.push(crate::model::BackgroundTask {
        description: "Running".into(),
        output: "/tmp/unused.output".into(),
        started_at: Utc::now(),
    });
    let mut view = View::default();
    view.set_sessions(
        vec![
            first,
            session("second", "Second", SessionStatus::Ready, 120),
        ],
        false,
    );
    assert!(view.has_background());
    key(&mut view, KeyCode::Down);
    assert!(!view.has_background());
    view.set_sessions(Vec::new(), false);
    assert!(!view.has_background());
}
