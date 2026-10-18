//! Opens, feeds and closes the session terminal shown in the log detail panel.

use super::{App, Input};
use crate::model::{Action, Session};
use crate::providers::{ProviderId, SessionInput, SessionKey};
use crate::session_terminal::SessionTerminal;
use std::process::Command;

/// Claude's attach client detaches on Ctrl-Z, leaving the background job running.
const CLAUDE_DETACH: &[u8] = b"\x1a";

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
        let (command, detach) = match self.terminal_command(&session) {
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

    /// The program that shows `session` in a terminal, and the bytes that detach it.
    fn terminal_command(
        &self,
        session: &Session,
    ) -> Result<(Command, Option<&'static [u8]>), &'static str> {
        let resume = || {
            self.hub
                .provider(session.id.provider)
                .launch(&Action::Resume(session.id.clone()), Some(session))
                .ok_or("Terminal is not supported")
        };
        match session.id.provider {
            // The app-server owns Codex turns, so closing the client leaves them running.
            ProviderId::Codex => Ok((resume()?, None)),
            ProviderId::Claude if session.is_live_background() => {
                Ok((resume()?, Some(CLAUDE_DETACH)))
            }
            ProviderId::Claude if session.process.is_some() => {
                Err(crate::model::FOREGROUND_SESSION)
            }
            ProviderId::Claude => {
                // A session with no live process wakes as a background job under the same
                // id, whose job id is the session id's first eight characters.
                let job = session.id.id.get(..8).ok_or("Terminal is not supported")?;
                let mut command = Command::new("sh");
                command
                    .args([
                        "-c",
                        r#"claude --resume "$1" --bg >/dev/null && exec claude attach "$2""#,
                        "sh",
                        &session.id.id,
                        job,
                    ])
                    .current_dir(&session.cwd);
                Ok((command, Some(CLAUDE_DETACH)))
            }
        }
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

    /// Reopens the remembered task's terminal, closes unselected or exited programs,
    /// and fits the selected terminal to the panel.
    pub(super) fn sync_terminal(&mut self) {
        let Some(terminal) = &mut self.terminal else {
            if let Some(id) = &self.terminal_task
                && self.view.selected_id() == Some(id)
            {
                let id = self.terminal_task.take().expect("remembered terminal task");
                self.open_terminal(id);
            }
            return;
        };
        if terminal.closed() {
            self.close_terminal();
            return;
        }
        if self.view.selected_id() != Some(&terminal.id) {
            let id = terminal.id.clone();
            self.close_terminal();
            self.terminal_task = Some(id);
            return;
        }
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

    /// Types a Claude prompt or interrupt into the session's open terminal, whose attach
    /// client already owns its input; returns false when the input takes the provider path.
    pub(super) fn send_to_terminal(&mut self, id: &SessionKey, input: &SessionInput) -> bool {
        let Some(terminal) = self
            .terminal
            .as_mut()
            .filter(|terminal| &terminal.id == id && id.provider == ProviderId::Claude)
        else {
            return false;
        };
        let result = match input {
            SessionInput::Prompt(text) => {
                self.view.input_sending(id, input);
                terminal.paste(text).and_then(|()| terminal.write(b"\r"))
            }
            SessionInput::Interrupt => terminal.write(b"\x1b"),
            _ => Err(std::io::Error::other(
                "Use the open terminal for this input",
            )),
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
        let terminal = SessionTerminal::spawn(
            id.clone(),
            command,
            Some(CLAUDE_DETACH),
            (20, 80),
            move || {
                let _ = sender.send(Input::TerminalOutput);
            },
        )
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
        let terminal =
            SessionTerminal::spawn(id, command, Some(CLAUDE_DETACH), (5, 20), move || {
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
