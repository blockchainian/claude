//! Session sources: the live registry and transcripts.

mod registry;
mod transcript;

use crate::model::{Process, Session, SessionStatus};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
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

pub fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(
        std::env::var_os("HOME").expect("HOME is required when CLAUDE_CONFIG_DIR is unset"),
    )
    .join(".claude")
}

pub struct Snapshot {
    pub sessions: Vec<Session>,
    pub has_more: bool,
}

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

pub struct Store {
    config_dir: PathBuf,
    pages: usize,
    index: HashMap<String, Transcript>,
    summaries: HashMap<PathBuf, CachedSummary>,
}

impl Store {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            pages: 1,
            index: HashMap::new(),
            summaries: HashMap::new(),
        }
    }

    pub fn show_more(&mut self) {
        self.pages = self.pages.saturating_add(1);
    }

    pub fn refresh(&mut self) -> Snapshot {
        self.list_transcripts();
        let live = registry::live(&self.config_dir);
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
                    .map(transcript::preview)
            });
            if !has_title
                && entry.name_source.as_deref() == Some("derived")
                && let Some(name) = entry.name.as_deref().filter(|name| !name.trim().is_empty())
            {
                session.title = transcript::preview(name);
            }
            session.status = entry
                .status
                .as_deref()
                .and_then(SessionStatus::from_registry)
                .unwrap_or(SessionStatus::Ready);
            if let Some(updated) = entry.updated_at.and_then(DateTime::from_timestamp_millis) {
                session.updated_at = updated;
            }
            session.process = entry.pid.map(|pid| Process {
                pid,
                status: entry.status.unwrap_or_default(),
                kind: entry.kind,
                version: entry.version,
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
        let shown = self.pages.saturating_mul(PAGE_SIZE);
        let has_more = inactive.len() > shown;
        for (id, _) in inactive.into_iter().take(shown) {
            let (summary, path, modified) = self.summary(&id);
            sessions.push(session(id, summary, path, modified));
        }
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
        let paths: HashSet<_> = self.index.values().map(|file| &file.path).collect();
        self.summaries.retain(|path, _| paths.contains(path));
    }

    fn summary(&mut self, id: &str) -> (Summary, Option<PathBuf>, DateTime<Utc>) {
        let Some(file) = self.index.get(id) else {
            return (Summary::default(), None, Utc::now());
        };
        let changed = self
            .summaries
            .get(&file.path)
            .is_none_or(|cached| cached.modified != file.modified || cached.len != file.len);
        if changed {
            // Files may disappear while Claude Code moves or removes a project.
            match transcript::read(&file.path, file.len) {
                Ok(summary) => {
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
        let summary = self
            .summaries
            .get(&file.path)
            .map(|cached| cached.summary.clone())
            .unwrap_or_default();
        (summary, Some(file.path.clone()), file.modified.into())
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
        id,
        cwd,
        name: None,
        title,
        prompt: summary.prompt,
        model: summary.model,
        effort: summary.effort,
        git_branch: summary.git_branch,
        status: SessionStatus::Inactive,
        process: None,
        updated_at,
        last_message: summary.last_message,
        last_prompt: summary.last_prompt,
        transcript: path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    const FIXTURE: &str = include_str!("../../tests/fixtures/transcript.jsonl");

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
        let mut store = Store::new(dir.path().into());
        let first = store.refresh();
        assert_eq!(first.sessions.len(), 10);
        assert!(first.has_more);
        assert!(
            first
                .sessions
                .iter()
                .all(|session| session.status == SessionStatus::Inactive)
        );
        assert!(first.sessions[0].id.ends_with("000000000024"));
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
        assert_eq!(store.refresh().sessions, first.sessions);
        store.show_more();
        store.show_more();
        let all = store.refresh();
        assert_eq!(all.sessions.len(), 25);
        assert!(!all.has_more);
        assert!(
            all.sessions
                .windows(2)
                .all(|pair| pair[0].updated_at > pair[1].updated_at)
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
        let mut store = Store::new(dir.path().into());
        let snapshot = store.refresh();
        assert_eq!(snapshot.sessions.len(), 11);
        assert!(snapshot.has_more);
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == id)
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
            snapshot
                .sessions
                .iter()
                .filter(|session| session.id == id)
                .count(),
            1
        );
        value["nameSource"] = "derived".into();
        fs::write(
            dir.path().join("sessions/live.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let snapshot = store.refresh();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == id)
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
        let mut store = Store::new(dir.path().into());
        let first = store.refresh().sessions.remove(0);
        let changed = FIXTURE.replace("Codex manager", "CODEX manager");
        assert_eq!(changed.len(), FIXTURE.len());
        fs::write(&path, changed).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(modified).unwrap();
        assert_eq!(store.refresh().sessions[0].title, first.title);
        file.set_modified(modified + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            store.refresh().sessions[0].title,
            "CODEX manager 停止时线程处理"
        );
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let title = FIXTURE.lines().nth(2).unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{title}").unwrap();
        file.set_modified(modified).unwrap();
        assert_eq!(store.refresh().sessions[0].title, first.title);
        fs::remove_file(path).unwrap();
        assert!(store.refresh().sessions.is_empty());
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
        let mut store = Store::new(dir.path().into());
        assert_eq!(
            store.refresh().sessions[0].display_title(),
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
        assert_eq!(store.refresh().sessions[0].display_title(), "Renamed live");
    }

    #[test]
    fn rename_appends_and_refreshes_latest_title_with_private_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-0000-0000-000000000001";
        let project = dir.path().join("projects/test");
        fs::create_dir_all(&project).unwrap();
        let path = project.join(format!("{id}.jsonl"));
        let mut original = include_str!("../../tests/fixtures/transcript.jsonl").to_owned();
        // Keep the original title in the head and append the rename in the tail.
        let title = original.lines().nth(2).unwrap().to_owned();
        for _ in 0..4000 {
            original.push_str(&title);
            original.push('\n');
        }
        fs::write(&path, &original).unwrap();
        let mut store = Store::new(dir.path().into());
        store.refresh();
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
                store.refresh().sessions[0].display_title(),
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
