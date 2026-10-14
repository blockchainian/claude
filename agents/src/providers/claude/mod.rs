//! Session sources: the live registry and transcripts.

mod background;
pub(crate) mod log;
mod peer;
mod registry;
mod subagents;
mod transcript;

use crate::model::{Action, Process, Session, SessionStatus};
use crate::providers::{Capabilities, Provider, ProviderId, SessionKey, Snapshot};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use transcript::Summary;

const PAGE_SIZE: usize = 10;

/// Persists the custom title in Claude Code's transcript and sidecar formats.
pub fn rename(transcript: &Path, id: &str, name: &str) -> io::Result<()> {
    let mut record = serde_json::to_vec(&serde_json::json!({
        "type": "custom-title", "customTitle": name, "sessionId": id
    }))?;
    record.push(b'\n');
    fs::OpenOptions::new()
        .append(true)
        .open(transcript)?
        .write_all(&record)?;

    let directory = transcript.with_file_name(id);
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let mut sidecar = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(directory.join("custom-title.json"))?;
    sidecar.set_permissions(fs::Permissions::from_mode(0o600))?;
    sidecar.write_all(&serde_json::to_vec(
        &serde_json::json!({"customTitle": name}),
    )?)
}

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
        stop_process(&entry)?;
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
    delete(transcript)
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

fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(
        std::env::var_os("HOME").expect("HOME is required when CLAUDE_CONFIG_DIR is unset"),
    )
    .join(".claude")
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
}

impl ClaudeProvider {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            index: HashMap::new(),
            summaries: HashMap::new(),
            starts: HashMap::new(),
            background: background::Tasks::default(),
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
        self.list_transcripts();
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
                    .map(transcript::printable)
            });
            if !has_title
                && entry.name_source.as_deref() == Some("derived")
                && let Some(name) = entry.name.as_deref().filter(|name| !name.trim().is_empty())
            {
                session.title = transcript::printable(name);
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
        let mut inactive: Vec<_> = self
            .index
            .values()
            .filter(|file| !live_ids.contains(&file.id))
            .map(|file| (file.id.clone(), file.modified))
            .collect();
        inactive.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let shown = pages.saturating_mul(PAGE_SIZE);
        let has_more = inactive.len() > shown;
        for (id, _) in inactive.into_iter().take(shown) {
            let (summary, path, modified) = self.summary(&id);
            self.background.observe(&id, &summary);
            sessions.push(session(id, summary, path, modified));
        }
        if let Err(error) =
            self.background
                .refresh(&self.config_dir, &mut sessions, &background::Processes)
        {
            eprintln!("agents: Claude background tasks: {error}");
        }
        let mut paths: HashSet<PathBuf> =
            self.index.values().map(|file| file.path.clone()).collect();
        let mut children = Vec::new();
        for parent in sessions.iter().filter(|session| session.process.is_some()) {
            children.extend(self.subagents(parent, &mut paths));
        }
        self.summaries.retain(|path, _| paths.contains(path));
        sessions.extend(children);
        Snapshot { sessions, has_more }
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
                transcript::read_subagent
            } else {
                transcript::read
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
            match read(&file.path, file.len) {
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

    fn subagents(&mut self, parent: &Session, paths: &mut HashSet<PathBuf>) -> Vec<Session> {
        let Some(transcript) = &parent.transcript else {
            return Vec::new();
        };
        let mut children = Vec::new();
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
            paths.insert(path.clone());
            let file = Transcript {
                id: id.to_owned(),
                path: path.clone(),
                modified,
                len: metadata.len(),
            };
            let Some(summary) = self.summary_file(&file, true) else {
                continue;
            };
            let Some(meta) = subagents::Metadata::read(&path.with_extension("meta.json")) else {
                continue;
            };
            let Some(status) = meta.status(
                &summary,
                &self.config_dir,
                parent.process.is_some() || parent.status != SessionStatus::Inactive,
            ) else {
                continue;
            };
            let mut child = session(file.id, summary, Some(path), modified.into());
            child.parent = Some(parent.id.clone());
            child.title = meta.title();
            child.prompt = child.title.clone();
            child.name = meta.name();
            child.status = status;
            children.push(child);
        }
        children.sort_by(|a, b| a.id.cmp(&b.id));
        children
    }
}

impl Provider for ClaudeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn watch_roots(&self) -> io::Result<Vec<PathBuf>> {
        let sessions = self.config_dir.join("sessions");
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&sessions)?;
        Ok(vec![sessions, self.config_dir.join("projects")])
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume: true,
            fork: true,
            rename: true,
            delete: true,
            ..Capabilities::default()
        }
    }

    fn refresh(&mut self, pages: usize) -> Snapshot {
        self.snapshot(pages)
    }
    fn log(
        &self,
        session: &Session,
        count: usize,
        sidechains: bool,
    ) -> io::Result<Vec<crate::log::Event>> {
        let path = session
            .transcript
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Session has no transcript"))?;
        log::tail_session(path, count, sidechains)
    }

    fn launch(&self, action: &Action, session: Option<&Session>) -> Option<Command> {
        let mut command = Command::new("claude");
        match action {
            Action::New => {
                command.current_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
            }
            Action::Open(_) | Action::Resume(_) | Action::Fork(_) | Action::Back => {
                let session = session?;
                if session.parent.is_some() || !session.cwd.is_dir() {
                    return None;
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

    fn rename(&self, session: &Session, name: &str) -> io::Result<()> {
        let path = session
            .transcript
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session has no transcript"))?;
        rename(path, &session.id.id, name)
    }

    fn delete(&self, session: &Session) -> io::Result<()> {
        delete_session(&self.config_dir, session)
    }
    fn archive(&self, _: &Session) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn stop(&self, _: &Session) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
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
            id,
        },
        parent: None,
        cwd,
        name: None,
        title,
        prompt: summary.prompt,
        prompt_images: Vec::new(),
        queued: summary.queued,
        prompt_at: summary.prompt_at,
        turn_messages: summary.turn_messages,
        last_record_at: summary.timestamp,
        model: summary.model,
        effort: summary.effort,
        git_branch: summary.git_branch,
        pr: summary.pr,
        status: SessionStatus::Inactive,
        process: None,
        updated_at,
        started_at: summary.started_at,
        last_message: summary.last_message,
        last_message_at: summary.last_message_at,
        activity: summary.activity,
        pending_questions: summary.pending_questions,
        background: summary.background,
        last_prompt: summary.last_prompt,
        transcript: path,
    }
}

#[cfg(test)]
mod tests {
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
        }
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
                archive: false,
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
            vec![dir.path().join("sessions"), dir.path().join("projects")]
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
            provider.log(&session, 8, false).unwrap(),
            log::tail(&path, 8).unwrap()
        );
        provider.rename(&session, "Provider title").unwrap();
        assert_eq!(
            provider.refresh(1).sessions[0].display_title(),
            "Provider title"
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
        provider.delete(&session).unwrap();
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
    use std::os::unix::fs::PermissionsExt;

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

    #[test]
    fn rename_appends_and_refreshes_latest_title_with_private_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-0000-0000-000000000001";
        let project = dir.path().join("projects/test");
        fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("{id}.jsonl"));
        let mut original = include_str!("../../../tests/fixtures/transcript.jsonl").to_owned();
        // Keep the original title in the head and append the rename in the tail.
        let title = original.lines().nth(2).unwrap().to_owned();
        for _ in 0..4000 {
            original.push_str(&title);
            original.push('\n');
        }
        fs::write(&path, &original).unwrap();
        let mut store = ClaudeProvider::new(dir.path().into());
        store.refresh(1);
        for name in ["Quoted \"title\" \n 中文", "Latest title"] {
            rename(&path, id, name).unwrap();
            let bytes = fs::read(&path).unwrap();
            assert!(bytes.starts_with(original.as_bytes()));
            assert!(bytes.ends_with(b"\n"));
            let record: serde_json::Value = serde_json::from_slice(
                bytes
                    .strip_suffix(b"\n")
                    .unwrap()
                    .rsplit(|b| *b == b'\n')
                    .next()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                record,
                serde_json::json!({"type": "custom-title", "customTitle": name, "sessionId": id})
            );
            let sidecar = project.join(id).join("custom-title.json");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&fs::read(&sidecar).unwrap()).unwrap(),
                serde_json::json!({"customTitle": name})
            );
            assert_eq!(
                fs::metadata(&sidecar).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(project.join(id)).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                transcript::read(&path, bytes.len() as u64)
                    .unwrap()
                    .custom_title
                    .as_deref(),
                Some(name)
            );
            assert_eq!(
                store.refresh(1).sessions[0].display_title(),
                name.lines().next().unwrap()
            );
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().lines().count(),
            original.lines().count() + 2
        );
        // A missing transcript must not be recreated.
        assert!(rename(&project.join("missing.jsonl"), id, "Missing").is_err());
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
