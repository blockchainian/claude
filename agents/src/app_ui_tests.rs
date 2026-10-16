use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;

fn fixture() -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../tests/fixtures/transcript.jsonl"),
    )
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
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    terminal.backend().buffer().clone()
}

fn lines(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn position(buffer: &ratatui::buffer::Buffer, text: &str) -> (u16, u16) {
    for y in (0..buffer.area.height).rev() {
        for x in 0..buffer.area.width {
            let suffix: String = (x..buffer.area.width)
                .map(|column| buffer[(column, y)].symbol())
                .collect();
            if suffix.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("missing {text:?}:\n{}", lines(buffer).join("\n"));
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

#[test]
fn selected_real_transcript_draws_log_and_prompt_at_narrow_and_wide_sizes() {
    let (_directory, mut app) = fixture();
    assert!(app.view.selected_id().is_none());
    for _ in 0..4 {
        key(&mut app, KeyCode::Tab);
    }
    key(&mut app, KeyCode::Home);
    assert!(app.view.selected_id().is_some());
    for width in [80, 120, 150, 200] {
        let buffer = draw(&mut app, width);
        assert!(
            app.log_stamp.is_some(),
            "draw must read the selected transcript"
        );
        let rendered = lines(&buffer).join("\n");
        assert!(rendered.contains("Stop hook error"), "{rendered}");
        assert!(rendered.contains("Assistant"), "{rendered}");
        let (x, y) = position(&buffer, "›");
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
fn draft_without_transcript_draws_and_accepts_paste_in_wide_layout() {
    let (directory, mut app) = fixture();
    app.handle_action(Some(Action::NewDraft {
        cwd: directory.path().to_owned(),
    }));
    for width in [150, 200] {
        let buffer = draw(&mut app, width);
        let rendered = lines(&buffer).join("\n");
        assert!(rendered.contains("New task"), "{rendered}");
        assert!(!app.handle_event(Event::Paste("draft-route-proof".into())));
        let rendered = lines(&draw(&mut app, width)).join("\n");
        assert!(rendered.contains("draft-route-proof"), "{rendered}");
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
    }
}

#[test]
fn paste_reaches_search_and_rename_only_after_their_keyboard_actions() {
    let (_directory, mut app) = fixture();
    for _ in 0..4 {
        key(&mut app, KeyCode::Tab);
    }
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Char('/'));
    app.handle_event(Event::Paste("search-route-proof".into()));
    let rendered = lines(&draw(&mut app, 150)).join("\n");
    assert!(rendered.contains("search-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('r'));
    app.handle_event(Event::Paste("rename-route-proof".into()));
    let rendered = lines(&draw(&mut app, 150)).join("\n");
    assert!(rendered.contains("rename-route-proof"), "{rendered}");
    assert!(!rendered.contains("search-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
}

#[test]
fn rendered_draft_controls_apply_model_and_cancel_effort_through_app() {
    let (directory, mut app) = fixture();
    app.handle_action(Some(Action::NewDraft {
        cwd: directory.path().to_owned(),
    }));
    key(&mut app, KeyCode::Esc);
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "Model:");
    click(&mut app, x + 7, y);
    click(&mut app, x + 7, y);
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "haiku");
    click(&mut app, x, y);
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("Model: haiku"), "{rendered}");
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "Effort:");
    click(&mut app, x + 8, y);
    click(&mut app, x + 8, y);
    let picker = lines(&draw(&mut app, 180)).join("\n");
    assert!(picker.contains("high"), "{picker}");
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("Model: haiku"), "{rendered}");
    assert!(!rendered.contains("› high"), "{rendered}");
}

#[test]
fn rendered_branch_editor_accepts_paste_and_escape_cancels_without_worktree() {
    let (directory, mut app) = fixture();
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
    key(&mut app, KeyCode::Esc);
    let buffer = draw(&mut app, 180);
    let (x, y) = position(&buffer, "Branch:");
    click(&mut app, x + 8, y);
    click(&mut app, x + 8, y);
    app.handle_event(Event::Paste("feature/paste-route-proof".into()));
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("feature/paste-route-proof"), "{rendered}");
    key(&mut app, KeyCode::Esc);
    let rendered = lines(&draw(&mut app, 180)).join("\n");
    assert!(rendered.contains("Branch: main"), "{rendered}");
    assert!(
        !rendered.contains("feature/paste-route-proof"),
        "{rendered}"
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

fn detail(buffer: &ratatui::buffer::Buffer) -> String {
    let rendered = lines(buffer);
    let separator = rendered
        .iter()
        .rposition(|line| line.contains("────"))
        .unwrap();
    rendered[separator + 1..rendered.len() - 1].join("\n")
}

#[test]
fn wide_preview_follows_real_appends_and_clicked_event_stays_open() {
    let (_directory, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = include_str!("../tests/fixtures/transcript.jsonl");
    let records: Vec<_> = payload.split_inclusive('\n').collect();
    std::fs::write(&path, records[0]).unwrap();
    app.refresh();
    select_archive(&mut app);
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("run_in_background"),
        "{}",
        lines(&buffer).join("\n")
    );
    std::fs::write(&path, payload).unwrap();
    app.refresh();
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("decision"),
        "{}",
        lines(&buffer).join("\n")
    );
    // Click the actual User event in the right-hand event list, not the preview.
    let (x, y) = lines(&buffer)
        .iter()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("User").filter(|_| y < 20).map(|_| {
                let x = (110..buffer.area.width)
                    .find(|&x| buffer[(x, y as u16)].symbol() == "U")
                    .unwrap();
                (x, y as u16)
            })
        })
        .expect("real user event row");
    click(&mut app, x, y);
    wait_for_click(&mut app);
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("run_in_background"),
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
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("run_in_background"),
        "{}",
        lines(&buffer).join("\n")
    );
    // Another list row replaces the pinned event through the same click deadline.
    let (x, y) = lines(&buffer)
        .iter()
        .enumerate()
        .find_map(|(y, line)| {
            if y >= 20 || !line.contains("Assistant") {
                return None;
            }
            (110..buffer.area.width)
                .find(|&x| buffer[(x, y as u16)].symbol() == "A")
                .map(|x| (x, y as u16))
        })
        .expect("real assistant event row");
    click(&mut app, x, y);
    wait_for_click(&mut app);
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("decision"),
        "{}",
        lines(&buffer).join("\n")
    );
}

#[test]
fn wide_log_double_click_focuses_prompt_and_keeps_preview_following() {
    let (_directory, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let payload = include_str!("../tests/fixtures/transcript.jsonl");
    let records: Vec<_> = payload.split_inclusive('\n').collect();
    std::fs::write(&path, records[0]).unwrap();
    app.refresh();
    select_archive(&mut app);
    let buffer = draw(&mut app, 180);
    let y = lines(&buffer)
        .iter()
        .position(|line| line.contains("User") && line.contains('│'))
        .unwrap() as u16;
    let x = (110..buffer.area.width)
        .find(|&x| buffer[(x, y)].symbol() == "U")
        .unwrap();
    click(&mut app, x, y);
    click(&mut app, x, y);
    app.handle_event(Event::Paste("double-click-prompt-proof".into()));
    let buffer = draw(&mut app, 180);
    assert!(
        lines(&buffer)
            .join("\n")
            .contains("double-click-prompt-proof")
    );
    std::fs::write(&path, payload).unwrap();
    app.refresh();
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("decision"),
        "{}",
        lines(&buffer).join("\n")
    );
}

#[test]
fn wide_list_and_detail_search_route_paste_without_changing_preview() {
    let (_directory, mut app) = fixture();
    select_archive(&mut app);
    let original = detail(&draw(&mut app, 180));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    app.handle_event(Event::Paste("Stop hook".into()));
    let buffer = draw(&mut app, 180);
    assert!(lines(&buffer).join("\n").contains("Search › Stop hook"));
    assert_eq!(
        detail(&buffer),
        original,
        "filtering the list must preserve newest preview"
    );
    key(&mut app, KeyCode::Esc);
    let buffer = draw(&mut app, 180);
    let y = lines(&buffer)
        .iter()
        .position(|line| line.contains("User") && line.contains('│'))
        .unwrap() as u16;
    let x = (110..buffer.area.width)
        .find(|&x| buffer[(x, y)].symbol() == "U")
        .unwrap();
    click(&mut app, x, y);
    wait_for_click(&mut app);
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('r'),
        KeyModifiers::CONTROL,
    )));
    app.handle_event(Event::Paste("await".into()));
    let buffer = draw(&mut app, 180);
    assert!(
        detail(&buffer).contains("Search › await"),
        "{}",
        lines(&buffer).join("\n")
    );
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('?'));
    app.handle_event(Event::Paste("must-not-enter-help".into()));
    let buffer = draw(&mut app, 180);
    assert!(!lines(&buffer).join("\n").contains("must-not-enter-help"));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Backspace);
    app.handle_event(Event::Paste("must-not-enter-confirmation".into()));
    let buffer = draw(&mut app, 180);
    let rendered = lines(&buffer).join("\n");
    assert!(rendered.contains("Permanently delete"), "{rendered}");
    assert!(!rendered.contains("must-not-enter-confirmation"));
    key(&mut app, KeyCode::Esc);
}

#[test]
fn new_key_without_selection_prepares_the_startup_repository() {
    let (_directory, mut app) = fixture();
    assert!(app.view.selected_id().is_none());
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let branch = String::from_utf8(output.stdout).unwrap();
    assert!(
        !branch.trim().is_empty(),
        "test requires a checked-out branch"
    );
    key(&mut app, KeyCode::Char('n'));
    let buffer = draw(&mut app, 240);
    let rendered = lines(&buffer).join("\n");
    assert!(rendered.contains("New task"), "{rendered}");
    assert!(
        rendered.contains(&format!("Branch: {}", branch.trim())),
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
fn wide_border_labels_stay_in_their_columns() {
    let (_directory, mut app) = border_fixture();
    for width in [150, 200] {
        let buffer = draw(&mut app, width);
        let rendered = lines(&buffer);
        let separators: Vec<_> = (0..width)
            .filter(|&x| buffer[(x, 3)].symbol() == "│")
            .collect();
        assert_eq!(separators.len(), 2);
        let prompt = position(&buffer, "›").1;
        let middle = prompt - 1;
        let tasks = 1..separators[0] - 1;
        let details = separators[0] + 2..separators[1] - 1;
        let log = separators[1] + 2..width - 1;
        assert!(
            segment(&buffer, tasks.clone(), middle)
                .chars()
                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
            "activity missing on middle border:\n{}",
            rendered.join("\n")
        );
        assert!(segment(&buffer, details.clone(), middle).contains("Working ·"));
        assert!(segment(&buffer, details.clone(), middle).contains(" · 2m"));
        assert!(segment(&buffer, 1..width - 1, 2).chars().all(|c| c == '─'));
        for y in 3..middle {
            for range in [details.clone(), log.clone()] {
                let text = segment(&buffer, range, y);
                assert!(
                    !text.contains("Working ·") && !text.contains("────"),
                    "extra label/rule row at {y}: {text}"
                );
            }
        }
        // Scroll the real event list through App, so its existing jump control
        // must move to the same border without adding a row inside the list.
        app.handle_event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: log.start,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        let buffer = draw(&mut app, width);
        assert!(segment(&buffer, log.clone(), middle).contains("↑ Latest"));
        for y in 3..middle {
            assert!(!segment(&buffer, log.clone(), y).contains("↑ Latest"));
        }
        let (x, y) = position(&buffer, "↑ Latest");
        click(&mut app, x, y);
        assert!(
            !lines(&draw(&mut app, width))
                .join("\n")
                .contains("↑ Latest")
        );
    }
}

#[test]
fn narrow_render_survives_wide_resize_unchanged() {
    let (_directory, mut app) = fixture();
    select_archive(&mut app);
    for width in [80, 120, 149] {
        let before = draw(&mut app, width);
        draw(&mut app, 200);
        let after = draw(&mut app, width);
        assert_eq!(before, after);
    }
}

#[test]
fn log_rows_keep_fixed_content_and_right_aligned_times() {
    use chrono::TimeZone;
    let (_directory, mut app) = fixture();
    for _ in 0..4 {
        key(&mut app, KeyCode::Tab);
    }
    key(&mut app, KeyCode::Home);
    draw(&mut app, 100);
    let source = crate::providers::claude::log::tail_session(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl"),
        200,
        true,
    )
    .unwrap()
    .into_iter()
    .find(|event| event.kind == crate::log::Kind::Tool)
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
        app.view.set_log(events.clone());
        let buffer = draw(&mut app, width);
        let (x, top) = position(&buffer, "Tool      long");
        let right = width - 1;
        assert_eq!(
            x,
            if width < 150 {
                1
            } else {
                width - width / 3 - 1
            }
        );
        for (offset, time) in ["2s", "9m 50s", "10:06:26"].into_iter().enumerate() {
            let y = top - offset as u16;
            let clock: String = (right - 8..right)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            assert_eq!(clock, format!("{time:>8}"));
            assert_eq!(buffer[(right - 9, y)].symbol(), " ");
            assert_eq!(buffer[(right - 10, y)].symbol(), "…");
            assert_eq!(buffer[(x, y)].symbol(), "T");
        }
    }
}

#[path = "app_details_tests.rs"]
mod details_alignment;

#[test]
fn app_log_padding_and_clicks_follow_real_transcript_rows() {
    for width in [149, 200] {
        for edge in [false, true] {
            let (_directory, mut app) = fixture();
            select_archive(&mut app);
            let buffer = draw(&mut app, width);
            let separators: Vec<_> = (0..width)
                .filter(|&x| buffer[(x, 3)].symbol() == "│")
                .collect();
            let left = if width == 200 { separators[1] + 2 } else { 1 };
            let (x, y) = position(&buffer, "User      ");
            assert_eq!(x, left);
            assert_eq!(
                (left..width).rfind(|&x| buffer[(x, y)].symbol() != " "),
                Some(width - 2)
            );
            click(&mut app, if edge { width - 2 } else { left }, y);
            app.view
                .flush_pending_click(Instant::now() + Duration::from_secs(1));
            let buffer = draw(&mut app, width);
            let (detail_x, detail_y) = position(&buffer, "User      ");
            let (_, prompt_y) = position(&buffer, "›");
            assert!(
                detail_y > prompt_y,
                "clicked event must open below the prompt"
            );
            assert_eq!(detail_x, 1);
            assert_eq!(
                (1..width).rfind(|&x| buffer[(x, detail_y)].symbol() != " "),
                Some(width - 2)
            );
            assert!(
                lines(&buffer)[usize::from(detail_y)..]
                    .join("\n")
                    .contains("⏺ Ran 4 stop hooks")
            );
        }
    }
}
