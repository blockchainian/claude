//! A session's own program running on a pty whose screen the log detail panel draws.

use crate::providers::SessionKey;
use crate::pty::Pty;
use std::io::{self, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The shortest interval between two redraw requests while output streams in.
const FRAME: Duration = Duration::from_millis(16);
const DETACH_TIMEOUT: Duration = Duration::from_secs(3);

pub type Screen = Arc<Mutex<vt100::Parser<Replies>>>;

pub struct SessionTerminal {
    pub id: SessionKey,
    pub screen: Screen,
    pty: Pty,
    /// Bytes that detach the program instead of ending it; without them closing kills it.
    detach: Option<&'static [u8]>,
    size: (u16, u16),
    /// Set by the reader when output arrives; cleared once the dashboard redraws.
    dirty: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}

impl SessionTerminal {
    /// Starts `command` on a pty of `rows` by `cols` cells; `wake` runs on the reader thread
    /// whenever the screen changes or the program closes its terminal.
    pub fn spawn(
        id: SessionKey,
        command: Command,
        detach: Option<&'static [u8]>,
        (rows, cols): (u16, u16),
        wake: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let pty = Pty::spawn(command, rows, cols)?;
        let screen: Screen = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            0,
            Replies::default(),
        )));
        let dirty = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let mut reader = pty.reader()?;
        let mut writer = pty.reader()?;
        let (thread_screen, thread_dirty, thread_closed) =
            (screen.clone(), dirty.clone(), closed.clone());
        thread::Builder::new()
            .name("agents-terminal".into())
            .spawn(move || {
                let mut last_wake = Instant::now() - FRAME;
                loop {
                    let mut output = Vec::new();
                    if !crate::pty::read_available(&mut reader, &mut output).unwrap_or(false) {
                        thread_closed.store(true, Ordering::Relaxed);
                        wake();
                        return;
                    }
                    let replies = {
                        let mut parser = thread_screen.lock().expect("terminal screen lock");
                        parser.process(&output);
                        std::mem::take(&mut parser.callbacks_mut().pending)
                    };
                    if !replies.is_empty() {
                        let _ = writer.write_all(&replies);
                    }
                    if !thread_dirty.swap(true, Ordering::Relaxed) {
                        wake();
                        last_wake = Instant::now();
                    }
                    // Output arriving within a frame is parsed together before the next redraw.
                    thread::sleep(FRAME.saturating_sub(last_wake.elapsed()));
                }
            })?;
        Ok(Self {
            id,
            screen,
            pty,
            detach,
            size: (rows, cols),
            dirty,
            closed,
        })
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// Marks the current screen drawn, so the next output requests another redraw.
    pub fn drawn(&self) {
        self.dirty.store(false, Ordering::Relaxed);
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pty.write(bytes)
    }

    /// Writes `text` as one paste, bracketed when the program asked for bracketed paste.
    pub fn paste(&mut self, text: &str) -> io::Result<()> {
        let bracketed = self
            .screen
            .lock()
            .expect("terminal screen lock")
            .screen()
            .bracketed_paste();
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        self.write(&bytes)
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if self.size == (rows, cols) {
            return Ok(());
        }
        self.size = (rows, cols);
        self.screen
            .lock()
            .expect("terminal screen lock")
            .screen_mut()
            .set_size(rows, cols);
        self.pty.resize(rows, cols)
    }

    /// Detaches or ends the program.
    pub fn close(mut self) -> io::Result<()> {
        self.pty.close(self.detach, DETACH_TIMEOUT)
    }
}

/// Answers the terminal queries programs send at startup, which vt100 leaves unhandled.
#[derive(Default)]
pub struct Replies {
    pending: Vec<u8>,
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first: Option<u8>,
        _second: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        match (first, command, params) {
            // Cursor position report.
            (None, 'n', [[6]]) => {
                let (row, col) = screen.cursor_position();
                self.pending
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            // Primary device attributes: a VT220 with ANSI color.
            (None, 'c', [] | [[0]]) => self.pending.extend_from_slice(b"\x1b[?62;22c"),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let light = crate::ui::light_background();
        let color = match params {
            [b"10", b"?"] if light => "0000/0000/0000",
            [b"10", b"?"] => "ffff/ffff/ffff",
            [b"11", b"?"] if light => "ffff/ffff/ffff",
            [b"11", b"?"] => "0000/0000/0000",
            _ => return,
        };
        let which = std::str::from_utf8(params[0]).expect("matched an ASCII parameter");
        self.pending
            .extend_from_slice(format!("\x1b]{which};rgb:{color}\x1b\\").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderId;
    use std::sync::mpsc;

    fn key() -> SessionKey {
        SessionKey {
            provider: ProviderId::Codex,
            id: "thread".into(),
        }
    }

    fn wait_for(terminal: &SessionTerminal, wakes: &mpsc::Receiver<()>, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let contents = terminal.screen.lock().unwrap().screen().contents();
            if contents.contains(text) {
                return contents;
            }
            assert!(Instant::now() < deadline, "never showed {text:?}:\n{contents}");
            let _ = wakes.recv_timeout(Duration::from_millis(100));
            terminal.drawn();
        }
    }

    #[test]
    fn startup_queries_are_answered() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            // Raw mode keeps the replies off the screen until the program prints them.
            "stty raw -echo; printf '\\033[3;5H\\033[6n\\033[c'; \
             dd bs=1 count=15 2>/dev/null | od -An -c | tr -d ' \\n'; printf ' END'; read x",
        ]);
        let (sender, wakes) = mpsc::channel();
        let terminal = SessionTerminal::spawn(key(), command, None, (10, 40), move || {
            let _ = sender.send(());
        })
        .unwrap();
        let contents = wait_for(&terminal, &wakes, "END");
        assert!(contents.contains(r"033[3;5R033[?62;22c"), "{contents}");
        terminal.close().unwrap();
    }

    #[test]
    fn exit_closes_the_terminal() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf bye"]);
        let (sender, wakes) = mpsc::channel();
        let terminal = SessionTerminal::spawn(key(), command, None, (5, 20), move || {
            let _ = sender.send(());
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !terminal.closed() {
            assert!(Instant::now() < deadline);
            let _ = wakes.recv_timeout(Duration::from_millis(100));
        }
        assert!(terminal.screen.lock().unwrap().screen().contents().contains("bye"));
    }

    #[test]
    fn resize_reaches_screen_and_program() {
        let mut command = Command::new("sh");
        command.args(["-c", "read x; stty size; read y"]);
        let (sender, wakes) = mpsc::channel();
        let mut terminal = SessionTerminal::spawn(key(), command, None, (5, 20), move || {
            let _ = sender.send(());
        })
        .unwrap();
        terminal.resize(8, 30).unwrap();
        assert_eq!(terminal.screen.lock().unwrap().screen().size(), (8, 30));
        terminal.write(b"\r").unwrap();
        wait_for(&terminal, &wakes, "8 30");
        terminal.close().unwrap();
    }
}
