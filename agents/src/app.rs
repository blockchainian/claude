//! Terminal loop: reacts to session changes and keyboard input, renders the view and executes actions.

use crate::model::Action;
use crate::model::{DraftId, Session};
use crate::providers::Snapshot;
use crate::providers::{Job, ProviderId, SessionInput, SessionKey};
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
    WeeklyUsage {
        provider: ProviderId,
        result: io::Result<Option<crate::providers::WeeklyUsage>>,
    },
    Terminal(io::Result<Event>),
    Filesystem(notify::Result<()>),
    LogLoaded {
        token: u64,
        reader: Box<dyn crate::log::Reader>,
        result: io::Result<(LogStamp, crate::log::Update)>,
    },
    Created {
        draft_id: DraftId,
        provider: ProviderId,
        result: io::Result<std::process::Output>,
    },
    /// A new task's worktree, with the task request to create in it.
    WorktreeCreated {
        draft_id: DraftId,
        provider: ProviderId,
        request: crate::tasks::NewTask,
        result: io::Result<(PathBuf, String)>,
    },
    Sent {
        id: SessionKey,
        input: SessionInput,
        result: io::Result<()>,
    },
    Deleted {
        id: SessionKey,
        result: io::Result<()>,
    },
    Archived {
        id: SessionKey,
        result: io::Result<()>,
    },
    /// The session terminal's screen changed or its program closed the terminal.
    TerminalOutput,
    /// A terminal client has detached or been killed and reaped.
    TerminalClosed {
        id: SessionKey,
        result: io::Result<()>,
    },
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
    sender: mpsc::Sender<Input>,
    receiver: mpsc::Receiver<Input>,
    pending_inputs: std::collections::HashSet<SessionKey>,
    config_dir: PathBuf,
    hub: Hub,
    preferences: Preferences,
    view: View,
    clipboard: crate::clipboard::Clipboard,
    sessions: Vec<Session>,
    /// Set when the user chose to hand the terminal to a provider process; the dashboard
    /// returns once it exits.
    launch: Option<Command>,
    /// The session program running in the log detail panel.
    terminal: Option<crate::session_terminal::SessionTerminal>,
    /// The task whose terminal opens once that task's previous client has closed.
    terminal_task: Option<SessionKey>,
    /// The waiting task whose terminal opened on its own, so closing it keeps it closed.
    waiting_terminal: Option<SessionKey>,
    /// Clients being reaped before the same session can open another terminal.
    closing_terminals: std::collections::HashMap<SessionKey, session_terminal::TerminalClose>,
    log_stamp: Option<LogStamp>,
    full_log_stamp: Option<LogStamp>,
    task_log: Option<(SessionKey, PathBuf, Box<dyn crate::log::Reader>)>,
    pending_log: Option<PendingLog>,
    log_request: u64,
    log_oldest_offset: u64,
    log_parsed_bytes: u64,
    /// Token usage of each transcript the user has selected, kept up to date while selected.
    usage: std::collections::HashMap<PathBuf, crate::usage::Series>,
    /// When each recently active transcript's turns ran.
    activity: std::collections::HashMap<PathBuf, crate::activity::Turns>,
}

#[derive(PartialEq, Eq)]
struct LogStamp {
    session: SessionKey,
    path: PathBuf,
    modified: std::time::SystemTime,
    length: u64,
    count: usize,
}

struct PendingLog {
    token: u64,
    session: SessionKey,
    path: PathBuf,
    visible: usize,
}

/// A new task's directory and branch: the repository's current branch, or none outside git.
pub(crate) fn draft_location(cwd: PathBuf) -> io::Result<(PathBuf, String)> {
    let cwd = std::fs::canonicalize(cwd)?;
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output()?;
    if !output.status.success() {
        if cwd.ancestors().any(|path| path.join(".git").exists()) {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        return Ok((cwd, String::new()));
    }
    let output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&cwd)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let branch = String::from_utf8(output.stdout)
        .map_err(io::Error::other)?
        .trim()
        .to_owned();
    Ok((cwd, branch))
}

impl App {
    pub fn new(mut hub: Hub, config_dir: PathBuf) -> Self {
        let preferences = Preferences::load(&config_dir);
        let mut view = View::new(preferences.grouping, true);
        hub.set_weekly_usage(preferences.usage.clone());
        view.weekly_usage = preferences.usage.clone();
        view.usage_view = preferences.usage_view;
        view.set_pinned(&preferences.pinned);
        view.set_drafts(&preferences.drafts);
        if !preferences.draft_notices.is_empty() {
            view.set_notice(Some(preferences.draft_notices.join("; ")));
        }
        view.set_double_click_ms(preferences.double_click_ms);
        view.set_loading(true);
        view.set_capabilities(hub.capabilities());
        view.set_input_choices(hub.input_choices());
        view.provider_names = hub.display_names();
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            pending_inputs: std::collections::HashSet::new(),
            hub,
            config_dir,
            preferences,
            view,
            clipboard: crate::clipboard::Clipboard::new(),
            sessions: Vec::new(),
            launch: None,
            terminal: None,
            terminal_task: None,
            waiting_terminal: None,
            closing_terminals: std::collections::HashMap::new(),
            log_stamp: None,
            full_log_stamp: None,
            task_log: None,
            pending_log: None,
            log_request: 0,
            log_oldest_offset: 0,
            log_parsed_bytes: 0,
            usage: std::collections::HashMap::new(),
            activity: std::collections::HashMap::new(),
        }
    }

    pub fn run(mut self) -> io::Result<()> {
        loop {
            let mut terminal = enter_terminal()?;
            // Raw mode is on, so the terminal's reply reaches us instead of the screen.
            let background = terminal_background::query(terminal_background::DEFAULT_TIMEOUT)
                .ok()
                .flatten();
            self.apply_background(background.is_some_and(terminal_background::is_light));
            let result = self.event_loop(&mut terminal);
            self.close_terminal();
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
        let filesystem = self.sender.clone();
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
        let _reader = InputReader::start(self.sender.clone())?;
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
            let persist = self.view.take_drafts_action();
            self.handle_action(persist);
            if selection_tick.elapsed() >= Duration::from_millis(50) {
                self.view.tick_text_selection();
                selection_tick = Instant::now();
            }
            self.view.expire_messages(Instant::now());
            self.refresh_weekly_usage();
            if schedule.due(Instant::now()) {
                self.refresh();
                schedule.refreshed(Instant::now());
            }
            self.view.set_cell_aspect(cell_aspect());
            self.sync_terminal();
            self.draw(terminal)?;
            if self.wait_for_input(&mut watcher, &mut schedule, selection_tick)? {
                return Ok(());
            }
        }
    }

    fn draw<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), B::Error> {
        self.draw_at(terminal, chrono::Utc::now())
    }

    fn draw_at<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), B::Error> {
        // Keep the drawing cursor hidden until the frame's input cursor is in place.
        terminal.hide_cursor()?;
        let mut cursor = None;
        terminal.draw(|frame| {
            let area = frame.area();
            self.refresh_log(self.view.log_event_count(area));
            self.refresh_usage();
            self.refresh_activity(area.width, now);
            self.view.render_at(area, frame.buffer_mut(), now);
            self.view.underline_links(frame.buffer_mut());
            cursor = self.view.cursor_pos(area);
        })?;
        self.terminal_drawn();
        if let Some(position) = cursor {
            terminal.set_cursor_position(position)?;
            terminal.show_cursor()?;
        } else if let Some(position) = self.view.marker_pos() {
            // The hidden cursor otherwise rests wherever the last repainted cell was, and a
            // terminal's cursor-line guide follows it there; park it on the selected row's marker.
            terminal.set_cursor_position(position)?;
        }
        Ok(())
    }

    fn wait_for_input(
        &mut self,
        watcher: &mut Option<notify::RecommendedWatcher>,
        schedule: &mut RefreshSchedule,
        selection_tick: Instant,
    ) -> io::Result<bool> {
        let timeout =
            self.input_timeout(schedule, selection_tick, Instant::now(), chrono::Utc::now());
        match self.receiver.recv_timeout(timeout) {
            Ok(input) => {
                if self.handle_inputs(input, watcher, schedule)? {
                    return Ok(true);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("event readers disconnected"));
            }
        }
        let action = self.view.flush_pending_click(Instant::now());
        Ok(self.handle_action(action))
    }

    fn input_timeout(
        &mut self,
        schedule: &RefreshSchedule,
        selection_tick: Instant,
        now: Instant,
        wall_now: chrono::DateTime<chrono::Utc>,
    ) -> Duration {
        let timeout = schedule.redraw_timeout(now, self.view.has_background());
        let timeout = self
            .view
            .log_redraw_in(wall_now)
            .map_or(timeout, |log| timeout.min(log));
        let timeout = self
            .view
            .tool_redraw_in(wall_now)
            .map_or(timeout, |tool| timeout.min(tool));
        let timeout = self
            .view
            .expire_messages(now)
            .map_or(timeout, |message| timeout.min(message));
        let timeout = self
            .view
            .pending_click_in(now)
            .map_or(timeout, |click| timeout.min(click));
        let timeout = self
            .view
            .timeline_redraw_in(wall_now)
            .map_or(timeout, |usage| timeout.min(usage));
        let timeout = crate::ui::commits::redraw_in().map_or(timeout, |links| timeout.min(links));
        if self.view.text_selection_dragging() {
            timeout.min(
                Duration::from_millis(50)
                    .saturating_sub(now.saturating_duration_since(selection_tick)),
            )
        } else {
            timeout
        }
    }

    /// Handles `first` and every input already queued behind it, so a burst of wheel reports
    /// costs one draw instead of one per report. Returns true when the dashboard should exit.
    fn handle_inputs(
        &mut self,
        first: Input,
        watcher: &mut Option<notify::RecommendedWatcher>,
        schedule: &mut RefreshSchedule,
    ) -> io::Result<bool> {
        let mut next = Some(first);
        while let Some(input) = next.take().or_else(|| self.receiver.try_recv().ok()) {
            match input {
                Input::WeeklyUsage { provider, result } => {
                    self.complete_weekly_usage(provider, result);
                }
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
                Input::LogLoaded {
                    token,
                    reader,
                    result,
                } => {
                    self.finish_log_load(token, reader, result);
                }
                Input::Created {
                    draft_id,
                    provider,
                    result,
                } => {
                    let result =
                        result.and_then(|output| self.hub.provider(provider).created_job(&output));
                    self.view.draft_created(draft_id, result);
                    schedule.changed(Instant::now());
                }
                Input::WorktreeCreated {
                    draft_id,
                    provider,
                    mut request,
                    result,
                } => match result {
                    Ok((cwd, branch)) => {
                        request.cwd = cwd.clone();
                        self.view
                            .draft_worktree_created(draft_id, Ok((cwd, branch)));
                        self.create_task(draft_id, provider, request);
                    }
                    Err(error) => self.view.draft_worktree_created(draft_id, Err(error)),
                },
                Input::Sent { id, input, result } => {
                    self.pending_inputs.remove(&id);
                    if matches!(input, SessionInput::Interrupt) {
                        self.refresh();
                        self.view.stop_complete(&id, result.is_ok());
                        if let Err(error) = result {
                            self.input_error(&input, error.to_string());
                        }
                    } else {
                        self.view.input_complete(&id, &input, result);
                    }
                }
                Input::Deleted { id, result } => self.deleted(&id, result),
                Input::Archived { id, result } => self.archived(&id, result),
                Input::TerminalOutput => {}
                Input::TerminalClosed { id, result } => self.terminal_closed(&id, result),
            }
        }
        Ok(false)
    }

    fn apply_background(&mut self, light: bool) {
        crate::ui::set_light_background(light);
        crate::theme::use_default_accents(light);
        if let Some(path) = &self.preferences.theme
            && let Err(error) = crate::theme::use_theme_file(path, light)
        {
            self.view
                .set_notice(Some(format!("Theme not loaded: {error}")));
        }
        if let Err(error) = crate::ui::use_highlight_theme(light, self.highlight_theme(light)) {
            self.view
                .set_notice(Some(format!("Theme not loaded: {error}")));
        }
    }

    /// The preference `.tmTheme` that highlights code on the given background, if any.
    fn highlight_theme(&self, light: bool) -> Option<&std::path::Path> {
        if light {
            self.preferences.light_theme.as_deref()
        } else {
            self.preferences.dark_theme.as_deref()
        }
    }

    fn handle_event(&mut self, event: Event) -> bool {
        let action = match event {
            Event::Key(key) => self.view.handle_key(key),
            Event::Mouse(mouse) => self.view.handle_mouse(mouse),
            Event::Paste(text) => {
                if let Some(action) = self.view.terminal_paste(&text) {
                    return self.handle_action(Some(action));
                }
                if self.view.pasting_prompt() {
                    match crate::images::ImageAttachment::from_paste(&text) {
                        Ok(Some(image)) => self.view.attach_image(image),
                        Ok(None) => self.view.handle_paste(&text),
                        Err(error) => self.view.set_notice(Some(error.to_string())),
                    }
                } else {
                    self.view.handle_paste(&text);
                }
                return false;
            }
            Event::ThemeChanged(theme) => {
                self.apply_background(theme == Theme::Light);
                return false;
            }
            _ => return false,
        };
        self.handle_action(action)
    }

    fn refresh_log(&mut self, visible: usize) {
        let count = if self.view.log_start_requested || self.view.log_search.active() {
            usize::MAX
        } else {
            self.view
                .log_read_count(visible, self.log_oldest_offset > 0)
        };
        let selected = self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .cloned();
        let Some(selected) = selected.filter(|session| session.transcript.is_some()) else {
            self.log_stamp = None;
            self.full_log_stamp = None;
            self.task_log = None;
            self.pending_log = None;
            self.log_oldest_offset = 0;
            self.view.log.clear();
            self.view.log_session = None;
            return;
        };
        let path = selected.transcript.as_ref().expect("transcript checked");
        if let Some(pending) = &self.pending_log {
            if pending.session == selected.id && &pending.path == path {
                return;
            }
            self.pending_log = None;
        }
        if self
            .task_log
            .as_ref()
            .is_none_or(|(id, cached_path, _)| id != &selected.id || cached_path != path)
        {
            match crate::providers::log_reader(&selected) {
                Ok(reader) => {
                    self.task_log = Some((selected.id.clone(), path.clone(), reader));
                    self.log_oldest_offset = u64::MAX;
                    if self.view.log_session.as_ref() != Some(&selected.id) {
                        self.view.apply_log_update(crate::log::Update {
                            reset: true,
                            ..Default::default()
                        });
                        self.view.log_session = Some(selected.id.clone());
                    }
                }
                Err(error) => {
                    self.apply_log_result(Err(error), visible);
                    return;
                }
            }
        }
        if self.view.log_search.active() && self.log_oldest_offset > 0 {
            let (session, path, mut reader) = self.task_log.take().expect("reader checked");
            self.log_request += 1;
            let token = self.log_request;
            self.pending_log = Some(PendingLog {
                token,
                session: session.clone(),
                path: path.clone(),
                visible,
            });
            let sender = self.sender.clone();
            thread::spawn(move || {
                let result = reader.refresh(usize::MAX).and_then(|update| {
                    let metadata = std::fs::metadata(&path)?;
                    Ok((
                        LogStamp {
                            session,
                            path,
                            modified: metadata.modified()?,
                            length: metadata.len(),
                            count: usize::MAX,
                        },
                        update,
                    ))
                });
                let _ = sender.send(Input::LogLoaded {
                    token,
                    reader,
                    result,
                });
            });
            return;
        }
        let result = (|| -> io::Result<(LogStamp, crate::log::Update)> {
            let update = self
                .task_log
                .as_mut()
                .expect("reader opened")
                .2
                .refresh(count)?;
            let metadata = std::fs::metadata(path)?;
            let stamp = LogStamp {
                session: selected.id.clone(),
                path: path.clone(),
                modified: metadata.modified()?,
                length: metadata.len(),
                count,
            };
            Ok((stamp, update))
        })();
        self.apply_log_result(result, visible);
    }

    fn finish_log_load(
        &mut self,
        token: u64,
        reader: Box<dyn crate::log::Reader>,
        result: io::Result<(LogStamp, crate::log::Update)>,
    ) {
        let Some(pending) = self
            .pending_log
            .as_ref()
            .filter(|pending| pending.token == token)
        else {
            return;
        };
        let current = self.view.selected_id() == Some(&pending.session)
            && self.sessions.iter().any(|session| {
                session.id == pending.session && session.transcript.as_ref() == Some(&pending.path)
            });
        let pending = self.pending_log.take().expect("request checked");
        if current {
            self.task_log = Some((pending.session, pending.path, reader));
            self.apply_log_result(result, pending.visible);
        }
    }

    fn apply_log_result(
        &mut self,
        result: io::Result<(LogStamp, crate::log::Update)>,
        visible: usize,
    ) {
        match result {
            Ok((stamp, update)) => {
                self.log_parsed_bytes += update.parsed_bytes;
                self.log_oldest_offset = update.oldest_offset;
                debug_assert!(update.oldest_offset <= update.read_offset);
                let changed = self.view.log_page_pending()
                    || update.reset
                    || update.prepended > 0
                    || update.appended > 0
                    || update.replaced > 0;
                if changed {
                    self.view.apply_log_update(update);
                }
                self.view.finish_log_start();
                self.view.log_session = Some(stamp.session.clone());
                self.view
                    .clamp_log_scroll(self.view.matching_log_count().saturating_sub(visible));
                if self.view.log_search.active() {
                    self.full_log_stamp = Some(stamp);
                    self.log_stamp = None;
                } else {
                    self.full_log_stamp = None;
                    self.log_stamp = Some(stamp);
                }
            }
            Err(error) => {
                self.task_log = None;
                self.log_oldest_offset = 0;
                self.log_stamp = None;
                self.full_log_stamp = None;
                self.view.log.clear();
                self.view.prepare_log_search();
                self.view.log_session = None;
                self.view
                    .set_notice(Some(format!("Failed to read task log: {error}")));
            }
        }
    }

    /// Reads when each task worked within a timeline `width` cells wide and hands the view the
    /// bins any of them worked in; a task idle since before the timeline is not read.
    fn refresh_activity(&mut self, width: u16, now: chrono::DateTime<chrono::Utc>) {
        let since = now - chrono::Duration::seconds(crate::ui::timeline_seconds(width));
        let mut bins = std::collections::BTreeSet::new();
        for session in &self.sessions {
            let Some(path) = &session.transcript else {
                continue;
            };
            let works = session.status == crate::model::SessionStatus::Working;
            let recent = std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|at| chrono::DateTime::<chrono::Utc>::from(at) >= since);
            if !works && !recent {
                self.activity.remove(path);
                continue;
            }
            let turns = self.activity.entry(path.clone()).or_default();
            if turns.update(path, session.id.provider, since).is_err() {
                continue;
            }
            for (start, end) in turns.spans(now, works) {
                bins.extend(crate::usage::bin_of(start.max(since))..=crate::usage::bin_of(end));
            }
        }
        self.view.set_activity(bins);
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
        self.view
            .set_context_usage(&session.id, series.context_percent());
        if changed || self.view.usage_session() != Some(&session.id) {
            self.view.set_usage(&session.id, series.bins.clone());
        }
    }

    fn complete_weekly_usage(
        &mut self,
        provider: ProviderId,
        result: io::Result<Option<crate::providers::WeeklyUsage>>,
    ) {
        if let Ok(Some(usage)) = &result {
            let saved = self
                .preferences
                .save_usage(&self.config_dir, provider, usage);
            self.report_save(saved);
        }
        self.hub.weekly_usage_complete(provider, result);
        self.view.weekly_usage = self.hub.weekly_usage().clone();
    }

    fn refresh_weekly_usage(&mut self) {
        for (provider, job) in self.hub.weekly_usage_jobs(Instant::now()) {
            let sender = self.sender.clone();
            thread::spawn(move || {
                let _ = sender.send(Input::WeeklyUsage {
                    provider,
                    result: job(),
                });
            });
        }
    }

    fn refresh(&mut self) {
        let Snapshot { sessions, has_more } = self.hub.refresh();
        self.sessions = sessions.clone();
        self.view.set_loading(false);
        self.view.set_sessions(sessions, has_more);
    }

    /// Returns true when the dashboard should close.
    fn send_input(&mut self, id: SessionKey, input: SessionInput) {
        if self.send_to_terminal(&id, &input) {
            return;
        }
        let Some(session) = self.session(&id) else {
            self.input_error(&input, "Task is no longer available".into());
            return;
        };
        let Some(job) = self.hub.provider(id.provider).send(&session, input.clone()) else {
            self.input_error(&input, "This task cannot receive that input".into());
            return;
        };
        // A closing terminal client still owns the session's input until it leaves.
        let closing = self
            .closing_terminals
            .get_mut(&id)
            .and_then(session_terminal::TerminalClose::take);
        let job: Job = match closing {
            Some(close) => Box::new(move || {
                close
                    .join()
                    .map_err(|_| io::Error::other("Terminal close panicked"))?;
                job()
            }),
            None => job,
        };
        self.start_input_job(id, input, job);
    }

    fn input_error(&mut self, input: &SessionInput, error: String) {
        if matches!(input, SessionInput::Interrupt) {
            self.view
                .set_notice(Some(format!("Failed to stop: {error}")));
        } else {
            self.view.input_failed(error);
        }
    }

    fn start_input_job(&mut self, id: SessionKey, input: SessionInput, job: Job) -> bool {
        if !self.pending_inputs.insert(id.clone()) {
            return false;
        }
        let interrupt = matches!(input, SessionInput::Interrupt);
        if !interrupt {
            self.view.input_sending(&id, &input);
        }
        let sender = self.sender.clone();
        let target = id.clone();
        let sent_input = input.clone();
        match thread::Builder::new()
            .name("agents-send".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    .unwrap_or_else(|_| Err(io::Error::other("Input job panicked")));
                let _ = sender.send(Input::Sent {
                    id: target,
                    input: sent_input,
                    result,
                });
            }) {
            Ok(_) => true,
            Err(error) => {
                self.pending_inputs.remove(&id);
                if interrupt {
                    self.view
                        .set_notice(Some(format!("Failed to stop: {error}")));
                } else {
                    self.view.input_complete(&id, &input, Err(error));
                }
                false
            }
        }
    }

    fn new_draft(&mut self, cwd: PathBuf) {
        let result: io::Result<crate::ui::DraftSetup> = (|| {
            let (cwd, branch) = draft_location(cwd)?;
            let (provider, defaults) = self
                .hub
                .task_defaults(&cwd)
                .ok_or_else(|| io::Error::other("No provider supports new tasks"))?;
            let defaults = defaults?;
            Ok(crate::ui::DraftSetup {
                provider,
                cwd,
                model: defaults.model,
                effort: defaults.effort,
                branch,
            })
        })();
        match result {
            Ok(setup) => {
                self.view.add_draft(setup);
            }
            Err(error) => self
                .view
                .set_notice(Some(format!("Cannot create draft: {error}"))),
        }
    }

    fn create_task(
        &mut self,
        draft_id: DraftId,
        provider: ProviderId,
        request: crate::tasks::NewTask,
    ) {
        let job = self
            .hub
            .provider(provider)
            .create(&request)
            .unwrap_or_else(|| Err(io::Error::other("Provider cannot create tasks")));
        let job = match job {
            Ok(job) => job,
            Err(error) => {
                self.view.draft_created(draft_id, Err(error));
                return;
            }
        };
        let sender = self.sender.clone();
        if let Err(error) = thread::Builder::new()
            .name("agents-create".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    .unwrap_or_else(|_| Err(io::Error::other("Task creation job panicked")));
                let _ = sender.send(Input::Created {
                    draft_id,
                    provider,
                    result,
                });
            })
        {
            self.view.draft_created(draft_id, Err(error));
        }
    }

    fn persist_drafts(&mut self) {
        self.preferences.drafts = self.view.stored_drafts();
        let saved = self.preferences.save_drafts(&self.config_dir);
        self.report_save(saved);
    }

    fn change_draft_directory(&mut self, draft_id: DraftId, cwd: PathBuf) {
        match draft_location(cwd) {
            Ok((cwd, branch)) => self.view.draft_directory_changed(draft_id, cwd, branch),
            Err(error) => self.view.draft_directory_failed(draft_id, error),
        }
    }

    /// Creates the task's worktree on `branch`, then the task in it.
    fn create_task_in_worktree(
        &mut self,
        draft_id: DraftId,
        provider: ProviderId,
        request: crate::tasks::NewTask,
        branch: crate::tasks::BranchChoice,
    ) {
        let Some(root) = self.preferences.worktree_root.clone() else {
            self.view.draft_worktree_created(
                draft_id,
                Err(io::Error::other("Set worktree_root in agents.json")),
            );
            return;
        };
        let config = crate::providers::claude::trust_config_path();
        let cwd = request.cwd.clone();
        let sender = self.sender.clone();
        if let Err(error) = thread::Builder::new()
            .name("agents-worktree".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::tasks::create_worktree(&cwd, &root, &branch, &config)
                        .map(|cwd| (cwd, branch.name().to_owned()))
                }))
                .unwrap_or_else(|_| Err(io::Error::other("Worktree job panicked")));
                let _ = sender.send(Input::WorktreeCreated {
                    draft_id,
                    provider,
                    request,
                    result,
                });
            })
        {
            self.view.draft_worktree_created(draft_id, Err(error));
        }
    }

    /// Hides the session at once and deletes it on a worker thread.
    fn delete(&mut self, session: Session) {
        let id = session.id.clone();
        self.view.hide(&id);
        let job = match self.hub.provider(id.provider).delete(&session) {
            Ok(job) => job,
            Err(error) => return self.deleted(&id, Err(error)),
        };
        if let Err(error) = self.run_session_job(
            id.clone(),
            job,
            "agents-delete",
            "Delete job panicked",
            |id, result| Input::Deleted { id, result },
        ) {
            self.deleted(&id, Err(error));
        }
    }

    fn archive(&mut self, session: Session) {
        let id = session.id.clone();
        self.view.hide(&id);
        let job = match self.hub.provider(id.provider).archive(&session) {
            Ok(job) => job,
            Err(error) => return self.archived(&id, Err(error)),
        };
        if let Err(error) = self.run_session_job(
            id.clone(),
            job,
            "agents-archive",
            "Archive job panicked",
            |id, result| Input::Archived { id, result },
        ) {
            self.archived(&id, Err(error));
        }
    }

    fn run_session_job(
        &self,
        id: SessionKey,
        job: Job,
        name: &str,
        panic_message: &'static str,
        complete: fn(SessionKey, io::Result<()>) -> Input,
    ) -> io::Result<()> {
        let sender = self.sender.clone();
        thread::Builder::new().name(name.into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(io::Error::other(panic_message)));
            let _ = sender.send(complete(id, result));
        })?;
        Ok(())
    }

    fn archived(&mut self, id: &SessionKey, result: io::Result<()>) {
        self.view.unhide(id);
        self.refresh();
        if let Err(error) = result {
            self.view
                .set_notice(Some(format!("Failed to archive: {error}")));
        }
    }

    fn deleted(&mut self, id: &SessionKey, result: io::Result<()>) {
        match result {
            Ok(()) => self.refresh(),
            Err(error) => {
                self.view.unhide(id);
                self.refresh();
                self.view
                    .set_notice(Some(format!("Failed to delete: {error}")));
            }
        }
    }

    fn handle_action(&mut self, action: Option<Action>) -> bool {
        let Some(action) = action else {
            return false;
        };
        let target = match &action {
            Action::Resume(id)
            | Action::OpenTerminal(id)
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
            if matches!(action, Action::Resume(_)) {
                return self.handle_action(Some(Action::Unsupported("Resume")));
            }
            if matches!(action, Action::OpenTerminal(_)) {
                return self.handle_action(Some(Action::Unsupported("Terminal")));
            }
            return false;
        }
        match action {
            Action::Batch(actions) => {
                for action in actions {
                    if self.handle_action(Some(action)) {
                        return true;
                    }
                }
            }
            Action::Quit => {
                self.persist_drafts();
                return true;
            }
            Action::PersistDrafts => self.persist_drafts(),
            Action::NewDraft { cwd } => self.new_draft(cwd),
            Action::CreateTask {
                draft_id,
                provider,
                request,
                branch: None,
            } => self.create_task(draft_id, provider, request),
            Action::CreateTask {
                draft_id,
                provider,
                request,
                branch: Some(branch),
            } => self.create_task_in_worktree(draft_id, provider, request, branch),
            Action::ChangeDraftDirectory { draft_id, cwd } => {
                self.change_draft_directory(draft_id, cwd)
            }
            Action::SetModel { id, model } => self.send_input(id, SessionInput::SetModel(model)),
            Action::SetEffort { id, effort } => {
                self.send_input(id, SessionInput::SetEffort(effort))
            }
            Action::SendPrompt { id, text } => {
                self.send_input(id, SessionInput::Prompt(text));
            }
            Action::AnswerQuestions { ask, answer } => {
                let id = ask.session.clone();
                if let Some(transcript) = self
                    .session(&id)
                    .and_then(|session| session.transcript.clone())
                {
                    self.send_input(
                        id,
                        SessionInput::Answer {
                            ask,
                            answer,
                            transcript,
                        },
                    );
                } else {
                    self.view
                        .set_notice(Some("Session has no transcript".into()));
                }
            }
            Action::SendImages { id, text, images } => {
                self.send_input(id, SessionInput::PromptWithImages { text, images })
            }
            Action::PasteImage => match self.clipboard.image() {
                Ok(image) => self.view.attach_image(image),
                Err(error) => self.view.set_notice(Some(error.to_string())),
            },
            Action::ShowMore => {
                self.hub.show_more();
                self.view.set_loading(true);
                self.refresh();
            }
            Action::Resume(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_session(Action::Resume(id), session);
                }
            }
            Action::Fork(id) => {
                if let Some(session) = self.session(&id) {
                    return self.launch_session(Action::Fork(id), session);
                }
            }
            Action::OpenTerminal(id) => self.open_terminal(id),
            Action::CloseTerminal => self.close_terminal(),
            Action::TerminalInput(bytes) => self.write_terminal(&bytes),
            Action::TerminalPaste(text) => self.paste_terminal(&text),
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
                        self.delete(session);
                    }
                } else {
                    self.view.set_notice(Some("Nothing to delete".into()));
                }
            }
            Action::Archive(id) => {
                if let Some(session) = self.session(&id) {
                    self.archive(session);
                }
            }
            Action::Stop(id) => {
                self.send_input(id, SessionInput::Interrupt);
            }
            Action::Hide(_) => {}
            Action::TogglePin { id, pinned } => {
                self.preferences.pinned.retain(|pinned_id| pinned_id != &id);
                if pinned {
                    self.preferences.pinned.insert(0, id);
                }
                let saved = self.preferences.save_pinned(&self.config_dir);
                self.report_save(saved);
                self.view.set_pinned(&self.preferences.pinned);
            }
            Action::PersistUsageView(view) => {
                self.preferences.usage_view = view;
                let saved = self.preferences.save_usage_view(&self.config_dir);
                self.report_save(saved);
            }
            Action::PersistGrouping(grouping) => {
                self.preferences.grouping = grouping;
                let saved = self.preferences.save_grouping(&self.config_dir);
                self.report_save(saved);
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

    fn report_save(&mut self, result: io::Result<()>) {
        if let Err(error) = result {
            self.view
                .set_notice(Some(format!("Failed to save preferences: {error}")));
        }
    }

    /// Hands the terminal to the selected session's provider process.
    fn launch_session(&mut self, action: Action, session: Session) -> bool {
        if self.pending_inputs.contains(&session.id) {
            return false;
        }

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

#[path = "app_keyboard.rs"]
mod keyboard;

#[path = "app_terminal.rs"]
mod session_terminal;

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste,
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        )
    )?;
    keyboard::enable_tmux_keys(&mut stdout)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn leave_terminal(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let reports = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::PopKeyboardEnhancementFlags,
        keyboard::ModifyOtherKeys(false),
        crossterm::event::DisableThemeChangeReports,
        crossterm::event::DisableBracketedPaste,
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

#[cfg(test)]
#[path = "app_overlay_tests.rs"]
mod overlay_tests;

/// How many times taller than wide one terminal cell is, when the terminal reports its pixels.
fn cell_aspect() -> Option<f64> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    let cell_height = f64::from(size.height) / f64::from(size.rows);
    let cell_width = f64::from(size.width) / f64::from(size.columns);
    Some(cell_height / cell_width)
}

#[cfg(test)]
#[path = "app_input_tests.rs"]
mod input_tests;

#[cfg(test)]
#[path = "app_ui_tests.rs"]
mod ui_tests;

#[cfg(test)]
#[path = "app_live_tests.rs"]
pub(crate) mod live_tests;

#[cfg(test)]
#[path = "app_loop_tests.rs"]
mod loop_tests;

#[cfg(test)]
#[path = "app_project_tests.rs"]
mod project_tests;

#[cfg(test)]
#[path = "app_log_search_tests.rs"]
mod log_search_tests;

#[cfg(test)]
#[path = "app_weekly_usage_tests.rs"]
mod weekly_usage_tests;
