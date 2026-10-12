//! Terminal loop: polls the session sources, renders the view and executes its actions.

use crate::model::Action;
use crate::model::Session;
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
use std::time::Duration;
use std::time::Instant;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct App {
    config_dir: PathBuf,
    store: Store,
    preferences: Preferences,
    view: View,
    sessions: Vec<Session>,
    /// Set when the user chose to hand the terminal to a `claude` process; the dashboard
    /// returns once it exits.
    launch: Option<Command>,
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
        let mut last_refresh = Instant::now() - REFRESH_INTERVAL;
        loop {
            if last_refresh.elapsed() >= REFRESH_INTERVAL {
                self.refresh();
                last_refresh = Instant::now();
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
            if crossterm::event::poll(POLL_INTERVAL)? {
                match crossterm::event::read()? {
                    Event::Key(key) if self.handle_key(key) => {
                        return Ok(());
                    }
                    Event::Resize(..) => {}
                    _ => {}
                }
            }
        }
    }

    fn refresh(&mut self) {
        let Snapshot { sessions, has_more } = self.store.refresh();
        self.sessions = sessions.clone();
        self.view.set_loading(false);
        self.view.set_sessions(sessions, has_more);
    }

    /// Returns true when the dashboard should close.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let Some(action) = self.view.handle_key(key) else {
            return false;
        };
        match action {
            Action::Quit => return true,
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
                let cwd = cwd
                    .filter(|cwd| cwd.is_dir())
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("/"));
                let mut command = Command::new("claude");
                command.current_dir(cwd);
                self.launch = Some(command);
                return true;
            }
            Action::Stop(id) => {
                if let Some(session) = self.session(&id) {
                    let notice = interrupt(&session);
                    self.view.set_notice(Some(notice));
                }
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
                self.view.set_notice(Some(format!(
                    "{what} is not supported for Claude Code sessions"
                )));
            }
        }
        false
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
        self.launch = Some(command);
        true
    }
}

/// Splits `session:@window.%pane` into its window and pane targets.
fn tmux_target(target: &str) -> Option<(&str, &str, &str)> {
    let (window, pane) = target.rsplit_once('.')?;
    let (tmux_session, _) = window.split_once(':')?;
    Some((tmux_session, window, pane))
}

fn tmux(args: &[&str]) -> Result<String, String> {
    match Command::new("tmux").args(args).output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        }
        Ok(output) => Err(format!(
            "tmux {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("tmux unavailable: {error}")),
    }
}

/// Stops the running turn the way the user would: Esc in the session's pane.
fn interrupt(session: &Session) -> String {
    let Some(process) = session.process.as_ref() else {
        return "Session is not running".to_owned();
    };
    let Some(target) = process.tmux.as_deref() else {
        return format!(
            "Session runs outside tmux (pid {}); press esc there",
            process.pid
        );
    };
    let Some((_, _, pane)) = tmux_target(target) else {
        return format!("Unexpected tmux target {target}");
    };
    match tmux(&["send-keys", "-t", pane, "Escape"]) {
        Ok(_) => format!("Sent esc to {}", session.display_title()),
        Err(error) => error,
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
