//! Output files establish liveness; transcripts only supply Bash descriptions.
use super::transcript;
use crate::model::{BackgroundTask, Session};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub(super) trait Liveness {
    fn held_outputs(&self) -> io::Result<HashSet<PathBuf>>;
    fn alive(&self, pid: u32) -> io::Result<bool>;
}

pub(super) struct Processes;
impl Liveness for Processes {
    fn held_outputs(&self) -> io::Result<HashSet<PathBuf>> {
        // Any process may hold an output: adopted shells outlive the session that started them.
        Ok(crate::processes::all_pids()
            .into_iter()
            .flat_map(|pid| [1, 2].map(|fd| crate::processes::fd_path(pid, fd)))
            .flatten()
            .collect())
    }
    fn alive(&self, pid: u32) -> io::Result<bool> {
        if pid == 0 || pid > i32::MAX as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid adopted shell pid",
            ));
        }
        match super::process_exists(pid) {
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => Ok(true),
            result => result,
        }
    }
}

struct Output {
    size: u64,
    finished: bool,
}

pub(super) struct Tasks {
    root: PathBuf,
    outputs: HashMap<PathBuf, Output>,
    descriptions: HashMap<(String, String), Option<BackgroundTask>>,
    announced: HashSet<(String, String)>,
    completed: HashSet<(String, String)>,
    #[cfg(test)]
    transcript_bytes: u64,
    #[cfg(test)]
    output_bytes: u64,
}
impl Default for Tasks {
    fn default() -> Self {
        let base = std::env::var_os("CLAUDE_CODE_TMPDIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let base = fs::canonicalize(&base).unwrap_or(base);
        let root = base.join(format!("claude-{}", unsafe { libc::getuid() }));
        Self::new(fs::canonicalize(&root).unwrap_or(root))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Adoption {
    shells: Vec<Shell>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shell {
    task_id: String,
    pid: u32,
    description: String,
    output_path: PathBuf,
}

impl Tasks {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            outputs: HashMap::new(),
            descriptions: HashMap::new(),
            announced: HashSet::new(),
            completed: HashSet::new(),
            #[cfg(test)]
            transcript_bytes: 0,
            #[cfg(test)]
            output_bytes: 0,
        }
    }

    pub(super) fn observe(&mut self, session: &str, summary: &transcript::Summary) {
        for id in &summary.background_ids {
            let key = (session.to_owned(), id.clone());
            if self.announced.insert(key.clone())
                && self.descriptions.get(&key).is_some_and(Option::is_none)
            {
                // A foreground shell can be moved to the background after its first scan.
                self.descriptions.remove(&key);
            }
        }
        self.completed.extend(
            summary
                .completed_background
                .iter()
                .map(|id| (session.to_owned(), id.clone())),
        );
    }

    fn directory(&self, session: &Session) -> PathBuf {
        self.root
            .join(project_slug(&session.cwd.to_string_lossy()))
            .join(&session.id.id)
            .join("tasks")
    }

    fn finished(&mut self, path: &Path) -> io::Result<bool> {
        if self.outputs.get(path).is_some_and(|output| output.finished) {
            return Ok(true);
        }
        let size = fs::metadata(path)?.len();
        if let Some(output) = self.outputs.get(path).filter(|output| output.size == size) {
            return Ok(output.finished);
        }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(size.saturating_sub(200)))?;
        let mut tail = Vec::new();
        file.take(200).read_to_end(&mut tail)?;
        #[cfg(test)]
        {
            self.output_bytes += tail.len() as u64;
        }
        let finished = footer(&String::from_utf8_lossy(&tail));
        self.outputs
            .insert(path.to_owned(), Output { size, finished });
        Ok(finished)
    }

    pub(super) fn refresh(
        &mut self,
        config: &Path,
        sessions: &mut [Session],
        liveness: &impl Liveness,
    ) -> io::Result<()> {
        let mut candidates = Vec::new();
        for (index, session) in sessions.iter_mut().enumerate() {
            session.background.clear();
            if session
                .process
                .as_ref()
                .is_some_and(|process| process.label.as_deref() == Some("bg"))
            {
                self.adopted(config, session, liveness)?;
            }
            let entries = match fs::read_dir(self.directory(session)) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if !entry.file_type()?.is_file()
                    || path.extension().is_none_or(|ext| ext != "output")
                {
                    continue;
                }
                match self.finished(&path) {
                    Ok(false) => candidates.push((index, path)),
                    Ok(true) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
        if candidates.is_empty() {
            return Ok(());
        }
        // One descriptor inventory serves every session in this refresh.
        let held = liveness.held_outputs()?;
        let mut running: HashMap<usize, HashMap<String, PathBuf>> = HashMap::new();
        for (index, path) in candidates {
            if held.contains(&path)
                && let Some(id) = path.file_stem().and_then(|id| id.to_str())
            {
                running
                    .entry(index)
                    .or_default()
                    .insert(id.to_owned(), path);
            }
        }
        for (index, outputs) in running {
            let session = &mut sessions[index];
            let Some(path) = &session.transcript else {
                continue;
            };
            let metadata = fs::metadata(path)?;
            let unknown = outputs
                .iter()
                .filter(|(id, _)| {
                    !self
                        .descriptions
                        .contains_key(&(session.id.id.clone(), (*id).clone()))
                })
                .map(|(id, path)| (id.clone(), path.clone()))
                .collect::<HashMap<_, _>>();
            if !unknown.is_empty() {
                let (mut found, bytes) =
                    transcript::background_tasks(path, metadata.len(), &unknown)?;
                #[cfg(test)]
                {
                    self.transcript_bytes += bytes;
                }
                #[cfg(not(test))]
                let _ = bytes;
                for id in unknown.keys() {
                    self.descriptions
                        .insert((session.id.id.clone(), id.clone()), found.remove(id));
                }
            }
            // Foreground Bash outputs have no backgroundTaskId result and never appear.
            for id in outputs.keys().filter(|id| {
                !self
                    .completed
                    .contains(&(session.id.id.clone(), (*id).clone()))
            }) {
                if let Some(task) = self
                    .descriptions
                    .get(&(session.id.id.clone(), id.clone()))
                    .cloned()
                    .flatten()
                    && !session
                        .background
                        .iter()
                        .any(|existing| existing.output == task.output)
                {
                    session.background.push(task);
                }
            }
            session.background.sort_by(|a, b| {
                a.started_at
                    .cmp(&b.started_at)
                    .then_with(|| a.output.cmp(&b.output))
            });
        }
        Ok(())
    }

    fn adopted(
        &mut self,
        config: &Path,
        session: &mut Session,
        liveness: &impl Liveness,
    ) -> io::Result<()> {
        let Some(job) = session.background_job.as_ref() else {
            return Ok(());
        };
        let bytes = match fs::read(config.join("jobs").join(&job.id).join("adopt.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let adopt: Adoption = serde_json::from_slice(&bytes)?;
        for shell in adopt.shells {
            if shell.task_id.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "empty adopted task id",
                ));
            }
            let finished = match self.finished(&shell.output_path) {
                Ok(finished) => finished,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !finished && liveness.alive(shell.pid)? {
                let started_at = crate::processes::start_time(shell.pid).ok_or_else(|| {
                    io::Error::other(format!(
                        "cannot read adopted shell {} start time",
                        shell.pid
                    ))
                })?;
                session.background.push(BackgroundTask {
                    description: shell.description,
                    output: Some(shell.output_path),
                    started_at,
                });
            }
        }
        Ok(())
    }
}

fn project_slug(cwd: &str) -> String {
    let slug: String = cwd
        .encode_utf16()
        .map(|unit| {
            if unit <= 127 && (unit as u8).is_ascii_alphanumeric() {
                char::from(unit as u8)
            } else {
                '-'
            }
        })
        .collect();
    if slug.len() <= 200 {
        return slug;
    }
    let hash = cwd.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    let mut value = i64::from(hash).unsigned_abs();
    let mut suffix = Vec::new();
    loop {
        suffix.push(char::from_digit((value % 36) as u32, 36).unwrap());
        value /= 36;
        if value == 0 {
            break;
        }
    }
    format!(
        "{}-{}",
        &slug[..200],
        suffix.iter().rev().collect::<String>()
    )
}

fn footer(tail: &str) -> bool {
    let line = tail.trim_end().rsplit('\n').next().unwrap_or("");
    line == "[killed]"
        || line.starts_with("[process exited while detached") && line.ends_with(']')
        || line
            .strip_prefix("[exited with code ")
            .and_then(|s| s.strip_suffix(']'))
            .is_some_and(|s| s.parse::<i32>().is_ok())
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
