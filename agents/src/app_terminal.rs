//! Opens, feeds and closes the session terminal shown in the event detail panel.

use super::{App, Input};
use crate::model::SessionStatus;
use crate::providers::{SessionInput, SessionKey, TerminalCommand, TerminalInput};
use crate::session_terminal::SessionTerminal;

impl App {
    pub(super) fn refresh_cell_pixels(&mut self) {
        match crate::terminal::cell_pixels() {
            Ok(cell_pixels) if cell_pixels != self.terminal_report.cell_pixels => {
                self.terminal_report.cell_pixels = cell_pixels;
                self.view
                    .set_inline_images(self.terminal_report.supports_images, cell_pixels);
            }
            Ok(_) => {}
            Err(error) => self.view.show_error(error.to_string()),
        }
    }

    pub(super) fn open_terminal(&mut self, id: SessionKey) {
        self.restore_terminal(id);
        self.view.focus_terminal();
    }

    fn restore_terminal(&mut self, id: SessionKey) {
        let restoring = self.view.terminal_requested();
        self.close_terminal();
        let Some(session) = self.session(&id) else {
            return;
        };
        if !session.cwd.is_dir() {
            self.view.show_error(format!(
                "Directory no longer exists: {}",
                session.cwd.display()
            ));
            return;
        }
        let Some(size) = self.view.terminal_size() else {
            self.view
                .show_error("The event detail panel is hidden".into());
            return;
        };
        if self.closing_terminals.contains_key(&id) {
            self.view.request_terminal();
            self.terminal_task = Some(id);
            return;
        }
        let cleanup = match self.hub.provider(id.provider).terminal_cleanup(&session) {
            Ok(cleanup) => cleanup,
            Err(error) => {
                self.view
                    .show_error(format!("Cannot open the terminal: {error}"));
                return;
            }
        };
        let TerminalCommand { command, detach } =
            match self.hub.provider(id.provider).terminal_command(&session) {
                Ok(command) => command,
                Err(reason) => {
                    self.view.show_error(reason.into());
                    return;
                }
            };
        let sender = self.sender.clone();
        match SessionTerminal::spawn(id.clone(), command, detach, size, move || {
            let _ = sender.send(Input::TerminalOutput);
        }) {
            Ok(mut terminal) => {
                terminal.cleanup = cleanup;
                if restoring {
                    self.view.request_terminal();
                }
                self.view.show_terminal(id, terminal.screen.clone());
                self.terminal = Some(terminal);
            }
            Err(error) => self
                .view
                .show_error(format!("Cannot open the terminal: {error}")),
        }
    }

    /// Returns the selected background task once per wait on input the dashboard cannot answer,
    /// such as a permission prompt, so the user replies in its terminal, or once per wait for
    /// a reply the user sends as a prompt.
    fn waiting_terminal(&mut self) -> Option<SessionKey> {
        let session = self
            .view
            .selected_id()
            .and_then(|id| self.session(id))
            .filter(|session| {
                session.status == SessionStatus::NeedsInput
                    && session.ask.is_none()
                    && session.is_live_background()
            });
        let Some(session) = session else {
            self.waiting_terminal = None;
            return None;
        };
        if self.waiting_terminal.as_ref() == Some(&session.id)
            || self.view.terminal_size().is_none()
        {
            return None;
        }
        self.waiting_terminal = Some(session.id.clone());
        Some(session.id)
    }

    /// Detaches or ends the open terminal and shows the event detail again.
    pub(super) fn close_terminal(&mut self) {
        self.terminal_task = None;
        self.view.hide_terminal();
        let Some(mut terminal) = self.terminal.take() else {
            return;
        };
        let id = terminal.id.clone();
        let sender = self.sender.clone();
        let thread_id = id.clone();
        let cleanup = terminal.cleanup.take();
        let thread = std::thread::spawn(move || {
            let detached = terminal.close();
            let cleaned = cleanup.map_or(Ok(()), |cleanup| cleanup());
            let result = detached.and(cleaned);
            let _ = sender.send(Input::TerminalClosed {
                id: thread_id,
                result,
            });
        });
        self.closing_terminals
            .insert(id, TerminalClose(Some(thread)));
    }

    pub(super) fn terminal_closed(&mut self, id: &SessionKey, result: std::io::Result<()>) {
        self.closing_terminals.remove(id);
        if let Err(error) = result {
            self.view
                .show_error(format!("Cannot close the terminal: {error}"));
        }
    }

    pub(super) fn write_terminal(&mut self, bytes: &[u8]) {
        if let Some(terminal) = &mut self.terminal
            && let Err(error) = terminal.write(bytes)
        {
            self.view
                .show_error(format!("Cannot write to the terminal: {error}"));
        }
    }

    pub(super) fn paste_terminal(&mut self, text: &str) {
        if let Some(terminal) = &mut self.terminal
            && let Err(error) = terminal.paste(text)
        {
            self.view
                .show_error(format!("Cannot write to the terminal: {error}"));
        }
    }

    /// Opens a terminal that waited for its task's previous client while that task is
    /// still selected, closes unselected or exited programs, and fits the selected
    /// terminal to the panel.
    pub(super) fn sync_terminal(&mut self) {
        if self
            .terminal
            .as_ref()
            .is_some_and(|terminal| self.view.selected_id() != Some(&terminal.id))
        {
            self.close_terminal();
            return;
        }
        let waiting = self.waiting_terminal();
        let Some(terminal) = &mut self.terminal else {
            if let Some(id) = self.terminal_task.take()
                && self.view.selected_id() == Some(&id)
            {
                self.restore_terminal(id);
            } else if self.view.terminal_requested() {
                if let Some(id) = self.view.selected_id().cloned() {
                    self.restore_terminal(id);
                }
            } else if let Some(id) = waiting {
                let awaits_prompt = self.session(&id).is_some_and(|session| {
                    session
                        .background_job
                        .as_ref()
                        .is_some_and(|job| job.awaits_prompt)
                });
                if awaits_prompt {
                    self.view.prepare_prompt();
                } else {
                    self.open_terminal(id);
                }
            }
            return;
        };
        if terminal.closed() {
            self.close_terminal();
            return;
        }
        terminal
            .screen
            .lock()
            .expect("terminal screen lock")
            .check_ready(std::time::Instant::now(), |screen| {
                self.hub
                    .provider(terminal.id.provider)
                    .terminal_screen_is_loading(screen)
            });
        if let Some((rows, cols)) = self.view.terminal_size()
            && let Err(error) = terminal.resize(rows, cols)
        {
            self.view
                .show_error(format!("Cannot resize the terminal: {error}"));
        }
    }

    pub(super) fn terminal_drawn(&self) {
        if let Some(terminal) = &self.terminal {
            terminal.drawn();
        }
    }

    /// Delivers input through the session's open terminal when its provider requests it,
    /// or closes the terminal for input it cannot take.
    pub(super) fn send_to_terminal(&mut self, id: &SessionKey, input: &SessionInput) -> bool {
        let Some(terminal) = self.terminal.as_mut().filter(|terminal| &terminal.id == id) else {
            return false;
        };
        let Some(delivery) = self.hub.provider(id.provider).terminal_input(input) else {
            return false;
        };
        let result = match delivery {
            TerminalInput::Paste {
                text,
                submit,
                prepare,
            } => {
                // Preparing waits on the screen, so the paste runs as an input job.
                let job = terminal.driver().map(|mut driver| {
                    let text = text.to_owned();
                    Box::new(move || {
                        if let Some(prepare) = prepare {
                            prepare(&mut driver)?;
                        }
                        driver.paste(&text)?;
                        crate::providers::ScreenInput::write(&mut driver, submit)
                    }) as crate::providers::SessionAction
                });
                match job {
                    Ok(job) => {
                        self.start_input_job(id.clone(), input.clone(), job);
                        return true;
                    }
                    Err(error) => Err(error),
                }
            }
            TerminalInput::Write(bytes) => terminal.write(bytes),
            TerminalInput::Close => {
                self.close_terminal();
                return false;
            }
        };
        match (input, result) {
            (SessionInput::Prompt(_), result) => self.view.input_complete(id, input, result),
            (_, Err(error)) => self.input_error(input, error.to_string()),
            (_, Ok(())) => {}
        }
        true
    }
}

/// Joins cleanup on shutdown, including when the completion message is still queued.
pub(super) struct TerminalClose(Option<std::thread::JoinHandle<()>>);

impl TerminalClose {
    /// Hands over the cleanup thread to a job that must wait for the client to leave.
    pub(super) fn take(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.0.take()
    }
}

impl Drop for TerminalClose {
    fn drop(&mut self) {
        if let Some(thread) = self.0.take() {
            thread.join().expect("terminal closer panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Hub;
    use ratatui::{Terminal, backend::TestBackend};
    use std::io;
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    const DETACH_TIMEOUT: Duration = Duration::from_millis(500);

    fn fixture() -> (tempfile::TempDir, App, SessionKey) {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects/probe");
        crate::providers::claude::write_fixture_cache(directory.path());
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
        app.sessions[0].status = crate::model::SessionStatus::Working;
        app.view.set_sessions(app.sessions.clone(), false);
        app.handle_event(crossterm::event::Event::Key(
            crossterm::event::KeyCode::Home.into(),
        ));
        let id = app.view.selected_id().unwrap().clone();
        (directory, app, id)
    }

    fn draw(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(103, 50)).unwrap();
        app.draw(&mut terminal).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn attach(app: &mut App, id: SessionKey, pid_file: &std::path::Path) -> u32 {
        let mut command = Command::new("sh");
        command.args(["-c", r#"stty raw -echo; printf '%s' "$$" > "$1"; printf READY; dd bs=1 count=1 >/dev/null 2>&1; printf DETACHED; while :; do read line; done"#, "sh"])
            .arg(pid_file);
        let sender = app.sender.clone();
        let terminal =
            SessionTerminal::spawn(id.clone(), command, Some(b"\x1a"), (20, 80), move || {
                let _ = sender.send(Input::TerminalOutput);
            })
            .unwrap()
            .with_detach_timeout(DETACH_TIMEOUT);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !terminal
            .screen
            .lock()
            .unwrap()
            .screen()
            .contents()
            .contains("READY")
        {
            app.receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
        }
        let pid = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        app.view.show_terminal(id, terminal.screen.clone());
        app.terminal = Some(terminal);
        pid
    }

    fn assert_reaped(pid: u32) {
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    fn finish_close(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let input = app
                .receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if matches!(input, Input::TerminalClosed { .. }) {
                let mut watcher = None;
                let mut schedule =
                    super::super::RefreshSchedule::new(Instant::now(), Duration::from_secs(60));
                app.handle_inputs(input, &mut watcher, &mut schedule)
                    .unwrap();
                break;
            }
        }
    }

    fn assert_command(command: &Command, program: &str, args: &[&str], cwd: &std::path::Path) {
        assert_eq!(command.get_program(), program);
        assert_eq!(command.get_args().collect::<Vec<_>>(), args);
        assert_eq!(command.get_current_dir(), Some(cwd));
    }

    #[test]
    fn terminal_commands_preserve_resume_attach_and_detach_rules() {
        use crate::model::{BackgroundJob, Process};
        let (directory, app, id) = fixture();
        let mut session = app.session(&id).unwrap();
        session.process = None;
        session.background_job = None;
        let TerminalCommand { command, detach } = app
            .hub
            .provider(session.id.provider)
            .terminal_command(&session)
            .unwrap();
        assert_command(
            &command,
            "sh",
            &[
                "-c",
                r#"claude --bg --allow-dangerously-skip-permissions --resume "$1" >/dev/null && exec claude attach "$2""#,
                "sh",
                &id.id,
                &id.id[..8],
            ],
            &session.cwd,
        );
        assert_eq!(detach, Some(b"\x1a".as_slice()));
        session.id.id = "short".into();
        assert_eq!(
            app.hub
                .provider(session.id.provider)
                .terminal_command(&session)
                .unwrap_err(),
            "Terminal is not supported"
        );
        session.id = id.clone();
        session.process = Some(Process {
            pid: 1,
            label: None,
            started_at: None,
        });
        assert_eq!(
            app.hub
                .provider(session.id.provider)
                .terminal_command(&session)
                .unwrap_err(),
            crate::model::FOREGROUND_SESSION
        );
        session.process.as_mut().unwrap().label = Some("bg".into());
        // A background process without its job is still rejected.
        assert_eq!(
            app.hub
                .provider(session.id.provider)
                .terminal_command(&session)
                .unwrap_err(),
            crate::model::FOREGROUND_SESSION
        );
        session.background_job = Some(BackgroundJob {
            id: "job-id".into(),
            needs: None,
            awaits_prompt: false,
        });
        let TerminalCommand { command, detach } = app
            .hub
            .provider(session.id.provider)
            .terminal_command(&session)
            .unwrap();
        assert_command(&command, "claude", &["attach", "job-id"], &session.cwd);
        assert_eq!(detach, Some(b"\x1a".as_slice()));
        session.parent = Some(id.clone());
        assert_eq!(
            app.hub
                .provider(session.id.provider)
                .terminal_command(&session)
                .unwrap_err(),
            "Terminal is not supported"
        );

        let app = App::new(
            Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
                directory.path().into(),
            ))]),
            directory.path().into(),
        );
        session.id.provider = crate::providers::ProviderId::Codex;
        session.parent = None;
        let TerminalCommand { command, detach } = app
            .hub
            .provider(session.id.provider)
            .terminal_command(&session)
            .unwrap();
        assert_command(&command, "codex", &["resume", &id.id], &session.cwd);
        assert_eq!(detach, None);
        session.parent = Some(session.id.clone());
        assert_eq!(
            app.hub
                .provider(session.id.provider)
                .terminal_command(&session)
                .unwrap_err(),
            "Terminal is not supported"
        );
    }

    /// Opens a terminal whose program draws `screens` in turn, each after one input byte, and
    /// then records the rest of its input to the returned file.
    fn open_screens(
        directory: &Path,
        app: &mut App,
        id: &SessionKey,
        screens: &[&str],
    ) -> std::path::PathBuf {
        let output = directory.join("input");
        let mut script = String::from("stty raw -echo; printf '\\033[?2004h'");
        for (index, screen) in screens.iter().enumerate() {
            let path = directory.join(format!("screen-{index}"));
            std::fs::write(&path, screen.replace('\n', "\r\n")).unwrap();
            if index > 0 {
                script.push_str("; head -c 1 >> \"$1\"; printf '\\033[H\\033[2J'");
            }
            script.push_str(&format!("; cat '{}'", path.display()));
        }
        script.push_str("; cat >> \"$1\"");
        let mut command = Command::new("sh");
        command.args(["-c", &script, "sh"]).arg(&output);
        let sender = app.sender.clone();
        let terminal = SessionTerminal::spawn(id.clone(), command, None, (50, 200), move || {
            let _ = sender.send(Input::TerminalOutput);
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !terminal
            .screen
            .lock()
            .unwrap()
            .screen()
            .contents()
            .contains('❯')
        {
            // Output wakes the dashboard again only after it draws the last frame.
            terminal.drawn();
            assert!(
                Instant::now() < deadline,
                "terminal never drew Claude's input"
            );
            let _ = app.receiver.recv_timeout(Duration::from_millis(50));
        }
        app.view.show_terminal(id.clone(), terminal.screen.clone());
        app.terminal = Some(terminal);
        output
    }

    /// Waits for the terminal input job and returns its result.
    fn sent(app: &mut App) -> io::Result<()> {
        loop {
            match app.receiver.recv_timeout(Duration::from_secs(20)).unwrap() {
                Input::Sent { result, .. } => return result,
                Input::TerminalOutput => {}
                _ => panic!("unexpected input event"),
            }
        }
    }

    fn written(output: &Path, expected: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read(output).unwrap_or_default().len() < expected.len() {
            assert!(Instant::now() < deadline, "terminal never received input");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::read(output).unwrap()
    }

    #[test]
    fn open_terminal_clears_left_claude_input_before_the_paste() {
        let (directory, mut app, id) = fixture();
        let output = open_screens(
            directory.path(),
            &mut app,
            &id,
            &[
                include_str!("../tests/fixtures/claude-attach/interrupted.screen.txt"),
                include_str!("../tests/fixtures/claude-attach/idle.screen.txt"),
            ],
        );
        assert!(app.send_to_terminal(&id, &SessionInput::Prompt("next".into())));
        sent(&mut app).unwrap();
        let expected = b"\x15\x1b[200~next\x1b[201~\r";
        assert_eq!(written(&output, expected), expected);
    }

    #[test]
    fn open_terminal_routes_only_its_providers_input_and_preserves_bytes() {
        let (directory, mut app, id) = fixture();
        assert!(!app.send_to_terminal(&id, &SessionInput::Interrupt));
        let output = open_screens(
            directory.path(),
            &mut app,
            &id,
            &[include_str!(
                "../tests/fixtures/claude-attach/idle.screen.txt"
            )],
        );
        let mut other = id.clone();
        other.id.push('x');
        assert!(!app.send_to_terminal(&other, &SessionInput::Interrupt));
        assert!(app.send_to_terminal(&id, &SessionInput::Prompt("line 1\nline 2".into())));
        sent(&mut app).unwrap();
        assert!(app.send_to_terminal(&id, &SessionInput::Interrupt));
        let expected = b"\x1b[200~line 1\nline 2\x1b[201~\r\x1b";
        assert_eq!(written(&output, expected), expected);
        let codex_id = SessionKey {
            provider: crate::providers::ProviderId::Codex,
            id: id.id.clone(),
        };
        app.hub = Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
            directory.path().into(),
        ))]);
        app.terminal.as_mut().unwrap().id = codex_id.clone();
        assert!(!app.send_to_terminal(&codex_id, &SessionInput::Prompt("not sent".into())));
        assert!(!app.send_to_terminal(&codex_id, &SessionInput::Interrupt));
        app.hub = Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().to_owned()),
        )]);
        app.terminal.as_mut().unwrap().id = id.clone();
        let images = SessionInput::PromptWithImages {
            text: "image".into(),
            images: vec![],
        };
        assert!(!app.send_to_terminal(&id, &images));
        assert!(app.terminal.is_none());
        assert!(app.closing_terminals.contains_key(&id));
        finish_close(&mut app);
    }

    #[test]
    fn selecting_another_event_closes_the_terminal() {
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };

        for input in ["wheel", "click", "Up", "Home", "PageUp"] {
            let (directory, mut app, id) = fixture();
            draw(&mut app);
            super::super::event_search_tests::finish_search(&mut app);
            let pid = attach(&mut app, id.clone(), &directory.path().join("pid"));
            let area = ratatui::layout::Rect::new(0, 0, 120, 23);
            app.view
                .render(area, &mut ratatui::buffer::Buffer::empty(area));
            assert!(app.view.terminal_requested());
            let selected = app.view.event_list.cursor;
            let mouse = |kind| {
                Event::Mouse(MouseEvent {
                    kind,
                    column: 2,
                    row: 11,
                    modifiers: KeyModifiers::NONE,
                })
            };
            match input {
                "wheel" => {
                    app.handle_event(mouse(MouseEventKind::ScrollUp));
                }
                "click" => {
                    app.handle_event(mouse(MouseEventKind::Down(MouseButton::Left)));
                    app.handle_event(mouse(MouseEventKind::Up(MouseButton::Left)));
                }
                _ => {
                    // Move focus from the output to the log list without selecting a row.
                    app.handle_event(Event::Key(KeyEvent::new(
                        KeyCode::Char('w'),
                        KeyModifiers::CONTROL,
                    )));
                    app.handle_event(Event::Key(KeyCode::Char('k').into()));
                    let key = match input {
                        "Up" => KeyCode::Up,
                        "Home" => KeyCode::Home,
                        "PageUp" => KeyCode::PageUp,
                        _ => unreachable!(),
                    };
                    app.handle_event(Event::Key(key.into()));
                }
            }
            assert_ne!(
                app.view.event_list.cursor, selected,
                "{input} must select another event"
            );
            assert!(
                !app.view.terminal_requested(),
                "{input} must restore event mode"
            );
            assert!(app.terminal.is_none(), "{input} must detach the terminal");
            assert!(app.terminal_task.is_none());
            assert!(app.closing_terminals.contains_key(&id));
            assert!(!draw(&mut app).contains("READY"));
            finish_close(&mut app);
            assert_reaped(pid);
        }
    }

    #[test]
    fn following_new_events_and_navigation_at_the_boundary_keep_the_terminal() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let (directory, mut app, id) = fixture();
        draw(&mut app);
        super::super::event_search_tests::finish_search(&mut app);
        let pid = attach(&mut app, id.clone(), &directory.path().join("pid"));
        let selected = app.view.event_list.cursor;
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-bg-metadata.jsonl");
        let mut events = app.view.event_panel.clone();
        events.extend(crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap());
        app.view.set_event(events);
        assert_ne!(
            app.view.event_list.cursor, selected,
            "new events must advance the followed selection"
        );
        assert!(app.view.terminal_requested());
        assert!(app.terminal.is_some());
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )));
        app.handle_event(Event::Key(KeyCode::Char('k').into()));
        for key in [KeyCode::Down, KeyCode::End, KeyCode::PageDown] {
            app.handle_event(Event::Key(key.into()));
            assert!(
                app.view.terminal_requested(),
                "{key:?} did not change the selected event"
            );
            assert!(app.terminal.is_some());
        }
        app.close_terminal();
        finish_close(&mut app);
        assert_reaped(pid);
    }

    #[test]
    fn terminal_cleanup_waits_for_detach_off_the_ui_thread_on_close_and_switch() {
        for switching in [false, true] {
            let (directory, mut app, id) = fixture();
            draw(&mut app);
            let pid = attach(&mut app, id.clone(), &directory.path().join("cleanup-pid"));
            let ui_thread = std::thread::current().id();
            let (sender, receiver) = std::sync::mpsc::channel();
            let (release, wait) = std::sync::mpsc::channel();
            app.terminal.as_mut().unwrap().cleanup = Some(Box::new(move || {
                assert_ne!(std::thread::current().id(), ui_thread);
                assert_reaped(pid);
                sender.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                Err(io::Error::other("terminal cleanup failed"))
            }));
            if switching {
                let mut other = app.sessions[0].clone();
                other.id.id = "cleanup-other".into();
                other.status = SessionStatus::NeedsInput;
                app.sessions.push(other);
                app.view.set_sessions(app.sessions.clone(), false);
                assert_ne!(app.view.selected_id(), Some(&id));
            }
            let start = Instant::now();
            if switching {
                app.sync_terminal();
            } else {
                app.close_terminal();
            }
            assert!(start.elapsed() < Duration::from_millis(100));
            assert!(app.terminal.is_none());
            assert!(app.closing_terminals.contains_key(&id));
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            release.send(()).unwrap();
            finish_close(&mut app);
            assert!(draw(&mut app).contains("Cannot close the terminal:"));
        }
    }

    #[test]
    #[ignore = "opens a real Claude background job from a temporary completed transcript"]
    fn real_woken_terminal_closes_back_to_inactive() {
        use crate::providers::claude::ClaudeProvider;
        let preferences = tempfile::tempdir().unwrap();
        let config = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".claude");
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let project = config.join("projects").join(
            cwd.to_str()
                .unwrap()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>(),
        );
        std::fs::create_dir_all(&project).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = project.join(format!("{id}.jsonl"));
        let mut lines = Vec::new();
        for line in include_str!("../tests/fixtures/transcript.jsonl")
            .lines()
            .take(2)
        {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["sessionId"] = id.clone().into();
            record["cwd"] = cwd.to_str().unwrap().into();
            record["timestamp"] = chrono::Utc::now().to_rfc3339().into();
            lines.push(serde_json::to_string(&record).unwrap());
        }
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
        let result = std::panic::catch_unwind(|| {
            let mut app = App::new(
                Hub::new(vec![Box::new(ClaudeProvider::new(config.clone()))]),
                preferences.path().to_owned(),
            );
            app.refresh();
            let key = SessionKey {
                provider: crate::providers::ProviderId::Claude,
                id: id.clone(),
            };
            assert_eq!(app.session(&key).unwrap().status, SessionStatus::Inactive);
            crate::app::live_tests::open_terminal(&mut app, key.clone(), |screen| {
                screen.lines().any(|line| line.trim() == "❯")
            });
            assert!(app.terminal.as_ref().unwrap().cleanup.is_some());
            app.close_terminal();
            finish_close(&mut app);
            app.refresh();
            assert_eq!(app.session(&key).unwrap().status, SessionStatus::Inactive);
        });
        // Only this probe's UUID and background job are removed.
        let _ = Command::new("claude").args(["stop", &id[..8]]).output();
        let _ = Command::new("claude").args(["rm", &id[..8]]).output();
        std::fs::remove_file(&path).unwrap();
        if path.with_extension("").exists() {
            std::fs::remove_dir_all(path.with_extension("")).unwrap();
        }
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn close_restores_events_before_a_stubborn_client_exits() {
        let (directory, mut app, id) = fixture();
        let event_panel = draw(&mut app);
        let pid = attach(&mut app, id, &directory.path().join("pid"));
        let screen = app.terminal.as_ref().unwrap().screen.clone();
        let start = Instant::now();
        app.close_terminal();
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "close blocked for {:?}",
            start.elapsed()
        );
        assert!(app.terminal.is_none());
        assert_eq!(draw(&mut app), event_panel);
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            0,
            "client must still be closing"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid as i32, 0) } == 0 {
            assert!(Instant::now() < deadline, "client never exited");
            let _ = app.receiver.recv_timeout(Duration::from_millis(100));
        }
        assert!(start.elapsed() >= DETACH_TIMEOUT);
        assert!(
            screen
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("DETACHED")
        );
        assert_reaped(pid);
    }

    #[test]
    fn waiting_terminal_opens_once_per_wait_without_questions() {
        let (_directory, mut app, id) = fixture();
        draw(&mut app);
        assert!(app.waiting_terminal().is_none());
        let session = &mut app.sessions[0];
        session.status = SessionStatus::NeedsInput;
        session.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        session.background_job = Some(crate::model::BackgroundJob {
            id: "job".into(),
            needs: None,
            awaits_prompt: false,
        });
        assert_eq!(app.waiting_terminal(), Some(id.clone()));
        assert!(
            app.waiting_terminal().is_none(),
            "a closed terminal stays closed"
        );
        app.sessions[0].status = SessionStatus::Working;
        assert!(app.waiting_terminal().is_none());
        app.sessions[0].status = SessionStatus::NeedsInput;
        assert_eq!(app.waiting_terminal(), Some(id.clone()));
        app.sessions[0].status = SessionStatus::Working;
        app.waiting_terminal();
        app.sessions[0].status = SessionStatus::NeedsInput;
        app.sessions[0].ask = Some(crate::ask::Ask {
            id: "ask".into(),
            kind: crate::ask::AskKind::Questions {
                questions: vec![],
                blocking: true,
            },
            session: id,
            since: chrono::Utc::now(),
        });
        assert!(
            app.waiting_terminal().is_none(),
            "questions have their own view"
        );
    }

    #[test]
    fn waiting_for_a_prompt_activates_prompt_once() {
        let (_directory, mut app, _id) = fixture();
        draw(&mut app);
        let session = &mut app.sessions[0];
        session.status = SessionStatus::NeedsInput;
        session.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        session.background_job = Some(crate::model::BackgroundJob {
            id: "job".into(),
            needs: Some("Reply with your preference: red or blue".into()),
            awaits_prompt: true,
        });
        app.view.set_sessions(app.sessions.clone(), false);
        app.sync_terminal();
        assert!(app.terminal.is_none());
        assert!(app.view.pasting_prompt());
        app.handle_event(crossterm::event::Event::Key(
            crossterm::event::KeyCode::Char('x').into(),
        ));
        assert!(draw(&mut app).contains("x"));
        app.handle_event(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        ));
        app.handle_event(crossterm::event::Event::Key(
            crossterm::event::KeyCode::Char('k').into(),
        ));
        assert!(!app.view.pasting_prompt());
        app.sync_terminal();
        assert!(!app.view.pasting_prompt());
    }

    #[test]
    fn queued_wait_opens_prompt_after_closing_previous_terminal() {
        let (directory, mut app, id) = fixture();
        draw(&mut app);
        attach(&mut app, id, &directory.path().join("pid"));
        let mut waiting = app.sessions[0].clone();
        waiting.id.id = "queued-wait".into();
        waiting.status = SessionStatus::Working;
        waiting.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        waiting.background_job = Some(crate::model::BackgroundJob {
            id: "queued-job".into(),
            needs: Some("Reply".into()),
            awaits_prompt: true,
        });
        app.sessions.push(waiting);
        app.view.set_sessions(app.sessions.clone(), false);
        app.sessions[1].status = SessionStatus::NeedsInput;
        app.view.set_sessions(app.sessions.clone(), false);
        assert_eq!(app.view.selected_id(), Some(&app.sessions[1].id));
        app.sync_terminal();
        assert!(app.terminal.is_none());
        app.sync_terminal();
        assert!(app.view.pasting_prompt());
        finish_close(&mut app);
    }

    #[test]
    fn terminal_task_switch_closes_and_queues_reopen() {
        let (directory, mut app, id) = fixture();
        draw(&mut app);
        let pid = attach(&mut app, id.clone(), &directory.path().join("switch-pid"));
        let mut other = app.sessions[0].clone();
        other.id.id = "terminal-other".into();
        other.status = SessionStatus::NeedsInput;
        other.ask = None;
        app.sessions.push(other);
        app.view.set_sessions(app.sessions.clone(), false);
        assert_ne!(app.view.selected_id(), Some(&id));
        app.sync_terminal();
        assert!(app.terminal.is_none());
        assert!(app.closing_terminals.contains_key(&id));
        app.sessions[0].status = SessionStatus::NeedsInput;
        app.sessions[1].status = SessionStatus::Working;
        app.view.set_sessions(app.sessions.clone(), false);
        assert_eq!(app.view.selected_id(), Some(&id));
        app.sync_terminal();
        assert_eq!(app.terminal_task.as_ref(), Some(&id));
        finish_close(&mut app);
        assert_reaped(pid);
        app.sessions[0].cwd = directory.path().join("removed-project");
        app.sync_terminal();
        assert!(draw(&mut app).contains("Directory no longer exists:"));
    }

    #[test]
    fn restored_terminal_o_restores_last_panel() {
        restored_terminal_activation(crossterm::event::KeyCode::Char('o'));
    }

    #[test]
    fn restored_terminal_enter_restores_last_panel() {
        restored_terminal_activation(crossterm::event::KeyCode::Enter);
    }

    fn restored_terminal_activation(key: crossterm::event::KeyCode) {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let (directory, mut app, id) = fixture();
        app.sessions[0].cwd = directory.path().to_owned();
        // Attach to a unique nonexistent job: the real provider and PTY run without
        // resuming a user session or sending a prompt.
        app.sessions[0].process = Some(crate::model::Process {
            pid: std::process::id(),
            label: Some("bg".into()),
            started_at: None,
        });
        app.sessions[0].background_job = Some(crate::model::BackgroundJob {
            id: uuid::Uuid::new_v4().to_string(),
            needs: None,
            awaits_prompt: false,
        });
        let mut other = app.sessions[0].clone();
        other.id.id = "terminal-other".into();
        app.sessions.push(other);
        app.view.set_sessions(app.sessions.clone(), false);
        app.handle_event(Event::Key(KeyCode::Home.into()));
        assert_eq!(app.view.selected_id(), Some(&id));
        draw(&mut app);

        app.open_terminal(id.clone());
        assert_eq!(
            app.terminal.as_ref().map(|terminal| &terminal.id),
            Some(&id)
        );
        assert!(
            app.view.terminal_paste("").is_some(),
            "initial open must focus EventDetail"
        );
        draw(&mut app);
        for direction in ['k', 'k', 'k', 'k', 'h'] {
            app.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char('w'),
                KeyModifiers::CONTROL,
            )));
            app.handle_event(Event::Key(KeyCode::Char(direction).into()));
        }
        app.handle_event(Event::Key(KeyCode::Down.into()));
        assert_ne!(
            app.view.selected_id(),
            Some(&id),
            "list navigation must leave A"
        );
        app.sync_terminal();
        assert!(app.terminal.is_none());
        finish_close(&mut app);

        app.handle_event(Event::Key(KeyCode::Home.into()));
        assert_eq!(app.view.selected_id(), Some(&id));
        draw(&mut app);
        app.sync_terminal();
        assert_eq!(
            app.terminal.as_ref().map(|terminal| &terminal.id),
            Some(&id)
        );
        assert!(
            app.view.terminal_paste("").is_none(),
            "automatic reopen must leave focus in list"
        );

        app.handle_event(Event::Key(key.into()));
        app.sync_terminal();
        let shown = app.terminal.as_ref().map(|terminal| &terminal.id) == Some(&id);
        let focused = app.view.terminal_paste("").is_some();
        app.close_terminal();
        finish_close(&mut app);
        assert!(shown, "entering must preserve the restored terminal");
        assert!(
            !focused,
            "entering with {key:?} must keep the last panel focus instead of opening the terminal"
        );
    }

    #[test]
    fn reopening_waits_for_the_previous_client_before_retrying() {
        let (directory, mut app, id) = fixture();
        draw(&mut app);
        let pid = attach(&mut app, id.clone(), &directory.path().join("pid"));
        let close_started = Instant::now();
        app.close_terminal();
        let start = Instant::now();
        app.open_terminal(id.clone());
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(app.terminal.is_none());
        assert_eq!(app.terminal_task.as_ref(), Some(&id));
        app.sync_terminal();
        assert!(app.terminal.is_none());
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
        finish_close(&mut app);
        assert!(close_started.elapsed() >= DETACH_TIMEOUT);
        assert_reaped(pid);
        assert!(app.closing_terminals.is_empty());
        // The real provider must never be launched by this unit test.
        app.sessions[0].cwd = directory.path().join("removed-project");
        app.sync_terminal();
        assert!(app.terminal_task.is_none());
        assert!(draw(&mut app).contains("Directory no longer exists:"));
    }

    #[test]
    fn close_errors_return_to_the_main_loop_as_a_notice() {
        let (_directory, mut app, id) = fixture();
        draw(&mut app);
        let mut command = Command::new("sh");
        command.args(["-c", "printf finished"]);
        let sender = app.sender.clone();
        let terminal = SessionTerminal::spawn(id, command, Some(b"\x1a"), (5, 20), move || {
            let _ = sender.send(Input::TerminalOutput);
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !terminal.closed() {
            app.receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
        }
        app.terminal = Some(terminal);
        app.close_terminal();
        finish_close(&mut app);
        assert!(draw(&mut app).contains("Cannot close the terminal:"));
    }

    #[test]
    fn app_shutdown_reaps_a_client_that_is_still_detaching() {
        let (directory, mut app, id) = fixture();
        let pid = attach(&mut app, id, &directory.path().join("pid"));
        let start = Instant::now();
        app.close_terminal();
        drop(app);
        assert!(start.elapsed() >= DETACH_TIMEOUT);
        assert_reaped(pid);
    }
}
