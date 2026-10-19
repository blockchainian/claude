//! A session's own program running on a pty whose screen the log detail panel draws.

use crate::providers::SessionKey;
use crate::pty::Pty;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Output arriving within this interval is published together as one terminal frame.
const FRAME: Duration = Duration::from_millis(16);
const DETACH_TIMEOUT: Duration = Duration::from_secs(3);
/// The longest an input job waits for the screen it expects.
const INPUT_TIMEOUT: Duration = Duration::from_secs(15);

pub type Screen = Arc<Mutex<TerminalScreen>>;

/// Parses the live output while keeping a synchronized update off the displayed screen.
/// The longest a starting program's screen stays hidden behind the loading indicator.
const STARTUP_LIMIT: Duration = Duration::from_secs(2);

pub struct TerminalScreen {
    parser: vt100::Parser<Replies>,
    displayed: vt100::Screen,
    opened: Instant,
    ready: bool,
}

impl TerminalScreen {
    pub fn new(rows: u16, cols: u16) -> Self {
        let parser = vt100::Parser::new_with_callbacks(rows, cols, 0, Replies::default());
        let displayed = parser.screen().clone();
        Self {
            parser,
            displayed,
            opened: Instant::now(),
            ready: false,
        }
    }

    pub fn process(&mut self, output: &[u8]) -> Vec<u8> {
        self.parser.process(output);
        if let Some(completed) = self.parser.callbacks_mut().completed.take() {
            self.displayed = completed;
        }
        if !self.parser.callbacks().synchronized {
            self.displayed = self.parser.screen().clone();
        }
        if !self.parser.callbacks().used_synchronization && !self.ready {
            self.parser
                .callbacks_mut()
                .frames
                .push(self.displayed.contents());
        }
        std::mem::take(&mut self.parser.callbacks_mut().pending)
    }

    /// The last published screen and cursor, held until a synchronized update completes.
    pub fn screen(&self) -> &vt100::Screen {
        &self.displayed
    }

    /// Checks every published frame, including frames sharing a single PTY read.
    pub fn check_ready(&mut self, now: Instant, is_loading: impl Fn(&str) -> bool) {
        let frames = std::mem::take(&mut self.parser.callbacks_mut().frames);
        self.ready = self.ready(now)
            || frames
                .iter()
                .any(|text| text.chars().any(|c| !c.is_whitespace()) && !is_loading(text));
        self.parser.callbacks_mut().collect_frames = !self.ready;
    }

    pub fn ready(&self, now: Instant) -> bool {
        self.ready || now.saturating_duration_since(self.opened) >= STARTUP_LIMIT
    }

    pub fn loading_remaining(&self, now: Instant) -> Option<Duration> {
        (!self.ready(now)).then(|| {
            STARTUP_LIMIT.saturating_sub(now.saturating_duration_since(self.opened))
        })
    }

    fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.displayed.set_size(rows, cols);
    }
}

pub struct SessionTerminal {
    pub id: SessionKey,
    pub screen: Screen,
    pty: Pty,
    /// Bytes that detach the program instead of ending it; without them closing kills it.
    detach: Option<&'static [u8]>,
    detach_timeout: Duration,
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
        let screen: Screen = Arc::new(Mutex::new(TerminalScreen::new(rows, cols)));
        let pty = Pty::spawn(command, rows, cols)?;
        let dirty = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let mut reader = pty.reader()?;
        let mut writer = pty.reader()?;
        let (thread_screen, thread_dirty, thread_closed) =
            (screen.clone(), dirty.clone(), closed.clone());
        thread::Builder::new()
            .name("agents-terminal".into())
            .spawn(move || {
                loop {
                    let mut output = Vec::new();
                    let open = read_frame(&mut reader, &mut output).unwrap_or(false);
                    let replies = {
                        let mut parser = thread_screen.lock().expect("terminal screen lock");
                        parser.process(&output)
                    };
                    if !replies.is_empty() {
                        let _ = writer.write_all(&replies);
                    }
                    if !thread_dirty.swap(true, Ordering::Relaxed) {
                        wake();
                    }
                    if !open {
                        thread_closed.store(true, Ordering::Relaxed);
                        wake();
                        return;
                    }
                }
            })?;
        Ok(Self {
            id,
            screen,
            pty,
            detach,
            detach_timeout: DETACH_TIMEOUT,
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
        let bytes = paste_bytes(&self.screen, text);
        self.write(&bytes)
    }

    /// The terminal's input and screen, for an input job off the dashboard thread.
    pub fn driver(&self) -> io::Result<TerminalDriver> {
        Ok(TerminalDriver {
            writer: self.pty.writer()?,
            screen: self.screen.clone(),
            closed: self.closed.clone(),
        })
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
            .set_size(rows, cols);
        self.pty.resize(rows, cols)
    }

    /// Detaches or ends the program.
    pub fn close(mut self) -> io::Result<()> {
        self.pty.close(self.detach, self.detach_timeout)
    }

    #[cfg(test)]
    pub(crate) fn with_detach_timeout(mut self, timeout: Duration) -> Self {
        self.detach_timeout = timeout;
        self
    }
}

fn paste_bytes(screen: &Screen, text: &str) -> Vec<u8> {
    let bracketed = screen
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
    bytes
}

/// A session terminal's input and published screen, used by an input job.
pub struct TerminalDriver {
    writer: std::fs::File,
    screen: Screen,
    closed: Arc<AtomicBool>,
}

impl TerminalDriver {
    pub fn paste(&mut self, text: &str) -> io::Result<()> {
        let bytes = paste_bytes(&self.screen, text);
        crate::providers::ScreenInput::write(self, &bytes)
    }
}

impl crate::providers::ScreenInput for TerminalDriver {
    fn wait_for(&mut self, stage: &str, ready: &dyn Fn(&str) -> bool) -> io::Result<String> {
        let deadline = Instant::now() + INPUT_TIMEOUT;
        loop {
            let screen = self
                .screen
                .lock()
                .expect("terminal screen lock")
                .screen()
                .contents();
            if ready(&screen) {
                return Ok(screen);
            }
            if self.closed.load(Ordering::Relaxed) {
                return Err(io::Error::other(format!("{stage}: the terminal closed")));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{stage}: the terminal timed out"),
                ));
            }
            thread::sleep(FRAME);
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }
}

/// Blocks for the first output, then drains arrivals through that frame's fixed boundary.
fn read_frame(reader: &mut std::fs::File, output: &mut Vec<u8>) -> io::Result<bool> {
    let mut open = crate::pty::read_available(reader, output)?;
    let deadline = Instant::now() + FRAME;
    let mut descriptor = libc::pollfd {
        fd: reader.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    while open {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        // Round up so a sub-millisecond remainder cannot publish before the boundary.
        let timeout = remaining.as_nanos().div_ceil(1_000_000) as i32;
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            break;
        }
        open = crate::pty::read_available(reader, output)?;
    }
    Ok(open)
}

/// Handles synchronized updates and terminal queries, which vt100 leaves unhandled.
struct Replies {
    pending: Vec<u8>,
    synchronized: bool,
    completed: Option<vt100::Screen>,
    used_synchronization: bool,
    collect_frames: bool,
    frames: Vec<String>,
}

impl Default for Replies {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            synchronized: false,
            completed: None,
            used_synchronization: false,
            collect_frames: true,
            frames: Vec::new(),
        }
    }
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first: Option<u8>,
        second: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        if first == Some(b'?') && second.is_none() && params.contains(&&[2026][..]) {
            match command {
                'h' => {
                    // DECSET is idempotent, including repeated begins in one update.
                    self.synchronized = true;
                    self.used_synchronization = true;
                }
                'l' if self.synchronized => {
                    self.synchronized = false;
                    // Retain this frame if another update begins in the same PTY read.
                    self.completed = Some(screen.clone());
                    if self.collect_frames {
                        self.frames.push(screen.contents());
                    }
                }
                _ => {}
            }
        }
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

    fn replay_ready(raw: &[u8], provider: &dyn crate::providers::Provider) -> Vec<(String, bool)> {
        let mut screen = TerminalScreen::new(14, 101);
        let now = Instant::now();
        let mut start = 0;
        let mut frames = Vec::new();
        for end in raw
            .windows(8)
            .enumerate()
            .filter_map(|(i, bytes)| (bytes == b"\x1b[?2026l").then_some(i + 8))
        {
            screen.process(&raw[start..end]);
            screen.check_ready(now, |text| provider.terminal_screen_is_loading(text));
            frames.push((screen.screen().contents(), screen.ready(now)));
            start = end;
        }
        frames
    }

    #[test]
    fn startup_waits_through_recorded_resume_frames() {
        let provider =
            crate::providers::codex::CodexProvider::new("unused".into(), "unused".into());
        let frames = replay_ready(
            include_bytes!("../tests/fixtures/codex-terminal/resume.raw"),
            &provider,
        );
        let resuming: Vec<_> = frames
            .iter()
            .filter(|(text, _)| text.contains("Resuming session"))
            .collect();
        assert!(resuming.len() > 20);
        assert!(resuming.iter().all(|(_, ready)| !ready));
        assert!(frames.last().unwrap().1);
        assert!(!frames.last().unwrap().0.contains("Resuming session"));
    }

    #[test]
    fn startup_recorded_read_boundaries_do_not_release_partial_frames() {
        let raw = include_bytes!("../tests/fixtures/codex-terminal/resume.raw");
        let provider =
            crate::providers::codex::CodexProvider::new("unused".into(), "unused".into());
        let mut screen = TerminalScreen::new(14, 101);
        let now = Instant::now();
        for chunk in include_str!("../tests/fixtures/codex-terminal/chunks.jsonl").lines() {
            let (_, offset, count): (f64, usize, usize) = serde_json::from_str(chunk).unwrap();
            screen.process(&raw[offset..offset + count]);
            screen.check_ready(now, |text| {
                crate::providers::Provider::terminal_screen_is_loading(&provider, text)
            });
            if screen.screen().contents().contains("Resuming session") {
                assert!(!screen.ready(now));
            }
        }
        assert!(screen.ready(now));
    }

    #[test]
    fn startup_attach_is_ready_on_its_first_text_frame() {
        let provider = crate::providers::claude::ClaudeProvider::new("unused".into());
        let frames = replay_ready(
            include_bytes!("../tests/fixtures/claude-attach/attach.raw"),
            &provider,
        );
        assert!(!frames[0].0.trim().is_empty());
        assert!(frames[0].1);
    }

    #[test]
    fn startup_empty_frames_wait_until_the_startup_limit() {
        let mut screen = TerminalScreen::new(14, 101);
        let start = Instant::now();
        screen.process(b"\x1b[?2026h\x1b[?2026l");
        screen.check_ready(start, |_| false);
        assert!(!screen.ready(start));
        assert!(!screen.ready(start + STARTUP_LIMIT - Duration::from_millis(1)));
        assert!(screen.ready(start + STARTUP_LIMIT));
    }

    #[test]
    fn startup_unsynchronized_text_is_ready_at_publication() {
        let mut screen = TerminalScreen::new(14, 101);
        let now = Instant::now();
        screen.process(b" \r\n");
        screen.check_ready(now, |_| false);
        assert!(!screen.ready(now));
        screen.process(b"READY");
        screen.check_ready(now, |_| false);
        assert!(screen.ready(now));
    }

    #[test]
    fn synchronized_updates_answer_queries_from_the_live_cursor() {
        let mut screen = TerminalScreen::new(20, 80);
        screen.process(b"\x1b[12;3H");
        let replies = screen.process(b"\x1b[?2026h\x1b[?25l\x1b[14;31H2\x1b[6n");
        assert_eq!(replies, b"\x1b[14;32R");
        assert_eq!(screen.screen().cursor_position(), (11, 2));
        assert!(!screen.screen().hide_cursor());

        // A repeated begin is a mode set, not another nested transaction.
        screen.process(b"\x1b[?2026h\x1b[?2026l");
        assert_eq!(screen.screen().cursor_position(), (13, 31));
        assert!(screen.screen().hide_cursor());
    }

    #[test]
    fn a_second_synchronized_update_retains_the_frame_completed_in_the_same_read() {
        let mut screen = TerminalScreen::new(20, 80);
        screen.process(b"\x1b[?2026hfirst\x1b[?2026l\x1b[?2026h\rsecond");
        assert_eq!(screen.screen().contents(), "first");
        assert_eq!(screen.screen().cursor_position(), (0, 5));
        screen.process(b"\x1b[?2026l");
        assert_eq!(screen.screen().contents(), "second");
        assert_eq!(screen.screen().cursor_position(), (0, 6));
    }

    #[test]
    fn resizing_a_synchronized_update_resizes_both_screens() {
        let mut screen = TerminalScreen::new(20, 80);
        screen.process(b"\x1b[?2026h\x1b[14;31H2");
        screen.set_size(25, 100);
        assert_eq!(screen.screen().size(), (25, 100));
        screen.process(b"\x1b[?2026l");
        assert_eq!(screen.screen().size(), (25, 100));
        assert_eq!(screen.screen().cell(13, 30).unwrap().contents(), "2");
    }

    #[test]
    fn the_first_wake_batches_a_clear_and_the_redraw_arriving_within_a_frame() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "stty -echo; printf 'READY\\033[12;3H'; read start; \
             printf '\\033[r\\033[0m\\033[H\\033[2J\\033[3J\\033[H'; \
             sleep 0.001; \
             printf '\\033[?2026h\\033[1;1HDRAWN\\033[12;3H\\033[?25h\\033[?2026l'; \
             read done",
        ]);
        let (sender, wakes) = mpsc::channel();
        let mut terminal = SessionTerminal::spawn(key(), command, None, (14, 101), move || {
            let _ = sender.send(());
        })
        .unwrap();
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            terminal
                .screen
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("READY")
        );
        terminal.drawn();
        // Let the reader finish its initial frame and block for the next burst.
        thread::sleep(FRAME * 2);
        terminal.write(b"\n").unwrap();
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
        {
            let screen = terminal.screen.lock().unwrap();
            assert_eq!(screen.screen().cursor_position(), (11, 2));
            assert!(screen.screen().contents().contains("DRAWN"));
        }
        terminal.close().unwrap();
    }

    fn wait_for(terminal: &SessionTerminal, wakes: &mpsc::Receiver<()>, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let contents = terminal.screen.lock().unwrap().screen().contents();
            if contents.contains(text) {
                return contents;
            }
            assert!(
                Instant::now() < deadline,
                "never showed {text:?}:\n{contents}"
            );
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
        assert!(
            terminal
                .screen
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("bye")
        );
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
