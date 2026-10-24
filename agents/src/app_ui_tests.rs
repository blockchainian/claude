use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;

fn fixture() -> (tempfile::TempDir, App) {
    fixture_with_double_click_ms(Preferences::default().double_click_ms)
}

fn fixture_with_double_click_ms(double_click_ms: u64) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/probe");
    crate::providers::claude::write_fixture_cache(directory.path());
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    Preferences {
        double_click_ms,
        ..Preferences::default()
    }
    .save(directory.path())
    .unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().to_owned()),
        )]),
        directory.path().to_owned(),
    );
    app.refresh();
    assert_eq!(app.sessions.len(), 1);
    (directory, app)
}

fn key(app: &mut App, code: KeyCode) {
    assert!(!app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
}

fn draw(app: &mut App, width: u16) -> ratatui::buffer::Buffer {
    draw_size(app, width, 40)
}

fn draw_size(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    draw_size_at(app, width, height, chrono::Utc::now())
}

fn draw_size_at(
    app: &mut App,
    width: u16,
    height: u16,
    now: chrono::DateTime<chrono::Utc>,
) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    app.draw_at(&mut terminal, now).unwrap();
    if app.pending_transcript.is_some()
        && !app.view.transcript_fully_loaded()
        && !app.view.restoring_event_position()
    {
        super::event_search_tests::finish_search(app);
        app.draw_at(&mut terminal, now).unwrap();
    }
    terminal.backend().buffer().clone()
}

fn lines(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn starts_at(
    buffer: &ratatui::buffer::Buffer,
    columns: std::ops::Range<u16>,
    y: u16,
    text: &str,
) -> bool {
    let mut chars = columns.flat_map(|x| buffer[(x, y)].symbol().chars());
    text.chars().all(|expected| chars.next() == Some(expected))
}

fn position(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
    for y in (0..buffer.area.height).rev() {
        for x in 0..buffer.area.width {
            if starts_at(buffer, x..buffer.area.width, y, text) {
                return (x, y);
            }
        }
    }
    panic!("missing {text:?}:\n{}", lines(buffer).join("\n"));
}

/// The leftmost and rightmost cells where `text` starts on its bottommost row.
fn row_ends(buffer: &ratatui::buffer::Buffer, text: &str) -> ((u16, u16), (u16, u16)) {
    let (_, y) = position(buffer, text);
    let starts: Vec<_> = (0..buffer.area.width)
        .filter(|&x| starts_at(buffer, x..buffer.area.width, y, text))
        .collect();
    ((starts[0], y), (*starts.last().unwrap(), y))
}

fn event_position(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
    let separators: Vec<_> = (0..buffer.area.width - 1)
        .filter(|&x| buffer[(x, 2)].symbol() == "│")
        .collect();
    let (_, prompt) = position(buffer, "❯");
    let (left, rows) = if separators.len() == 2 {
        (separators[1] + 2, 2..prompt - 1)
    } else {
        (1, prompt + 2..event_list_end(buffer, prompt))
    };
    for y in rows.rev() {
        for x in left..buffer.area.width - 1 {
            if starts_at(buffer, x..buffer.area.width - 1, y, text) {
                return (x, y);
            }
        }
    }
    panic!("missing log body {text:?}:\n{}", lines(buffer).join("\n"));
}

fn event_list_end(buffer: &ratatui::buffer::Buffer, prompt: u16) -> u16 {
    (prompt + 2..buffer.area.height - 1)
        .find(|&y| {
            (1..buffer.area.width - 1)
                .filter(|&x| buffer[(x, y)].symbol() == "─")
                .count()
                > usize::from(buffer.area.width) / 2
        })
        .unwrap_or(buffer.area.height - 1)
}

fn click(app: &mut App, x: u16, y: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        assert!(!app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })));
    }
}

fn wheel(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    assert!(!app.handle_event(Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })));
}

fn assert_event_cursor(buffer: &ratatui::buffer::Buffer, row: u16) {
    let separators: Vec<_> = (0..buffer.area.width - 1)
        .filter(|&x| buffer[(x, 2)].symbol() == "│")
        .collect();
    let task_row = (2..position(buffer, "❯").1)
        .find(|&y| {
            buffer[(0, y)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        })
        .expect("selected task cursor");
    let cursor = buffer[(0, task_row)].style();
    let start = if separators.len() == 2 {
        separators[1] + 1
    } else {
        0
    };
    // Terminal diff output omits continuation cells covered by a wide grapheme.
    let mut x = start;
    while x < buffer.area.width {
        assert_eq!(
            buffer[(x, row)].bg,
            cursor.bg.unwrap(),
            "cursor fill at ({x}, {row})"
        );
        x += unicode_width::UnicodeWidthStr::width(buffer[(x, row)].symbol()).max(1) as u16;
    }
    if separators.len() == 2 {
        let separator = separators[1];
        assert_eq!(buffer[(separator, row)].symbol(), "│");
        assert_ne!(buffer[(separator, row)].bg, cursor.bg.unwrap());
    }
}

#[test]
fn settled_zero_duration_leaves_the_time_column_empty_through_app() {
    for width in [100, 149, 150, 200] {
        let (_directory, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        // Real transcript payloads with scrubbed text and equal adjacent timestamps.
        std::fs::write(
            path,
            include_bytes!("../tests/fixtures/claude-equal-timestamps.jsonl"),
        )
        .unwrap();
        app.refresh();
        select_archive(&mut app);
        let buffer = draw(&mut app, width);
        let (x, y) = event_position(&buffer, "Equal timestamp reply");
        let right = width - 2;
        assert_eq!(segment(&buffer, right - 8..right, y), "        ");
        assert_eq!(segment(&buffer, right - 10..right - 8, y), "  ");
        assert_eq!(buffer[(right - 11, y)].symbol(), "…");
        let (prompt_x, prompt_y) = event_position(&buffer, "Equal timestamp prompt");
        assert_eq!(x, prompt_x);
        assert_eq!(buffer[(right - 11, prompt_y)].symbol(), "…");
        assert!(segment(&buffer, right - 8..right, prompt_y).contains(':'));
        let (_, latest_y) = event_position(&buffer, "Following message");
        assert!(
            !segment(&buffer, right - 8..right, latest_y)
                .trim()
                .is_empty()
        );
    }
}

#[test]
fn event_selection_cursor_fills_padding_through_app() {
    for width in [120, 180] {
        let (_directory, mut app) = fixture();
        select_archive(&mut app);
        let buffer = draw(&mut app, width);
        assert_event_cursor(&buffer, event_position(&buffer, "对").1);
    }
}

#[test]
fn event_selection_wheel_updates_detail_and_cursor_through_app() {
    for (width, height) in [(120, 45), (180, 40)] {
        let (_directory, mut app) = fixture();
        select_archive(&mut app);
        let buffer = draw_size(&mut app, width, height);
        let (x, newest) = event_position(&buffer, "对");
        assert!(
            event_position(&buffer, "⏺").1 < newest,
            "older rows are above latest"
        );
        wheel(&mut app, MouseEventKind::ScrollUp, x, newest);
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("codex thread"));
        let (_, older) = event_position(&buffer, "⏺");
        assert_event_cursor(&buffer, older);
        assert!(!lines(&buffer).join("\n").contains("↓ Latest"));
        wheel(&mut app, MouseEventKind::ScrollDown, x, older);
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("decision"));
        assert!(!lines(&buffer).join("\n").contains("↓ Latest"));
    }
}

#[test]
fn event_selection_follows_appends_and_jump_resumes_tail_through_app() {
    for (width, height) in [(120, 45), (180, 40)] {
        let (_directory, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        let payload = include_str!("../tests/fixtures/transcript.jsonl");
        let first = payload.lines().next().unwrap();
        std::fs::write(&path, format!("{first}\n")).unwrap();
        app.refresh();
        select_archive(&mut app);
        draw_size(&mut app, width, height);
        std::fs::write(&path, payload).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("decision"));
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("codex thread"));
        assert_event_cursor(&buffer, event_position(&buffer, "⏺").1);
        assert!(!lines(&buffer).join("\n").contains("↓ Latest"));
        let mut arrival: serde_json::Value = serde_json::from_str(first).unwrap();
        arrival["timestamp"] = "2026-10-06T16:40:00Z".into();
        arrival["uuid"] = "tail-arrival".into();
        arrival["message"]["content"] = format!(
            "tail-arrival-proof {}",
            arrival["message"]["content"].as_str().unwrap()
        )
        .into();
        std::fs::write(&path, format!("{payload}{arrival}\n")).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(!event_detail_content(&buffer).contains("tail-arrival-proof"));
        assert!(event_detail_content(&buffer).contains("codex thread"));
        assert_event_cursor(&buffer, y);
        let (x, y) = position(&buffer, "↓ 1 New");
        click(&mut app, x, y);
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("tail-arrival-proof"));
        assert_event_cursor(&buffer, event_position(&buffer, "tail-arrival-proof").1);
        assert!(!lines(&buffer).join("\n").contains("↓ 1 New"));
        let previous = arrival.to_string();
        arrival["uuid"] = "tail-following".into();
        arrival["timestamp"] = "2026-10-06T16:41:00Z".into();
        arrival["message"]["content"] = "tail-following-proof".into();
        std::fs::write(&path, format!("{payload}{previous}\n{arrival}\n")).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(event_detail_content(&buffer).contains("tail-following-proof"));
        assert_event_cursor(&buffer, event_position(&buffer, "tail-following-proof").1);
    }
}

#[test]
fn event_selection_loads_older_rows_and_keeps_window_edge_across_appends() {
    // Nineteen rows keep all three panels visible in both layouts.
    for width in [120, 180] {
        let (_directory, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        let mut record: serde_json::Value = serde_json::from_str(
            include_str!("../tests/fixtures/transcript.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let body = record["message"]["content"].as_str().unwrap().to_owned();
        let start =
            chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap()).unwrap();
        let mut payload = String::new();
        for index in 0..40 {
            record["uuid"] = format!("selection-{index}").into();
            record["timestamp"] = (start + chrono::Duration::seconds(index))
                .to_rfc3339()
                .into();
            record["message"]["content"] = format!("event-{index:02} {body}").into();
            payload.push_str(&format!("{record}\n"));
        }
        std::fs::write(&path, &payload).unwrap();
        app.refresh();
        select_archive(&mut app);
        let buffer = draw_size(&mut app, width, 19);
        let (x, y) = event_position(&buffer, "event-39");
        // A burst is dispatched before the next frame, just like the real App loop.
        for _ in 0..12 {
            wheel(&mut app, MouseEventKind::ScrollUp, x, y);
        }
        let buffer = draw_size(&mut app, width, 19);
        assert!(event_detail_content(&buffer).contains("event-27"));
        assert_event_cursor(&buffer, event_position(&buffer, "event-27").1);
        let selected_position = event_position(&buffer, "event-27");
        record["uuid"] = "selection-40".into();
        record["timestamp"] = (start + chrono::Duration::seconds(40)).to_rfc3339().into();
        record["message"]["content"] = format!("event-40 {body}").into();
        payload.push_str(&format!("{record}\n"));
        std::fs::write(&path, &payload).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, 19);
        assert!(event_detail_content(&buffer).contains("event-27"));
        assert_event_cursor(&buffer, event_position(&buffer, "event-27").1);
        assert!(lines(&buffer).join("\n").contains("↓ 1 New"));
        assert_eq!(event_position(&buffer, "event-27"), selected_position);
        for expected in (0..27).rev() {
            wheel(&mut app, MouseEventKind::ScrollUp, x, y);
            let buffer = draw_size(&mut app, width, 19);
            let label = format!("event-{expected:02}");
            assert!(event_detail_content(&buffer).contains(&label));
            assert_event_cursor(&buffer, event_position(&buffer, &label).1);
        }
        wheel(&mut app, MouseEventKind::ScrollUp, x, y);
        assert!(event_detail_content(&draw_size(&mut app, width, 19)).contains("event-00"));
    }
}

#[test]
fn event_selection_switching_tasks_restores_selection_through_app() {
    for width in [120, 180] {
        let (_directory, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        let other = "5165651d-b9e3-458d-8612-f8114e312363";
        std::fs::write(
            path.with_file_name(format!("{other}.jsonl")),
            include_str!("../tests/fixtures/transcript.jsonl")
                .replace("4165651d-b9e3-458d-8612-f8114e312363", other),
        )
        .unwrap();
        app.refresh();
        select_archive(&mut app);
        let first = app.view.selected_id().cloned().unwrap();
        let buffer = draw(&mut app, width);
        let (x, y) = event_position(&buffer, "对");
        wheel(&mut app, MouseEventKind::ScrollUp, x, y);
        assert!(event_detail_content(&draw(&mut app, width)).contains("codex thread"));
        click(&mut app, 0, 2);
        key(&mut app, KeyCode::Down);
        let buffer = draw(&mut app, width);
        assert_ne!(app.view.selected_id(), Some(&first));
        assert!(event_detail_content(&buffer).contains("decision"));
        assert_event_cursor(&buffer, event_position(&buffer, "对").1);
        key(&mut app, KeyCode::Up);
        draw(&mut app, width);
        super::event_search_tests::finish_search(&mut app);
        let buffer = draw(&mut app, width);
        assert_eq!(app.view.selected_id(), Some(&first));
        assert!(event_detail_content(&buffer).contains("codex thread"));
        assert_event_cursor(&buffer, event_position(&buffer, "Stop hook error").1);
    }
}

#[test]
fn selected_real_transcript_draws_event_and_prompt_at_medium_and_large_sizes() {
    let (_directory, mut app) = fixture();
    assert!(app.view.selected_id().is_none());
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Home);
    assert!(app.view.selected_id().is_some());
    for width in [80, 120, 150, 200] {
        let buffer = draw(&mut app, width);
        assert!(
            app.transcript_stamp.is_some(),
            "draw must read the selected transcript"
        );
        let rendered = lines(&buffer).join("\n");
        assert!(rendered.contains("Stop hook error"), "{rendered}");
        assert!(rendered.contains("codex-manager"), "{rendered}");
        let (x, y) = position(&buffer, "❯");
        click(&mut app, x, y);
        assert!(!app.handle_event(Event::Paste("ui-route-proof".into())));
        let rendered = lines(&draw(&mut app, width)).join("\n");
        assert!(rendered.contains("ui-route-proof"), "{rendered}");
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn draft_without_transcript_draws_and_accepts_paste_in_large_layout() {
    let (directory, mut app) = fixture();
    for width in [150, 200] {
        app.handle_action(Some(Action::NewDraft {
            cwd: directory.path().to_owned(),
        }));
        let buffer = draw(&mut app, width);
        let rendered = lines(&buffer).join("\n");
        assert!(rendered.contains("New task"), "{rendered}");
        assert!(!app.handle_event(Event::Paste("draft".into())));
        let rendered = lines(&draw(&mut app, width)).join("\n");
        assert!(rendered.contains("draft"), "{rendered}");
        key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn paste_reaches_search_and_rename_only_after_their_keyboard_actions() {
    let (_directory, mut app) = fixture();
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Char('/'));
    app.handle_event(Event::Paste("search-route-proof".into()));
    let rendered = lines(&draw(&mut app, 150)).join("\n");
    assert!(rendered.contains("search-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('r'));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    app.handle_event(Event::Paste("name".into()));
    let rendered = lines(&draw(&mut app, 150)).join("\n");
    assert!(rendered.contains("name"), "{rendered}");
    assert!(!rendered.contains("search-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
}

#[test]
fn rendered_draft_controls_apply_model_and_cancel_effort_through_app() {
    let (directory, mut app) = fixture();
    app.handle_action(Some(Action::NewDraft {
        cwd: directory.path().to_owned(),
    }));
    let buffer = draw(&mut app, 180);
    let ((x, y), _) = row_ends(&buffer, "Opus 5.5");
    click(&mut app, x, y);
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "Haiku 5.5");
    click(&mut app, x, y);
    click(&mut app, x, y);
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("╴Haiku 5.5 "), "{rendered}");
    let buffer = draw(&mut app, 180);
    let (_, (x, y)) = row_ends(&buffer, "medium");
    click(&mut app, x, y);
    let selector = lines(&draw(&mut app, 180)).join("\n");
    assert!(selector.contains("xhigh"), "{selector}");
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("╴Haiku 5.5 "), "{rendered}");
    assert!(!rendered.contains("xhigh"), "{rendered}");
}

#[test]
fn rendered_new_branch_name_accepts_paste_and_escape_cancels_without_worktree() {
    let (directory, mut app) = fixture_with_double_click_ms(20);
    let repository = directory.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
    for arguments in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "probe",
        ],
    ] {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&repository)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    app.handle_action(Some(Action::NewDraft {
        cwd: repository.clone(),
    }));
    draw(&mut app, 180);
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    key(&mut app, KeyCode::Char('k'));
    app.handle_event(Event::Paste("branch flow task".into()));
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter);
    let buffer = draw(&mut app, 180);
    let rendered = lines(&buffer).join("\n");
    assert!(rendered.contains("Create new branch"), "{rendered}");
    let (x, y) = position(&buffer, "Create new branch");
    click(&mut app, x, y);
    wait_for_click(&mut app);
    assert!(app.view.pending_click_in(Instant::now()).is_none());
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "New branch ❯ ");
    app.handle_event(Event::Paste("feature/paste-route-proof".into()));
    let buffer = draw(&mut app, 180);
    assert_eq!(
        position(&buffer, "New branch ❯ feature/paste-route-proof"),
        (x, y)
    );
    assert_eq!(buffer[(x - 1, y)].symbol(), " ");
    assert_eq!(buffer[(x - 1, y)].bg, ratatui::style::Color::Reset);
    key(&mut app, KeyCode::Esc);
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains(" main╶"), "{rendered}");
    assert!(rendered.contains("branch flow task ❯"), "{rendered}");
    assert!(rendered.contains("Create new branch"), "{rendered}");
    assert!(!rendered.contains("New branch ❯"), "{rendered}");
    assert!(!rendered.contains("paste-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("branch flow task"), "{rendered}");
    assert!(!rendered.contains("Create new branch"), "{rendered}");
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    key(&mut app, KeyCode::Char('j'));
    app.handle_event(Event::Paste("saved prompt".into()));
    key(&mut app, KeyCode::Esc);
    assert!(
        lines(&draw(&mut app, 180))
            .join("\n")
            .contains("Permanently delete")
    );
    key(&mut app, KeyCode::Esc);
    assert!(
        lines(&draw(&mut app, 180))
            .join("\n")
            .contains("saved prompt")
    );
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(output.status.success());
    let worktrees = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        worktrees
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
}

fn select_archive(app: &mut App) {
    key(app, KeyCode::BackTab);
    key(app, KeyCode::Home);
    assert!(app.view.selected_id().is_some());
}

fn wait_for_click(app: &mut App) {
    let mut schedule = RefreshSchedule::new(Instant::now(), Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(1);
    while app.view.pending_click_in(Instant::now()).is_some() {
        assert!(
            Instant::now() < deadline,
            "event loop did not flush its click"
        );
        assert!(
            !app.wait_for_input(&mut None, &mut schedule, Instant::now())
                .unwrap()
        );
    }
}

fn event_detail_content(buffer: &ratatui::buffer::Buffer) -> String {
    let rendered = lines(buffer);
    let bottom = rendered.len() - 2;
    let separator = rendered[..bottom]
        .iter()
        .rposition(|line| line.contains("────"))
        .unwrap();
    rendered[separator + 1..bottom].join("\n")
}

#[test]
fn medium_and_large_preview_follows_real_appends_and_clicked_event_stays_open() {
    for (width, height) in [(120, 60), (100, 45), (180, 40)] {
        let (_directory, mut app) = fixture_with_double_click_ms(20);
        let path = app.sessions[0].transcript.clone().unwrap();
        let payload = include_str!("../tests/fixtures/transcript.jsonl");
        let records: Vec<_> = payload.split_inclusive('\n').collect();
        std::fs::write(&path, records[0]).unwrap();
        app.refresh();
        select_archive(&mut app);
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("codex thread"),
            "{}",
            lines(&buffer).join("\n")
        );
        std::fs::write(&path, payload).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("decision"),
            "{}",
            lines(&buffer).join("\n")
        );
        // Click the actual User event in the list, not the preview.
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("codex thread"),
            "{}",
            lines(&buffer).join("\n")
        );
        // Re-read a growing real transcript while the earlier event is pinned.
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(records[1].as_bytes())
            .unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("codex thread"),
            "{}",
            lines(&buffer).join("\n")
        );
        // Another list row replaces the pinned event through the same click deadline.
        let (x, y) = event_position(&buffer, "对");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("decision"),
            "{}",
            lines(&buffer).join("\n")
        );
    }
}

#[test]
fn medium_and_large_event_double_click_focuses_list_and_keeps_preview_following() {
    for (width, height) in [(120, 60), (100, 45), (180, 40)] {
        let (_directory, mut app) = fixture();
        let path = app.sessions[0].transcript.clone().unwrap();
        let payload = include_str!("../tests/fixtures/transcript.jsonl");
        let records: Vec<_> = payload.split_inclusive('\n').collect();
        std::fs::write(&path, records[0]).unwrap();
        app.refresh();
        select_archive(&mut app);
        let buffer = draw_size(&mut app, width, height);
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        click(&mut app, x, y);
        key(&mut app, KeyCode::Char('/'));
        app.handle_event(Event::Paste("double-click-list-proof".into()));
        let buffer = draw_size(&mut app, width, height);
        assert!(
            lines(&buffer)
                .join("\n")
                .contains("Search ❯ double-click-list-proof")
        );
        key(&mut app, KeyCode::Esc);
        std::fs::write(&path, payload).unwrap();
        app.refresh();
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("decision"),
            "{}",
            lines(&buffer).join("\n")
        );
    }
}

#[test]
fn medium_and_large_list_and_detail_search_route_paste_without_changing_preview() {
    for (width, height) in [(120, 60), (100, 45), (180, 40)] {
        let (_directory, mut app) = fixture_with_double_click_ms(20);
        select_archive(&mut app);
        let initial = draw_size(&mut app, width, height);
        let (column, row) = event_position(&initial, "Stop hook");
        click(&mut app, column, row);
        let original = event_detail_content(&draw_size(&mut app, width, height));
        key(&mut app, KeyCode::Char('/'));
        app.handle_event(Event::Paste("Stop hook".into()));
        let buffer = draw_size(&mut app, width, height);
        assert!(lines(&buffer).join("\n").contains("Search ❯ Stop hook"));
        assert_eq!(
            event_detail_content(&buffer),
            original,
            "filtering the list must preserve selected preview"
        );
        key(&mut app, KeyCode::Esc);
        let buffer = draw_size(&mut app, width, height);
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        click(&mut app, 1, height - 3);
        key(&mut app, KeyCode::Char('/'));
        app.handle_event(Event::Paste("await".into()));
        let buffer = draw_size(&mut app, width, height);
        assert!(
            event_detail_content(&buffer).contains("Search ❯ await"),
            "{}",
            lines(&buffer).join("\n")
        );
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('?'));
        app.handle_event(Event::Paste("must-not-enter-help".into()));
        let buffer = draw_size(&mut app, width, height);
        assert!(!lines(&buffer).join("\n").contains("must-not-enter-help"));
        key(&mut app, KeyCode::Esc);
        click(&mut app, 1, 2);
        key(&mut app, KeyCode::Backspace);
        app.handle_event(Event::Paste("must-not-enter-confirmation".into()));
        let buffer = draw_size(&mut app, width, height);
        let rendered = lines(&buffer).join("\n");
        assert!(rendered.contains("Permanently delete"), "{rendered}");
        assert!(!rendered.contains("must-not-enter-confirmation"));
        key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn medium_real_transcript_keeps_three_panels_and_click_opens_event_detail() {
    for (width, height, expected) in [(120, 60, [18, 18, 17]), (100, 45, [13, 13, 12])] {
        let (_directory, mut app) = fixture_with_double_click_ms(20);
        select_archive(&mut app);
        let buffer = draw_size(&mut app, width, height);
        let prompt = position(&buffer, "❯").1;
        let middle = prompt - 1;
        let list_end = event_list_end(&buffer, prompt);
        let heights = [
            middle - 2,
            list_end.saturating_sub(prompt + 2),
            height.saturating_sub(2 + list_end),
        ];
        assert_eq!(
            heights,
            expected,
            "{width}x{height}:\n{}",
            lines(&buffer).join("\n")
        );
        assert!(event_detail_content(&buffer).contains("decision"));
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        let opened = draw_size(&mut app, width, height);
        assert!(event_detail_content(&opened).contains("codex thread"));
        assert_eq!(
            lines(&opened)[usize::from(prompt + 2)..usize::from(list_end)],
            lines(&buffer)[usize::from(prompt + 2)..usize::from(list_end)],
            "opening an event must preserve the visible event list"
        );
        let (x, y) = event_position(&opened, "对");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        let changed = draw_size(&mut app, width, height);
        assert!(event_detail_content(&changed).contains("decision"));
        assert!(event_position(&changed, "⏺").1 < list_end);
    }
}

#[test]
fn medium_and_large_event_panels_scroll_and_copy_independently() {
    // The large layout detail must stay one row short of its event so that the wheel has room to scroll.
    for (width, height) in [(120, 30), (100, 30), (180, 23)] {
        let (_directory, mut app) = border_fixture();
        app.view.set_double_click_ms(20);
        let buffer = draw_size(&mut app, width, height);
        let (x, y) = event_position(&buffer, "⏺");
        click(&mut app, x, y);
        wait_for_click(&mut app);
        let buffer = draw_size(&mut app, width, height);
        let (text_x, text_y) = position(&buffer, "Stop hook error");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
        ] {
            let column = text_x
                + if matches!(kind, MouseEventKind::Drag(_)) {
                    9
                } else {
                    0
                };
            app.view.handle_mouse(MouseEvent {
                kind,
                column,
                row: text_y,
                modifiers: KeyModifiers::NONE,
            });
        }
        assert_eq!(
            app.view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: text_x + 9,
                row: text_y,
                modifiers: KeyModifiers::NONE,
            }),
            Some(Action::Copy {
                text: "Stop hook".into(),
                label: "Copied message".into()
            }),
            "copy the real event body at {width}x{height}"
        );
        let before = event_detail_content(&buffer);
        for (kind, column, row) in [
            (MouseEventKind::ScrollUp, x, y),
            (MouseEventKind::ScrollDown, text_x, text_y),
        ] {
            app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }));
            draw_size(&mut app, width, height);
        }
        let scrolled = draw_size(&mut app, width, height);
        assert_ne!(event_detail_content(&scrolled), before);
        assert!(!lines(&scrolled).join("\n").contains("↓ Latest"));
        assert!(!lines(&scrolled).join("\n").contains("↑ Top"));
        click(&mut app, x, y);
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        let latest = draw_size(&mut app, width, height);
        assert!(!lines(&latest).join("\n").contains("↓ Latest"));
        assert_eq!(
            event_detail_content(&latest)
                .lines()
                .skip(1)
                .collect::<Vec<_>>(),
            before.lines().skip(1).collect::<Vec<_>>()
        );
    }
}

#[test]
fn new_key_without_selection_prepares_the_startup_repository() {
    let (directory, mut app) = fixture();
    let branch = "workstream/branch-name-tests";
    let output = Command::new("git")
        .args(["init", "-b", branch])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    app.view.startup_cwd = directory.path().to_owned();
    assert!(app.view.selected_id().is_none());
    key(&mut app, KeyCode::Char('n'));
    let buffer = draw(&mut app, 240);
    let rendered = lines(&buffer).join("\n");
    assert!(rendered.contains("New task"), "{rendered}");
    let (title_x, title_y) = position(&buffer, "New task");
    let separators: Vec<_> = (0..buffer.area.width - 1)
        .filter(|&x| buffer[(x, 2)].symbol() == "│")
        .collect();
    assert_eq!(separators.len(), 2);
    let rule_start = separators[0];
    let rule_end = separators[1] + 1;
    assert_eq!(title_x, rule_start + (rule_end - rule_start - 10) / 2 + 1);
    assert!(segment(&buffer, title_x + 8..rule_end, title_y).contains("workstream/"));
    // A wider panel can show the complete branch alongside the centered title.
    let rendered = lines(&draw(&mut app, 360)).join("\n");
    assert!(
        rendered.contains(&format!(" {branch}╶")),
        "startup repository was not prepared:\n{rendered}"
    );
}

// Read a real prompt through the provider; move its timestamp into the visible
// timeline so activity does not depend on when this test is run.
fn border_fixture() -> (tempfile::TempDir, App) {
    let (directory, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let mut prompt: serde_json::Value = serde_json::from_str(
        include_str!("../tests/fixtures/transcript.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    prompt["timestamp"] = (chrono::Utc::now() - chrono::Duration::seconds(120))
        .to_rfc3339()
        .into();
    let mut transcript = String::new();
    for index in 0..40 {
        let mut record = prompt.clone();
        record["uuid"] = format!("border-prompt-{index}").into();
        record["timestamp"] =
            (chrono::DateTime::parse_from_rfc3339(prompt["timestamp"].as_str().unwrap()).unwrap()
                + chrono::Duration::milliseconds(index))
            .to_rfc3339()
            .into();
        transcript.push_str(&format!("{record}\n"));
    }
    std::fs::write(path, transcript).unwrap();
    app.refresh();
    app.sessions[0].status = crate::model::SessionStatus::Working;
    app.sessions[0].started_at = Some(
        chrono::DateTime::parse_from_rfc3339(prompt["timestamp"].as_str().unwrap())
            .unwrap()
            .to_utc(),
    );
    app.view.set_sessions(app.sessions.clone(), false);
    key(&mut app, KeyCode::Home);
    (directory, app)
}

fn segment(buffer: &ratatui::buffer::Buffer, x: std::ops::Range<u16>, y: u16) -> String {
    x.map(|x| buffer[(x, y)].symbol()).collect()
}

#[test]
fn large_border_labels_stay_in_their_columns() {
    let (_directory, mut app) = border_fixture();
    let now = app.sessions[0].started_at.unwrap() + chrono::Duration::seconds(120);
    for width in [150, 200] {
        let buffer = draw_size_at(&mut app, width, 40, now);
        let rendered = lines(&buffer);
        let separators: Vec<_> = (0..width - 1)
            .filter(|&x| buffer[(x, 2)].symbol() == "│")
            .collect();
        assert_eq!(separators.len(), 2);
        let prompt = position(&buffer, "❯").1;
        let middle = prompt - 1;
        let tasks = 1..separators[0] - 1;
        let task_details = separators[0] + 2..separators[1] - 1;
        let event_panel = separators[1] + 2..width - 1;
        assert!(
            segment(&buffer, tasks.clone(), middle).contains("Opus 5.5"),
            "model belongs to the task list's borrowed bottom edge:\n{}",
            rendered.join("\n")
        );
        let bottom = segment(&buffer, task_details.clone(), middle);
        // The fixture's recorded prompt cache has long expired, so ⏸︎ follows the status.
        let start = (task_details.len() - 11) / 2;
        assert_eq!(
            segment(
                &buffer,
                task_details.start + start as u16..task_details.start + start as u16 + 11,
                1
            ),
            " Working ⏸\u{fe0e} "
        );
        assert!(!bottom.contains("2m 0s"));
        assert!(!bottom.contains("Working"));
        assert!(segment(&buffer, task_details.clone(), 1).ends_with(" 2m 0s"));
        let title_prefix: String = app.sessions[0].display_title().chars().take(8).collect();
        assert!(bottom.contains(&title_prefix));
        assert!(!segment(&buffer, task_details.clone(), 1).contains(&title_prefix));
        assert!(segment(&buffer, tasks.clone(), 1).chars().all(|c| c == '─'));
        assert!(
            segment(&buffer, event_panel.clone(), 1)
                .chars()
                .all(|c| c == '─')
        );
        for y in 2..middle {
            for range in [task_details.clone(), event_panel.clone()] {
                let text = segment(&buffer, range, y);
                assert!(
                    !text.contains("Working ·") && !text.contains("────"),
                    "extra label/rule row at {y}: {text}"
                );
            }
        }
        // Scrolling without arrivals leaves the borders free of jump buttons.
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: event_panel.start,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        let buffer = draw(&mut app, width);
        assert!(!segment(&buffer, event_panel.clone(), middle).contains("↓ Latest"));
        assert!(!segment(&buffer, event_panel.clone(), 1).contains("↓ Latest"));
        for y in 2..middle {
            assert!(!segment(&buffer, event_panel.clone(), y).contains("↓ Latest"));
        }
        click(&mut app, event_panel.start + 1, middle);
        click(&mut app, event_panel.start + 1, middle);
        assert!(
            !lines(&draw(&mut app, width))
                .join("\n")
                .contains("↓ Latest")
        );
    }
}

#[test]
fn medium_render_survives_large_resize_unchanged() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (_directory, mut app) = fixture();
    select_archive(&mut app);
    let now = chrono::Utc::now();
    let mut draw = |width| {
        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
        app.draw_at(&mut terminal, now).unwrap();
        terminal.backend().buffer().clone()
    };
    for width in [100, 120, 149] {
        let before = draw(width);
        draw(200);
        let after = draw(width);
        let changed: Vec<_> = before
            .content
            .iter()
            .zip(&after.content)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(index, (a, b))| (index % width as usize, index / width as usize, a, b))
            .take(12)
            .collect();
        assert!(changed.is_empty(), "resize changed cells: {changed:?}");
    }
}

#[test]
fn event_rows_keep_fixed_content_and_right_aligned_times() {
    use chrono::TimeZone;
    let (_directory, mut app) = fixture();
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Home);
    draw(&mut app, 100);
    let source = crate::providers::claude::event_reader::tail_session(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl"),
        200,
        true,
    )
    .unwrap()
    .into_iter()
    .find(|event| event.kind == crate::event::Kind::Tool)
    .unwrap();
    let at = chrono::Local
        .with_ymd_and_hms(2026, 10, 12, 10, 6, 26)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let events = [2, 590, 0]
        .into_iter()
        .map(|seconds| {
            let mut event = source.clone();
            event.body = "long body ".repeat(40);
            event.calls.clear();
            event.paths.clear();
            event.started_at = at;
            event.ended_at = Some(at);
            event.previous_end = (seconds > 0).then_some(at - chrono::Duration::seconds(seconds));
            event
        })
        .collect::<Vec<_>>();
    for width in [100, 149, 150, 200] {
        draw(&mut app, width);
        app.view.set_event(events.clone());
        let buffer = draw(&mut app, width);
        let (x, top) = event_position(&buffer, "long body");
        let right = width - 2;
        assert_eq!(
            x,
            if width < 150 {
                1
            } else {
                2 * ((width - 8) / 4) + 7
            }
        );
        for (offset, time) in ["10:06:26", "9m 50s", "2s"].into_iter().enumerate() {
            let y = top - offset as u16;
            let clock: String = (right - 8..right)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            assert_eq!(clock, format!("{time:>8}"));
            assert_eq!(buffer[(right - 9, y)].symbol(), " ");
            assert_eq!(buffer[(right - 10, y)].symbol(), " ");
            assert_eq!(buffer[(right - 11, y)].symbol(), "…");
            assert_eq!(buffer[(x, y)].symbol(), "l");
        }
    }
}

#[path = "app_task_details_tests.rs"]
mod task_details_alignment;

#[test]
fn app_event_padding_and_clicks_follow_real_transcript_rows() {
    for width in [149, 200] {
        for edge in [false, true] {
            let (_directory, mut app) = fixture_with_double_click_ms(20);
            select_archive(&mut app);
            let buffer = draw(&mut app, width);
            let separators: Vec<_> = (0..width - 1)
                .filter(|&x| buffer[(x, 2)].symbol() == "│")
                .collect();
            let left = if width == 200 { separators[1] + 2 } else { 1 };
            let (x, y) = event_position(&buffer, "⏺ Ran 4 stop hooks");
            assert_eq!(x, left);
            assert_eq!(
                (left..width - 1).rfind(|&x| buffer[(x, y)].symbol() != " "),
                Some(width - 3)
            );
            click(&mut app, if edge { width - 2 } else { left }, y);
            wait_for_click(&mut app);
            let buffer = draw(&mut app, width);
            let (event_detail_x, event_detail_y) = position(&buffer, "User      ");
            let (_, prompt_y) = position(&buffer, "❯");
            assert!(
                event_detail_y > prompt_y,
                "clicked event must open below the prompt"
            );
            assert_eq!(event_detail_x, width / 2 - "User".len() as u16 / 2);
            assert_eq!(buffer[(width - 1, event_detail_y)].symbol(), "▕");
            assert_eq!(
                (1..width - 1).rfind(|&x| buffer[(x, event_detail_y)].symbol() != " "),
                Some(width - 3)
            );
            assert!(
                lines(&buffer)[usize::from(event_detail_y)..]
                    .join("\n")
                    .contains("⏺ Ran 4 stop hooks")
            );
        }
    }
}

fn tab_fixture(count: usize) -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    for index in 0..count {
        let path = project.join(format!("00000000-0000-0000-0000-{index:012}.jsonl"));
        std::fs::write(&path, include_bytes!("../tests/fixtures/transcript.jsonl")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(index as u64))
            .unwrap();
    }
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().into()),
        )]),
        directory.path().into(),
    );
    app.refresh();
    (directory, app)
}

#[test]
fn task_tabs_only_active_and_inactive_with_loaded_counts() {
    let (_directory, mut app) = tab_fixture(23);
    let rendered = lines(&draw(&mut app, 160)).join("\n");
    assert!(rendered.contains("Active 0"));
    assert!(rendered.contains("Inactive 10"), "{rendered}");
    for removed in ["Ready 0", "Working 0", "Needs you 0"] {
        assert!(!rendered.contains(removed), "{rendered}");
    }
}

#[test]
fn task_tabs_cycle_both_directions() {
    let (_directory, mut app) = tab_fixture(1);
    for (event, inactive) in [
        (KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), true),
        (KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE), false),
        (KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT), true),
        (KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
    ] {
        app.handle_event(Event::Key(event));
        assert_eq!(app.view.selected_id().is_some(), inactive);
    }
}

#[test]
fn task_tabs_inactive_starts_with_ten_then_reveals_next_page() {
    let (_directory, mut app) = tab_fixture(23);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.sessions.len(), 10);
    assert!(
        app.sessions
            .iter()
            .all(|session| session.updated_at.timestamp() >= 13)
    );
    let rendered = lines(&draw(&mut app, 160)).join("\n");
    assert!(rendered.contains("Show more"), "{rendered}");
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Enter);
    app.finish_refreshes();
    assert_eq!(app.sessions.len(), 20);
    assert!(
        lines(&draw(&mut app, 160))
            .join("\n")
            .contains("Inactive 20")
    );
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Enter);
    app.finish_refreshes();
    assert_eq!(app.sessions.len(), 23);
    let rendered = lines(&draw(&mut app, 160)).join("\n");
    assert!(rendered.contains("Inactive 23"));
    assert!(!rendered.contains("Show more"));
}

#[test]
fn task_tabs_ignore_saved_removed_filter() {
    for filter in ["Ready", "Working", "Needs you"] {
        let (directory, _) = tab_fixture(1);
        std::fs::write(
            directory.path().join("agents.json"),
            serde_json::to_vec(&serde_json::json!({
                "grouping": "project", "pinned": {"active": [], "inactive": []}, "status_filter": filter,
            }))
            .unwrap(),
        )
        .unwrap();
        let mut app = App::new(
            Hub::new(vec![Box::new(
                crate::providers::claude::ClaudeProvider::new(directory.path().into()),
            )]),
            directory.path().into(),
        );
        app.refresh();
        assert!(app.view.selected_id().is_none());
        key(&mut app, KeyCode::Tab);
        assert!(app.view.selected_id().is_some());
        app.preferences.save_grouping(&app.config_dir).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.path().join("agents.json")).unwrap())
                .unwrap();
        assert!(saved.get("status_filter").is_none());
    }
}

#[test]
fn incremental_refresh_parses_only_appended_fixture_bytes() {
    use std::io::Write;
    let (_directory, mut app) = fixture();
    app.refresh();
    select_archive(&mut app);
    draw_size(&mut app, 200, 45);
    let path = app.sessions[0].transcript.clone().unwrap();
    let before = app.transcript_parsed_bytes;
    draw_size(&mut app, 200, 45);
    assert_eq!(
        app.transcript_parsed_bytes, before,
        "unchanged file must decode zero bytes"
    );
    let payload = include_str!("../tests/fixtures/claude-task-notifications.jsonl");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    app.refresh();
    draw_size(&mut app, 200, 45);
    assert_eq!(app.transcript_parsed_bytes - before, payload.len() as u64);
}

// A real PTY program isolates selection and shutdown from provider availability.
fn terminal_restore_fixture() -> (tempfile::TempDir, App, SessionKey) {
    let (directory, mut app) = fixture();
    app.sessions[0].status = crate::model::SessionStatus::Working;
    app.view.set_sessions(app.sessions.clone(), false);
    key(&mut app, KeyCode::Home);
    let id = app.view.selected_id().unwrap().clone();
    // Any reopen attempt fails open_terminal's directory validation visibly, without
    // launching a provider client against the user's sessions during a unit test.
    app.sessions[0].cwd = directory.path().join("removed-project");
    draw(&mut app, 200);
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "read line"]);
    let sender = app.sender.clone();
    let terminal = crate::session_terminal::SessionTerminal::spawn(
        id.clone(),
        command,
        None,
        app.view.terminal_size().unwrap(),
        move || {
            let _ = sender.send(Input::TerminalOutput);
        },
    )
    .unwrap();
    app.view.show_terminal(id.clone(), terminal.screen.clone());
    app.terminal = Some(terminal);
    draw(&mut app, 200);
    for direction in ['j', 'j'] {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )));
        key(&mut app, KeyCode::Char(direction));
    }
    (directory, app, id)
}

#[test]
fn terminal_repaint_never_shows_the_hardware_cursor_over_painted_cells() {
    use ratatui::backend::CrosstermBackend;
    use ratatui::{TerminalOptions, Viewport};

    let (_directory, mut app, _id) = terminal_restore_fixture();
    app.terminal
        .as_ref()
        .unwrap()
        .screen
        .lock()
        .unwrap()
        .process(b"READY\x1b[4;6H");
    app.sync_terminal();
    let area = ratatui::layout::Rect::new(0, 0, 100, 45);
    let output = tempfile::NamedTempFile::new().unwrap();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(output.reopen().unwrap()),
        TerminalOptions {
            viewport: Viewport::Fixed(area),
        },
    )
    .unwrap();
    let now = chrono::Utc::now();
    app.draw_at(&mut terminal, now).unwrap();
    let mut hardware = vt100::Parser::new(area.height, area.width, 0);
    let first = std::fs::read(output.path()).unwrap();
    hardware.process(&first);
    assert!(!hardware.screen().hide_cursor());
    let cursor = hardware.screen().cursor_position();

    app.view.show_info("repaint".into());
    app.draw_at(&mut terminal, now).unwrap();
    let repainted = std::fs::read(output.path()).unwrap();
    for byte in &repainted[first.len()..] {
        hardware.process(&[*byte]);
        if !hardware.screen().hide_cursor() {
            assert_eq!(hardware.screen().cursor_position(), cursor);
        }
    }
    assert!(!hardware.screen().hide_cursor());
    assert_eq!(hardware.screen().cursor_position(), cursor);
}

fn terminal_restore_round_trip(app: &mut App, id: &SessionKey) {
    let mut other = app.sessions[0].clone();
    other.id.id = "another-task".into();
    app.view.set_sessions(vec![other], false);
    key(app, KeyCode::Home);
    assert_ne!(app.view.selected_id(), Some(id));
    app.sync_terminal();
    assert!(app.terminal.is_none(), "switching away must close the PTY");
    app.view.set_sessions(app.sessions.clone(), false);
    key(app, KeyCode::Home);
    assert_eq!(app.view.selected_id(), Some(id));
    draw(app, 200);
    app.sync_terminal();
}

#[test]
fn terminal_reopens_when_the_task_is_selected_again() {
    let (_directory, mut app, id) = terminal_restore_fixture();
    terminal_restore_round_trip(&mut app, &id);
    assert!(app.terminal.is_none());
    assert!(
        lines(&draw(&mut app, 200))
            .join("\n")
            .contains("Directory no longer exists:"),
        "returning must retry opening the terminal and report the missing directory"
    );
}

fn archive_codex_fixture() -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    rusqlite::Connection::open(directory.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    let transcript = directory.path().join("rollout.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../tests/fixtures/codex-thread-clock.jsonl"),
    )
    .unwrap();
    rusqlite::Connection::open(directory.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=?1",
            [transcript.to_str().unwrap()],
        )
        .unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        directory.path().into(),
        directory.path().join("manager"),
    );
    let mut app = App::new(Hub::new(vec![Box::new(provider)]), directory.path().into());
    app.refresh();
    (directory, app)
}

#[test]
fn archive_errors_arrive_through_workers_for_both_providers() {
    for (directory, mut app) in [fixture(), archive_codex_fixture()] {
        let target = app
            .sessions
            .iter()
            .find(|s| s.can_resume())
            .unwrap()
            .clone();
        app.sessions = vec![target.clone()];
        app.view.set_sessions(app.sessions.clone(), false);
        key(&mut app, KeyCode::Tab);
        draw(&mut app, 120);
        assert!(!app.handle_action(Some(Action::Archive(target.id.clone()))));
        let event = app
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("archive must report the provider error through a worker");
        let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
        app.handle_inputs(event, &mut None, &mut schedule).unwrap();
        let rendered = lines(&draw(&mut app, 160)).join("\n");
        assert!(rendered.contains("Cannot archive:"), "{rendered}");
        key(&mut app, KeyCode::Home);
        assert!(
            app.view.selected_id().is_some(),
            "failed archive must unhide the task"
        );
        drop(directory);
    }
}

#[test]
fn archive_success_keeps_active_tab_and_releases_hidden_task() {
    let (_directory, mut app) = archive_codex_fixture();
    let mut target = app
        .sessions
        .iter()
        .find(|s| s.can_resume())
        .unwrap()
        .clone();
    // Archiving an already archived Codex thread is a real provider no-op.
    target.archived = true;
    target.status = crate::model::SessionStatus::Ready;
    app.sessions = vec![target.clone()];
    app.view.set_sessions(app.sessions.clone(), false);
    draw(&mut app, 120);
    assert_eq!(app.view.selected_id(), Some(&target.id));
    key(&mut app, KeyCode::Char('a'));
    assert!(!lines(&draw(&mut app, 120)).join("\n").contains("Archive “"));
    assert!(
        app.view.selected_id().is_none(),
        "archive must move selection before refreshing"
    );
    let event = app.receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    app.finish_refreshes();
    draw(&mut app, 120);
    assert!(
        app.view.selected_id().is_none(),
        "must stay on empty Active tab"
    );
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Home);
    let mut found = false;
    for _ in 0..app.sessions.len() {
        found |= app.view.selected_id() == Some(&target.id);
        key(&mut app, KeyCode::Down);
    }
    assert!(found, "archived task must remain available on Inactive");
}
