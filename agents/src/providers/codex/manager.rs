//! Cross-provider ownership and pending requests recorded by codex-manager.

use super::{ProviderId, SessionKey};
use serde::Deserialize;
use std::{collections::HashMap, fs, io, path::Path};

pub(super) struct Link {
    pub parent: SessionKey,
    pub name: Option<String>,
    pub needs_input: bool,
}

#[derive(Deserialize)]
struct Supervisor {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "pid")]
    _pid: u32,
}

#[derive(Deserialize)]
struct State {
    threads: Vec<Thread>,
}

#[derive(Deserialize)]
struct Thread {
    id: String,
    name: Option<String>,
    #[serde(default)]
    detached: bool,
    #[serde(default)]
    waiting: Vec<serde_json::Value>,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn links(home: &Path) -> io::Result<HashMap<String, Link>> {
    let entries = match fs::read_dir(home.join("threads")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let mut states = HashMap::new();
    let mut links = HashMap::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(supervisor) = read::<Supervisor>(&path)? else {
            continue;
        };
        let session_id = supervisor.session_id;
        if !states.contains_key(&session_id) {
            let dir = home.join(&session_id);
            let state = read::<State>(&dir.join("state.json"))?;
            let mut threads = HashMap::new();
            if let Some(state) = state {
                let files = fs::read_dir(&dir)?
                    .map(|entry| {
                        entry.map(|entry| entry.file_name().to_string_lossy().into_owned())
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                for thread in state.threads.into_iter().filter(|thread| !thread.detached) {
                    let prefix = format!("{}.ask.", thread.id);
                    let needs_input = !thread.waiting.is_empty()
                        || files
                            .iter()
                            .any(|name| name.starts_with(&prefix) && name.ends_with(".json"));
                    threads.insert(thread.id, (thread.name, needs_input));
                }
            }
            states.insert(session_id.clone(), threads);
        }
        let Some(id) = path.file_stem().and_then(|id| id.to_str()) else {
            continue;
        };
        if let Some((name, needs_input)) = states[&session_id].get(id) {
            links.insert(
                id.to_owned(),
                Link {
                    parent: SessionKey {
                        provider: ProviderId::Claude,
                        id: session_id,
                    },
                    name: name.clone().filter(|name| !name.trim().is_empty()),
                    needs_input: *needs_input,
                },
            );
        }
    }
    Ok(links)
}
