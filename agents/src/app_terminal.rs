//! Opens, feeds and closes the session terminal shown in the log detail panel.

use super::{App, Input};
use crate::model::SessionStatus;
use crate::providers::{SessionInput, SessionKey, TerminalCommand, TerminalInput};
use crate::session_terminal::SessionTerminal;

impl App {
    pub(super) fn open_terminal(&mut self, id: SessionKey) {
        self.close_terminal();
        let Some(session) = self.session(&id) else {
            return;
        };
        if !session.cwd.is_dir() {
            self.view.set_notice(Some(format!(
                "Directory no longer exists: {}",
                session.cwd.display()
            )));
            return;
        }
        let Some(size) = self.view.terminal_size() else {
            self.view
                .set_notice(Some("The log detail panel is hidden".into()));
            return;
        };
        if self.closing_terminals.contains_key(&id) {
            self.terminal_task = Some(id);
            return;
        }
        let TerminalCommand { command, detach } =
            match self.hub.provider(id.provider).terminal_command(&session) {
                Ok(command) => command,
                Err(reason) => {
                    self.view.set_notice(Some(reason.into()));
                    return;
                }
            };
        let sender = self.sender.clone();
        match SessionTerminal::spawn(id.clone(), command, detach, size, move || {
            let _ = sender.send(Input::TerminalOutput);
        }) {
            Ok(terminal) => {
                self.view.show_terminal(id, terminal.screen.clone());
                self.terminal = Some(terminal);
            }
            Err(error) => self
                .view
                .set_notice(Some(format!("Failed to open the terminal: {error}"))),
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

    /// Detaches or ends the open terminal and shows the log detail again.
    pub(super) fn close_terminal(&mut self) {
        self.terminal_task = None;
        let Some(terminal) = self.terminal.take() else {
            return;
        };
        self.view.hide_terminal();
        let id = terminal.id.clone();
        let sender = self.sender.clone();
        let thread_id = id.clone();
        let thread = std::thread::spawn(move || {
            let result = terminal.close();
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
                .set_notice(Some(format!("Failed to close the terminal: {error}")));
        }
    }

    pub(super) fn write_terminal(&mut self, bytes: &[u8]) {
        if let Some(terminal) = &mut self.terminal
            && let Err(error) = terminal.write(bytes)
        {
            self.view
                .set_notice(Some(format!("Failed to write to the terminal: {error}")));
        }
    }

    pub(super) fn paste_terminal(&mut self, text: &str) {
        if let Some(terminal) = &mut self.terminal
            && let Err(error) = terminal.paste(text)
        {
            self.view
                .set_notice(Some(format!("Failed to write to the terminal: {error}")));
        }
    }

    /// Opens a terminal that waited for its task's previous client while that task is
    /// still selected, closes unselected or exited programs, and fits the selected
    /// terminal to the panel.
    pub(super) fn sync_terminal(&mut self) {
        let waiting = self.waiting_terminal();
        let Some(terminal) = &mut self.terminal else {
            if let Some(id) = self.terminal_task.take()
                && self.view.selected_id() == Some(&id)
            {
                self.open_terminal(id);
            } else if let Some(id) = waiting {
                let awaits_prompt = self
                    .session(&id)
                    .is_some_and(|session| {
                        session
                            .background_job
                            .as_ref()
                            .is_some_and(|job| job.awaits_prompt)
                    });
                if awaits_prompt {
                    self.view.focus_input();
                } else {
                    self.open_terminal(id);
                }
            }
            return;
        };
        if terminal.closed() || self.view.selected_id() != Some(&terminal.id) {
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
                .set_notice(Some(format!("Failed to resize the terminal: {error}")));
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
                    }) as crate::providers::Job
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
            .unwrap();
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
                r#"claude --resume "$1" --bg >/dev/null && exec claude attach "$2""#,
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
                directory.path().join("manager"),
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
            assert!(Instant::now() < deadline, "terminal never drew Claude's input");
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
            directory.path().join("manager"),
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
    fn close_restores_logs_before_a_stubborn_client_exits() {
        let (directory, mut app, id) = fixture();
        let log = draw(&mut app);
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
        assert_eq!(draw(&mut app), log);
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
    fn waiting_for_a_prompt_focuses_the_input_instead_of_a_terminal() {
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
    }

    #[test]
    fn reopening_waits_for_the_previous_client_before_retrying() {
        let (directory, mut app, id) = fixture();
        draw(&mut app);
        let pid = attach(&mut app, id.clone(), &directory.path().join("pid"));
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
        assert!(draw(&mut app).contains("Failed to close the terminal:"));
    }

    #[test]
    fn app_shutdown_reaps_a_client_that_is_still_detaching() {
        let (directory, mut app, id) = fixture();
        let pid = attach(&mut app, id, &directory.path().join("pid"));
        app.close_terminal();
        drop(app);
        assert_reaped(pid);
    }
}
