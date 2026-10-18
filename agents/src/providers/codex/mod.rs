mod app_server;
#[cfg(test)]
mod inner_tests;
mod manager;
#[cfg(test)]
mod memory_tests;
pub(super) mod rollout;
#[cfg(test)]
mod send_tests;
#[cfg(test)]
mod subagents_tests;
#[cfg(test)]
mod tests;

use super::{
    Capabilities, Job, Provider, ProviderId, SessionInput, SessionKey, Snapshot, TerminalCommand,
};
#[cfg(test)]
use crate::log;
use crate::model::{Action, Process, Session, SessionStatus};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, Row};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::SystemTime,
};

const PAGE_SIZE: usize = 10;

pub struct CodexProvider {
    home: PathBuf,
    manager_home: PathBuf,
    tasks: Mutex<HashMap<PathBuf, rollout::Tasks>>,
    models: Mutex<ModelCatalog>,
}

impl CodexProvider {
    pub fn new(home: PathBuf, manager_home: PathBuf) -> Self {
        Self {
            manager_home,
            home,
            tasks: Mutex::new(HashMap::new()),
            models: Mutex::new(ModelCatalog::default()),
        }
    }

    pub fn from_config() -> io::Result<Self> {
        let home = match std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
            Some(path) => {
                let path = PathBuf::from(path);
                if !path.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "CODEX_HOME must be an existing directory",
                    ));
                }
                path.canonicalize()?
            }
            None => PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?,
            )
            .join(".codex"),
        };
        let user_home = std::env::var_os("HOME")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?;
        let manager_home = std::env::var_os("CODEX_MANAGER_STATE_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(user_home).join(".local/state/codex-manager"));
        Ok(Self::new(home, manager_home))
    }

    fn snapshot(&self, pages: usize) -> io::Result<Snapshot> {
        let path = self.home.join("state_5.sqlite");
        if !path.exists() {
            return Ok(Snapshot {
                sessions: vec![],
                has_more: false,
            });
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(io::Error::other)?;
        let manager = manager::links(&self.manager_home, &super::claude::config_dir())?;
        let names = indexed_names(&self.home)?;
        let parents = parents(&db).map_err(io::Error::other)?;
        let live = live_writers(&self.home.join("thread-writer-locks"))?;
        let starts = process_starts(&live.values().copied().collect::<Vec<_>>())?;
        let mut statement = db
            .prepare("SELECT * FROM threads")
            .map_err(io::Error::other)?;
        let rows = statement
            .query_map([], |row| {
                let mut session = session(row, &parents)?;
                if column::<String>(row, "history_mode")?.as_deref() != Some("paginated")
                    && let Some(session) = session.as_mut()
                    && session.name.is_none()
                {
                    session.name = names.get(&session.id.id).cloned();
                }
                Ok(session)
            })
            .map_err(io::Error::other)?;
        let mut models = self
            .models
            .lock()
            .map_err(|_| io::Error::other("Codex model cache lock poisoned"))?;
        models.refresh(&self.home.join("models_cache.json"))?;
        let mut sessions = Vec::new();
        for row in rows {
            if let Some(mut session) = row.map_err(io::Error::other)? {
                session.model_label = session
                    .model
                    .as_ref()
                    .and_then(|slug| models.labels.get(slug))
                    .cloned();
                if let Some(pid) = live.get(&session.id.id) {
                    session.process = Some(Process {
                        pid: *pid,
                        label: Some("codex".into()),
                        started_at: starts.get(pid).copied(),
                    });
                }
                if let Some(link) = manager.get(&session.id.id) {
                    if !session.archived {
                        session.parent = Some(link.parent.clone());
                    }
                    if session.name.is_none() {
                        session.name = link.name.clone();
                    }
                    if let Some(name) = &session.name {
                        session.title = name.clone();
                    }
                }
                sessions.push(session);
            }
        }
        drop(models);
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let inactive: Vec<_> = sessions
            .iter()
            .filter(|s| s.parent.is_none() && s.process.is_none())
            .map(|s| s.id.clone())
            .collect();
        let shown = pages.saturating_mul(PAGE_SIZE);
        let has_more = inactive.len() > shown;
        let mut selected: HashSet<_> = sessions
            .iter()
            .filter(|s| {
                (s.parent.is_none() && s.process.is_some())
                    || s.parent
                        .as_ref()
                        .is_some_and(|parent| parent.provider == ProviderId::Claude)
            })
            .map(|s| s.id.clone())
            .chain(inactive.into_iter().take(shown))
            .collect();
        // Descendants follow their visible parent without consuming another page slot.
        loop {
            let before = selected.len();
            for session in &sessions {
                if session
                    .parent
                    .as_ref()
                    .is_some_and(|p| selected.contains(p))
                {
                    selected.insert(session.id.clone());
                }
            }
            if selected.len() == before {
                break;
            }
        }
        sessions.retain(|s| selected.contains(&s.id));
        let mut tasks = self
            .tasks
            .lock()
            .map_err(|_| io::Error::other("Codex task cache lock poisoned"))?;
        for session in &mut sessions {
            let summary = match session
                .transcript
                .as_deref()
                .map(|path| tasks.entry(path.to_owned()).or_default().summary(path))
                .transpose()
            {
                Ok(summary) => summary,
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            if let Some(summary) = summary {
                if let Some(path) = session.transcript.as_deref() {
                    (session.prompt_images, session.detail_sources[2]) =
                        rollout::first_prompt(path, &session.prompt)?;
                }
                apply_rollout(session, summary);
            } else {
                session.status = status(session.process.is_some(), false);
            }
            if manager
                .get(&session.id.id)
                .is_some_and(|link| link.needs_input)
            {
                session.status = SessionStatus::NeedsInput;
            }
        }
        Ok(Snapshot { sessions, has_more })
    }
}

#[derive(Default)]
struct ModelCatalog {
    modified: Option<SystemTime>,
    labels: HashMap<String, String>,
    /// The models Codex lists in its picker, by priority.
    choices: crate::providers::InputChoices,
}

impl ModelCatalog {
    fn display_name(&self, slug: &str) -> String {
        self.labels
            .get(slug)
            .cloned()
            .unwrap_or_else(|| slug.to_owned())
    }

    fn refresh(&mut self, path: &Path) -> io::Result<()> {
        let modified = match std::fs::metadata(path) {
            Ok(metadata) => metadata.modified()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.modified = None;
                self.labels.clear();
                self.choices = crate::providers::InputChoices::default();
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if self.modified == Some(modified) {
            return Ok(());
        }
        #[derive(serde::Deserialize)]
        struct Catalog {
            models: Vec<Model>,
        }
        #[derive(serde::Deserialize)]
        struct Model {
            slug: String,
            display_name: String,
            visibility: Option<String>,
            #[serde(default)]
            priority: i64,
            #[serde(default)]
            supported_reasoning_levels: Vec<Level>,
            default_reasoning_level: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Level {
            effort: String,
        }
        let catalog: Catalog = serde_json::from_reader(io::BufReader::new(File::open(path)?))?;
        let mut listed: Vec<_> = catalog
            .models
            .iter()
            .filter(|model| model.visibility.as_deref() == Some("list"))
            .collect();
        listed.sort_by_key(|model| model.priority);
        self.choices = crate::providers::InputChoices {
            models: listed
                .into_iter()
                .map(|model| crate::providers::ModelChoice {
                    id: model.slug.clone(),
                    label: model.display_name.clone(),
                    efforts: model
                        .supported_reasoning_levels
                        .iter()
                        .map(|level| level.effort.clone())
                        .collect(),
                    default_effort: model.default_reasoning_level.clone(),
                })
                .collect(),
            aliases: HashMap::new(),
        };
        self.labels = catalog
            .models
            .into_iter()
            .map(|model| (model.slug, model.display_name))
            .collect();
        self.modified = Some(modified);
        Ok(())
    }
}

/// Uses the same cached catalog names as the session model label.
pub(super) fn model_display_name(slug: &str) -> io::Result<String> {
    static CATALOG: std::sync::OnceLock<Mutex<(PathBuf, ModelCatalog)>> =
        std::sync::OnceLock::new();
    let home = match std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?,
        )
        .join(".codex"),
    };
    let path = home.join("models_cache.json");
    let mut cached = CATALOG
        .get_or_init(|| Mutex::new((PathBuf::new(), ModelCatalog::default())))
        .lock()
        .map_err(|_| io::Error::other("Codex model cache lock poisoned"))?;
    if cached.0 != path {
        *cached = (path.clone(), ModelCatalog::default());
    }
    cached.1.refresh(&path)?;
    Ok(cached.1.display_name(slug))
}

fn apply_rollout(session: &mut Session, summary: rollout::Summary) {
    session.prompt_at = summary.prompt_at;
    session.detail_sources[..2].clone_from_slice(&summary.detail_sources[..2]);
    session.last_message = summary.last_message;
    session.last_message_at = summary.last_message_at;
    session.last_record_at = summary.updated_at;
    session.activity = summary.activity;
    if let Some(at) = summary.updated_at {
        session.updated_at = session.updated_at.max(at);
    }
    session.status = status(session.process.is_some(), summary.turn_ended);
    session.tasks = summary.tasks;
    session.pending_questions = summary
        .pending_questions
        .into_iter()
        .map(|question| question.text)
        .collect();
    if session.process.is_some() && !session.pending_questions.is_empty() {
        session.status = SessionStatus::NeedsInput;
    }
}

fn key(id: String) -> SessionKey {
    SessionKey {
        provider: ProviderId::Codex,
        id,
    }
}

fn column<T: rusqlite::types::FromSql>(row: &Row<'_>, name: &str) -> rusqlite::Result<Option<T>> {
    match row.get(name) {
        Err(rusqlite::Error::InvalidColumnName(_)) => Ok(None),
        result => result,
    }
}

fn time(row: &Row<'_>, name: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
    if let Some(value) = column::<i64>(row, &format!("{name}_ms"))? {
        return Ok(DateTime::from_timestamp_millis(value));
    }
    Ok(column::<i64>(row, name)?.and_then(|value| DateTime::from_timestamp(value, 0)))
}

fn session(row: &Row<'_>, parents: &HashMap<String, String>) -> rusqlite::Result<Option<Session>> {
    let source = column::<String>(row, "source")?.unwrap_or_default();
    let thread_source = column::<String>(row, "thread_source")?;
    let source_json: Option<serde_json::Value> = serde_json::from_str(&source).ok();
    let subagent = source_json.as_ref().and_then(|v| v.get("subagent"));
    if thread_source.as_deref() == Some("guardian_review")
        || subagent.is_some_and(|v| {
            v.as_str() == Some("review")
                || v.get("other").and_then(|v| v.as_str()) == Some("guardian")
        })
    {
        return Ok(None);
    }
    let id: String = row.get("id")?;
    let prompt = column::<String>(row, "first_user_message")?.unwrap_or_default();
    let agent_path = subagent
        .and_then(|value| value.get("thread_spawn"))
        .and_then(|spawn| spawn.get("agent_path"))
        .and_then(|path| path.as_str());
    let title = if let Some(path) = agent_path {
        path.rsplit('/').next().unwrap_or_default().to_owned()
    } else {
        column::<String>(row, "title")?
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| prompt.clone())
    };
    let updated_at = time(row, "updated_at")?.unwrap_or(DateTime::UNIX_EPOCH);
    Ok(Some(Session {
        parent: parents.get(&id).cloned().map(key),
        id: key(id),
        cwd: PathBuf::from(column::<String>(row, "cwd")?.unwrap_or_default()),
        name: match column::<String>(row, "history_mode")?.as_deref() {
            _ if agent_path.is_some() => None,
            Some("paginated") => column::<String>(row, "name")?
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
            _ => {
                let title = title.trim();
                (!title.is_empty() && title != prompt.trim()).then(|| title.to_owned())
            }
        },
        title,
        prompt: if agent_path.is_some() {
            String::new()
        } else {
            prompt.chars().take(512).collect()
        },
        detail_sources: Default::default(),
        prompt_images: Vec::new(),
        queued: vec![],
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        model_label: None,
        model: column(row, "model")?,
        effort: column(row, "reasoning_effort")?,
        git_branch: column(row, "git_branch")?,
        pr: None,
        status: SessionStatus::Inactive,
        process: None,
        background_job: None,
        updated_at,
        started_at: column::<i64>(row, "created_at_ms")?.and_then(DateTime::from_timestamp_millis),
        last_message: None,
        last_message_at: None,
        activity: None,
        ask: None,
        pending_questions: vec![],
        tasks: vec![],
        background: vec![],
        last_prompt: None,
        transcript: column::<String>(row, "rollout_path")?
            .filter(|p| !p.is_empty())
            .map(PathBuf::from),
        archived: column::<bool>(row, "archived")?.unwrap_or(false),
    }))
}

fn indexed_names(home: &Path) -> io::Result<HashMap<String, String>> {
    #[derive(serde::Deserialize)]
    struct Entry {
        id: String,
        thread_name: String,
        #[serde(rename = "updated_at")]
        _updated_at: String,
    }
    let file = match File::open(home.join("session_index.jsonl")) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let mut names = HashMap::new();
    for line in io::BufReader::new(file).lines() {
        // Codex skips malformed index records and keeps the latest nonempty name.
        if let Ok(entry) = serde_json::from_str::<Entry>(&line?) {
            let name = entry.thread_name.trim();
            if !name.is_empty() {
                names.insert(entry.id, name.to_owned());
            }
        }
    }
    Ok(names)
}

fn parents(db: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let present: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges')", [], |row| row.get(0))?;
    if !present {
        return Ok(HashMap::new());
    }
    let mut statement = db.prepare("SELECT * FROM thread_spawn_edges")?;
    let mut parents = HashMap::new();
    let rows = statement.query_map([], |row| {
        Ok((
            column::<String>(row, "child_thread_id")?,
            column::<String>(row, "parent_thread_id")?,
        ))
    })?;
    for row in rows {
        if let (Some(child), Some(parent)) = row? {
            parents.insert(child, parent);
        }
    }
    Ok(parents)
}

fn status(live: bool, turn_ended: bool) -> SessionStatus {
    if !live {
        SessionStatus::Inactive
    } else if turn_ended {
        SessionStatus::Ready
    } else {
        SessionStatus::Working
    }
}

fn lock_held(path: &Path) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

fn live_writers(directory: &Path) -> io::Result<HashMap<String, u32>> {
    if !directory.is_dir() {
        return Ok(HashMap::new());
    }
    // A shared, read-only lock probe rejects stale open files.
    let mut live = HashMap::new();
    for pid in crate::processes::all_pids() {
        if !crate::processes::name(pid)
            .is_some_and(|name| name == "codex" || name.starts_with("codex-"))
        {
            continue;
        }
        for path in crate::processes::open_files(pid) {
            if path.parent() == Some(directory)
                && path.extension().is_some_and(|ext| ext == "lock")
                && let Some(id) = path.file_stem().and_then(|s| s.to_str())
                && lock_held(&path)?
            {
                live.insert(id.to_owned(), pid);
            }
        }
    }
    Ok(live)
}

fn process_starts(pids: &[u32]) -> io::Result<HashMap<u32, DateTime<Utc>>> {
    Ok(pids
        .iter()
        .filter_map(|&pid| crate::processes::start_time(pid).map(|start| (pid, start)))
        .collect())
}

impl Provider for CodexProvider {
    fn terminal_command(&self, session: &Session) -> Result<TerminalCommand, &'static str> {
        let command = self
            .launch(&Action::Resume(session.id.clone()), Some(session))
            .ok_or("Terminal is not supported")?;
        // The app-server owns turns, so closing the client leaves them running.
        Ok(TerminalCommand {
            command,
            detach: None,
        })
    }

    fn terminal_screen_is_loading(&self, screen: &str) -> bool {
        screen.contains("Resuming session")
    }

    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }
    fn input_choices(&self) -> crate::providers::InputChoices {
        let Ok(mut models) = self.models.lock() else {
            return crate::providers::InputChoices::default();
        };
        if models
            .refresh(&self.home.join("models_cache.json"))
            .is_err()
        {
            return crate::providers::InputChoices::default();
        }
        models.choices.clone()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            fork: true,
            delete: true,
            rename: true,
            stop: true,
            archive: true,
        }
    }
    fn watch_roots(&self) -> io::Result<Vec<PathBuf>> {
        Ok(["sessions", "archived_sessions", "thread-writer-locks"]
            .into_iter()
            .map(|name| self.home.join(name))
            .chain(std::iter::once(self.manager_home.clone()))
            .filter(|path| path.is_dir())
            .collect())
    }
    fn refresh(&mut self, pages: usize) -> Snapshot {
        self.snapshot(pages).unwrap_or_else(|error| {
            eprintln!("agents: Codex: {error}");
            Snapshot {
                sessions: vec![],
                has_more: false,
            }
        })
    }
    #[cfg(test)]
    fn log(&self, session: &Session, count: usize, _: bool) -> io::Result<Vec<log::Event>> {
        session
            .transcript
            .as_deref()
            .map(|path| rollout::tail(path, count))
            .unwrap_or_else(|| Ok(vec![]))
    }
    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command> {
        if session.is_some_and(|session| !session.can_resume()) {
            return None;
        }
        let mut command = Command::new("codex");
        match action {
            Action::Resume(_) | Action::Fork(_) => {
                let session = session?;
                if !session.cwd.is_dir() {
                    return None;
                }
                let subcommand = if matches!(action, Action::Fork(_)) {
                    "fork"
                } else {
                    "resume"
                };
                // `codex resume` asks before unarchiving; unarchive here so it opens directly.
                // On failure it still asks, so the error needs no handling.
                if session.archived && subcommand == "resume" {
                    let _ = app_server::Client::connect(&self.home).and_then(|mut client| {
                        app_server::thread::unarchive(&mut client, &session.id.id)
                    });
                }
                command
                    .args([subcommand, &session.id.id])
                    .current_dir(&session.cwd);
            }
            _ => return None,
        }
        Some(command)
    }
    fn send(&self, session: &Session, input: SessionInput) -> Option<Job> {
        if !session.can_resume()
            || matches!(
                input,
                SessionInput::SetModel(_)
                    | SessionInput::SetEffort(_)
                    | SessionInput::Answer { .. }
            )
        {
            return None;
        }
        let home = self.home.clone();
        let id = session.id.id.clone();
        let archived = session.archived;
        Some(Box::new(move || {
            let mut client = app_server::Client::connect(&home)?;
            // Codex refuses to resume an archived thread, so a prompt brings it back first.
            if archived
                && matches!(
                    input,
                    SessionInput::Prompt(_) | SessionInput::PromptWithImages { .. }
                )
            {
                app_server::thread::unarchive(&mut client, &id)?;
            }
            match input {
                SessionInput::Prompt(text) => {
                    app_server::thread::prompt(&mut client, &id, &text, &[])
                }
                SessionInput::PromptWithImages { text, images } => {
                    app_server::thread::prompt(&mut client, &id, &text, &images)
                }
                SessionInput::Interrupt => {
                    match app_server::thread::running_turn(&mut client, &id)? {
                        Some(turn) => app_server::thread::interrupt(&mut client, &id, &turn),
                        None => Ok(()),
                    }
                }
                SessionInput::SetModel(_)
                | SessionInput::SetEffort(_)
                | SessionInput::Answer { .. } => Err(io::ErrorKind::Unsupported.into()),
            }
        }))
    }
    fn rename(&self, session: &Session, name: &str) -> io::Result<()> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let mut client = app_server::Client::connect(&self.home)?;
        app_server::thread::set_name(&mut client, &session.id.id, name)
    }
    fn archive(&self, session: &Session) -> io::Result<Job> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let home = self.home.clone();
        let id = session.id.id.clone();
        let archived = session.archived;
        Ok(Box::new(move || {
            if archived {
                return Ok(());
            }
            let mut client = app_server::Client::connect(&home)?;
            app_server::thread::archive(&mut client, &id)
        }))
    }
    fn delete(&self, session: &Session) -> io::Result<Job> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let home = self.home.clone();
        let id = session.id.id.clone();
        Ok(Box::new(move || {
            let mut client = app_server::Client::connect(&home)?;
            app_server::thread::delete(&mut client, &id)
        }))
    }
}
