//! Terminal loop: reacts to session changes and keyboard input, renders the view and executes actions.

use crate::model::Action;
use crate::model::Session;
use crate::refresh::{FALLBACK_INTERVAL, POLLING_INTERVAL, RefreshSchedule};
use crate::sessions::Snapshot;
use crate::sessions::Store;
use crate::state::Preferences;
use crate::terminal_background;
use crate::ui::View;
use crossterm::event::Event;
use crossterm::event::KeyEvent;
use crossterm::terminal;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::path::Path;
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

// Bounds reader shutdown before handing the terminal to Claude.
const INPUT_STOP_INTERVAL: Duration = Duration::from_millis(250);

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
    store: Store,
    preferences: Preferences,
    view: View,
    sessions: Vec<Session>,
    /// Set when the user chose to hand the terminal to a `claude` process; the dashboard
    /// returns once it exits.
    launch: Option<Command>,
    last_session: Option<String>,
    log_stamp: Option<LogStamp>,
}

#[derive(PartialEq, Eq)]
struct LogStamp {
    session: String,
    path: PathBuf,
    modified: std::time::SystemTime,
    length: u64,
}

impl App {
    pub fn new(config_dir: PathBuf) -> Self {
        let preferences = Preferences::load(&config_dir);
        let mut view = View::new(preferences.grouping, true);
        view.set_pinned(&preferences.pinned);
        view.set_loading(true);
        Self {
            store: Store::new(config_dir.clone()),
            config_dir,
            preferences,
            view,
            sessions: Vec::new(),
            launch: None,
            last_session: None,
            log_stamp: None,
        }
    }

    pub fn run(mut self) -> io::Result<()> {
        loop {
            let mut terminal = enter_terminal()?;
            // Raw mode is on, so the terminal's reply reaches us instead of the screen.
            let light = terminal_background::query(terminal_background::DEFAULT_TIMEOUT)
                .ok()
                .flatten()
                .is_some_and(terminal_background::is_light);
            crate::ui::set_light_background(light);
            crate::theme::use_default_accents(light);
            if let Some(path) = self.preferences.theme.clone()
                && let Err(error) = crate::theme::use_theme_file(&path)
            {
                self.view
                    .set_notice(Some(format!("Theme not loaded: {error}")));
            }
            let result = self.event_loop(&mut terminal);
            leave_terminal(terminal)?;
            result?;
            let Some(mut command) = self.launch.take() else {
                return Ok(());
            };
            // The dashboard comes back when claude exits, so Ctrl+D in claude returns here.
            let notice = match command.status() {
                Ok(status) if status.success() => None,
                Ok(status) => Some(format!("claude exited with {status}")),
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
        let mut watcher = match crate::sessions::watch(&self.config_dir, move |event| {
            let _ = filesystem.send(Input::Filesystem(event.map(|_| ())));
        }) {
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
        self.refresh();
        loop {
            if schedule.due(Instant::now()) {
                self.refresh();
                schedule.refreshed(Instant::now());
            }
            terminal.draw(|frame| {
                let area = frame.area();
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
            match receiver.recv_timeout(schedule.redraw_timeout(
                Instant::now(),
                self.view.has_background() || self.log_ticks(),
            )) {
                Ok(Input::Terminal(event)) => match event? {
                    Event::Key(key) if self.handle_key(key) => return Ok(()),
                    _ => {}
                },
                Ok(Input::Filesystem(Ok(()))) if watcher.is_some() => {
                    schedule.changed(Instant::now());
                }
                Ok(Input::Filesystem(Err(error))) if watcher.is_some() => {
                    self.view
                        .set_notice(Some(format!("Falling back to polling: {error}")));
                    watcher = None;
                    schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
                }
                Ok(Input::Filesystem(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("event readers disconnected"));
                }
            }
        }
    }

    fn log_ticks(&self) -> bool {
        self.view.log.last().is_some_and(|event| {
            matches!(
                event.kind,
                crate::sessions::log::Kind::Tool
                    | crate::sessions::log::Kind::Error
                    | crate::sessions::log::Kind::Denied
            ) && event.ended_at.is_none()
                && !event.body.ends_with(" *")
        }) && self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|s| s.id == id))
            .is_some_and(|session| {
                session.process.is_some() && session.status != crate::model::SessionStatus::Inactive
            })
    }

    fn refresh_log(&mut self) {
        let selected = self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|session| session.id == id))
            .and_then(|session| Some((session.id.clone(), session.transcript.clone()?)));
        let Some((session, path)) = selected else {
            self.log_stamp = None;
            self.view.log.clear();
            self.view.log_session = None;
            return;
        };
        let load = || -> io::Result<Option<(LogStamp, Vec<crate::sessions::log::Event>)>> {
            let metadata = std::fs::metadata(&path)?;
            let stamp = LogStamp {
                session: session.clone(),
                path: path.clone(),
                modified: metadata.modified()?,
                length: metadata.len(),
            };
            if self.log_stamp.as_ref() == Some(&stamp) {
                return Ok(None);
            }
            Ok(Some((stamp, crate::sessions::log::tail(&path, 8)?)))
        };
        match load() {
            Ok(Some((stamp, events))) => {
                self.log_stamp = Some(stamp);
                self.view.log = events;
                self.view.log_session = Some(session);
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

    fn refresh(&mut self) {
        let Snapshot { sessions, has_more } = self.store.refresh();
        self.sessions = sessions.clone();
        self.view.set_loading(false);
        self.view.set_sessions(sessions, has_more);
        self.refresh_log();
    }

    /// Returns true when the dashboard should close.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let action = self.view.handle_key(key);
        self.refresh_log();
        let Some(action) = action else {
            return false;
        };
        match action {
            Action::Quit => return true,
            Action::Back => {
                let Some(session) = self.last_session.as_deref().and_then(|id| self.session(id))
                else {
                    return self.launch_new(None);
                };
                let id = session.id.clone();
                return self.launch_claude(session, &["--resume", &id]);
            }
            Action::ShowMore => {
                self.store.show_more();
                self.view.set_loading(true);
                self.refresh();
            }
            Action::Open(id) | Action::Resume(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_claude(session, &["--resume", &id]);
                }
            }
            Action::Fork(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_claude(session, &["--resume", &id, "--fork-session"]);
                }
            }
            Action::New(cwd) => {
                return self.launch_new(cwd);
            }
            Action::Rename { id, name } => {
                if let Some(session) = self.session(&id) {
                    if let Some(path) = session.transcript {
                        match crate::sessions::rename(&path, &id, &name) {
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
                    if session.transcript.is_none() {
                        self.view.set_notice(Some("Nothing to delete".into()));
                    } else {
                        match crate::sessions::delete_session(&self.config_dir, &session) {
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
            Action::Unsupported(what) => {
                self.view
                    .set_notice(Some(format!("{what} is not supported")));
            }
        }
        self.refresh_log();
        false
    }

    fn launch_new(&mut self, cwd: Option<PathBuf>) -> bool {
        let cwd = cwd
            .filter(|cwd| cwd.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        let mut command = Command::new("claude");
        command.current_dir(cwd);
        self.launch = Some(command);
        true
    }

    fn session(&self, id: &str) -> Option<Session> {
        self.sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
    }

    fn save_preferences(&mut self) {
        if let Err(error) = self.preferences.save(&self.config_dir) {
            self.view
                .set_notice(Some(format!("Failed to save preferences: {error}")));
        }
    }

    /// Hands the terminal to `claude <args>` in the session's directory until it exits.
    fn launch_claude(&mut self, session: Session, args: &[&str]) -> bool {
        let cwd = if session.cwd.is_dir() {
            session.cwd.clone()
        } else {
            self.view.set_notice(Some(format!(
                "Directory no longer exists: {}",
                session.cwd.display()
            )));
            return false;
        };
        let mut command = Command::new("claude");
        command.args(args).current_dir(cwd);
        self.last_session = Some(session.id);
        self.launch = Some(command);
        true
    }
}

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, terminal::EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn leave_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    terminal::disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()
}

pub fn config_dir_exists(config_dir: &Path) -> bool {
    config_dir.is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionStatus;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn fixture() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_str!("../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut app = App::new(dir.path().to_owned());
        app.refresh();
        assert_eq!(app.sessions.len(), 1);
        app.sessions[0].cwd = dir.path().to_owned();
        app.sessions[0].status = SessionStatus::Ready;
        app.view.set_sessions(app.sessions.clone(), false);
        (dir, app)
    }

    fn key(app: &mut App, code: KeyCode) -> bool {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn assert_resume(app: &App) {
        let command = app.launch.as_ref().expect("session should resume");
        assert_eq!(command.get_program(), "claude");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--resume", app.sessions[0].id.as_str()]
        );
        assert_eq!(
            command.get_current_dir(),
            Some(app.sessions[0].cwd.as_path())
        );
    }

    #[test]
    fn task_log_cache_uses_selection_length_and_mtime() {
        use std::io::Write;
        let (_dir, mut app) = fixture();
        key(&mut app, KeyCode::BackTab);
        app.refresh();
        assert_eq!(app.view.log.len(), 2);
        let path = app.sessions[0].transcript.clone().unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let changed = original.replace("Codex", "CODEX");
        assert_eq!(original.len(), changed.len());
        std::fs::write(&path, &changed).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(modified).unwrap();
        let cached = app.view.log.clone();
        app.refresh_log();
        assert_eq!(app.view.log, cached);
        file.set_modified(modified + Duration::from_secs(1))
            .unwrap();
        app.refresh_log();
        assert_ne!(app.view.log, cached);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", serde_json::json!({"type":"queue-operation", "operation":"enqueue", "timestamp":"2026-10-06T16:40:00Z", "content":"queued"})).unwrap();
        file.set_modified(modified + Duration::from_secs(1))
            .unwrap();
        app.refresh();
        assert_eq!(app.sessions.len(), 1);
        assert!(
            app.view.selected_id().is_some(),
            "selected disappeared after refresh"
        );
        assert_eq!(app.view.log.last().unwrap().body, "queued");
    }

    #[test]
    fn task_log_reloads_on_keyboard_selection_and_clears_without_transcript() {
        let (dir, mut app) = fixture();
        let mut second = app.sessions[0].clone();
        second.id = "second".into();
        second.updated_at -= chrono::Duration::seconds(1);
        let path = dir.path().join("second.jsonl");
        std::fs::write(&path, serde_json::json!({"type":"user", "timestamp":"2026-10-06T16:40:00Z", "message":{"content":"second prompt"}}).to_string()).unwrap();
        second.transcript = Some(path);
        let mut third = second.clone();
        third.id = "third".into();
        third.updated_at -= chrono::Duration::seconds(1);
        third.transcript = None;
        app.sessions.extend([second, third]);
        app.view.set_sessions(app.sessions.clone(), false);
        app.refresh_log();
        key(&mut app, KeyCode::Down);
        assert_eq!(app.view.log_session.as_deref(), Some("second"));
        assert_eq!(app.view.log[0].body, "second prompt");
        key(&mut app, KeyCode::Down);
        assert!(app.view.log.is_empty());
        assert!(app.view.log_session.is_none());
    }

    #[test]
    fn escape_without_launched_session_opens_new_session() {
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Esc));
        let command = app.launch.as_ref().expect("new session should launch");
        assert_eq!(command.get_program(), "claude");
        assert_eq!(command.get_args().len(), 0);
        assert_eq!(
            command.get_current_dir(),
            Some(std::env::current_dir().unwrap().as_path())
        );
        assert!(app.last_session.is_none());
    }

    #[test]
    fn escape_resumes_last_opened_session() {
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Enter));
        app.launch.take().unwrap();
        // True leaves the dashboard for either quit or launch; the command distinguishes them.
        assert!(key(&mut app, KeyCode::Esc));
        assert_resume(&app);
    }

    #[test]
    fn escape_after_session_removed_opens_new_session() {
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Enter));
        app.launch.take().unwrap();
        let last_session = app.last_session.clone();
        app.sessions.clear();
        app.view.set_sessions(Vec::new(), false);
        assert!(key(&mut app, KeyCode::Esc));
        let command = app.launch.as_ref().expect("new session should launch");
        assert_eq!(command.get_program(), "claude");
        assert_eq!(command.get_args().len(), 0);
        assert_eq!(
            command.get_current_dir(),
            Some(std::env::current_dir().unwrap().as_path())
        );
        assert_eq!(app.last_session, last_session);
    }

    #[test]
    fn escape_remembers_resume_and_fork_but_not_new() {
        for code in [KeyCode::Char('o'), KeyCode::Char('f')] {
            let (_dir, mut app) = fixture();
            assert!(key(&mut app, code));
            app.launch.take().unwrap();
            assert!(key(&mut app, KeyCode::Char('n')));
            app.launch.take().unwrap();
            assert!(key(&mut app, KeyCode::Esc));
            assert_resume(&app);
        }
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Char('n')));
        app.launch.take().unwrap();
        assert!(key(&mut app, KeyCode::Esc));
        assert!(app.launch.is_some());
        assert!(app.last_session.is_none());
    }

    #[test]
    fn escape_with_missing_directory_stays_in_dashboard() {
        let (dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Enter));
        app.launch.take().unwrap();
        app.sessions[0].cwd = dir.path().join("missing");
        assert!(!key(&mut app, KeyCode::Esc));
        assert!(app.launch.is_none());
    }

    #[test]
    fn control_c_after_open_quits() {
        let (_dir, mut app) = fixture();
        assert!(key(&mut app, KeyCode::Enter));
        app.launch.take().unwrap();
        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(app.launch.is_none());
    }
}
