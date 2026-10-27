//! Terminal loop: reacts to session changes and keyboard input, renders the view and executes actions.

#[path = "app_transcript.rs"]
mod transcript_cache;
use transcript_cache::{FileStamp, UsageCache};

use crate::event::ALL_EVENTS;
use crate::model::Action;
use crate::model::{DraftKey, Session, SessionStatus};
use crate::providers::Snapshot;
use crate::providers::{ProviderId, SessionAction, SessionInput, SessionKey};
use crate::refresh::{FALLBACK_INTERVAL, POLLING_INTERVAL, RefreshSchedule};
use crate::sessions::Hub;
use crate::state::Preferences;
use crate::terminal;
use crate::ui::View;
use crossterm::SynchronizedUpdate;
use crossterm::event::Event;
use crossterm::event::Theme;
use crossterm::terminal as crossterm_terminal;
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
    AccountUsage {
        provider: ProviderId,
        result: io::Result<crate::providers::AccountUsage>,
    },
    ResetCreditRedeemed {
        provider: ProviderId,
        request: crate::providers::ResetCreditRequest,
        result: io::Result<crate::providers::ResetCreditRedemption>,
    },
    Terminal(io::Result<Event>),
    Filesystem(notify::Result<notify::Event>),
    EventOutput(notify::Result<notify::Event>),
    Sessions {
        refresh: Box<crate::sessions::Refresh>,
        generation: u64,
        result: io::Result<Snapshot>,
    },
    EventsLoaded {
        token: u64,
        reader: Box<dyn crate::event::Reader>,
        result: io::Result<LoadedEvents>,
    },
    Created {
        draft_key: DraftKey,
        provider: ProviderId,
        result: io::Result<std::process::Output>,
    },
    JobCreated {
        draft_key: DraftKey,
        job: String,
    },
    /// A new task's worktree, with the task request to create in it.
    WorktreeCreated {
        draft_key: DraftKey,
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
        result: io::Result<Option<String>>,
    },
    Archived {
        id: SessionKey,
        result: io::Result<Option<String>>,
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
    session_refresh: SessionRefresh,
    preferences: Preferences,
    view: View,
    clipboard: crate::clipboard::Clipboard,
    sessions: Vec<Session>,
    /// The latest supervision relationships, used to route archive actions.
    supervisions: Vec<crate::providers::Supervision>,
    /// Set when the user chose to hand the terminal to a provider process; the dashboard
    /// returns once it exits.
    launch: Option<Command>,
    terminal_report: terminal::Report,
    /// The session program running in the event detail panel.
    terminal: Option<crate::session_terminal::SessionTerminal>,
    /// The task whose terminal opens once that task's previous client has closed.
    terminal_task: Option<SessionKey>,
    /// The waiting task whose terminal opened on its own, so closing it keeps it closed.
    waiting_terminal: Option<SessionKey>,
    /// Clients being reaped before the same session can open another terminal.
    closing_terminals: std::collections::HashMap<SessionKey, session_terminal::TerminalClose>,
    transcript_stamp: Option<TranscriptStamp>,
    full_transcript_stamp: Option<TranscriptStamp>,
    transcript: Option<(SessionKey, PathBuf, Box<dyn crate::event::Reader>)>,
    pending_transcript: Option<PendingTranscript>,
    transcript_request: u64,
    transcript_has_older: bool,
    transcript_parsed_bytes: u64,
    /// Visible token usage, refreshed independently of paints.
    usage: UsageCache,
}

#[derive(PartialEq, Eq)]
struct TranscriptStamp {
    session: SessionKey,
    path: PathBuf,
    file: FileStamp,
    count: usize,
}

struct LoadedEvents {
    transcript: (TranscriptStamp, crate::event::Update),
    pinned: Option<Vec<(crate::state::PinnedEvent, Option<crate::event::Event>)>>,
}

struct PendingTranscript {
    token: u64,
    session: SessionKey,
    path: PathBuf,
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
        view.restore_question_answers(&preferences);
        view.restore_attention_skips(&preferences);
        view.set_task_repositories(&preferences);
        if let Some(notice) = preferences.configuration_notice() {
            view.show_error(notice);
        } else if !preferences.draft_notices.is_empty() {
            view.show_error(preferences.draft_notices.join("; "));
        }
        view.set_double_click_ms(preferences.double_click_ms);
        view.set_loading(true);
        view.set_capabilities(hub.capabilities());
        view.set_input_choices(hub.input_choices());
        for draft in &preferences.drafts {
            if let Some((_, defaults)) = hub.task_defaults(&draft.cwd) {
                match defaults {
                    Ok(defaults) => view.set_draft_defaults(&draft.cwd, defaults),
                    Err(error) => view.show_error(format!("Cannot create draft: {error}")),
                }
            }
        }
        view.provider_names = hub.display_names();
        let (sender, receiver) = mpsc::channel();
        let session_refresh = SessionRefresh::new(hub.session_source());
        Self {
            sender,
            receiver,
            pending_inputs: std::collections::HashSet::new(),
            hub,
            session_refresh,
            config_dir,
            preferences,
            view,
            clipboard: crate::clipboard::Clipboard::new(),
            sessions: Vec::new(),
            supervisions: Vec::new(),
            launch: None,
            terminal_report: terminal::Report::default(),
            terminal: None,
            terminal_task: None,
            waiting_terminal: None,
            closing_terminals: std::collections::HashMap::new(),
            transcript_stamp: None,
            full_transcript_stamp: None,
            transcript: None,
            pending_transcript: None,
            transcript_request: 0,
            transcript_has_older: false,
            transcript_parsed_bytes: 0,
            usage: UsageCache::default(),
        }
    }

    pub fn run(mut self) -> io::Result<()> {
        loop {
            let mut terminal = enter_terminal()?;
            // Raw mode is on, so the terminal's reply reaches us instead of the screen.
            self.terminal_report = terminal::query(terminal::DEFAULT_TIMEOUT).unwrap_or_default();
            self.apply_background(
                self.terminal_report
                    .background
                    .is_some_and(terminal::is_light),
            );
            self.view.set_inline_images(
                self.terminal_report.supports_images,
                self.terminal_report.cell_pixels,
            );
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
                Err(error) => Some(format!("Cannot start {:?}: {error}", command.get_program())),
            };
            if let Some(notice) = notice {
                self.view.show_error(notice);
            }
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
                    let _ = filesystem.send(Input::Filesystem(event));
                })
            });
        let mut watcher = watch.ok();
        let output = self.sender.clone();
        let mut output_watcher = crate::sessions::OutputWatcher::new(move |event| {
            let _ = output.send(Input::EventOutput(event));
        })
        .map_err(io::Error::other)?;
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
        self.request_refresh();
        let mut selection_tick = Instant::now();
        loop {
            let persist = self.view.take_question_action();
            self.handle_action(persist);
            let persist = self.view.take_drafts_action();
            self.handle_action(persist);
            if selection_tick.elapsed() >= Duration::from_millis(50) {
                self.view.tick_text_selection();
                selection_tick = Instant::now();
            }
            self.view.expire_messages(Instant::now());
            self.refresh_weekly_usage();
            self.view.present_reset_credit();
            if let Some(request) = schedule.take_due(Instant::now()) {
                self.request_sessions(request);
            }
            self.view.set_cell_aspect(cell_aspect());
            self.sync_terminal();
            let action = self.view.take_pending_action();
            if self.handle_action(action) {
                return Ok(());
            }
            match output_watcher.set_path(self.view.event_output_path()) {
                Ok(true) => self.view.refresh_event_output(),
                Ok(false) => {}
                Err(error) => self.view.show_error(error.to_string()),
            }
            // Publish placeholder cells as one frame; tmux otherwise forwards partial rows.
            io::stdout().sync_update(|out| {
                self.draw_frame(terminal, chrono::Utc::now(), |commands| {
                    terminal::send_image_commands(out, commands)
                })
            })??;
            if self.wait_for_input(&mut watcher, &mut schedule, selection_tick)? {
                return Ok(());
            }
        }
    }

    #[cfg(test)]
    fn refresh_event_for_list_rows(&mut self, rows: usize) {
        self.initialize_selected_event_list();
        self.refresh_event(self.view.event_read_count_for(rows, true));
    }

    #[cfg(test)]
    fn draw<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), B::Error> {
        self.draw_at(terminal, chrono::Utc::now())
    }

    #[cfg(test)]
    fn draw_at<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), B::Error> {
        self.draw_frame(terminal, now, |_| Ok(()))
    }

    fn draw_frame<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        now: chrono::DateTime<chrono::Utc>,
        send_commands: impl FnOnce(&[u8]) -> Result<(), B::Error>,
    ) -> Result<(), B::Error> {
        // Keep the drawing cursor hidden until the frame's input cursor is in place.
        terminal.hide_cursor()?;
        let mut cursor = None;
        terminal.try_draw(|frame| {
            let area = frame.area();
            self.initialize_selected_event_list();
            self.refresh_event(self.view.event_read_count(area));
            self.refresh_usage(self.view.usage_width(area), now);
            self.view.render_at(area, frame.buffer_mut(), now);
            cursor = self.view.cursor_pos(area);
            // try_draw flushes cells only after this callback returns successfully.
            send_commands(&self.view.take_image_commands())
        })?;
        self.terminal_drawn();
        self.view.begin_image_frame();
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
            .event_redraw_in(wall_now)
            .map_or(timeout, |event_panel| timeout.min(event_panel));
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
                Input::AccountUsage { provider, result } => {
                    self.complete_account_usage(provider, result);
                }
                Input::ResetCreditRedeemed {
                    provider,
                    request,
                    result,
                } => {
                    self.complete_reset_credit(provider, request, result);
                }
                Input::Terminal(event) => {
                    if self.handle_event(event?) {
                        return Ok(true);
                    }
                }
                Input::Sessions {
                    refresh,
                    generation,
                    result,
                } => {
                    self.finish_sessions(*refresh, generation, result)?;
                }
                Input::Filesystem(Ok(event)) if watcher.is_some() => {
                    let request = if event.need_rescan() {
                        crate::sessions::RefreshRequest::Full
                    } else {
                        crate::sessions::RefreshRequest::paths(event.paths)
                    };
                    schedule.paths_changed(Instant::now(), request);
                }
                Input::Filesystem(Err(_)) if watcher.is_some() => {
                    *watcher = None;
                    *schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
                }
                Input::Filesystem(_) => {}
                Input::EventOutput(Ok(_)) => self.view.refresh_event_output(),
                Input::EventOutput(Err(error)) => {
                    self.view.show_error(error.to_string());
                }
                Input::EventsLoaded {
                    token,
                    reader,
                    result,
                } => {
                    self.finish_event_load(token, reader, result);
                }
                Input::JobCreated { draft_key, job } => {
                    self.view.draft_job_created(draft_key, job);
                    schedule.changed(Instant::now());
                }
                Input::Created {
                    draft_key,
                    provider,
                    result,
                } => {
                    let result =
                        result.and_then(|output| self.hub.provider(provider).created_job(&output));
                    if let Err(error) = &result {
                        self.view.show_error(error.to_string());
                    }
                    self.view.draft_created(draft_key, result);
                    schedule.changed(Instant::now());
                }
                Input::WorktreeCreated {
                    draft_key,
                    provider,
                    mut request,
                    result,
                } => match result {
                    Ok((cwd, branch)) => {
                        request.cwd = cwd.clone();
                        self.view
                            .draft_worktree_created(draft_key, Ok((cwd, branch)));
                        self.create_task(draft_key, provider, request);
                    }
                    Err(error) => self.view.draft_worktree_created(draft_key, Err(error)),
                },
                Input::Sent { id, input, result } => {
                    if matches!(input, SessionInput::Interrupt) {
                        let generation = self.request_refresh();
                        self.session_refresh
                            .stops
                            .push((generation, id, result.is_ok()));
                        if let Err(error) = result {
                            self.input_error(&input, error.to_string());
                        }
                    } else {
                        self.pending_inputs.remove(&id);
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
            self.view.show_error(format!("Theme not loaded: {error}"));
        }
        if let Err(error) = crate::ui::use_highlight_theme(light, self.highlight_theme(light)) {
            self.view.show_error(format!("Theme not loaded: {error}"));
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
        let task = self.view.selected_id().cloned();
        let selected_event = self.view.event_list.cursor;
        let action = match event {
            Event::Key(key) => self.view.handle_key(key),
            Event::Mouse(mouse) => self.view.handle_mouse(mouse),
            Event::Paste(text) => return self.paste(&text),
            Event::Resize(width, height) => {
                self.refresh_cell_pixels();
                self.view.resize(width, height);
                return false;
            }
            Event::ThemeChanged(theme) => {
                self.apply_background(theme == Theme::Light);
                return false;
            }
            _ => return false,
        };
        // Explicit input changing the selected log returns the output to event details.
        if self.view.terminal_requested()
            && self.view.selected_id() == task.as_ref()
            && self.view.event_list.cursor != selected_event
        {
            self.handle_action(Some(Action::CloseTerminal));
        }
        self.handle_action(action)
    }

    fn paste(&mut self, text: &str) -> bool {
        if let Some(action) = self.view.terminal_paste(text) {
            return self.handle_action(Some(action));
        }
        if !self.view.pasting_prompt() {
            if self.view.handle_paste(text) {
                return false;
            }
            if !self.view.prepare_paste_prompt() {
                self.view.show_error("Cannot paste here".into());
                return false;
            }
            return self.paste(text);
        }
        match crate::images::ImageAttachment::from_paste(text) {
            Ok(Some(image)) => self.view.attach_image(image),
            Ok(None) => {
                self.view.handle_paste(text);
            }
            Err(error) => self.view.show_error(error.to_string()),
        }
        false
    }

    /// Loads the selected task's saved sort and filter into the event list.
    fn initialize_selected_event_list(&mut self) {
        if let Some(id) = self.view.selected_id() {
            let sorting = self.preferences.task_sort(id);
            self.view
                .initialize_event_list(sorting, self.preferences.task_filter(id));
        }
    }

    /// Reads the newest `count` events; `ALL_EVENTS` reads in the background.
    fn refresh_event(&mut self, count: usize) {
        let selected = self
            .view
            .selected_id()
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .cloned();
        let Some(selected) = selected.filter(|session| session.transcript.is_some()) else {
            self.transcript_stamp = None;
            self.full_transcript_stamp = None;
            self.transcript = None;
            self.pending_transcript = None;
            self.transcript_has_older = false;
            self.view.event_panel.clear();
            self.view.prepare_event_search();
            self.view.event_session = None;
            return;
        };
        let path = selected.transcript.as_ref().expect("transcript checked");
        if let Some(pending) = &self.pending_transcript {
            if pending.session == selected.id && &pending.path == path {
                return;
            }
            self.pending_transcript = None;
        }
        let first_load = self
            .transcript
            .as_ref()
            .is_none_or(|(id, cached_path, _)| id != &selected.id || cached_path != path);
        let previous_reader = if first_load {
            self.transcript.take()
        } else {
            None
        };
        if first_load {
            match crate::providers::event_reader(&selected) {
                Ok(reader) => {
                    self.transcript = Some((selected.id.clone(), path.clone(), reader));
                    self.transcript_has_older = true;
                    if self.view.event_session.as_ref() != Some(&selected.id) {
                        self.view.event_panel.clear();
                        self.view.prepare_event_search();
                        self.view.event_session = Some(selected.id.clone());
                    }
                }
                Err(error) => {
                    self.apply_event_result(Err(error));
                    return;
                }
            }
        }
        if first_load || (count == ALL_EVENTS && self.transcript_has_older) {
            let (session, path, mut reader) = self.transcript.take().expect("reader checked");
            self.transcript_request += 1;
            let token = self.transcript_request;
            self.pending_transcript = Some(PendingTranscript {
                token,
                session: session.clone(),
                path: path.clone(),
            });
            let pinned = if first_load && !self.view.has_pinned_events(&session) {
                let pinned = self.preferences.task_pinned_events(&session);
                (!pinned.is_empty()).then(|| pinned.to_vec())
            } else {
                None
            };
            let sender = self.sender.clone();
            thread::spawn(move || {
                drop(previous_reader);
                let result = (|| {
                    // Capture inputs before reading, so writes during the read are checked again.
                    let file = FileStamp::read(&path)?;
                    let update = reader.refresh(count)?;
                    let pinned = pinned
                        .map(|pinned| {
                            let groups = pinned
                                .iter()
                                .map(|pin| pin.records.clone())
                                .collect::<Vec<_>>();
                            let events = reader.read_events(&groups)?;
                            if events.len() != pinned.len() {
                                return Err(io::Error::other(
                                    "Pinned event reader returned an unexpected count",
                                ));
                            }
                            Ok(pinned.into_iter().zip(events).collect())
                        })
                        .transpose()?;
                    Ok(LoadedEvents {
                        transcript: (
                            TranscriptStamp {
                                session,
                                path,
                                file,
                                count,
                            },
                            update,
                        ),
                        pinned,
                    })
                })();
                let _ = sender.send(Input::EventsLoaded {
                    token,
                    reader,
                    result,
                });
            });
            return;
        }
        let result = (|| -> io::Result<Option<(TranscriptStamp, crate::event::Update)>> {
            let file = FileStamp::read(path)?;
            let stamp = TranscriptStamp {
                session: selected.id.clone(),
                path: path.clone(),
                file,
                count,
            };
            let previous = self
                .transcript_stamp
                .as_ref()
                .or(self.full_transcript_stamp.as_ref());
            if previous.is_some_and(|previous| {
                previous.session == stamp.session
                    && previous.path == stamp.path
                    && previous.file == stamp.file
                    && (previous.count >= count || !self.transcript_has_older)
            }) {
                self.finish_event_refresh(stamp);
                return Ok(None);
            }
            let update = self
                .transcript
                .as_mut()
                .expect("reader opened")
                .2
                .refresh(count)?;
            Ok(Some((stamp, update)))
        })();
        match result {
            Ok(None) => {}
            Ok(Some(result)) => self.apply_event_result(Ok(result)),
            Err(error) => self.apply_event_result(Err(error)),
        }
    }

    fn finish_event_load(
        &mut self,
        token: u64,
        reader: Box<dyn crate::event::Reader>,
        result: io::Result<LoadedEvents>,
    ) {
        let Some(pending) = self
            .pending_transcript
            .as_ref()
            .filter(|pending| pending.token == token)
        else {
            return;
        };
        let current = self.view.selected_id() == Some(&pending.session)
            && self.sessions.iter().any(|session| {
                session.id == pending.session && session.transcript.as_ref() == Some(&pending.path)
            });
        let pending = self.pending_transcript.take().expect("request checked");
        if current {
            self.transcript = Some((pending.session.clone(), pending.path, reader));
            let result = result.map(|loaded| {
                if let Some(pinned) = loaded.pinned {
                    self.restore_pinned_events(&pending.session, pinned);
                }
                loaded.transcript
            });
            self.apply_event_result(result);
        }
    }

    fn restore_pinned_events(
        &mut self,
        task: &SessionKey,
        pinned: Vec<(crate::state::PinnedEvent, Option<crate::event::Event>)>,
    ) {
        // Ignore a read whose stored pins changed while the transcript was loading.
        if !self
            .preferences
            .task_pinned_events(task)
            .iter()
            .eq(pinned.iter().map(|(pin, _)| pin))
        {
            return;
        }
        let count = pinned.len();
        let rows = pinned
            .into_iter()
            .filter_map(|(pin, event)| {
                let event = event?;
                if event.source.as_deref() != Some(pin.source.as_str()) {
                    return None;
                }
                Some(crate::ui::PinnedRow { pin, event })
            })
            .collect::<Vec<_>>();
        if rows.len() != count {
            let saved = self.preferences.save_task_pinned_events(
                &self.config_dir,
                task,
                rows.iter().map(|row| row.pin.clone()).collect(),
            );
            self.report_save(saved);
        }
        self.view.set_pinned_events(task, rows);
    }

    fn apply_event_result(&mut self, result: io::Result<(TranscriptStamp, crate::event::Update)>) {
        match result {
            Ok((stamp, update)) => {
                self.transcript_parsed_bytes += update.parsed_bytes;
                self.transcript_has_older = update.has_older;
                debug_assert!(update.oldest_offset <= update.read_offset);
                let changed = update.reset
                    || update.prepended > 0
                    || update.appended > 0
                    || update.replaced > 0;
                if changed {
                    self.view.apply_event_update(update);
                }
                self.finish_event_refresh(stamp);
            }
            Err(error) => {
                self.transcript = None;
                self.transcript_has_older = false;
                self.transcript_stamp = None;
                self.full_transcript_stamp = None;
                if self.view.event_list.sort != crate::ui::EventSort::Duration {
                    self.view.event_panel.clear();
                    self.view.prepare_event_search();
                    self.view.event_session = None;
                }
                self.view
                    .show_error(format!("Cannot read task events: {error}"));
            }
        }
    }

    fn finish_event_refresh(&mut self, stamp: TranscriptStamp) {
        self.view.finish_event_load(self.transcript_has_older);
        self.view.event_session = Some(stamp.session.clone());
        if stamp.count == ALL_EVENTS {
            self.full_transcript_stamp = Some(stamp);
            self.transcript_stamp = None;
        } else {
            self.full_transcript_stamp = None;
            self.transcript_stamp = Some(stamp);
        }
    }

    /// Updates the active tasks' usage and splits out the selected task without rereading it.
    fn refresh_usage(&mut self, width: u16, now: chrono::DateTime<chrono::Utc>) {
        let selected = self.view.selected_id().cloned();
        if let Some((total, others)) =
            self.usage
                .update(&self.sessions, selected.as_ref(), width, now)
        {
            self.view.set_usage_totals(total, others);
        }
        if let Some(id) = selected.as_ref() {
            self.view
                .set_context_usage(id, self.usage.context_percent(id));
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
        for (provider, job) in self.hub.account_usage_jobs(Instant::now()) {
            let sender = self.sender.clone();
            thread::spawn(move || {
                let _ = sender.send(Input::AccountUsage {
                    provider,
                    result: job(),
                });
            });
        }
    }

    fn complete_account_usage(
        &mut self,
        provider: ProviderId,
        result: io::Result<crate::providers::AccountUsage>,
    ) {
        match result {
            Ok(usage) => {
                self.view.offer_reset_credit(provider, usage.reset_offers);
                self.complete_weekly_usage(provider, Ok(usage.weekly));
            }
            Err(error) => self.complete_weekly_usage(provider, Err(error)),
        }
    }

    fn redeem_reset_credit(
        &mut self,
        provider: ProviderId,
        request: crate::providers::ResetCreditRequest,
    ) {
        let Some(job) = self
            .hub
            .provider(provider)
            .redeem_reset_credit(request.clone())
        else {
            self.view.reset_credit_completed(provider);
            self.view.show_error(io::ErrorKind::Unsupported.to_string());
            return;
        };
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(io::Error::other("Reset credit job panicked")));
            let _ = sender.send(Input::ResetCreditRedeemed {
                provider,
                request,
                result,
            });
        });
    }

    fn complete_reset_credit(
        &mut self,
        provider: ProviderId,
        request: crate::providers::ResetCreditRequest,
        result: io::Result<crate::providers::ResetCreditRedemption>,
    ) {
        use crate::providers::ResetCreditOutcome;
        match result {
            Ok(redemption) => {
                self.view.reset_credit_completed(provider);
                self.complete_account_usage(provider, Ok(redemption.usage));
                let failure = match redemption.outcome {
                    ResetCreditOutcome::Reset
                    | ResetCreditOutcome::AlreadyRedeemed
                    | ResetCreditOutcome::NothingToReset => None,
                    ResetCreditOutcome::NoCredit => Some("noCredit"),
                    ResetCreditOutcome::Cooldown => Some("cooldown"),
                    ResetCreditOutcome::Ineligible => Some("ineligible"),
                    ResetCreditOutcome::Unavailable => Some("unavailable"),
                };
                if let Some(failure) = failure {
                    self.view
                        .show_error(redemption.reason.unwrap_or_else(|| failure.into()));
                }
            }
            Err(error) => {
                self.view.reset_credit_failed(provider, &request);
                self.view.show_error(error.to_string());
            }
        }
    }

    fn apply_sessions(
        &mut self,
        Snapshot {
            sessions,
            has_more,
            supervisions,
        }: Snapshot,
    ) {
        self.forget_inactive_state(&sessions);
        self.sessions = sessions.clone();
        self.supervisions = supervisions;
        self.view.set_loading(false);
        self.view.set_task_repositories(&self.preferences);
        self.view.set_sessions(sessions, has_more);
    }

    /// Finished tasks discard saved interface state except event pins; task pins last while they or their root are active.
    fn forget_inactive_state(&mut self, sessions: &[Session]) {
        for session in sessions
            .iter()
            .filter(|session| session.status == SessionStatus::Inactive)
        {
            let saved = self
                .preferences
                .keep_only_task_event_pins(&self.config_dir, &session.id);
            self.report_save(saved);
        }
        let pinned = self.preferences.pinned.active.len();
        self.preferences.pinned.active.retain(|id| {
            let Some(session) = sessions.iter().find(|session| &session.id == id) else {
                return true;
            };
            if session.status != SessionStatus::Inactive {
                return true;
            }
            let mut root = session;
            for _ in 0..sessions.len() {
                let Some(parent) = root
                    .parent
                    .as_ref()
                    .and_then(|id| sessions.iter().find(|session| &session.id == id))
                else {
                    break;
                };
                root = parent;
            }
            root.parent.is_none() && root.status != SessionStatus::Inactive
        });
        if self.preferences.pinned.active.len() != pinned {
            let saved = self.preferences.save_pinned(&self.config_dir);
            self.report_save(saved);
            self.view.set_pinned(&self.preferences.pinned);
        }
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
        let job: SessionAction = match closing {
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
            self.view.show_error(format!("Cannot stop: {error}"));
        } else {
            self.view.input_failed(error);
        }
    }

    fn start_input_job(&mut self, id: SessionKey, input: SessionInput, job: SessionAction) -> bool {
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
                    self.view.show_error(format!("Cannot stop: {error}"));
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
                .show_error(format!("Cannot create draft: {error}")),
        }
    }

    fn create_task(
        &mut self,
        draft_key: DraftKey,
        provider: ProviderId,
        request: crate::tasks::NewTask,
    ) {
        let created_sender = self.sender.clone();
        let job = self
            .hub
            .provider(provider)
            .create(
                &request,
                Box::new(move |job| {
                    let _ = created_sender.send(Input::JobCreated { draft_key, job });
                }),
            )
            .unwrap_or_else(|| Err(io::Error::other("Provider cannot create tasks")));
        let job = match job {
            Ok(job) => job,
            Err(error) => {
                self.view.draft_created(draft_key, Err(error));
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
                    draft_key,
                    provider,
                    result,
                });
            })
        {
            self.view.draft_created(draft_key, Err(error));
        }
    }

    fn persist_drafts(&mut self) {
        self.preferences.drafts = self.view.stored_drafts();
        let saved = self.preferences.save_drafts(&self.config_dir);
        self.report_save(saved);
    }

    fn change_draft_directory(&mut self, draft_key: DraftKey, cwd: PathBuf) {
        match draft_location(cwd) {
            Ok((cwd, branch)) => self.view.draft_directory_changed(draft_key, cwd, branch),
            Err(error) => self.view.draft_directory_failed(draft_key, error),
        }
    }

    /// Creates the task's worktree on `branch`, then the task in it.
    fn create_task_in_worktree(
        &mut self,
        draft_key: DraftKey,
        provider: ProviderId,
        request: crate::tasks::NewTask,
        branch: crate::tasks::BranchChoice,
    ) {
        let Some(root) = self.preferences.worktree_root.clone() else {
            self.view.draft_worktree_created(
                draft_key,
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
                        .and_then(draft_location)
                }))
                .unwrap_or_else(|_| Err(io::Error::other("Worktree job panicked")));
                let _ = sender.send(Input::WorktreeCreated {
                    draft_key,
                    provider,
                    request,
                    result,
                });
            })
        {
            self.view.draft_worktree_created(draft_key, Err(error));
        }
    }

    /// Removes the session from the list while its deletion runs on a worker thread.
    fn delete(&mut self, session: Session, worktree: Option<PathBuf>) {
        let id = session.id.clone();
        self.view.begin_removal(&id);
        let job = match self.hub.provider(id.provider).delete(&session) {
            Ok(job) => job,
            Err(error) => return self.deleted(&id, Err(error)),
        };
        let job = Box::new(move || {
            job()?;
            Ok(worktree
                .and_then(|path| crate::tasks::remove_worktree(&path).err())
                .map(|error| error.to_string()))
        });
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

    fn archive(&mut self, session: Session, worktree: Option<PathBuf>) {
        if let Some(path) = &worktree {
            let Some(identity) = crate::repository::repository_identity(path) else {
                self.view
                    .show_error("Cannot identify worktree repository".into());
                return;
            };
            let saved = self.preferences.save_task_repository(
                &self.config_dir,
                &session.id,
                identity.primary_root,
            );
            let failed = saved.is_err();
            self.report_save(saved);
            if failed {
                return;
            }
            self.view.set_task_repositories(&self.preferences);
        }
        let id = session.id.clone();
        self.view.begin_removal(&id);
        let supervision = self.supervisions.iter().find(|s| s.session == id);
        let job = match self.hub.archive(&session, supervision) {
            Ok(job) => job,
            Err(error) => return self.archived(&id, Err(error)),
        };
        let job = Box::new(move || {
            job()?;
            Ok(worktree
                .and_then(|path| crate::tasks::remove_worktree(&path).err())
                .map(|error| error.to_string()))
        });
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

    fn run_session_job<T: Send + 'static>(
        &self,
        id: SessionKey,
        job: Box<dyn FnOnce() -> io::Result<T> + Send>,
        name: &str,
        panic_message: &'static str,
        complete: fn(SessionKey, io::Result<T>) -> Input,
    ) -> io::Result<()> {
        let sender = self.sender.clone();
        thread::Builder::new().name(name.into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(io::Error::other(panic_message)));
            let _ = sender.send(complete(id, result));
        })?;
        Ok(())
    }

    fn archived(&mut self, id: &SessionKey, result: io::Result<Option<String>>) {
        let generation = self.request_refresh();
        match result {
            // The row stays hidden until a refresh lists the session as archived.
            Ok(worktree_error) => {
                self.session_refresh.archives.push((generation, id.clone()));
                if let Some(error) = worktree_error {
                    self.view
                        .show_error(format!("Cannot delete worktree: {error}"));
                }
            }
            Err(error) => {
                self.view.finish_removal(id);
                self.view.show_error(format!("Cannot archive: {error}"));
            }
        }
    }

    fn deleted(&mut self, id: &SessionKey, result: io::Result<Option<String>>) {
        match result {
            Ok(worktree_error) => {
                let saved = self.preferences.forget_task(&self.config_dir, id);
                self.report_save(saved);
                self.request_refresh();
                if let Some(error) = worktree_error {
                    self.view
                        .show_error(format!("Cannot remove worktree: {error}"));
                }
            }
            Err(error) => {
                self.view.finish_removal(id);
                self.request_refresh();
                self.view.show_error(format!("Cannot delete: {error}"));
            }
        }
    }

    fn handle_action(&mut self, action: Option<Action>) -> bool {
        let Some(action) = action else {
            return false;
        };
        let target = match &action {
            Action::Archive(id) | Action::ArchiveWorktree { id, .. }
                if id.provider == ProviderId::Codex =>
            {
                None
            }
            Action::Resume(id)
            | Action::OpenTerminal(id)
            | Action::Fork(id)
            | Action::Rename { id, .. }
            | Action::Delete(id)
            | Action::DeleteWorktree { id, .. }
            | Action::Archive(id)
            | Action::ArchiveWorktree { id, .. }
            | Action::Stop(id) => Some(id),
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
            Action::RedeemResetCredit { provider, request } => {
                self.redeem_reset_credit(provider, request)
            }
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
                draft_key,
                provider,
                request,
                branch: None,
            } => self.create_task(draft_key, provider, request),
            Action::CreateTask {
                draft_key,
                provider,
                request,
                branch: Some(branch),
            } => self.create_task_in_worktree(draft_key, provider, request, branch),
            Action::ChangeDraftDirectory { draft_key, cwd } => {
                self.change_draft_directory(draft_key, cwd)
            }
            Action::SetModel { id, model } => self.send_input(id, SessionInput::SetModel(model)),
            Action::SetEffort { id, effort } => {
                self.send_input(id, SessionInput::SetEffort(effort))
            }
            Action::SendPrompt { id, text } => {
                self.send_input(id, SessionInput::Prompt(text));
            }
            Action::PersistQuestion { id, ask } => {
                let saved = self.preferences.save_task_ask(&self.config_dir, &id, ask);
                self.report_save(saved);
            }
            Action::PersistSkip { id, skipped_at } => {
                let saved =
                    self.preferences
                        .save_task_skipped_at(&self.config_dir, &id, skipped_at);
                self.report_save(saved);
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
                    self.view.show_error("Session has no transcript".into());
                }
            }
            Action::SendImages { id, text, images } => {
                self.send_input(id, SessionInput::PromptWithImages { text, images })
            }
            Action::PasteText => match self.clipboard.text() {
                Ok(text) => return self.paste(&text),
                Err(error) => self.view.show_error(error.to_string()),
            },
            Action::ShowMore => {
                self.hub.show_more();
                self.view.set_loading(true);
                self.request_refresh();
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
            Action::Rename { id, name } if id.provider == ProviderId::Claude => {
                self.send_input(id, SessionInput::Rename(name));
            }
            Action::Rename { id, name } => {
                if let Some(session) = self.session(&id) {
                    if session.transcript.is_some() {
                        match self.hub.provider(id.provider).rename(&session, &name) {
                            Ok(()) => {
                                self.request_refresh();
                            }
                            Err(error) => self.view.show_error(format!("Cannot rename: {error}")),
                        }
                    } else {
                        self.view
                            .show_error("Cannot rename: session has no transcript".into());
                    }
                }
            }
            Action::Delete(id) => {
                let generation = self.request_refresh();
                self.session_refresh.deletes.push((generation, id, None));
            }
            Action::DeleteWorktree { id, path } => {
                let generation = self.request_refresh();
                self.session_refresh
                    .deletes
                    .push((generation, id, Some(path)));
            }
            Action::Archive(id) => {
                if let Some(session) = self.session(&id) {
                    self.archive(session, None);
                }
            }
            Action::ArchiveWorktree { id, path } => {
                if let Some(session) = self.session(&id) {
                    self.archive(session, Some(path));
                }
            }
            Action::Stop(id) => {
                self.send_input(id, SessionInput::Interrupt);
            }
            Action::TogglePin { id, pinned, tab } => {
                let pins = self.preferences.pinned.for_tab_mut(tab);
                pins.retain(|pinned_id| pinned_id != &id);
                if pinned {
                    pins.insert(0, id);
                }
                let saved = self.preferences.save_pinned(&self.config_dir);
                self.report_save(saved);
                self.view.set_pinned(&self.preferences.pinned);
            }
            Action::ToggleEventPin { task, pinned } => {
                let saved =
                    self.preferences
                        .save_task_pinned_events(&self.config_dir, &task, pinned);
                self.report_save(saved);
            }
            Action::PersistUsageView(view) => {
                self.preferences.usage_view = view;
                let saved = self.preferences.save_usage_view(&self.config_dir);
                self.report_save(saved);
            }
            Action::PersistSorting { id, sorting } => {
                let saved = self
                    .preferences
                    .save_task_sort(&self.config_dir, &id, sorting);
                self.report_save(saved);
            }
            Action::PersistFiltering { id, filter } => {
                let saved = self
                    .preferences
                    .save_task_filter(&self.config_dir, &id, filter);
                self.report_save(saved);
            }
            Action::PersistGrouping(grouping) => {
                self.preferences.grouping = grouping;
                let saved = self.preferences.save_grouping(&self.config_dir);
                self.report_save(saved);
            }
            Action::Copy { text, label } => match self.clipboard.copy(&text) {
                Ok(()) => self.view.show_copied(label, Instant::now()),
                Err(error) => self.view.show_error(format!("Cannot copy: {error}")),
            },
            Action::Unsupported(what) => {
                self.view.show_error(format!("{what} is not supported"));
            }
            Action::Open(target) => {
                let target = match target {
                    crate::ui::links::LinkTarget::Path(path) => path,
                    crate::ui::links::LinkTarget::Image(source) => match source.local_file() {
                        Ok(path) => path,
                        Err(error) => {
                            self.view.show_error(format!("Cannot open image: {error}"));
                            return false;
                        }
                    },
                };
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
                    Err(error) => self
                        .view
                        .show_error(format!("Cannot open {}: {error}", target.display())),
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
            let unsupported = error
                .get_ref()
                .is_some_and(|error| error.is::<crate::state::UnsupportedConfiguration>());
            self.view.show_error(if unsupported {
                error.to_string()
            } else {
                format!("Cannot save preferences: {error}")
            });
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
            self.view.show_error(format!(
                "Directory no longer exists: {}",
                session.cwd.display()
            ));
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

#[path = "app_refresh.rs"]
mod session_refresh;
use session_refresh::SessionRefresh;

#[path = "app_keyboard.rs"]
mod keyboard;

#[path = "app_terminal.rs"]
mod session_terminal;

fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    crossterm_terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        crossterm_terminal::EnterAlternateScreen,
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
    crossterm_terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm_terminal::LeaveAlternateScreen
    )?;
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
#[path = "app_event_search_tests.rs"]
mod event_search_tests;

#[cfg(test)]
#[path = "app_weekly_usage_tests.rs"]
mod weekly_usage_tests;

#[cfg(test)]
#[path = "app_reset_credit_tests.rs"]
mod reset_credit_tests;

#[cfg(test)]
#[path = "app_transcript_tests.rs"]
mod transcript_tests;

#[cfg(test)]
#[path = "app_refresh_tests.rs"]
mod refresh_tests;

#[cfg(test)]
#[path = "app_codex_paging_tests.rs"]
mod codex_paging_tests;

#[cfg(test)]
#[path = "app_event_pin_tests.rs"]
mod event_pin_tests;
