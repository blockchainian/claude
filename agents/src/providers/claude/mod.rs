//! Session sources: the live registry and transcripts.

mod account;
mod api;
#[cfg(test)]
mod archive_live_tests;
mod attach;
#[cfg(test)]
mod attach_live_tests;
mod background;
pub(crate) mod context;
#[cfg(test)]
mod create_tests;
mod defaults;
mod models;
#[cfg(test)]
mod rename_live_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
pub use models::write_fixture_cache;
mod ask_user_question;
pub(crate) mod event_reader;
mod images;
#[cfg(test)]
mod lazy_sessions_tests;
mod peer;
pub(super) mod registry;
mod session_socket;
mod settings;
#[cfg(test)]
mod socket_tests;
mod subagents;
mod transcript;
mod weekly_usage;

use crate::model::{Action, Process, Session, SessionStatus};
use crate::providers::{
    Capabilities, Provider, ProviderId, SessionAction, SessionInput, SessionKey, Snapshot,
    TerminalCommand, TerminalInput,
};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use transcript::Summary;

/// Why a session running in another terminal takes no input here, and how to fix it.
pub const FOREGROUND_SESSION: &str = "Running in a terminal · /bg";

/// The attach client detaches on Ctrl-Z, leaving the background job running.
const DETACH: &[u8] = b"\x1a";

const PAGE_SIZE: usize = 10;
const ASK_USER_QUESTION_PORT: u16 = 47831;

/// Removes the transcript and its sibling directory without following symlinks.
pub fn delete(transcript: &Path) -> io::Result<()> {
    fs::remove_file(transcript)?;
    match fs::remove_dir_all(transcript.with_extension("")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Stops the matching registered process before deleting its session files.
pub fn delete_session(config_dir: &Path, session: &Session) -> io::Result<()> {
    let transcript = session
        .transcript
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Nothing to delete"))?;
    if let Some(process) = &session.process {
        let entry = session_entry(config_dir, session)?.expect("session has a process");
        let pid = process.pid;
        let directory = config_dir.join("sessions");
        let path = directory.join(format!("{pid}.json"));
        stop_session(session, &entry)?;
        // A process may remove its own registry entry while shutting down.
        match fs::read(&path) {
            Ok(bytes) => {
                let current: registry::Entry = serde_json::from_slice(&bytes)?;
                if current.pid != entry.pid
                    || current.session_id != entry.session_id
                    || current.proc_start != entry.proc_start
                    || current.started_at != entry.started_at
                {
                    return Err(io::Error::other("Session registry changed"));
                }
                remove_if_present(&path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        for file in fs::read_dir(directory)? {
            let file = file?;
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name
                .strip_prefix(&format!("{pid}."))
                .is_some_and(|suffix| suffix.ends_with(".key"))
            {
                remove_if_present(&file.path())?;
            }
        }
    }
    if let Some(job) = &session.background_job {
        background_command("rm", &job.id)?;
    }
    delete(transcript)
}

/// Reads the registry entry only if it still belongs to this session process.
fn session_entry(config_dir: &Path, session: &Session) -> io::Result<Option<registry::Entry>> {
    let Some(process) = &session.process else {
        return Ok(None);
    };
    let pid = process.pid;
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid process id",
        ));
    }
    let directory = config_dir.join("sessions");
    let path = directory.join(format!("{pid}.json"));
    let entry: registry::Entry = serde_json::from_slice(&fs::read(&path)?)?;
    if entry.pid != Some(pid)
        || entry.session_id.as_deref() != Some(session.id.id.as_str())
        || entry.started_at != process.started_at.map(|time| time.timestamp_millis())
    {
        return Err(io::Error::other("Session registry changed"));
    }
    Ok(Some(entry))
}

/// Finds a job woken by this terminal that has remained idle without another turn.
fn idle_terminal_session(
    config_dir: &Path,
    session: &Session,
    offset: u64,
) -> io::Result<Option<(Session, registry::Entry)>> {
    let Some(entry) = registry::live(config_dir).into_iter().find(|entry| {
        entry.session_id.as_deref() == Some(&session.id.id)
            && entry.kind.as_deref() == Some("bg")
            && entry.job_id.as_deref() == session.id.id.get(..8)
            && entry.status.as_deref() == Some("idle")
    }) else {
        return Ok(None);
    };
    let path = session
        .transcript
        .as_deref()
        .ok_or_else(|| io::Error::other("Session has no transcript"))?;
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < offset {
        return Ok(None);
    }
    let mut turn = false;
    let mut invalid = None;
    let end = crate::jsonl::read_lines(&mut file, offset, |line| {
        match serde_json::from_slice::<serde_json::Value>(line) {
            Ok(record) => turn |= matches!(record["type"].as_str(), Some("user" | "assistant")),
            Err(error) => invalid = Some(error),
        }
    })?;
    if let Some(error) = invalid {
        return Err(io::Error::new(io::ErrorKind::InvalidData, error));
    }
    // An incomplete record or rewritten transcript cannot prove that no turn started.
    if turn || end != len {
        return Ok(None);
    }
    let mut session = session.clone();
    session.process = Some(Process {
        pid: entry.pid.expect("live registry process"),
        label: entry.kind.clone(),
        started_at: entry.started_at.and_then(DateTime::from_timestamp_millis),
    });
    session.background_job = Some(crate::model::BackgroundJob {
        id: entry.job_id.clone().expect("matching background job"),
        needs: None,
        awaits_prompt: false,
    });
    Ok(Some((session, entry)))
}

/// Stops a registered session through its background job or terminal process.
fn stop_session(session: &Session, entry: &registry::Entry) -> io::Result<()> {
    let pid = entry.pid.expect("validated process id");
    if let Some(job) = &session.background_job {
        if entry.kind.as_deref() != Some("bg") || entry.job_id.as_deref() != Some(&job.id) {
            return Err(io::Error::other("Background job identity changed"));
        }
        background_command("stop", &job.id)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while process_exists(pid)? {
            let starts = registry::process_starts(&[pid]);
            if !starts
                .get(&pid)
                .is_some_and(|actual| registry::matches_start(entry, actual))
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::other("Claude background session did not stop"));
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    } else {
        stop_process(entry)?;
    }
    Ok(())
}

fn background_command(action: &str, job_id: &str) -> io::Result<()> {
    if job_id.is_empty() || job_id.starts_with('-') {
        return Err(io::Error::other("Invalid background job id"));
    }
    let output = Command::new("claude").args([action, job_id]).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Claude {action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn process_exists(pid: u32) -> io::Result<bool> {
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        Err(error)
    }
}

fn stop_process(entry: &registry::Entry) -> io::Result<()> {
    use std::time::{Duration, Instant};
    let pid = entry.pid.expect("validated process id");
    for (signal, timeout) in [(libc::SIGTERM, 5), (libc::SIGKILL, 2)] {
        if !process_exists(pid)? {
            return Ok(());
        }
        let starts = registry::process_starts(&[pid]);
        if !starts
            .get(&pid)
            .is_some_and(|actual| registry::matches_start(entry, actual))
        {
            return Err(io::Error::other(
                "Process start time does not match registry",
            ));
        }
        if unsafe { libc::kill(pid as libc::pid_t, signal) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(timeout);
        loop {
            if !process_exists(pid)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Session process did not exit",
    ))
}

pub(super) fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(
        std::env::var_os("HOME").expect("HOME is required when CLAUDE_CONFIG_DIR is unset"),
    )
    .join(".claude")
}

pub(crate) fn trust_config_path() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir().with_extension("json"))
}

#[derive(Clone)]
struct Transcript {
    id: String,
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

struct CachedSummary {
    modified: SystemTime,
    len: u64,
    summary: Summary,
}

pub struct ClaudeProvider {
    config_dir: PathBuf,
    index: HashMap<String, Transcript>,
    summaries: HashMap<PathBuf, CachedSummary>,
    starts: HashMap<PathBuf, Option<DateTime<Utc>>>,
    background: background::Tasks,
    ask_user_question: Option<std::sync::Arc<ask_user_question::AskUserQuestion>>,
}

const BACKGROUND_ARGS: [&str; 2] = ["--bg", "--allow-dangerously-skip-permissions"];

fn created_job(output: &std::process::Output) -> io::Result<String> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Claude task creation failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = std::str::from_utf8(&output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut ids = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("backgrounded · "))
        .map(|id| {
            let id = id.split_once(" · ").map_or(id, |(id, _)| id);
            id.strip_suffix(" (idle — send a prompt to start)")
                .unwrap_or(id)
        });
    let id = ids.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude did not report a background job id",
        )
    })?;
    if id.is_empty()
        || id.starts_with('-')
        || id.chars().any(char::is_whitespace)
        || ids.next().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude reported an invalid background job id",
        ));
    }
    Ok(id.to_owned())
}

fn finish_creation(
    output: std::process::Output,
    request: crate::tasks::NewTask,
    settings: &Path,
    created: &dyn Fn(String),
) -> io::Result<std::process::Output> {
    let job_id = created_job(&output)?;
    created(job_id.clone());
    if !request.images.is_empty() {
        attach::send(
            &job_id,
            &request.cwd,
            settings,
            SessionInput::PromptWithImages {
                text: request.prompt,
                images: request.images,
            },
        )?;
    }
    Ok(output)
}

impl ClaudeProvider {
    fn terminal_question_pending(&self, session: &Session) -> bool {
        self.ask_user_question
            .as_ref()
            .is_some_and(|server| server.pending().iter().any(|ask| ask.session == session.id))
    }

    fn create_command(request: &crate::tasks::NewTask) -> Command {
        let mut command = Command::new("claude");
        command.args(BACKGROUND_ARGS).current_dir(&request.cwd);
        if let Some(name) = &request.name {
            command.args(["--name", name]);
        }
        if let Some(model) = &request.model {
            command.args(["--model", model]);
        }
        if let Some(effort) = &request.effort {
            command.args(["--effort", effort]);
        }
        if request.images.is_empty() {
            command.arg(&request.prompt);
        }
        command
    }

    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            index: HashMap::new(),
            summaries: HashMap::new(),
            starts: HashMap::new(),
            background: background::Tasks::default(),
            ask_user_question: None,
        }
    }

    pub fn from_config() -> io::Result<Self> {
        let config_dir = config_dir();
        if !config_dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "no Claude Code config directory at {}",
                    config_dir.display()
                ),
            ));
        }
        Ok(Self::new(config_dir))
    }

    pub fn preferences_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }

    fn snapshot(&mut self, pages: usize) -> Snapshot {
        self.snapshot_sessions(pages, None, &[])
    }

    fn snapshot_sessions(
        &mut self,
        pages: usize,
        selected: Option<&[SessionKey]>,
        previous: &[Session],
    ) -> Snapshot {
        let cached = |id: &str| {
            if selected.is_some_and(|ids| !ids.iter().any(|key| key.id == id)) {
                previous
                    .iter()
                    .find(|session| {
                        session.id.provider == ProviderId::Claude && session.id.id == id
                    })
                    .cloned()
            } else {
                None
            }
        };
        let mut unchanged = Vec::new();
        self.list_transcripts();
        let jobs = registry::background_jobs(&self.config_dir);
        // Claude Code writes a transcript only after the first prompt, and
        // `claude --resume` cannot open a session without one.
        let live: Vec<_> = registry::live(&self.config_dir)
            .into_iter()
            .filter(|entry| {
                entry
                    .session_id
                    .as_ref()
                    .is_some_and(|id| self.index.contains_key(id))
            })
            .collect();
        let live_ids: HashSet<_> = live
            .iter()
            .filter_map(|entry| entry.session_id.clone())
            .collect();
        let mut sessions = Vec::new();
        for entry in live {
            let id = entry
                .session_id
                .as_deref()
                .expect("live entries have session ids");
            if let Some(session) = cached(id) {
                unchanged.push(session);
                continue;
            }
            let (summary, path, modified) = self.summary(id);
            self.background.observe(id, &summary);
            let custom_title = summary.custom_title.clone();
            let has_title = custom_title.is_some() || summary.ai_title.is_some();
            let mut session = session(id.to_owned(), summary, path, modified);
            if let Some(cwd) = entry.cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
                session.cwd = PathBuf::from(cwd);
            } else if session.cwd.as_os_str().is_empty() {
                session.cwd = self.config_dir.clone();
            }
            session.name = custom_title.or_else(|| {
                entry
                    .name
                    .as_deref()
                    .filter(|_| entry.name_source.as_deref() == Some("user"))
                    .map(crate::providers::text::printable)
            });
            if !has_title
                && entry.name_source.as_deref() == Some("derived")
                && let Some(name) = entry.name.as_deref().filter(|name| !name.trim().is_empty())
            {
                session.title = crate::providers::text::printable(name);
            }
            session.status = entry
                .status
                .as_deref()
                .and_then(registry::from_registry)
                .unwrap_or(SessionStatus::Ready);
            // The registry only rewrites updatedAt on a status change, so a
            // long busy turn keeps its start time unless the transcript is newer.
            if let Some(updated) = entry.updated_at.and_then(DateTime::from_timestamp_millis) {
                session.updated_at = session.last_record_at.map_or(updated, |at| at.max(updated));
            }
            if entry.kind.as_deref() == Some("bg") {
                session.background_job = entry.job_id.filter(|id| !id.is_empty()).map(|id| {
                    let needs = jobs
                        .get(&session.id.id)
                        .filter(|state| state.job.id == id)
                        .and_then(|state| state.job.needs.clone());
                    crate::model::BackgroundJob {
                        id,
                        needs,
                        awaits_prompt: false,
                    }
                });
            }
            if let Some(state) = jobs.get(&session.id.id).filter(|state| {
                session
                    .background_job
                    .as_ref()
                    .is_some_and(|job| job.id == state.job.id)
            }) {
                if let Some(model) = &state.model {
                    session.model = Some(model.clone());
                }
                if let Some(effort) = &state.effort {
                    session.effort = Some(effort.clone());
                }
            }
            session.process = entry.pid.map(|pid| Process {
                pid,
                label: entry.kind,
                started_at: entry.started_at.and_then(DateTime::from_timestamp_millis),
            });
            sessions.push(session);
        }
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        // Child transcripts share the history page; only descendants of live
        // sessions are selected independently of that page.
        let child_files: Vec<_> = self
            .index
            .values()
            .flat_map(|file| {
                let parent = SessionKey {
                    provider: ProviderId::Claude,
                    id: file.id.clone(),
                };
                subagent_files(&file.path)
                    .into_iter()
                    .map(move |file| (file, parent.clone()))
            })
            .collect();
        let mut paths: HashSet<PathBuf> =
            self.index.values().map(|file| file.path.clone()).collect();
        paths.extend(child_files.iter().map(|(file, _)| file.path.clone()));
        let mut children = Vec::new();
        for (file, parent) in child_files
            .iter()
            .filter(|(_, parent)| live_ids.contains(&parent.id))
        {
            if let Some(session) = cached(&file.id) {
                unchanged.push(session);
                continue;
            }
            if let Some(child) = self.subagent(file, parent.clone(), true) {
                children.push(child);
            }
        }
        let mut inactive: Vec<_> = self
            .index
            .values()
            .filter(|file| !live_ids.contains(&file.id))
            .cloned()
            .map(|file| (file, None))
            .chain(
                child_files
                    .into_iter()
                    .map(|(file, parent)| (file, Some(parent))),
            )
            .collect();
        inactive.sort_by(|a, b| {
            b.0.modified
                .cmp(&a.0.modified)
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        let shown = pages.saturating_mul(PAGE_SIZE);
        let has_more = inactive.len() > shown;
        for (file, parent) in inactive.into_iter().take(shown) {
            if let Some(parent) = parent {
                // An already selected live descendant still consumes its history slot.
                if live_ids.contains(&parent.id) {
                    continue;
                }
                if let Some(session) = cached(&file.id) {
                    unchanged.push(session);
                    continue;
                }
                if let Some(child) = self.subagent(&file, parent, false) {
                    children.push(child);
                }
            } else {
                if let Some(session) = cached(&file.id) {
                    unchanged.push(session);
                    continue;
                }
                let (summary, path, modified) = self.summary(&file.id);
                self.background.observe(&file.id, &summary);
                sessions.push(session(file.id, summary, path, modified));
            }
        }
        for session in &mut sessions {
            if session.process.is_none() {
                session.background_job = jobs.get(&session.id.id).map(|state| state.job.clone());
            }
            // A background job that ended its turn on a request for the user
            // still shows idle in the session registry.
            if session.status == SessionStatus::Ready
                && let Some(job) = &mut session.background_job
                && job.needs.is_some()
            {
                job.awaits_prompt = true;
                session.status = SessionStatus::NeedsInput;
            }
        }
        if let Err(error) =
            self.background
                .refresh(&self.config_dir, &mut sessions, &background::Processes)
        {
            eprintln!("agents: Claude background tasks: {error}");
        }
        self.summaries.retain(|path, _| paths.contains(path));
        sessions.extend(children);
        sessions.extend(unchanged);
        for session in &mut sessions {
            session.ask = None;
        }
        if let Some(ask_user_question) = &self.ask_user_question {
            for ask in ask_user_question.pending() {
                if let Some(session) = sessions
                    .iter_mut()
                    .find(|session| session.id == ask.session)
                    && session.background_job.is_some()
                {
                    let crate::ask::AskKind::Questions { questions, .. } = &ask.kind;
                    session.pending_questions = questions.iter().map(|q| q.text.clone()).collect();
                    session.status = SessionStatus::NeedsInput;
                    session.ask = Some(ask);
                }
            }
        }
        Snapshot {
            sessions,
            has_more,
            supervisions: vec![],
        }
    }

    fn list_transcripts(&mut self) {
        self.index.clear();
        for project in fs::read_dir(self.config_dir.join("projects"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            for entry in fs::read_dir(project.path())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                let path = entry.path();
                if path
                    .extension()
                    .is_none_or(|extension| extension != "jsonl")
                {
                    continue;
                }
                let Some(id) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|id| is_uuid(id))
                else {
                    continue;
                };
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                let id = id.to_owned();
                let file = Transcript {
                    id: id.clone(),
                    path,
                    modified,
                    len: metadata.len(),
                };
                if self
                    .index
                    .get(&id)
                    .is_none_or(|previous| previous.modified < modified)
                {
                    self.index.insert(id, file);
                }
            }
        }
    }

    fn summary(&mut self, id: &str) -> (Summary, Option<PathBuf>, DateTime<Utc>) {
        let Some(file) = self.index.get(id).cloned() else {
            return (Summary::default(), None, Utc::now());
        };
        let summary = self.summary_file(&file, false).unwrap_or_default();
        (summary, Some(file.path.clone()), file.modified.into())
    }

    fn summary_file(&mut self, file: &Transcript, subagent: bool) -> Option<Summary> {
        let changed = self
            .summaries
            .get(&file.path)
            .is_none_or(|cached| cached.modified != file.modified || cached.len != file.len);
        if changed {
            // Files may disappear while Claude Code moves or removes a project.
            let read = if subagent {
                transcript::read_subagent_cached
            } else {
                transcript::read_cached
            };
            let start = match self.starts.get(&file.path).copied() {
                Some(start) => start,
                None => match transcript::start_time(&file.path) {
                    Ok(start) => {
                        self.starts.insert(file.path.clone(), start);
                        start
                    }
                    Err(error) => {
                        eprintln!("agents: Claude transcript start: {error}");
                        return None;
                    }
                },
            };
            let previous = self
                .summaries
                .get(&file.path)
                .filter(|cached| cached.len < file.len)
                .map(|cached| &cached.summary);
            match read(&file.path, file.len, previous) {
                Ok(mut summary) => {
                    summary.started_at = start;
                    self.summaries.insert(
                        file.path.clone(),
                        CachedSummary {
                            modified: file.modified,
                            len: file.len,
                            summary,
                        },
                    );
                }
                Err(_) => {
                    self.summaries.remove(&file.path);
                }
            }
        }
        self.summaries
            .get(&file.path)
            .map(|cached| cached.summary.clone())
    }

    fn subagent(&mut self, file: &Transcript, parent: SessionKey, alive: bool) -> Option<Session> {
        let summary = self.summary_file(file, true)?;
        let meta = subagents::Metadata::read(&file.path.with_extension("meta.json"))?;
        let status = meta.status(&summary, &self.config_dir, alive);
        let mut child = session(
            file.id.clone(),
            summary,
            Some(file.path.clone()),
            file.modified.into(),
        );
        child.parent = Some(parent);
        child.title = meta.title();
        child.prompt = child.title.clone();
        child.name = meta.name();
        child.status = status;
        Some(child)
    }
}

fn subagent_files(transcript: &Path) -> Vec<Transcript> {
    let mut files = Vec::new();
    for entry in fs::read_dir(transcript.with_extension("").join("subagents"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "jsonl")
        {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|id| id.strip_prefix("agent-").is_some_and(|id| !id.is_empty()))
        else {
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let file = Transcript {
            id: id.to_owned(),
            path: path.clone(),
            modified,
            len: metadata.len(),
        };
        files.push(file);
    }
    files.sort_by(|a, b| a.id.cmp(&b.id));
    files
}

impl Provider for ClaudeProvider {
    fn session_source(&self) -> Box<dyn Provider> {
        let mut source = Self::new(self.config_dir.clone());
        source.ask_user_question = self.ask_user_question.clone();
        Box::new(source)
    }

    fn refresh_sessions(
        &mut self,
        pages: usize,
        sessions: &[SessionKey],
        previous: &[Session],
    ) -> Snapshot {
        self.snapshot_sessions(pages, Some(sessions), previous)
    }

    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn setup(&mut self) -> io::Result<()> {
        settings::ensure_ask_user_question(
            &self.config_dir.join("settings.json"),
            ASK_USER_QUESTION_PORT,
        )?;
        match ask_user_question::AskUserQuestion::bind(
            ASK_USER_QUESTION_PORT,
            self.config_dir.clone(),
        ) {
            Ok(server) => self.ask_user_question = Some(std::sync::Arc::new(server)),
            // Without a listener, Claude owns question input and the dashboard only displays it.
            Err(_) => self.ask_user_question = None,
        }
        Ok(())
    }

    fn terminal_command(&self, session: &Session) -> Result<TerminalCommand, &'static str> {
        let ask_user_question_pending = self.terminal_question_pending(session);
        let command = if ask_user_question_pending {
            let job = session
                .background_job
                .as_ref()
                .ok_or("Terminal is not supported")?;
            let mut command = Command::new("claude");
            command.args(["attach", &job.id]).current_dir(&session.cwd);
            command
        } else if session.is_live_background() {
            self.launch(&Action::Resume(session.id.clone()), Some(session))
                .ok_or("Terminal is not supported")?
        } else if session.process.is_some() {
            return Err(FOREGROUND_SESSION);
        } else {
            // A session with no live process wakes as a background job under the same
            // id, whose job id is the session id's first eight characters.
            let job = session.id.id.get(..8).ok_or("Terminal is not supported")?;
            let mut command = Command::new("sh");
            command
                .args([
                    "-c",
                    &format!(
                        r#"claude {} --resume "$1" >/dev/null && exec claude attach "$2""#,
                        BACKGROUND_ARGS.join(" ")
                    ),
                    "sh",
                    &session.id.id,
                    job,
                ])
                .current_dir(&session.cwd);
            command
        };
        if let Some(ask_user_question) = &self.ask_user_question {
            ask_user_question
                .release(&session.id.id)
                .map_err(|_| "Cannot release AskUserQuestion")?;
        }
        Ok(TerminalCommand {
            command,
            detach: Some(DETACH),
        })
    }

    fn terminal_cleanup(&self, session: &Session) -> io::Result<Option<SessionAction>> {
        if session.process.is_some() || self.terminal_question_pending(session) {
            return Ok(None);
        }
        use std::os::unix::fs::MetadataExt;
        let path = session
            .transcript
            .as_deref()
            .ok_or_else(|| io::Error::other("Session has no transcript"))?;
        let mut file = fs::File::open(path)?;
        let metadata = file.metadata()?;
        let offset = crate::jsonl::complete_end(&mut file)?;
        let mut boundary = crate::jsonl::Boundary::default();
        boundary.capture(&mut file, offset)?;
        let config_dir = self.config_dir.clone();
        let session = session.clone();
        Ok(Some(Box::new(move || {
            let path = session.transcript.as_ref().expect("terminal transcript");
            let mut file = fs::File::open(path)?;
            let current = file.metadata()?;
            if current.ino() != metadata.ino()
                || current.len() < offset
                || !boundary.matches(&mut file)?
            {
                return Ok(());
            }
            if let Some((session, entry)) = idle_terminal_session(&config_dir, &session, offset)? {
                // Revalidate after reading the transcript, just as manual Stop validates its target.
                let current =
                    session_entry(&config_dir, &session)?.expect("woken session has a process");
                if current.status.as_deref() == Some("idle")
                    && current.proc_start == entry.proc_start
                {
                    stop_session(&session, &current)?;
                }
            }
            Ok(())
        })))
    }

    fn terminal_input<'a>(&self, input: &'a SessionInput) -> Option<TerminalInput<'a>> {
        // The open attach client already owns this session's input.
        Some(match input {
            SessionInput::Prompt(text) => TerminalInput::Paste {
                text,
                submit: b"\r",
                prepare: Some(attach::clear_left_input),
            },
            SessionInput::Interrupt => TerminalInput::Write(b"\x1b"),
            _ => TerminalInput::Close,
        })
    }

    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn watch_roots(&self) -> io::Result<Vec<PathBuf>> {
        let sessions = self.config_dir.join("sessions");
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&sessions)?;
        let jobs = self.config_dir.join("jobs");
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&jobs)?;
        Ok(vec![sessions, self.config_dir.join("projects"), jobs])
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            fork: true,
            rename: true,
            delete: true,
            archive: true,
            ..Capabilities::default()
        }
    }

    fn refresh(&mut self, pages: usize) -> Snapshot {
        self.snapshot(pages)
    }
    #[cfg(test)]
    fn event_panel(
        &self,
        session: &Session,
        count: usize,
        sidechains: bool,
    ) -> io::Result<Vec<crate::event::Event>> {
        let path = session
            .transcript
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Session has no transcript"))?;
        event_reader::tail_session(path, count, sidechains)
    }

    fn create(
        &self,
        request: &crate::tasks::NewTask,
        created: Box<dyn Fn(String) + Send>,
    ) -> Option<io::Result<crate::providers::CreateJob>> {
        let mut command = Self::create_command(request);
        let request = request.clone();
        let settings = self.config_dir.join("settings.json");
        Some(Ok(Box::new(move || {
            let output = command.output()?;
            finish_creation(output, request, &settings, &created)
        })))
    }

    fn input_choices(&self) -> crate::providers::InputChoices {
        models::input_choices(&self.config_dir, models::installed_version().as_deref())
    }

    fn account_usage(&self) -> Option<crate::providers::AccountUsageJob> {
        Some(Box::new(weekly_usage::read_account_usage))
    }

    fn redeem_reset_credit(
        &self,
        request: crate::providers::ResetCreditRequest,
    ) -> Option<crate::providers::ResetCreditJob> {
        let config_dir = self.config_dir.clone();
        Some(Box::new(move || weekly_usage::redeem(&config_dir, request)))
    }

    fn weekly_usage(&self) -> Option<crate::providers::WeeklyUsageJob> {
        Some(Box::new(weekly_usage::read))
    }

    fn task_defaults(&self, cwd: &Path) -> Option<io::Result<crate::tasks::TaskDefaults>> {
        Some(
            defaults::resolve(&self.config_dir, cwd, |name| std::env::var(name).ok()).map(
                |mut defaults| {
                    let builtin = defaults::builtin();
                    // With no configured effort, Claude uses the model's own default.
                    if defaults.effort.is_none() {
                        defaults.effort = defaults.model.as_deref().and_then(|model| {
                            self.input_choices().model(model)?.default_effort.clone()
                        });
                        if defaults.effort.is_none() && defaults.model == builtin.model {
                            defaults.effort = builtin.effort;
                        }
                    }
                    defaults
                },
            ),
        )
    }

    fn created_job(&self, output: &std::process::Output) -> io::Result<String> {
        created_job(output)
    }

    fn send(&self, session: &Session, input: SessionInput) -> Option<SessionAction> {
        if let SessionInput::Answer { ask, answer, .. } = &input {
            if ask.session != session.id {
                return Some(Box::new(|| {
                    Err(io::Error::other("Claude question session changed"))
                }));
            }
            if let Some(server) = &self.ask_user_question
                && let Some(pending) = server
                    .pending()
                    .into_iter()
                    .find(|pending| pending.id == ask.id && pending.session == ask.session)
            {
                let server = server.clone();
                let answer = answer.clone();
                return Some(Box::new(move || server.answer(&pending, &answer)));
            }
            return Some(Box::new(|| {
                Err(io::Error::other(
                    "Claude AskUserQuestion is no longer pending",
                ))
            }));
        }
        if !session.can_resume() {
            return None;
        }
        let session = session.clone();
        let config = self.config_dir.clone();
        if let SessionInput::Rename(name) = input {
            return Some(Box::new(move || {
                let process = session
                    .process
                    .as_ref()
                    .ok_or_else(|| io::Error::other("This input requires a live Claude session"))?;
                let mut client =
                    session_socket::Client::connect(&config, process.pid, &session.id.id)?;
                session_socket::session::set_name(&mut client, &name)
            }));
        }
        Some(Box::new(move || match &session.process {
            Some(process) if process.label.as_deref() == Some("bg") => {
                let entry: registry::Entry = serde_json::from_slice(&fs::read(
                    config
                        .join("sessions")
                        .join(format!("{}.json", process.pid)),
                )?)
                .map_err(io::Error::other)?;
                if entry.session_id.as_deref() != Some(&session.id.id)
                    || entry.kind.as_deref() != Some("bg")
                {
                    return Err(io::Error::other(
                        "Claude background session identity changed",
                    ));
                }
                let job_id = entry
                    .job_id
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| io::Error::other("Claude background session has no job id"))?;
                attach::send(&job_id, &session.cwd, &config.join("settings.json"), input)
            }
            Some(_) => Err(io::Error::other(FOREGROUND_SESSION)),
            None => match input {
                input @ (SessionInput::Prompt(_)
                | SessionInput::SetModel(_)
                | SessionInput::SetEffort(_)) => {
                    let output = resume_input(&session, &input)?.output()?;
                    if output.status.success() {
                        Ok(())
                    } else {
                        Err(io::Error::other(format!(
                            "Claude resume failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        )))
                    }
                }
                _ => Err(io::Error::other(
                    "This input requires a live background session",
                )),
            },
        }))
    }

    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command> {
        let mut command = Command::new("claude");
        match action {
            Action::New => {
                command.current_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
            }
            Action::Resume(_) | Action::Fork(_) => {
                let session = session?;
                if session.parent.is_some() || !session.cwd.is_dir() {
                    return None;
                }
                if session.is_live_background() && !matches!(action, Action::Fork(_)) {
                    command
                        .args([
                            "attach",
                            &session
                                .background_job
                                .as_ref()
                                .expect("live background job")
                                .id,
                        ])
                        .current_dir(&session.cwd);
                    return Some(command);
                }
                command
                    .args(["--resume", &session.id.id])
                    .current_dir(&session.cwd);
                if matches!(action, Action::Fork(_)) {
                    command.arg("--fork-session");
                }
            }
            _ => return None,
        }
        Some(command)
    }

    fn delete(&self, session: &Session) -> io::Result<SessionAction> {
        let config_dir = self.config_dir.clone();
        let session = session.clone();
        Ok(Box::new(move || delete_session(&config_dir, &session)))
    }
    fn archive(&self, session: &Session) -> io::Result<SessionAction> {
        let config_dir = self.config_dir.clone();
        let session = session.clone();
        Ok(Box::new(move || {
            let ended = || {
                io::Error::other(
                    "session has already ended; Claude Code archived it when it stopped",
                )
            };
            let entry = match session_entry(&config_dir, &session) {
                Ok(Some(entry)) => entry,
                Ok(None) => return Err(ended()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(ended()),
                Err(error) => return Err(error),
            };
            let pid = entry.pid.expect("validated process id");
            if !process_exists(pid)? {
                return Err(ended());
            }
            let Some(bridge_id) = entry
                .bridge_session_id
                .as_deref()
                .filter(|id| !id.is_empty())
            else {
                return stop_session(&session, &entry);
            };
            let mut client = api::Client::connect(&config_dir)?;
            stop_session(&session, &entry)?;
            api::session::archive(&mut client, bridge_id)
        }))
    }
}

fn resume_input(session: &Session, input: &SessionInput) -> io::Result<Command> {
    let mut command = Command::new("claude");
    command
        .args(BACKGROUND_ARGS)
        .args(["--resume", &session.id.id])
        .current_dir(&session.cwd);
    match input {
        SessionInput::Prompt(text) => {
            command.arg(text);
        }
        SessionInput::SetModel(model) => {
            command.args(["--model", model]);
        }
        SessionInput::SetEffort(effort) => {
            command.args(["--effort", effort]);
        }
        _ => return Err(io::ErrorKind::Unsupported.into()),
    }
    Ok(command)
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn first_line(prompt: &str) -> String {
    prompt
        .lines()
        .next()
        .filter(|line| !line.trim().is_empty())
        .unwrap_or("Untitled session")
        .to_owned()
}

fn session(
    id: String,
    summary: Summary,
    path: Option<PathBuf>,
    updated_at: DateTime<Utc>,
) -> Session {
    let title = summary
        .custom_title
        .or(summary.ai_title)
        .unwrap_or_else(|| first_line(&summary.prompt));
    let cwd = summary.cwd.unwrap_or_else(|| {
        path.as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""))
            .to_owned()
    });
    Session {
        id: SessionKey {
            provider: ProviderId::Claude,
            id: id.clone(),
        },
        parent: None,
        cwd,
        name: None,
        title,
        prompt: summary.prompt,
        task_details_sources: summary.task_details_sources,
        prompt_images: summary.prompt_images,
        queued: summary.queued,
        prompt_at: summary.prompt_at,
        turn_messages: summary.turn_messages,
        last_record_at: summary.timestamp,
        cache_expires_at: summary.cache_expires_at,
        model: summary.model,
        effort: summary.effort,
        git_branch: summary.git_branch,
        pr: summary.pr,
        status: SessionStatus::Inactive,
        process: None,
        background_job: None,
        updated_at,
        started_at: summary.started_at,
        last_message: summary.last_message,
        last_message_at: summary.last_message_at,
        activity: summary.activity,
        ask: None,
        tasks: summary.tasks,
        pending_questions: summary.pending_questions,
        background: summary.background,
        last_prompt: summary.last_prompt,
        transcript: path,
        archived: false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminal_woken_job_stops_only_without_a_turn() {
        use std::io::Write;
        for change in [
            "none",
            "metadata",
            "busy",
            "waiting",
            "shell",
            "partial",
            "truncated",
            "changed-start",
            "other-session",
            "started",
            "completed",
            "other-job",
            "foreground",
            "stopped",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let id = "4165651d-b9e3-458d-8612-f8114e312363";
            let path = transcript(directory.path(), id, 0);
            fs::write(&path, FIXTURE).unwrap();
            let mut provider = ClaudeProvider::new(directory.path().into());
            let session = provider.refresh(1).sessions.remove(0);
            let path = session.transcript.as_ref().unwrap();
            let offset = fs::metadata(path).unwrap().len();
            let mut entry = registry::tests::fixture(directory.path());
            entry["sessionId"] = session.id.id.clone().into();
            entry["kind"] = "bg".into();
            entry["jobId"] = session.id.id[..8].into();
            entry["status"] = "idle".into();
            match change {
                "busy" | "waiting" | "shell" => entry["status"] = change.into(),
                "partial" => {
                    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
                    file.write_all(b"{\"type\":\"user\"").unwrap();
                }
                "truncated" => fs::write(path, "").unwrap(),
                "changed-start" => entry["procStart"] = "Thu Jan  1 00:00:00 1970".into(),
                "other-session" => entry["sessionId"] = uuid::Uuid::new_v4().to_string().into(),
                "foreground" => entry["kind"] = "interactive".into(),
                "other-job" => entry["jobId"] = "other-job".into(),
                "stopped" => entry["pid"] = 999999.into(),
                "started" | "completed" | "metadata" => {
                    let lines = include_str!("../../../tests/fixtures/transcript.jsonl");
                    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
                    for line in lines.lines() {
                        let record: serde_json::Value = serde_json::from_str(line).unwrap();
                        let wanted = match change {
                            "started" => record["type"] == "user",
                            "completed" => {
                                matches!(record["type"].as_str(), Some("user" | "assistant"))
                            }
                            _ => record["type"] == "ai-title",
                        };
                        if wanted {
                            writeln!(file, "{line}").unwrap();
                            if change != "completed" {
                                break;
                            }
                        }
                    }
                }
                _ => {}
            }
            fs::write(
                directory
                    .path()
                    .join(format!("sessions/{}.json", entry["pid"])),
                serde_json::to_vec(&entry).unwrap(),
            )
            .unwrap();
            let candidate = idle_terminal_session(directory.path(), &session, offset).unwrap();
            assert_eq!(
                candidate.is_some(),
                matches!(change, "none" | "metadata"),
                "{change}"
            );
            if let Some((candidate, entry)) = candidate {
                assert_eq!(candidate.process.unwrap().pid, std::process::id());
                assert_eq!(candidate.background_job.unwrap().id, session.id.id[..8]);
                assert_eq!(entry.status.as_deref(), Some("idle"));
            }
        }
    }

    #[test]
    fn terminal_cleanup_belongs_only_to_a_woken_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = transcript(directory.path(), "4165651d-b9e3-458d-8612-f8114e312363", 0);
        fs::write(path, FIXTURE).unwrap();
        let mut provider = ClaudeProvider::new(directory.path().into());
        let mut session = provider.refresh(1).sessions.remove(0);
        assert!(provider.terminal_cleanup(&session).unwrap().is_some());
        session.process = Some(Process {
            pid: std::process::id(),
            label: Some("bg".into()),
            started_at: None,
        });
        session.background_job = Some(crate::model::BackgroundJob {
            id: session.id.id[..8].into(),
            needs: None,
            awaits_prompt: false,
        });
        assert!(provider.terminal_cleanup(&session).unwrap().is_none());
        session.process.as_mut().unwrap().label = None;
        assert!(provider.terminal_cleanup(&session).unwrap().is_none());
    }

    #[test]
    fn live_background_settings_override_the_previous_turn() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects/probe");
        fs::create_dir_all(&project).unwrap();
        let id = "4165651d-b9e3-458d-8612-f8114e312363";
        fs::write(project.join(format!("{id}.jsonl")), FIXTURE).unwrap();
        let mut entry = registry::tests::fixture(directory.path());
        entry["sessionId"] = id.into();
        entry["kind"] = "bg".into();
        entry["jobId"] = "settings-probe".into();
        fs::write(
            directory.path().join("sessions/live.json"),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();
        let mut state: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/claude-background-settings.json"
        ))
        .unwrap();
        state["sessionId"] = id.into();
        let job = directory.path().join("jobs/settings-probe");
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
        let mut provider = ClaudeProvider::new(directory.path().into());
        let session = provider.refresh(1).sessions.remove(0);
        assert!(session.is_live_background());
        assert_eq!(session.model.as_deref(), Some("sonnet"));
        assert_eq!(session.effort.as_deref(), Some("low"));
    }

    #[test]
    fn inactive_settings_resume_without_a_prompt() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects/probe");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
            include_bytes!("../../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        let mut provider = ClaudeProvider::new(directory.path().into());
        let session = provider.refresh(1).sessions.remove(0);
        for (input, option, value) in [
            (SessionInput::SetModel("sonnet".into()), "--model", "sonnet"),
            (SessionInput::SetEffort("high".into()), "--effort", "high"),
        ] {
            let command = resume_input(&session, &input).unwrap();
            let args: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect();
            assert_eq!(
                args,
                [
                    "--bg",
                    "--allow-dangerously-skip-permissions",
                    "--resume",
                    &session.id.id,
                    option,
                    value
                ]
            );
            assert_eq!(command.get_current_dir(), Some(session.cwd.as_path()));
        }
    }

    #[test]
    fn model_label_reads_the_transcript_name_of_the_session_model() {
        let fixture = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/claude-model-identity.jsonl"),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (transcript, label) in [
            (fixture.clone(), Some("Opus 5.5")),
            (
                fixture.replacen(
                    "\"modelId\":\"claude-opus-5-5\"",
                    "\"modelId\":\"claude-fable-5-1\"",
                    1,
                ),
                None,
            ),
        ] {
            let path = dir.path().join("model-identity.jsonl");
            std::fs::write(&path, transcript).unwrap();
            let len = std::fs::metadata(&path).unwrap().len();
            let summary = transcript::read(&path, len).unwrap();
            assert_eq!(
                summary
                    .model
                    .as_ref()
                    .and_then(|id| summary.model_identities.get(id))
                    .map(String::as_str),
                label
            );
        }
    }

    use super::*;
    use std::io::Write;
    use std::time::Duration;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/transcript.jsonl");

    #[test]
    fn ask_user_question_pending_questions_refresh_with_answers() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = transcript(dir.path(), id, 0);
        let pending = include_str!("../../../tests/fixtures/claude-questions-pending.jsonl");
        let answered = include_str!("../../../tests/fixtures/claude-questions-answered.jsonl");
        let record: serde_json::Value = serde_json::from_str(pending).unwrap();
        let expected = record["message"]["content"][0]["input"]["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|question| question["question"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut provider = ClaudeProvider::new(dir.path().into());
        for (fixture, questions) in [
            (pending, expected.clone()),
            (answered, vec![]),
            (pending, expected),
        ] {
            fs::write(&path, fixture).unwrap();
            let sessions = provider.refresh(1).sessions;
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].pending_questions, questions);
            assert!(sessions[0].ask.is_none());
        }
    }

    #[test]
    fn setup_leaves_questions_read_only_when_listener_is_unavailable() {
        use std::net::TcpListener;
        let _occupied =
            match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, ASK_USER_QUESTION_PORT)) {
                Ok(listener) => Some(listener),
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => None,
                Err(error) => panic!("cannot reserve question port: {error}"),
            };
        let dir = tempfile::tempdir().unwrap();
        let mut provider = ClaudeProvider::new(dir.path().into());
        provider.setup().unwrap();
        assert!(provider.ask_user_question.is_none());
        let settings: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"],
            3600
        );
    }

    pub(super) fn transcript(dir: &Path, id: &str, age: u64) -> PathBuf {
        let project = dir.join("projects/ambiguous-project-slug");
        fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("{id}.jsonl"));
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(FIXTURE.as_bytes()).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + age))
            .unwrap();
        path
    }

    #[test]
    fn transcript_start_is_cached_across_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = transcript(dir.path(), id, 0);
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/claude-start.jsonl"),
        )
        .unwrap();
        let expected = Some("2026-10-06T16:39:12.595Z".parse().unwrap());
        let mut provider = ClaudeProvider::new(dir.path().into());
        assert_eq!(provider.refresh(1).sessions[0].started_at, expected);
        // Re-reading this head would now select birthtime.
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/claude-start-fork.jsonl"),
        )
        .unwrap();
        assert_eq!(provider.refresh(1).sessions[0].started_at, expected);
    }

    #[test]
    fn fork_start_uses_birthtime_and_title_only_stays_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = transcript(dir.path(), id, 0);
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/claude-start-fork.jsonl"),
        )
        .unwrap();
        let expected = fs::metadata(&path)
            .unwrap()
            .created()
            .ok()
            .map(DateTime::<Utc>::from);
        let mut provider = ClaudeProvider::new(dir.path().into());
        assert_eq!(provider.refresh(1).sessions[0].started_at, expected);
        let title = FIXTURE.lines().nth(2).unwrap();
        let other = transcript(dir.path(), "00000000-0000-4000-8000-000000000002", 1);
        fs::write(other, title).unwrap();
        assert_eq!(provider.refresh(1).sessions[0].started_at, None);
    }

    #[test]
    fn archive_rejects_ended_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = transcript(dir.path(), "00000000-0000-0000-0000-000000000001", 0);
        let mut provider = ClaudeProvider::new(dir.path().into());
        let session = provider.refresh(1).sessions.remove(0);
        assert_eq!(
            provider.archive(&session).unwrap()()
                .unwrap_err()
                .to_string(),
            "session has already ended; Claude Code archived it when it stopped"
        );
        assert!(path.exists());
    }

    #[test]
    fn archive_without_remote_control_stops_process() {
        for bridge_id in [None, Some(serde_json::Value::Null), Some("".into())] {
            let dir = tempfile::tempdir().unwrap();
            let mut child = Command::new("sleep").arg("60").spawn().unwrap();
            let pid = child.id();
            let reaper = std::thread::spawn(move || child.wait().unwrap());
            let start = registry::process_starts(&[pid])[&pid][0];
            let mut entry = registry::tests::fixture(dir.path());
            entry["pid"] = pid.into();
            entry["startedAt"] = start.timestamp_millis().into();
            entry["procStart"] = start.format("%a %b %e %H:%M:%S %Y").to_string().into();
            match bridge_id {
                Some(value) => entry["bridgeSessionId"] = value,
                None => {
                    entry.as_object_mut().unwrap().remove("bridgeSessionId");
                }
            }
            let id = entry["sessionId"].as_str().unwrap();
            let path = transcript(dir.path(), id, 0);
            let registry_path = dir.path().join("sessions").join(format!("{pid}.json"));
            fs::write(&registry_path, serde_json::to_vec(&entry).unwrap()).unwrap();
            let mut provider = ClaudeProvider::new(dir.path().into());
            let session = provider.refresh(1).sessions.remove(0);
            assert_eq!(session.process.as_ref().unwrap().pid, pid);
            let result = provider.archive(&session).unwrap()();
            let running = process_exists(pid).unwrap();
            if running {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            }
            reaper.join().unwrap();
            result.unwrap();
            assert!(!running, "archiving must stop the local process");
            assert!(path.exists(), "archiving must keep the transcript");
        }
    }

    #[test]
    fn idle_background_job_with_needs_is_needs_input() {
        let dir = tempfile::tempdir().unwrap();
        let mut entry = registry::tests::fixture(dir.path());
        entry["kind"] = "bg".into();
        entry["jobId"] = "recorded".into();
        entry["status"] = "idle".into();
        let id = entry["sessionId"].as_str().unwrap().to_owned();
        transcript(dir.path(), &id, 0);
        fs::write(
            dir.path()
                .join("sessions")
                .join(format!("{}.json", std::process::id())),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();
        let job = dir.path().join("jobs/recorded");
        fs::create_dir_all(&job).unwrap();
        let state = serde_json::json!({
            "state": "blocked", "template": "bg", "sessionId": id,
            "needs": "Reply 改 to add the headers",
        });
        fs::write(job.join("state.json"), state.to_string()).unwrap();
        let mut provider = ClaudeProvider::new(dir.path().into());
        let session = provider.refresh(1).sessions.remove(0);
        assert_eq!(session.status, SessionStatus::NeedsInput);
        assert!(session.background_job.unwrap().awaits_prompt);
    }

    #[test]
    fn provider_identity_and_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let provider: Box<dyn Provider> = Box::new(ClaudeProvider::new(dir.path().into()));
        assert_eq!(provider.id(), ProviderId::Claude);
        assert_eq!(
            provider.capabilities(),
            Capabilities {
                resume: true,
                fork: true,
                rename: true,
                delete: true,
                stop: false,
                archive: true,
            }
        );
    }

    #[test]
    fn provider_watch_roots_and_actions_preserve_session_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-0000-0000-000000000001";
        let path = transcript(dir.path(), id, 0);
        let mut provider = ClaudeProvider::new(dir.path().into());
        assert_eq!(
            provider.watch_roots().unwrap(),
            vec![
                dir.path().join("sessions"),
                dir.path().join("projects"),
                dir.path().join("jobs")
            ]
        );
        assert_eq!(
            fs::metadata(dir.path().join("sessions"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let mut session = provider.refresh(1).sessions.remove(0);
        session.cwd = dir.path().into();
        let resume = provider
            .launch(&Action::Resume(session.id.clone()), Some(&session))
            .unwrap();
        assert_eq!(resume.get_program(), "claude");
        assert_eq!(resume.get_args().collect::<Vec<_>>(), ["--resume", id]);
        assert_eq!(resume.get_current_dir(), Some(dir.path()));
        let fork = provider
            .launch(&Action::Fork(session.id.clone()), Some(&session))
            .unwrap();
        assert_eq!(
            fork.get_args().collect::<Vec<_>>(),
            ["--resume", id, "--fork-session"]
        );
        let new = provider.launch(&Action::New, None).unwrap();
        assert_eq!(
            new.get_current_dir(),
            Some(std::env::current_dir().unwrap().as_path())
        );
        assert_eq!(new.get_args().count(), 0);
        assert_eq!(
            provider.event_panel(&session, 8, false).unwrap(),
            event_reader::tail(&path, 8).unwrap()
        );
        session.parent = Some(session.id.clone());
        assert!(
            provider
                .launch(&Action::Resume(session.id.clone()), Some(&session))
                .is_none()
        );
        session.parent = None;
        session.cwd = dir.path().join("missing");
        assert!(
            provider
                .launch(&Action::Resume(session.id.clone()), Some(&session))
                .is_none()
        );
        provider
            .delete(&session)
            .and_then(|delete| delete())
            .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn watch_roots_reports_sessions_directory_creation_failure() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("sessions"), "occupied").unwrap();
        let provider = ClaudeProvider::new(dir.path().into());
        assert!(provider.watch_roots().is_err());
    }

    #[test]
    #[ignore = "reads real config; set AGENTS_BENCH_CONFIG explicitly"]
    fn measure_refresh() {
        let config = std::env::var_os("AGENTS_BENCH_CONFIG").expect("AGENTS_BENCH_CONFIG required");
        let mut store = ClaudeProvider::new(config.into());
        let start = std::time::Instant::now();
        let snapshot = store.refresh(1);
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..20 {
            std::hint::black_box(store.refresh(1));
        }
        eprintln!(
            "refresh cold={cold:?} warm_mean={:?} transcripts={} cached={} sessions={}",
            start.elapsed() / 20,
            store.index.len(),
            store.summaries.len(),
            snapshot.sessions.len()
        );
    }

    #[test]
    fn changing_one_transcript_preserves_other_cached_allocations() {
        let dir = tempfile::tempdir().unwrap();
        let first_id = "00000000-0000-0000-0000-000000000001";
        let first = transcript(dir.path(), first_id, 1);
        let second = transcript(dir.path(), "00000000-0000-0000-0000-000000000002", 2);
        let mut store = ClaudeProvider::new(dir.path().into());
        store.refresh(1);
        let first_prompt = store.summaries[&first].summary.prompt.as_ptr() as usize;
        let second_prompt = store.summaries[&second].summary.prompt.as_ptr() as usize;
        let title = FIXTURE.lines().nth(2).unwrap();
        writeln!(
            fs::OpenOptions::new().append(true).open(&first).unwrap(),
            "{title}"
        )
        .unwrap();
        store.refresh(1);
        // A parsed summary allocates its prompt while the previous cached one still lives.
        assert_ne!(
            store.summaries[&first].summary.prompt.as_ptr() as usize,
            first_prompt
        );
        assert_eq!(
            store.summaries[&second].summary.prompt.as_ptr() as usize,
            second_prompt
        );
    }

    #[test]
    fn paging_orders_inactive_transcripts_and_caches_summaries() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..25 {
            transcript(
                dir.path(),
                &format!("00000000-0000-0000-0000-{index:012}"),
                index,
            );
        }
        let project = dir.path().join("projects/ambiguous-project-slug");
        fs::write(project.join("not-a-uuid.jsonl"), FIXTURE).unwrap();
        fs::create_dir_all(project.join("subagents")).unwrap();
        fs::write(
            project.join("subagents/ffffffff-ffff-ffff-ffff-ffffffffffff.jsonl"),
            FIXTURE,
        )
        .unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let first = store.refresh(1);
        assert_eq!(first.sessions.len(), 10);
        assert!(first.has_more);
        assert!(
            first
                .sessions
                .iter()
                .all(|session| session.status == SessionStatus::Inactive)
        );
        assert!(first.sessions[0].id.id.ends_with("000000000024"));
        assert_eq!(store.summaries.len(), 10);
        let cached = store
            .summaries
            .values()
            .next()
            .unwrap()
            .summary
            .prompt
            .clone();
        assert!(!cached.is_empty());
        assert_eq!(store.refresh(1).sessions, first.sessions);
        let all = store.refresh(3);
        assert_eq!(all.sessions.len(), 25);
        assert!(!all.has_more);
        assert!(
            all.sessions
                .windows(2)
                .all(|pair| pair[0].updated_at > pair[1].updated_at)
        );
    }

    #[test]
    fn live_session_update_time_follows_transcript_activity_after_status_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = registry::tests::fixture(dir.path());
        let id = value["sessionId"].as_str().unwrap().to_owned();
        transcript(dir.path(), &id, 0);
        let status_changed_at: DateTime<Utc> = "2026-10-06T16:39:12.595Z".parse().unwrap();
        value["updatedAt"] = status_changed_at.timestamp_millis().into();
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let snapshot = ClaudeProvider::new(dir.path().into()).refresh(1);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id.id == id)
            .unwrap();
        assert_eq!(
            session.updated_at,
            "2026-10-06T16:39:29.554Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn live_session_without_transcript_is_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let value = registry::tests::fixture(dir.path());
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(
            ClaudeProvider::new(dir.path().into())
                .refresh(1)
                .sessions
                .is_empty()
        );
    }

    #[test]
    fn live_sessions_join_transcripts_without_consuming_page_slots() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = registry::tests::fixture(dir.path());
        let id = value["sessionId"].as_str().unwrap().to_owned();
        let path = transcript(dir.path(), &id, 0);
        for index in 0..11 {
            transcript(
                dir.path(),
                &format!("00000000-0000-0000-0000-{index:012}"),
                index,
            );
        }
        value["status"] = "unrecognized".into();
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let snapshot = store.refresh(1);
        assert_eq!(snapshot.sessions.len(), 11);
        assert!(snapshot.has_more);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id.id == id)
            .unwrap();
        assert_eq!(session.status, SessionStatus::Ready);
        assert_eq!(session.name.as_deref(), Some("translate-papers"));
        assert_eq!(session.cwd, PathBuf::from(value["cwd"].as_str().unwrap()));
        assert_eq!(
            session.updated_at.timestamp_millis(),
            value["updatedAt"].as_i64().unwrap()
        );
        assert_eq!(session.transcript.as_ref(), Some(&path));
        assert_eq!(session.title, "Codex manager 停止时线程处理");
        assert!(session.process.is_some());
        assert_eq!(
            session.prompt_at,
            Some("2026-10-06T16:39:12.595Z".parse().unwrap())
        );
        assert_eq!(session.turn_messages, 1);
        assert_eq!(
            session.last_record_at,
            Some("2026-10-06T16:39:29.554Z".parse().unwrap())
        );
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .filter(|session| session.id.id == id)
                .count(),
            1
        );
        value["nameSource"] = "derived".into();
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let snapshot = store.refresh(1);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id.id == id)
            .unwrap();
        assert!(session.name.is_none());
        assert_eq!(session.title, "Codex manager 停止时线程处理");
    }

    #[test]
    fn cache_refreshes_on_mtime_or_length_and_prunes_removed_files() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-0000-0000-000000000001";
        let path = transcript(dir.path(), id, 0);
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        let first = store.refresh(1).sessions.remove(0);
        let changed = FIXTURE.replace("Codex manager", "CODEX manager");
        assert_eq!(changed.len(), FIXTURE.len());
        fs::write(&path, changed).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(modified).unwrap();
        assert_eq!(store.refresh(1).sessions[0].title, first.title);
        file.set_modified(modified + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            store.refresh(1).sessions[0].title,
            "CODEX manager 停止时线程处理"
        );
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let title = FIXTURE.lines().nth(2).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{title}").unwrap();
        file.set_modified(modified).unwrap();
        assert_eq!(store.refresh(1).sessions[0].title, first.title);
        fs::remove_file(path).unwrap();
        assert!(store.refresh(1).sessions.is_empty());
        assert!(store.summaries.is_empty());
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rename_overrides_live_registry_name() {
        let dir = tempfile::tempdir().unwrap();
        let value = registry::tests::fixture(dir.path());
        let id = value["sessionId"].as_str().unwrap();
        let path = tests::transcript(dir.path(), id, 0);
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        assert_eq!(
            store.refresh(1).sessions[0].display_title(),
            "translate-papers"
        );
        // This is the real custom-title record shape appended by saveCustomTitle.
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"custom-title", "customTitle":"Renamed live", "sessionId":id})
        )
        .unwrap();
        assert_eq!(store.refresh(1).sessions[0].display_title(), "Renamed live");
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    #[test]
    fn delete_removes_transcript_children_and_refresh_prunes_cache() {
        for children in [false, true] {
            let config = tempfile::tempdir().unwrap();
            let id = "00000000-0000-0000-0000-000000000001";
            let path = tests::transcript(config.path(), id, 0);
            let directory = path.with_extension("");
            if children {
                fs::create_dir_all(directory.join("subagents/nested")).unwrap();
                fs::write(directory.join("subagents/nested/agent.jsonl"), "child").unwrap();
                fs::write(directory.join("custom-title.json"), "{}").unwrap();
            }
            let registry = config.path().join("sessions/keep.json");
            fs::create_dir_all(registry.parent().unwrap()).unwrap();
            fs::write(&registry, "{}").unwrap();
            let mut store = ClaudeProvider::new(config.path().into());
            assert_eq!(store.refresh(1).sessions.len(), 1);
            assert!(store.summaries.contains_key(&path));
            delete(&path).unwrap();
            assert!(!path.exists());
            assert!(!directory.exists());
            assert!(registry.exists());
            assert!(store.refresh(1).sessions.is_empty());
            assert!(!store.summaries.contains_key(&path));
        }
    }

    #[test]
    fn delete_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;
        let config = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = tests::transcript(config.path(), "00000000-0000-0000-0000-000000000001", 0);
        let keep = outside.path().join("keep.jsonl");
        fs::write(&keep, "keep").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&keep, &path).unwrap();
        symlink(outside.path(), path.with_extension("")).unwrap();
        delete(&path).unwrap();
        assert!(keep.exists());
        assert!(fs::symlink_metadata(path.with_extension("")).is_err());
    }
}

#[cfg(test)]
mod delete_live_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn stop_session_rejects_changed_background_identity_without_signalling() {
        for (kind, job_id) in [("interactive", "expected-job"), ("bg", "another-job")] {
            let mut child = Command::new("sleep").arg("60").spawn().unwrap();
            let mut entry: registry::Entry =
                serde_json::from_str(include_str!("../../../tests/fixtures/registry.json"))
                    .unwrap();
            entry.pid = Some(child.id());
            entry.kind = Some(kind.into());
            entry.job_id = Some(job_id.into());
            let mut session = session(
                entry.session_id.clone().unwrap(),
                Summary::default(),
                None,
                Utc::now(),
            );
            session.background_job = Some(crate::model::BackgroundJob {
                id: "expected-job".into(),
                needs: None,
                awaits_prompt: false,
            });

            let result = stop_session(&session, &entry);
            let running = child.try_wait().unwrap().is_none();
            if running {
                child.kill().unwrap();
            }
            child.wait().unwrap();
            assert_eq!(
                result.unwrap_err().to_string(),
                "Background job identity changed"
            );
            assert!(running, "a mismatched background job must not be signalled");
        }
    }

    #[test]
    fn delete_stops_matching_process_and_removes_registry_and_keys() {
        for (mismatch, ignore_term) in [(false, false), (true, false), (false, true)] {
            let config = tempfile::tempdir().unwrap();
            let child = if ignore_term {
                Command::new("sh")
                    .args(["-c", "trap '' TERM; exec sleep 60"])
                    .spawn()
                    .unwrap()
            } else {
                Command::new("sleep").arg("60").spawn().unwrap()
            };
            let pid = child.id();
            let mut child = child;
            let reaper = std::thread::spawn(move || child.wait().unwrap());
            let start = registry::process_starts(&[pid])[&pid][0];
            let id = "00000000-0000-0000-0000-000000000001";
            let path = tests::transcript(config.path(), id, 0);
            let registry_dir = config.path().join("sessions");
            fs::create_dir_all(&registry_dir).unwrap();
            let entry = registry_dir.join(format!("{pid}.json"));
            fs::write(&entry, serde_json::to_vec(&serde_json::json!({
                "pid": pid, "sessionId": id, "startedAt": start.timestamp_millis(),
                "procStart": if mismatch { "Thu Jan  1 00:00:00 1970".into() } else { start.format("%a %b %e %H:%M:%S %Y").to_string() },
                "kind": "interactive", "status": "idle"
            })).unwrap()).unwrap();
            let key = registry_dir.join(format!("{pid}.token.key"));
            let other_key = registry_dir.join(format!("{pid}0.token.key"));
            fs::write(&key, "private").unwrap();
            fs::write(&other_key, "private").unwrap();
            let mut store = ClaudeProvider::new(config.path().into());
            let mut session = store.refresh(1).sessions.remove(0);
            session.process = Some(Process {
                pid,
                label: None,
                started_at: Some(start),
            });
            let result = delete_session(config.path(), &session);
            if mismatch {
                assert!(result.is_err());
                assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
                assert!(entry.exists());
                assert!(path.exists());
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            } else {
                result.unwrap();
                assert!(!entry.exists());
                assert!(!key.exists());
                assert!(other_key.exists());
                assert!(!path.exists());
                assert!(store.refresh(1).sessions.is_empty());
            }
            reaper.join().unwrap();
        }
    }
}
