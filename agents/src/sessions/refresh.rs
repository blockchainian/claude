use super::*;

#[derive(Debug, PartialEq, Eq)]
pub enum RefreshRequest {
    Full,
    Paths(HashSet<PathBuf>),
}

impl RefreshRequest {
    pub fn paths(paths: Vec<PathBuf>) -> Self {
        if paths.is_empty() {
            Self::Full
        } else {
            Self::Paths(paths.into_iter().collect())
        }
    }

    pub fn merge(&mut self, request: Self) {
        match (self, request) {
            (Self::Paths(paths), Self::Paths(more)) => paths.extend(more),
            (pending, _) => *pending = Self::Full,
        }
    }
}

/// Owns provider caches for the refresh worker, separate from UI action handles.
pub struct Refresh {
    hub: Hub,
    snapshot: Option<Snapshot>,
}

impl Refresh {
    pub(super) fn new(hub: Hub) -> Self {
        Self {
            hub,
            snapshot: None,
        }
    }

    pub fn run(&mut self, pages: usize, request: RefreshRequest) -> Snapshot {
        let changed_pages = self.hub.pages != pages;
        self.hub.pages = pages;
        let snapshot = match (request, self.snapshot.take()) {
            (RefreshRequest::Paths(paths), Some(previous)) if !changed_pages => self
                .hub
                .refresh_paths(&paths.into_iter().collect::<Vec<_>>(), previous),
            _ => self.hub.refresh(),
        };
        self.snapshot = Some(Snapshot {
            sessions: snapshot.sessions.clone(),
            has_more: snapshot.has_more,
        });
        snapshot
    }
}

pub(super) fn affected_sessions(
    paths: &[PathBuf],
    sessions: &[crate::model::Session],
) -> Option<HashSet<SessionKey>> {
    if paths.is_empty() {
        return None;
    }
    let mut selected = HashSet::new();
    for path in paths {
        let exact: Vec<_> = sessions
            .iter()
            .filter(|session| session.transcript.as_deref() == Some(path.as_path()))
            .map(|session| session.id.clone())
            .collect();
        // A transcript identifies its row before any ancestor identifies its parent.
        if !exact.is_empty() {
            selected.extend(exact);
            continue;
        }
        let mut closest = None;
        let mut owners = HashSet::new();
        for session in sessions {
            let by_name = path.components().rev().position(|part| {
                part.as_os_str().to_str().is_some_and(|name| {
                    matches_id(name, &session.id.id)
                        || session
                            .background_job
                            .as_ref()
                            .is_some_and(|job| matches_id(name, &job.id))
                })
            });
            let by_directory = session.transcript.as_deref().and_then(|file| {
                let directory = file.with_extension("");
                path.starts_with(&directory)
                    .then(|| path.components().count() - directory.components().count())
            });
            let registry = path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "sessions")
                && session.process.as_ref().is_some_and(|process| {
                    path.file_stem().and_then(|stem| stem.to_str())
                        == Some(process.pid.to_string().as_str())
                });
            let Some(distance) = [by_name, by_directory, registry.then_some(0)]
                .into_iter()
                .flatten()
                .min()
            else {
                continue;
            };
            if closest.is_none_or(|best| distance < best) {
                closest = Some(distance);
                owners.clear();
            }
            if closest == Some(distance) {
                owners.insert(session.id.clone());
            }
        }
        if owners.is_empty() {
            return None;
        }
        selected.extend(owners);
    }
    loop {
        let before = selected.len();
        for session in sessions {
            if session
                .parent
                .as_ref()
                .is_some_and(|parent| selected.contains(parent))
            {
                selected.insert(session.id.clone());
            }
        }
        if selected.len() == before {
            break;
        }
    }
    Some(selected)
}

fn matches_id(name: &str, id: &str) -> bool {
    name == id
        || name
            .strip_prefix(id)
            .is_some_and(|suffix| suffix.starts_with('.'))
}
