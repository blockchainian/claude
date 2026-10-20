//! Cross-provider ownership and pending requests recorded by codex-manager.

use super::{ProviderId, SessionKey};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
struct InstalledPlugins {
    plugins: HashMap<String, Vec<InstalledPlugin>>,
}

#[derive(Deserialize)]
struct InstalledPlugin {
    scope: String,
    #[serde(rename = "installPath")]
    install_path: PathBuf,
}

pub(super) fn detach_command(
    config: &Path,
    state: &Path,
    codex_home: &Path,
    thread_id: &str,
) -> io::Result<Command> {
    let installed: InstalledPlugins =
        serde_json::from_slice(&fs::read(config.join("plugins/installed_plugins.json"))?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let plugin = installed
        .plugins
        .get("codex@blockchainian")
        .and_then(|entries| entries.iter().find(|entry| entry.scope == "user"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "codex@blockchainian is not installed at user scope",
            )
        })?;
    let mut command = Command::new("node");
    command
        .arg(plugin.install_path.join("codex-manager/manager.mjs"))
        .args(["detach", "--thread", thread_id])
        .env("CODEX_MANAGER_STATE_DIR", state)
        .env("CODEX_HOME", codex_home);
    Ok(command)
}

pub(super) struct Link {
    pub parent: SessionKey,
    pub name: Option<String>,
    pub needs_input: bool,
    pub detached: bool,
}

#[derive(Deserialize)]
struct Supervisor {
    #[serde(rename = "sessionId")]
    session_id: String,
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

fn state_links(home: &Path, parent: &SessionKey) -> io::Result<HashMap<String, Link>> {
    let dir = home.join(&parent.id);
    let Some(state) = read::<State>(&dir.join("state.json"))? else {
        return Ok(HashMap::new());
    };
    let files = fs::read_dir(&dir)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(state
        .threads
        .into_iter()
        .map(|thread| {
            let prefix = format!("{}.ask.", thread.id);
            let needs_input = !thread.detached
                && (!thread.waiting.is_empty()
                    || files
                        .iter()
                        .any(|name| name.starts_with(&prefix) && name.ends_with(".json")));
            (
                thread.id,
                Link {
                    parent: parent.clone(),
                    name: thread.name.filter(|name| !name.trim().is_empty()),
                    needs_input,
                    detached: thread.detached,
                },
            )
        })
        .collect())
}

pub(super) fn parent_links(home: &Path, parent: &SessionKey) -> io::Result<HashMap<String, Link>> {
    if parent.provider != ProviderId::Claude {
        return Ok(HashMap::new());
    }
    let mut links = HashMap::new();
    for (id, link) in state_links(home, parent)? {
        let supervisor = read::<Supervisor>(&home.join("threads").join(format!("{id}.json")))?;
        if supervisor.is_some_and(|supervisor| supervisor.session_id == parent.id) {
            links.insert(id, link);
        }
    }
    Ok(links)
}

pub(super) fn thread_links(
    home: &Path,
    ids: impl IntoIterator<Item = String>,
) -> io::Result<HashMap<String, Link>> {
    let mut states = HashMap::new();
    let mut links = HashMap::new();
    for id in ids {
        let Some(supervisor) =
            read::<Supervisor>(&home.join("threads").join(format!("{id}.json")))?
        else {
            continue;
        };
        let session_id = supervisor.session_id;
        if !states.contains_key(&session_id) {
            let parent = SessionKey {
                provider: ProviderId::Claude,
                id: session_id.clone(),
            };
            states.insert(session_id.clone(), state_links(home, &parent)?);
        }
        if let Some(link) = states.get_mut(&session_id).unwrap().remove(&id) {
            links.insert(id, link);
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attached_parent(home: &Path) -> SessionKey {
        fs::create_dir_all(home.join("claude-parent")).unwrap();
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::write(
            home.join("claude-parent/state.json"),
            include_bytes!("../../../tests/fixtures/codex-manager/attached-state.json"),
        )
        .unwrap();
        fs::write(
            home.join("threads/thread-12.json"),
            br#"{"sessionId":"claude-parent","pid":70792}"#,
        )
        .unwrap();
        SessionKey {
            provider: ProviderId::Claude,
            id: "claude-parent".into(),
        }
    }

    #[test]
    fn parent_lookup_only_reads_its_own_state_and_registered_children() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home);
        fs::create_dir(home.join("unrelated-parent")).unwrap();
        fs::write(
            home.join("unrelated-parent/state.json"),
            b"invalid unrelated state",
        )
        .unwrap();
        fs::write(
            home.join("threads/unrelated.json"),
            b"invalid unrelated registry",
        )
        .unwrap();
        let links = parent_links(home, &parent).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links["thread-12"].name.as_deref(), Some("Managed worker"));
        assert_eq!(links["thread-12"].parent, parent);
        fs::write(home.join("claude-parent/thread-12.ask.pending.json"), b"{}").unwrap();
        assert!(parent_links(home, &parent).unwrap()["thread-12"].needs_input);
        fs::write(
            home.join("threads/thread-12.json"),
            br#"{"sessionId":"other-parent","pid":70792}"#,
        )
        .unwrap();
        assert!(parent_links(home, &parent).unwrap().is_empty());
        fs::remove_file(home.join("threads/thread-12.json")).unwrap();
        assert!(parent_links(home, &parent).unwrap().is_empty());
        assert!(
            parent_links(
                home,
                &SessionKey {
                    provider: ProviderId::Codex,
                    id: parent.id
                }
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn thread_lookup_only_reads_requested_registries_and_their_states() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home);
        fs::create_dir(home.join("unrelated-parent")).unwrap();
        fs::write(
            home.join("unrelated-parent/state.json"),
            b"invalid unrelated state",
        )
        .unwrap();
        fs::write(
            home.join("threads/unrelated.json"),
            b"invalid unrelated registry",
        )
        .unwrap();
        let links = thread_links(home, ["thread-12".into(), "missing".into()]).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links["thread-12"].parent, parent);
        assert_eq!(links["thread-12"].name.as_deref(), Some("Managed worker"));
        fs::write(
            home.join("threads/thread-12.json"),
            br#"{"sessionId":"other-parent","pid":70792}"#,
        )
        .unwrap();
        assert!(thread_links(home, ["thread-12".into()]).unwrap().is_empty());
    }

    #[test]
    fn detach_uses_the_installed_user_plugin_and_provider_directories() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("claude");
        fs::create_dir_all(config.join("plugins")).unwrap();
        let record = config.join("plugins/installed_plugins.json");
        let mut installed: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/installed-plugins.json"
        ))
        .unwrap();
        for path in ["plugin cache/codex/first", "plugin cache/codex/second"] {
            let install = directory.path().join(path);
            installed["plugins"]["codex@blockchainian"][0]["installPath"] =
                install.to_str().unwrap().into();
            fs::write(&record, installed.to_string()).unwrap();
            let state = directory.path().join("manager");
            let home = directory.path().join("codex");
            let command = detach_command(&config, &state, &home, "thread-12").unwrap();
            assert_eq!(command.get_program(), "node");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                [
                    install.join("codex-manager/manager.mjs").as_os_str(),
                    "detach".as_ref(),
                    "--thread".as_ref(),
                    "thread-12".as_ref(),
                ]
            );
            assert_eq!(
                command.get_envs().collect::<HashMap<_, _>>(),
                HashMap::from([
                    ("CODEX_HOME".as_ref(), Some(home.as_os_str())),
                    ("CODEX_MANAGER_STATE_DIR".as_ref(), Some(state.as_os_str())),
                ])
            );
        }
        installed["plugins"].as_object_mut().unwrap().clear();
        fs::write(&record, installed.to_string()).unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        fs::write(&record, "invalid").unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn stored_parent_survives_live_claude_session_changes_and_exit() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("manager");
        let config = directory.path().join("claude");
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::create_dir_all(config.join("sessions")).unwrap();
        let state = home.join("claude-parent");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("state.json"),
            include_bytes!("../../../tests/fixtures/codex-manager/attached-state.json"),
        )
        .unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        // Scrubbed real supervisor and PID-keyed registry payload shapes.
        let mut supervisor: serde_json::Value =
            serde_json::from_str(r#"{"sessionId":"claude-parent","pid":70792}"#).unwrap();
        supervisor["pid"] = child.id().into();
        let registry = config.join(format!("sessions/{}.json", std::process::id()));
        let mut entry: serde_json::Value =
            serde_json::from_str(r#"{"pid":70748,"sessionId":"claude-current"}"#).unwrap();
        entry["pid"] = std::process::id().into();
        fs::write(&registry, entry.to_string()).unwrap();
        let mut state_payload: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("state.json")).unwrap()).unwrap();
        let mut thread = state_payload["threads"][0].clone();
        thread["id"] = "thread-13".into();
        state_payload["threads"]
            .as_array_mut()
            .unwrap()
            .push(thread);
        fs::write(state.join("state.json"), state_payload.to_string()).unwrap();
        for id in ["thread-12", "thread-13"] {
            fs::write(
                home.join(format!("threads/{id}.json")),
                supervisor.to_string(),
            )
            .unwrap();
        }
        let current = thread_links(&home, ["thread-12".into(), "thread-13".into()]).unwrap();
        entry["sessionId"] = "claude-after-another-clear".into();
        fs::write(&registry, entry.to_string()).unwrap();
        let after_clear = thread_links(&home, ["thread-12".into(), "thread-13".into()]).unwrap();
        fs::remove_file(&registry).unwrap();
        let missing_registry =
            thread_links(&home, ["thread-12".into(), "thread-13".into()]).unwrap();
        fs::write(&registry, entry.to_string()).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let exited = thread_links(&home, ["thread-12".into(), "thread-13".into()]).unwrap();
        for id in ["thread-12", "thread-13"] {
            assert_eq!(current[id].parent.id, "claude-parent");
            assert_eq!(after_clear[id].parent.id, "claude-parent");
            assert_eq!(missing_registry[id].parent.id, "claude-parent");
            assert_eq!(exited[id].parent.id, "claude-parent");
            assert_eq!(current[id].name.as_deref(), Some("Managed worker"));
        }
    }

    #[test]
    fn detached_threads_keep_stored_parent_without_pending_input() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let supervisor: Supervisor = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/inactive-tree-supervisor.json"
        ))
        .unwrap();
        let state: State = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/inactive-tree-state.json"
        ))
        .unwrap();
        let id = &state.threads[0].id;
        let session = home.join(&supervisor.session_id);
        fs::create_dir_all(&session).unwrap();
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::write(
            session.join("state.json"),
            include_bytes!("../../../tests/fixtures/codex-manager/inactive-tree-state.json"),
        )
        .unwrap();
        fs::write(
            home.join(format!("threads/{id}.json")),
            include_bytes!("../../../tests/fixtures/codex-manager/inactive-tree-supervisor.json"),
        )
        .unwrap();
        let check = || {
            let links = thread_links(home, [id.clone()]).unwrap();
            let link = links.get(id).expect("detached thread retains its parent");
            assert_eq!(link.parent.provider, ProviderId::Claude);
            assert_eq!(link.parent.id, supervisor.session_id);
            assert_eq!(link.name, state.threads[0].name);
            assert!(!link.needs_input);
            assert!(link.detached);
            let parent = SessionKey {
                provider: ProviderId::Claude,
                id: supervisor.session_id.clone(),
            };
            let children = parent_links(home, &parent).unwrap();
            assert!(children[id].detached);
            assert!(!children[id].needs_input);
            let selected = thread_links(home, [id.clone()]).unwrap();
            assert!(selected[id].detached);
            assert!(!selected[id].needs_input);
        };
        check();
        // A stale request file cannot make a detached thread need input.
        fs::write(session.join(format!("{id}.ask.stale.json")), b"{}").unwrap();
        check();
    }
}
