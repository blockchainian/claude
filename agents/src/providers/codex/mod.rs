mod app_server;
#[cfg(test)]
mod inner_tests;
mod manager;
mod rollout;
#[cfg(test)]
mod tests;

use super::{Capabilities, Provider, ProviderId, SessionKey, Snapshot};
use crate::{
    log,
    model::{Action, Process, Session, SessionStatus},
};
use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{Connection, OpenFlags, Row};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const PAGE_SIZE: usize = 10;

pub struct CodexProvider {
    home: PathBuf,
    manager_home: PathBuf,
}

impl CodexProvider {
    pub fn new(home: PathBuf, manager_home: PathBuf) -> Self {
        Self { manager_home, home }
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
        let manager = manager::links(&self.manager_home)?;
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
        let mut sessions = Vec::new();
        for row in rows {
            if let Some(mut session) = row.map_err(io::Error::other)? {
                if let Some(pid) = live.get(&session.id.id) {
                    session.process = Some(Process {
                        pid: *pid,
                        label: Some("codex".into()),
                        started_at: starts.get(pid).copied(),
                    });
                }
                if let Some(link) = manager.get(&session.id.id) {
                    session.parent = Some(link.parent.clone());
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
        for session in &mut sessions {
            let summary = match session
                .transcript
                .as_deref()
                .map(rollout::summary)
                .transpose()
            {
                Ok(summary) => summary,
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            if let Some(summary) = summary {
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

fn apply_rollout(session: &mut Session, summary: rollout::Summary) {
    session.last_message = summary.last_message;
    session.last_message_at = summary.last_message_at;
    session.last_record_at = summary.updated_at;
    session.activity = summary.activity;
    if let Some(at) = summary.updated_at {
        session.updated_at = session.updated_at.max(at);
    }
    session.status = status(session.process.is_some(), summary.turn_ended);
    session.pending_questions = summary.pending_questions;
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
    // Codex's agents overview lists only unarchived threads.
    if column::<bool>(row, "archived")? == Some(true) {
        return Ok(None);
    }
    let id: String = row.get("id")?;
    let prompt = column::<String>(row, "first_user_message")?.unwrap_or_default();
    let title = column::<String>(row, "title")?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| prompt.clone());
    let updated_at = time(row, "updated_at")?.unwrap_or(DateTime::UNIX_EPOCH);
    Ok(Some(Session {
        parent: parents.get(&id).cloned().map(key),
        id: key(id),
        cwd: PathBuf::from(column::<String>(row, "cwd")?.unwrap_or_default()),
        name: match column::<String>(row, "history_mode")?.as_deref() {
            Some("paginated") => column::<String>(row, "name")?
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
            _ => {
                let title = title.trim();
                (!title.is_empty() && title != prompt.trim()).then(|| title.to_owned())
            }
        },
        title,
        prompt: prompt.chars().take(512).collect(),
        queued: vec![],
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        model: column(row, "model")?,
        effort: column(row, "reasoning_effort")?,
        git_branch: column(row, "git_branch")?,
        pr: None,
        status: SessionStatus::Inactive,
        process: None,
        updated_at,
        started_at: column::<i64>(row, "created_at_ms")?.and_then(DateTime::from_timestamp_millis),
        last_message: None,
        last_message_at: None,
        activity: None,
        pending_questions: vec![],
        background: vec![],
        last_prompt: None,
        transcript: column::<String>(row, "rollout_path")?
            .filter(|p| !p.is_empty())
            .map(PathBuf::from),
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
    // lsof identifies the owning process; a shared, read-only lock probe rejects stale open files.
    let output = Command::new("lsof")
        .args(["-nP", "-Fpcn", "+D"])
        .arg(directory)
        .output()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(io::Error::other(
            "lsof failed to inspect Codex writer locks",
        ));
    }
    let mut pid = None;
    let mut codex = false;
    let mut live = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.parse::<u32>().ok();
            codex = false;
        } else if let Some(value) = line.strip_prefix('c') {
            codex = value == "codex" || value.starts_with("codex-");
        } else if let Some(value) = line.strip_prefix('n') {
            let path = Path::new(value);
            if codex
                && path.parent() == Some(directory)
                && path.extension().is_some_and(|ext| ext == "lock")
                && let (Some(pid), Some(id)) = (pid, path.file_stem().and_then(|s| s.to_str()))
                && lock_held(path)?
            {
                live.insert(id.to_owned(), pid);
            }
        }
    }
    Ok(live)
}

fn process_starts(pids: &[u32]) -> io::Result<HashMap<u32, DateTime<Utc>>> {
    if pids.is_empty() {
        return Ok(HashMap::new());
    }
    let output = Command::new("ps")
        .args(["-o", "pid=,lstart=", "-p"])
        .arg(
            pids.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        )
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let (pid, start) = line
                .trim()
                .split_once(char::is_whitespace)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid ps output"))?;
            let pid = pid.parse::<u32>().map_err(io::Error::other)?;
            let start = NaiveDateTime::parse_from_str(start.trim(), "%a %b %e %H:%M:%S %Y")
                .map_err(io::Error::other)?
                .and_utc();
            Ok((pid, start))
        })
        .collect()
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

fn stop_process(home: &Path, session: &Session, process: &Process) -> io::Result<()> {
    use std::time::{Duration, Instant};
    let pid = process.pid;
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid process id",
        ));
    }
    for (signal, timeout) in [(libc::SIGTERM, 5), (libc::SIGKILL, 2)] {
        if !process_exists(pid)? {
            return Ok(());
        }
        // Recheck both process identity and session ownership before each signal.
        let starts = process_starts(&[pid])?;
        if process.started_at.is_none() || starts.get(&pid).copied() != process.started_at {
            return Err(io::Error::other("Session process start time changed"));
        }
        if live_writers(&home.join("thread-writer-locks"))?.get(&session.id.id) != Some(&pid) {
            return Err(io::Error::other("Session writer lock changed"));
        }
        if unsafe { libc::kill(pid as libc::pid_t, signal) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(timeout);
        while process_exists(pid)? {
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    if process_exists(pid)? {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Session process did not exit",
        ))
    } else {
        Ok(())
    }
}

impl Provider for CodexProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            fork: true,
            new: true,
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
    fn log(
        &self,
        session: &Session,
        count: usize,
        filter: &log::Filter,
        _: bool,
    ) -> io::Result<Vec<log::Event>> {
        session
            .transcript
            .as_deref()
            .map(|path| rollout::tail_filtered(path, count, filter))
            .unwrap_or_else(|| Ok(vec![]))
    }
    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command> {
        if !matches!(action, Action::New(_)) && session.is_some_and(|session| !session.can_resume())
        {
            return None;
        }
        let mut command = Command::new("codex");
        match action {
            Action::New(cwd) => {
                let cwd = match cwd {
                    Some(cwd) if cwd.is_dir() => cwd.clone(),
                    _ => std::env::current_dir().ok()?,
                };
                command.current_dir(cwd);
            }
            Action::Open(_) | Action::Resume(_) | Action::Fork(_) | Action::Back => {
                let session = session?;
                if !session.cwd.is_dir() {
                    return None;
                }
                let subcommand = if matches!(action, Action::Fork(_)) {
                    "fork"
                } else {
                    "resume"
                };
                command
                    .args([subcommand, &session.id.id])
                    .current_dir(&session.cwd);
            }
            _ => return None,
        }
        Some(command)
    }
    fn rename(&self, session: &Session, name: &str) -> io::Result<()> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let mut client = app_server::Client::connect(&self.home)?;
        app_server::thread::set_name(&mut client, &session.id.id, name)
    }
    fn archive(&self, session: &Session) -> io::Result<()> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let mut client = app_server::Client::connect(&self.home)?;
        app_server::thread::archive(&mut client, &session.id.id)
    }
    fn stop(&self, session: &Session) -> io::Result<()> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let mut client = app_server::Client::connect(&self.home)?;
        match app_server::thread::running_turn(&mut client, &session.id.id)? {
            Some(turn) => app_server::thread::interrupt(&mut client, &session.id.id, &turn),
            None => Ok(()),
        }
    }
    fn delete(&self, session: &Session) -> io::Result<()> {
        if !session.can_resume() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        if let Some(process) = &session.process {
            stop_process(&self.home, session, process)?;
        }
        if live_writers(&self.home.join("thread-writer-locks"))?.contains_key(&session.id.id) {
            return Err(io::Error::other("Session writer lock changed"));
        }
        let output = Command::new("codex")
            .args(["delete", "--force", &session.id.id])
            .env("CODEX_HOME", &self.home)
            .stdin(Stdio::null())
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ))
        }
    }
}
