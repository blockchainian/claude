//! Terminal loop: reacts to session changes and keyboard input, renders the view and executes actions.

use crate::model::Action;
use crate::model::Session;
use crate::providers::Snapshot;
use crate::providers::{ProviderId, SessionKey};
use crate::refresh::{FALLBACK_INTERVAL, POLLING_INTERVAL, RefreshSchedule};
use crate::sessions::Hub;
use crate::state::Preferences;
use crate::terminal_background;
use crate::ui::View;
use crossterm::event::Event;
use crossterm::event::Theme;
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;
use std::time::Instant;

// Bounds reader shutdown before handing the terminal to a provider process.
const INPUT_STOP_INTERVAL: Duration = Duration::from_millis(100);

enum Input {
    Terminal(io::Result<Event>),
    Filesystem(notify::Result<()>),
}

struct InputReader {
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl InputReader {
    fn start(sender: mpsc::Sender<Input>) -> io::Result<Self> {
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = thread::Builder::new()
            .name("agents-input".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let event = match crossterm::event::poll(INPUT_STOP_INTERVAL) {
                        Ok(false) => continue,
                        Ok(true) if stop.load(Ordering::Relaxed) => break,
                        Ok(true) => crossterm::event::read(),
                        Err(error) => Err(error),
                    };
                    let failed = event.is_err();
                    if sender.send(Input::Terminal(event)).is_err() || failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            stopped,
            thread: Some(thread),
        })
    }
}

impl Drop for InputReader {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // The reader returns before raw mode ends or a child inherits stdin.
            thread.join().expect("terminal reader panicked");
        }
    }
}

pub struct App {
    config_dir: PathBuf,
    hub: Hub,
    preferences: Preferences,
    view: View,
    clipboard: crate::clipboard::Clipboard,
    sessions: Vec<Session>,
    /// Set when the user chose to hand the terminal to a provider process; the dashboard
    /// returns once it exits.
    launch: Option<Command>,
    last_session: Option<SessionKey>,
    log_stamp: Option<LogStamp>,
    full_log_stamp: Option<LogStamp>,
    /// Token usage of each transcript the user has selected, kept up to date while selected.
    usage: std::collections::HashMap<PathBuf, crate::usage::Series>,
}

#[derive(PartialEq, Eq)]
struct LogStamp {
    session: SessionKey,
    path: PathBuf,
    modified: std::time::SystemTime,
    length: u64,
    count: usize,
}

impl App {
    pub fn new(hub: Hub, config_dir: PathBuf) -> Self {
        let preferences = Preferences::load(&config_dir);
        let mut view = View::new(preferences.grouping, true);
        view.set_pinned(&preferences.pinned);
        view.set_loading(true);
        view.set_capabilities(hub.capabilities());
        Self {
            hub,
            config_dir,
            preferences,
            view,
            clipboard: crate::clipboard::Clipboard::new(),
            sessions: Vec::new(),
            launch: None,
            last_session: None,
            log_stamp: None,
            full_log_stamp: None,
            usage: std::collections::HashMap::new(),
        }
    }

    pub fn run(mut self) -> io::Result<()> {
        loop {
            let mut terminal = enter_terminal()?;
            // Raw mode is on, so the terminal's reply reaches us instead of the screen.
            let background = terminal_background::query(terminal_background::DEFAULT_TIMEOUT)
                .ok()
                .flatten();
            self.apply_background(
                background,
                background.is_some_and(terminal_background::is_light),
            );
            let result = self.event_loop(&mut terminal);
            leave_terminal(terminal)?;
            result?;
            let Some(mut command) = self.launch.take() else {
                return Ok(());
            };
            // The dashboard comes back when the launched process exits.
            let notice = match command.status() {
                Ok(status) if status.success() => None,
                Ok(status) => Some(format!(
                    "{} exited with {status}",
                    command.get_program().to_string_lossy()
                )),
                Err(error) => Some(format!(
                    "Failed to start {:?}: {error}",
                    command.get_program()
                )),
            };
            self.view.set_notice(notice);
        }
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<()> {
        let (sender, receiver) = mpsc::channel();
        let filesystem = sender.clone();
        let watch = self
            .hub
            .watch_roots()
            .map_err(notify::Error::io)
            .and_then(|roots| {
                crate::sessions::watch(&roots, move |event| {
                    let _ = filesystem.send(Input::Filesystem(event.map(|_| ())));
                })
            });
        let mut watcher = match watch {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                self.view
                    .set_notice(Some(format!("Falling back to polling: {error}")));
                None
            }
        };
        let _reader = InputReader::start(sender)?;
        let mut schedule = RefreshSchedule::new(
            Instant::now(),
            if watcher.is_some() {
                FALLBACK_INTERVAL
            } else {
                POLLING_INTERVAL
            },
        );
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::event::EnableThemeChangeReports,
            MouseReports::Enable
        )?;
        self.refresh();
        let mut selection_tick = Instant::now();
        loop {
            if selection_tick.elapsed() >= Duration::from_millis(50) {
                self.view.tick_text_selection();
                selection_tick = Instant::now();
            }
            self.view.expire_messages(Instant::now());
            if schedule.due(Instant::now()) {
                self.refresh();
                schedule.refreshed(Instant::now());
            }
            terminal.draw(|frame| {
                let area = frame.area();
                self.refresh_log(self.view.log_event_count(area));
                self.refresh_usage();
                self.view.render(area, frame.buffer_mut());
                if let Some((x, y)) = self.view.cursor_pos(area) {
                    frame.set_cursor_position((x, y));
                }
            })?;
            // The hidden cursor otherwise rests wherever the last repainted cell was, and a
            // terminal's cursor-line guide follows it there; park it on the selected row's marker.
            if self.view.cursor_pos(terminal.get_frame().area()).is_none()
                && let Some((x, y)) = self.view.marker_pos()
            {
                terminal.set_cursor_position((x, y))?;
            }
            let timeout = schedule.redraw_timeout(Instant::now(), self.view.has_background());
            let timeout = self
                .view
                .log_redraw_in(chrono::Utc::now())
                .map_or(timeout, |log| timeout.min(log));
            let timeout = self
                .view
                .expire_messages(Instant::now())
                .map_or(timeout, |message| timeout.min(message));
            let timeout = self
                .view
                .usage_redraw_in(chrono::Utc::now())
                .map_or(timeout, |usage| timeout.min(usage));
            let timeout =
                crate::ui::commits::redraw_in().map_or(timeout, |links| timeout.min(links));
            let timeout = if self.view.text_selection_dragging() {
                timeout.min(Duration::from_millis(50).saturating_sub(selection_tick.elapsed()))
            } else {
                timeout
            };
            match receiver.recv_timeout(timeout) {
                Ok(input) => {
                    if self.handle_inputs(input, &receiver, &mut watcher, &mut schedule)? {
                        return Ok(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("event readers disconnected"));
                }
            }
        }
    }

    /// Handles `first` and every input already queued behind it, so a burst of wheel reports
    /// costs one draw instead of one per report. Returns true when the dashboard should exit.
    fn handle_inputs(
        &mut self,
        first: Input,
        receiver: &mpsc::Receiver<Input>,
        watcher: &mut Option<notify::RecommendedWatcher>,
        schedule: &mut RefreshSchedule,
    ) -> io::Result<bool> {
        for input in std::iter::once(first).chain(std::iter::from_fn(|| receiver.try_recv().ok())) {
            match input {
                Input::Terminal(event) => {
                    if self.handle_event(event?) {
                        return Ok(true);
                    }
                }
                Input::Filesystem(Ok(())) if watcher.is_some() => {
                    schedule.changed(Instant::now());
                }
                Input::Filesystem(Err(error)) if watcher.is_some() => {
                    self.view
                        .set_notice(Some(format!("Falling back to polling: {error}")));
                    *watcher = None;
                    *schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
                }
                Input::Filesystem(_) => {}
            }
        }
        Ok(false)
    }

    /// `background` is the terminal's reported color, when known.
    fn apply_background(&mut self, background: Option<(u8, u8, u8)>, light: bool) {
        crate::ui::set_light_background(light);
        crate::theme::set_background(background, light);
        crate::theme::use_default_accents(light);
        if let Some(path) = &self.preferences.theme
            && let Err(error) = crate::theme::use_theme_file(path, light)
        {
            self.view
                .set_notice(Some(format!("Theme not loaded: {error}")));
        }
        if let Err(error) = crate::ui::use_highlight_theme(light, self.preferences.theme.as_deref())
        {
            self.view
                .set_notice(Some(format!("Theme not loaded: {error}")));
        }
    }

    fn handle_event(&mut self, event: Event) -> bool {
        let action = match event {
            Event::Key(key) => self.view.handle_key(key),
            Event::Mouse(mouse) => self.view.handle_mouse(mouse),
            Event::ThemeChanged(theme) => {
                self.apply_background(None, theme == Theme::Light);
                return false;
            }
            _ => return false,
        };
        self.handle_action(action)
    }

    fn refresh_log(&mut self, visible: usize) {
        let count = visible.saturating_add(self.view.log_scroll());
        let selected = self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .and_then(|session| {
                Some((
                    session.id.clone(),
                    session.transcript.clone()?,
                    session.clone(),
                ))
            });
        let Some((session, path, selected_session)) = selected else {
            self.log_stamp = None;
            self.full_log_stamp = None;
            self.view.log.clear();
            self.view.log_session = None;
            return;
        };
        if self.view.log_search.active() {
            let load = || -> io::Result<Option<(LogStamp, Vec<crate::log::Event>)>> {
                let metadata = std::fs::metadata(&path)?;
                let stamp = LogStamp {
                    session: session.clone(),
                    path: path.clone(),
                    modified: metadata.modified()?,
                    length: metadata.len(),
                    count: usize::MAX,
                };
                if self.full_log_stamp.as_ref() == Some(&stamp) {
                    return Ok(None);
                }
                let events = self.hub.provider(session.provider).log(
                    &selected_session,
                    usize::MAX,
                    selected_session.parent.is_some(),
                )?;
                Ok(Some((stamp, events)))
            };
            match load() {
                Ok(loaded) => {
                    if let Some((stamp, events)) = loaded {
                        self.full_log_stamp = Some(stamp);
                        self.view.set_log(events);
                    }
                    self.log_stamp = None;
                    self.view.log_session = Some(session);
                    self.view
                        .clamp_log_scroll(self.view.matching_log_count().saturating_sub(visible));
                }
                Err(error) => {
                    self.full_log_stamp = None;
                    self.log_stamp = None;
                    self.view.log.clear();
                    self.view.prepare_log_search();
                    self.view.log_session = None;
                    self.view
                        .set_notice(Some(format!("Failed to read task log: {error}")));
                }
            }
            return;
        }
        self.full_log_stamp = None;
        let load = || -> io::Result<Option<(LogStamp, Vec<crate::log::Event>)>> {
            let metadata = std::fs::metadata(&path)?;
            let stamp = LogStamp {
                session: session.clone(),
                path: path.clone(),
                modified: metadata.modified()?,
                length: metadata.len(),
                count,
            };
            if self.log_stamp.as_ref() == Some(&stamp) {
                return Ok(None);
            }
            let events = self.hub.provider(session.provider).log(
                &selected_session,
                count,
                selected_session.parent.is_some(),
            )?;
            Ok(Some((stamp, events)))
        };
        match load() {
            Ok(Some((mut stamp, events))) => {
                if events.len() < count {
                    self.view
                        .clamp_log_scroll(events.len().saturating_sub(visible));
                    stamp.count = visible.saturating_add(self.view.log_scroll());
                }
                self.log_stamp = Some(stamp);
                let scroll = self.view.log_scroll();
                self.view.set_log(events);
                self.view.log_session = Some(session);
                // Keeping the rows still scrolled further back than the loaded window reaches.
                if self.view.log_scroll() > scroll {
                    self.refresh_log(visible);
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.log_stamp = None;
                self.view.log.clear();
                self.view.log_session = None;
                self.view
                    .set_notice(Some(format!("Failed to read task log: {error}")));
            }
        }
    }

    /// Reads what the selected task's transcript gained and hands its usage to the view.
    fn refresh_usage(&mut self) {
        let Some(session) = self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
        else {
            return;
        };
        let Some(path) = &session.transcript else {
            return;
        };
        let series = self.usage.entry(path.clone()).or_default();
        let Ok(changed) = series.update(path, session.id.provider) else {
            return;
        };
        if changed || self.view.usage_session() != Some(&session.id) {
            self.view.set_usage(&session.id, series.bins.clone());
        }
    }

    fn refresh(&mut self) {
        let Snapshot { sessions, has_more } = self.hub.refresh();
        self.sessions = sessions.clone();
        self.view.set_loading(false);
        self.view.set_sessions(sessions, has_more);
    }

    /// Returns true when the dashboard should close.
    fn handle_action(&mut self, action: Option<Action>) -> bool {
        let Some(action) = action else {
            return false;
        };
        let target = match &action {
            Action::Open(id)
            | Action::Resume(id)
            | Action::Fork(id)
            | Action::Rename { id, .. }
            | Action::Delete(id)
            | Action::Archive(id)
            | Action::Stop(id)
            | Action::TogglePin { id, .. } => Some(id),
            _ => None,
        };
        if let Some(session) = target.and_then(|id| self.session(id))
            && !session.can_resume()
        {
            if matches!(action, Action::Open(_) | Action::Resume(_)) {
                return self.handle_action(Some(Action::Unsupported("Resume")));
            }
            return false;
        }
        match action {
            Action::Quit => return true,
            Action::Back => {
                let Some(session) = self.last_session.as_ref().and_then(|id| self.session(id))
                else {
                    return self.launch_new();
                };
                let id = session.id.clone();
                return self.launch_session(Action::Resume(id), session);
            }
            Action::ShowMore => {
                self.hub.show_more();
                self.view.set_loading(true);
                self.refresh();
            }
            Action::Open(id) | Action::Resume(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_session(Action::Resume(id), session);
                }
            }
            Action::Fork(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_session(Action::Fork(id), session);
                }
            }
            Action::New => {
                return self.launch_new();
            }
            Action::Rename { id, name } => {
                if let Some(session) = self.session(&id) {
                    if session.transcript.is_some() {
                        match self.hub.provider(id.provider).rename(&session, &name) {
                            Ok(()) => self.refresh(),
                            Err(error) => self
                                .view
                                .set_notice(Some(format!("Failed to rename: {error}"))),
                        }
                    } else {
                        self.view
                            .set_notice(Some("Cannot rename: session has no transcript".into()));
                    }
                }
            }
            Action::Delete(id) => {
                self.refresh();
                if let Some(session) = self.session(&id) {
                    if !session.can_resume() {
                        return false;
                    }
                    if session.transcript.is_none() {
                        self.view.set_notice(Some("Nothing to delete".into()));
                    } else {
                        match self.hub.provider(id.provider).delete(&session) {
                            Ok(()) => {
                                self.view.hide(&id);
                                self.refresh();
                            }
                            Err(error) => {
                                self.refresh();
                                self.view
                                    .set_notice(Some(format!("Failed to delete: {error}")));
                            }
                        }
                    }
                } else {
                    self.view.set_notice(Some("Nothing to delete".into()));
                }
            }
            Action::Archive(id) => {
                if let Some(session) = self.session(&id) {
                    match self.hub.provider(id.provider).archive(&session) {
                        Ok(()) => {
                            self.view.hide(&id);
                            self.refresh();
                        }
                        Err(error) => self
                            .view
                            .set_notice(Some(format!("Failed to archive: {error}"))),
                    }
                }
            }
            Action::Stop(id) => {
                if let Some(session) = self.session(&id) {
                    match self.hub.provider(id.provider).stop(&session) {
                        Ok(()) => self.refresh(),
                        Err(error) => self
                            .view
                            .set_notice(Some(format!("Failed to stop: {error}"))),
                    }
                }
            }
            Action::Hide(_) => {}
            Action::TogglePin { id, pinned } => {
                self.preferences.pinned.retain(|pinned_id| pinned_id != &id);
                if pinned {
                    self.preferences.pinned.push(id);
                }
                self.save_preferences();
                self.view.set_pinned(&self.preferences.pinned);
            }
            Action::PersistGrouping(grouping) => {
                self.preferences.grouping = grouping;
                self.save_preferences();
            }
            Action::Copy { text, label } => match self.clipboard.copy(&text) {
                Ok(()) => self.view.show_copied(label, Instant::now()),
                Err(error) => self
                    .view
                    .set_notice(Some(format!("Failed to copy: {error}"))),
            },
            Action::Unsupported(what) => {
                self.view
                    .set_notice(Some(format!("{what} is not supported")));
            }
            Action::OpenPath(target) => {
                let link = crate::ui::vscode::open_target(&target);
                let opened = Command::new("open")
                    .arg(link.as_deref().map_or(target.as_os_str(), OsStr::new))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match opened {
                    // Reaping off the loop keeps `open` from lingering as a zombie.
                    Ok(mut child) => {
                        thread::spawn(move || child.wait());
                    }
                    Err(error) => self.view.set_notice(Some(format!(
                        "Failed to open {}: {error}",
                        target.display()
                    ))),
                }
            }
        }
        false
    }

    fn launch_new(&mut self) -> bool {
        self.launch = self
            .hub
            .provider(ProviderId::Claude)
            .launch(&Action::New, None);
        self.launch.is_some()
    }

    fn session(&self, id: &SessionKey) -> Option<Session> {
        self.sessions
            .iter()
            .find(|session| &session.id == id)
            .cloned()
    }

    fn save_preferences(&mut self) {
        if let Err(error) = self.preferences.save(&self.config_dir) {
            self.view
                .set_notice(Some(format!("Failed to save preferences: {error}")));
        }
    }

    /// Hands the terminal to the selected session's provider process.
    fn launch_session(&mut self, action: Action, session: Session) -> bool {
        if !session.can_resume() {
            return false;
        }
        if !session.cwd.is_dir() {
            self.view.set_notice(Some(format!(
                "Directory no longer exists: {}",
                session.cwd.display()
            )));
            return false;
        }
        self.launch = self
            .hub
            .provider(session.id.provider)
            .launch(&action, Some(&session));
        if self.launch.is_some() {
            self.last_session = Some(session.id);
            return true;
        }
        false
    }
}

enum MouseReports {
    Enable,
    Disable,
}

impl crossterm::Command for MouseReports {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str(match self {
            Self::Enable => "\x1b[?1000h\x1b[?1002h\x1b[?1006h",
            Self::Disable => "\x1b[?1006l\x1b[?1002l\x1b[?1000l",
        })
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Mouse reporting requires ANSI support",
        ))
    }
}

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, terminal::EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn leave_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let reports = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableThemeChangeReports,
        MouseReports::Disable
    );
    terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    reports
}

#[cfg(test)]
#[path = "providers/claude/app_tests.rs"]
mod tests;
