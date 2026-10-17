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
        let Some(terminal) = self.terminal.take() else {
            return;
        };
        self.view.hide_terminal();
        if let Err(error) = terminal.close() {
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

    /// Closes the terminal when its program exited or its session is no longer selected,
    /// and fits it to the panel otherwise.
    pub(super) fn sync_terminal(&mut self) {
        let Some(terminal) = &mut self.terminal else {
            return;
        };
        if terminal.closed() || self.view.selected_id() != Some(&terminal.id) {
            self.close_terminal();
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
            _ => Err(std::io::Error::other("Use the open terminal for this input")),
        };
        match (input, result) {
            (SessionInput::Prompt(_), result) => self.view.input_complete(id, input, result),
            (_, Err(error)) => self.input_error(input, error.to_string()),
            (_, Ok(())) => {}
        }
        true
    }
}
