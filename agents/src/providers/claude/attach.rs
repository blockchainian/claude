//! Sends input through a temporary, screen-checked Claude attach client.

use crate::providers::SessionInput;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const DETACH_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn send(
    job_id: &str,
    cwd: &Path,
    settings_path: &Path,
    input: SessionInput,
) -> io::Result<()> {
    if job_id.is_empty() || job_id.starts_with('-') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Claude job id",
        ));
    }
    match &input {
        SessionInput::Prompt(text) => validate_text(text)?,
        SessionInput::SetModel(value) | SessionInput::SetEffort(value) => {
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid Claude setting",
                ));
            }
            validate_text(value)?;
        }
        SessionInput::Interrupt => {}
    }
    let restore = matches!(
        input,
        SessionInput::SetModel(_) | SessionInput::SetEffort(_)
    );
    let operation = || {
        let mut terminal = Terminal::spawn(job_id, cwd)?;
        let result = terminal.deliver(input);
        let detach = terminal.detach();
        result.and(detach)
    };
    if restore {
        super::settings::with_restored_defaults(settings_path, operation)
    } else {
        operation()
    }
}

/// Captures a confirmed input screen without sending any session input.
#[cfg(test)]
pub(super) fn inspect(job_id: &str, cwd: &Path) -> io::Result<String> {
    let mut terminal = Terminal::spawn(job_id, cwd)?;
    let result = terminal
        .wait_at("inspect-ready", ready)
        .map(|()| terminal.screen());
    let detached = terminal.detach();
    match result {
        Ok(screen) => detached.map(|()| screen),
        Err(error) => Err(error),
    }
}

fn validate_text(text: &str) -> io::Result<()> {
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Input contains terminal control characters",
        ));
    }
    Ok(())
}

struct Terminal {
    child: Child,
    master: File,
    parser: vt100::Parser,
    #[cfg(test)]
    raw: Vec<u8>,
}

impl Terminal {
    fn spawn(job_id: &str, cwd: &Path) -> io::Result<Self> {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 50,
            ws_col: 200,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // openpty returns independent owned descriptors on success.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        // Only the standard descriptors belong in the executed attach process.
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut command = Command::new("claude");
        command
            .args(["attach", job_id])
            .current_dir(cwd)
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            child,
            master,
            parser: vt100::Parser::new(50, 200, 0),
            #[cfg(test)]
            raw: Vec::new(),
        })
    }

    fn screen(&self) -> String {
        self.parser.screen().contents()
    }

    fn unexpected(&self, reason: &str) -> io::Error {
        let screen = self.screen();
        let first = screen
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("empty screen");
        io::Error::other(format!("{reason}: {first}"))
    }

    fn diagnostic(&self, stage: &str, error: io::Error) -> io::Error {
        #[cfg(test)]
        {
            let directory = std::env::temp_dir().join("agents-stage3-live");
            let name = format!("driver-{}-{stage}", self.child.id());
            let saved = std::fs::create_dir_all(&directory)
                .and_then(|()| {
                    std::fs::write(directory.join(format!("{name}.screen.txt")), self.screen())
                })
                .and_then(|()| std::fs::write(directory.join(format!("{name}.raw")), &self.raw));
            if let Err(diagnostic_error) = saved {
                return io::Error::new(
                    error.kind(),
                    format!("{stage}: {error}; could not save diagnostics: {diagnostic_error}"),
                );
            }
        }
        io::Error::new(error.kind(), format!("{stage}: {error}"))
    }

    fn wait_at(&mut self, stage: &str, predicate: impl Fn(&str) -> bool) -> io::Result<()> {
        self.wait(predicate)
            .map_err(|error| self.diagnostic(stage, error))
    }

    fn wait(&mut self, predicate: impl Fn(&str) -> bool) -> io::Result<()> {
        self.wait_allowing(predicate, None)
    }

    fn wait_allowing(
        &mut self,
        predicate: impl Fn(&str) -> bool,
        allowed_dialog: Option<&str>,
    ) -> io::Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let screen = self.screen();
            if predicate(&screen) {
                return Ok(());
            }
            if known_dialog(&screen)
                && !allowed_dialog.is_some_and(|marker| screen.contains(marker))
            {
                return Err(self.unexpected("Claude attach needs a response"));
            }
            if self.child.try_wait()?.is_some() {
                return Err(self.unexpected("Claude attach exited"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.unexpected("Claude attach timed out"));
            }
            self.read(remaining.min(Duration::from_millis(100)))?;
        }
    }

    fn read(&mut self, timeout: Duration) -> io::Result<()> {
        let mut descriptor = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                timeout.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(())
            } else {
                Err(error)
            };
        }
        if result == 0 {
            return Ok(());
        }
        let mut bytes = [0; 65536];
        match self.master.read(&mut bytes) {
            Ok(0) => Err(self.unexpected("Claude attach closed its terminal")),
            Ok(count) => {
                #[cfg(test)]
                self.raw.extend_from_slice(&bytes[..count]);
                self.parser.process(&bytes[..count]);
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.master.flush()
    }

    fn submit(&mut self, text: &str) -> io::Result<String> {
        self.write(b"\x1b[200~")?;
        self.write(text.as_bytes())?;
        self.write(b"\x1b[201~")?;
        // Observe the paste before Enter, so the unchanged empty prompt cannot acknowledge it.
        self.wait_at("paste-visible", paste_visible)?;
        let screen = self.screen();
        let pasted_input = prompt(&screen)
            .ok_or_else(|| self.unexpected("Claude input disappeared before submission"))?
            .1
            .to_owned();
        self.write(b"\r")?;
        Ok(pasted_input)
    }

    fn deliver(&mut self, input: SessionInput) -> io::Result<()> {
        self.wait_at("initial-ready", ready)?;
        match input {
            SessionInput::Prompt(text) => {
                if text.is_empty() {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "Empty prompt"));
                }
                let pasted_input = self.submit(&text)?;
                self.wait_at("submission-accepted", |screen| {
                    prompt_accepted(screen, &pasted_input)
                })
            }
            SessionInput::Interrupt => self.write(b"\x1b"),
            SessionInput::SetModel(model) => {
                self.submit(&format!("/model {model}"))?;
                self.wait_at("model-confirmation", |screen| {
                    screen.contains("Switch model?")
                })?;
                self.write(b"\r")?;
                // The parsed confirmation remains visible until Claude redraws after Enter.
                self.wait_allowing(
                    |screen| !screen.contains("Switch model?") && actual_empty(screen),
                    Some("Switch model?"),
                )
                .map_err(|error| self.diagnostic("model-confirmation-cleared", error))
            }
            SessionInput::SetEffort(effort) => {
                self.submit(&format!("/effort {effort}"))?;
                self.wait_at("effort-submission-cleared", actual_empty)
            }
        }
    }

    fn detach(&mut self) -> io::Result<()> {
        let write = self.write(b"\x1a");
        let deadline = Instant::now() + DETACH_TIMEOUT;
        loop {
            if self.child.try_wait()?.is_some() {
                return write;
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                self.child.wait()?;
                return write;
            }
            // Drain output while waiting: a full PTY buffer must not block the child's exit.
            let _ = self.read(Duration::from_millis(50));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
}

fn prompt(screen: &str) -> Option<(usize, &str)> {
    screen
        .lines()
        .enumerate()
        .filter_map(|(row, line)| line.trim_start().strip_prefix('❯').map(|text| (row, text)))
        .last()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptState<'a> {
    Empty,
    Queued,
    Editing(&'a str),
}

fn prompt_state(screen: &str) -> Option<PromptState<'_>> {
    prompt(screen).map(|(_, text)| match text.trim() {
        "" => PromptState::Empty,
        // Claude renders this placeholder over an empty editor after accepting a busy prompt.
        "Press up to edit queued messages" => PromptState::Queued,
        text => PromptState::Editing(text),
    })
}

fn ready(screen: &str) -> bool {
    matches!(
        prompt_state(screen),
        Some(PromptState::Empty | PromptState::Queued)
    )
}

fn actual_empty(screen: &str) -> bool {
    prompt_state(screen) == Some(PromptState::Empty)
}

fn paste_visible(screen: &str) -> bool {
    matches!(prompt_state(screen), Some(PromptState::Editing(_)))
}

fn prompt_accepted(screen: &str, pasted_input: &str) -> bool {
    match prompt_state(screen) {
        Some(PromptState::Empty) => true,
        Some(PromptState::Queued) => {
            prompt(screen).is_some_and(|(_, input)| input.trim() != pasted_input.trim())
        }
        Some(PromptState::Editing(_)) | None => false,
    }
}

fn known_dialog(screen: &str) -> bool {
    [
        "Do you trust",
        "Workspace not trusted",
        "Do you want to proceed?",
        "Allow once",
        "Yes, allow",
        "Enter to confirm",
        "Switch model?",
        "Sign in",
        "Select login method",
    ]
    .iter()
    .any(|marker| screen.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_queue_placeholder_accepts_another_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let sent_text = screen
            .lines()
            .find_map(|line| line.strip_prefix("❯ Steer"))
            .unwrap();
        assert!(prompt_accepted(screen, sent_text));
        assert!(ready(screen));
    }

    #[test]
    fn captured_queue_placeholder_is_not_a_visible_paste() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        assert!(!paste_visible(screen));
    }

    #[test]
    fn captured_queue_placeholder_does_not_acknowledge_a_setting_change() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        assert!(!actual_empty(screen));
    }

    #[test]
    fn captured_queue_hint_does_not_acknowledge_unchanged_pasted_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let (_, unchanged_input) = prompt(screen).unwrap();
        assert!(!prompt_accepted(screen, unchanged_input));
    }

    #[test]
    fn captured_historical_queue_hint_does_not_acknowledge_input() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/queued.screen.txt");
        let submitted = screen
            .lines()
            .find(|line| line.starts_with("❯ Steer"))
            .unwrap();
        let with_pending_input = format!("{screen}\n{submitted}");
        assert!(!prompt_accepted(&with_pending_input, submitted));
    }

    #[test]
    fn captured_terminal_output_contains_ready_before_detaching() {
        let raw = include_bytes!("../../../tests/fixtures/claude-attach/attach.raw");
        let mut parser = vt100::Parser::new(50, 200, 0);
        let mut saw_ready = false;
        for chunk in raw.chunks(37) {
            parser.process(chunk);
            saw_ready |= ready(&parser.screen().contents());
        }
        let screen = parser.screen().contents();
        assert!(
            saw_ready,
            "No empty input appeared in the real terminal capture: {screen}"
        );
        assert!(screen.contains("Resume this session with:"), "{screen}");
        assert!(!ready(&screen));
    }

    #[test]
    fn captured_idle_and_busy_screens_accept_input() {
        for screen in [
            include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt"),
            include_str!("../../../tests/fixtures/claude-attach/busy.screen.txt"),
        ] {
            assert!(ready(screen), "{screen}");
        }
    }

    #[test]
    fn captured_empty_input_without_footer_is_ready() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt");
        let (row, _) = prompt(screen).unwrap();
        let without_footer = screen.lines().take(row + 1).collect::<Vec<_>>().join("\n");
        assert!(ready(&without_footer));
    }

    #[test]
    fn captured_transcript_prompt_is_not_an_input_screen() {
        let screen = include_str!("../../../tests/fixtures/claude-attach/idle.screen.txt");
        let (row, _) = prompt(screen).unwrap();
        let before_input = screen.lines().take(row).collect::<Vec<_>>().join("\n");
        assert!(!ready(&before_input));
    }

    #[test]
    fn prompt_rejects_terminal_escape_injection() {
        assert!(validate_text("hello\u{1b}[201~\r/model opus").is_err());
        assert!(validate_text("hello\u{1a}").is_err());
    }

    #[test]
    fn prompt_accepts_multiline_unicode_text() {
        assert!(validate_text("第一行\nsecond line\t✓").is_ok());
    }
}
