//! Cross-provider ownership and pending requests recorded by codex-manager.

use crate::providers::{ProviderId, SessionKey, Supervision, SupervisionState};
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

/// `node <install>/codex-manager/manager.mjs detach --thread <id>`, from `installed_plugins.json`.
pub(in crate::providers::claude) fn detach_command(
    config: &Path,
    state: &Path,
    thread: &str,
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
        .args(["detach", "--thread", thread])
        .env("CODEX_MANAGER_STATE_DIR", state);
    Ok(command)
}

/// `$CODEX_MANAGER_STATE_DIR`, else `~/.local/state/codex-manager`.
pub(crate) fn state_dir() -> io::Result<PathBuf> {
    if let Some(path) =
        std::env::var_os("CODEX_MANAGER_STATE_DIR").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unset"))?;
    Ok(PathBuf::from(home).join(".local/state/codex-manager"))
}

#[derive(Deserialize)]
struct Supervisor {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct State {
    threads: Vec<Thread>,
}

#[derive(Deserialize)]
struct Thread {
    id: String,
    #[serde(default)]
    detached: bool,
}

#[derive(Deserialize)]
struct Ask {
    since: u64,
    text: String,
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

/// The ids of the `threads/*.json` records that have no `sessionId`.
pub(in crate::providers) fn unsupervised_threads(state: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(state.join("threads")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut threads = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        // Atomic writes leave no partial records; skip unreadable or malformed external files.
        if let Ok(Some(Supervisor { session_id: None })) = read::<Supervisor>(&path) {
            threads.push(id.to_owned());
        }
    }
    Ok(threads)
}

/// Removes the thread's record; a missing record is not an error.
pub(in crate::providers) fn remove_thread_record(state: &Path, thread_id: &str) -> io::Result<()> {
    match fs::remove_file(state.join("threads").join(format!("{thread_id}.json"))) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The Codex threads the given Claude sessions supervise, as codex-manager records them.
pub(in crate::providers::claude) fn supervisions(
    state: &Path,
    sessions: &[SessionKey],
) -> io::Result<Vec<Supervision>> {
    let mut supervisions = Vec::new();
    for session in sessions
        .iter()
        .filter(|session| session.provider == ProviderId::Claude)
    {
        let dir = state.join(&session.id);
        let Some(record) = read::<State>(&dir.join("state.json"))? else {
            continue;
        };
        let files = fs::read_dir(&dir)?
            .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<Vec<_>>>()?;
        for thread in record.threads {
            let supervisor =
                read::<Supervisor>(&state.join("threads").join(format!("{}.json", thread.id)))?;
            if supervisor
                .is_none_or(|supervisor| supervisor.session_id.as_deref() != Some(&session.id))
            {
                continue;
            }
            let prefix = format!("{}.ask.", thread.id);
            let supervision_state = if thread.detached {
                SupervisionState::Detached
            } else {
                let mut asks = Vec::new();
                for name in files
                    .iter()
                    .filter(|name| name.starts_with(&prefix) && name.ends_with(".json"))
                {
                    if let Some(ask) = read::<Ask>(&dir.join(name))? {
                        asks.push(ask);
                    }
                }
                asks.sort_by_key(|ask| ask.since);
                if asks.is_empty() {
                    SupervisionState::Active
                } else {
                    SupervisionState::Asking(asks.into_iter().map(|ask| ask.text).collect())
                }
            };
            supervisions.push(Supervision {
                supervisor: session.clone(),
                session: SessionKey {
                    provider: ProviderId::Codex,
                    id: thread.id,
                },
                state: supervision_state,
            });
        }
    }
    Ok(supervisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn key(id: &str) -> SessionKey {
        SessionKey {
            provider: ProviderId::Claude,
            id: id.into(),
        }
    }

    pub(super) fn attached_parent(home: &Path, id: &str) -> SessionKey {
        fs::create_dir_all(home.join(id)).unwrap();
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::write(
            home.join(id).join("state.json"),
            include_bytes!("../../../../tests/fixtures/codex-manager/attached-state.json"),
        )
        .unwrap();
        // Scrubbed real supervisor payload from codex-manager.
        let mut supervisor: serde_json::Value =
            serde_json::from_str(r#"{"sessionId":"claude-parent","pid":70792}"#).unwrap();
        supervisor["sessionId"] = id.into();
        fs::write(home.join("threads/thread-12.json"), supervisor.to_string()).unwrap();
        key(id)
    }

    #[test]
    fn empty_record_does_not_supervise_a_session() {
        let directory = tempfile::tempdir().unwrap();
        let parent = attached_parent(directory.path(), "claude-parent");
        fs::write(directory.path().join("threads/thread-12.json"), b"{}").unwrap();
        assert!(
            supervisions(directory.path(), &[parent])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unsupervised_scan_selects_empty_records_and_skips_bad_files() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        attached_parent(home, "claude-parent");
        fs::write(home.join("threads/unsupervised.json"), b"{}").unwrap();
        fs::write(home.join("threads/truncated.json"), br#"{"sessionId":"#).unwrap();
        fs::write(home.join("threads/invalid.json"), b"invalid").unwrap();
        fs::write(home.join("threads/ignored.txt"), b"{}").unwrap();
        fs::create_dir(home.join("threads/unreadable.json")).unwrap();
        symlink(home.join("missing.json"), home.join("threads/missing.json")).unwrap();
        assert_eq!(unsupervised_threads(home).unwrap(), ["unsupervised"]);
    }

    #[test]
    fn unsupervised_scan_allows_missing_directory() {
        let directory = tempfile::tempdir().unwrap();
        assert!(unsupervised_threads(directory.path()).unwrap().is_empty());
    }

    #[test]
    fn removing_thread_record_allows_missing_record() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        remove_thread_record(home, "thread-12").unwrap();
        attached_parent(home, "claude-parent");
        remove_thread_record(home, "thread-12").unwrap();
        assert!(!home.join("threads/thread-12.json").exists());
        remove_thread_record(home, "thread-12").unwrap();
    }

    #[test]
    fn requested_supervisors_only_and_clear_alias_uses_thread_registry() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home, "claude-parent");
        fs::create_dir(home.join("unrelated-parent")).unwrap();
        fs::write(home.join("unrelated-parent/state.json"), b"invalid").unwrap();
        fs::write(home.join("threads/unrelated.json"), b"invalid").unwrap();
        symlink(home.join("claude-parent"), home.join("after-clear")).unwrap();
        let alias = key("after-clear");
        let check = |expected: &SessionKey| {
            let links = supervisions(home, &[parent.clone(), alias.clone()]).unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(&links[0].supervisor, expected);
            assert_eq!(
                links[0].session,
                SessionKey {
                    provider: ProviderId::Codex,
                    id: "thread-12".into()
                }
            );
            assert_eq!(links[0].state, SupervisionState::Active);
        };
        check(&parent);
        fs::write(
            home.join("threads/thread-12.json"),
            br#"{"sessionId":"after-clear","pid":70792}"#,
        )
        .unwrap();
        check(&alias);
        fs::remove_file(home.join("threads/thread-12.json")).unwrap();
        assert!(
            supervisions(home, &[parent.clone(), alias])
                .unwrap()
                .is_empty()
        );
        assert!(
            supervisions(
                home,
                &[
                    SessionKey {
                        provider: ProviderId::Codex,
                        id: parent.id
                    },
                    key("missing")
                ]
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn asking_uses_ask_files_and_ignores_approval_waiting() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home, "claude-parent");
        let state = home.join("claude-parent/state.json");
        let mut payload: serde_json::Value =
            serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
        // Only the presence of an approval matters; its contents are not read here.
        payload["threads"][0]["waiting"] = serde_json::json!([{}]);
        fs::write(&state, payload.to_string()).unwrap();
        let status = || {
            supervisions(home, std::slice::from_ref(&parent)).unwrap()[0]
                .state
                .clone()
        };
        assert_eq!(status(), SupervisionState::Active);
        fs::write(home.join("claude-parent/thread-12.ask.pending.txt"), b"{}").unwrap();
        fs::write(home.join("claude-parent/other.ask.pending.json"), b"{}").unwrap();
        assert_eq!(status(), SupervisionState::Active);
        let ask = home.join("claude-parent/thread-12.ask.pending.json");
        fs::write(&ask, pending_ask().to_string()).unwrap();
        assert_eq!(
            status(),
            SupervisionState::Asking(vec![pending_ask()["text"].as_str().unwrap().into()])
        );
        fs::remove_file(ask).unwrap();
        assert_eq!(status(), SupervisionState::Active);
    }

    fn pending_ask() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/supervision-ask.json"
        ))
        .unwrap()
    }

    #[test]
    fn asking_carries_the_question_text() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home, "claude-parent");
        let ask = pending_ask();
        fs::write(
            home.join("claude-parent/thread-12.ask.pending.json"),
            ask.to_string(),
        )
        .unwrap();
        let links = supervisions(home, &[parent]).unwrap();
        assert_eq!(
            links[0].state,
            SupervisionState::Asking(vec![ask["text"].as_str().unwrap().into()])
        );
    }

    #[test]
    fn asking_orders_questions_by_since() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home, "claude-parent");
        let first = pending_ask();
        let mut second = first.clone();
        second["since"] = (first["since"].as_u64().unwrap() + 1).into();
        second["callId"] = "later".into();
        second["text"] = "Should the later question follow the first?".into();
        for (name, ask) in [("a-later", &second), ("z-earlier", &first)] {
            fs::write(
                home.join(format!("claude-parent/thread-12.ask.{name}.json")),
                ask.to_string(),
            )
            .unwrap();
        }
        let links = supervisions(home, &[parent]).unwrap();
        assert_eq!(
            links[0].state,
            SupervisionState::Asking(vec![
                first["text"].as_str().unwrap().into(),
                second["text"].as_str().unwrap().into(),
            ])
        );
    }

    #[test]
    fn asking_skips_files_missing_when_read() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let parent = attached_parent(home, "claude-parent");
        let target = home.join("ask.json");
        fs::write(&target, pending_ask().to_string()).unwrap();
        symlink(
            &target,
            home.join("claude-parent/thread-12.ask.pending.json"),
        )
        .unwrap();
        // The directory entry remains, but reading it returns NotFound after withdrawal.
        fs::remove_file(target).unwrap();
        let links = supervisions(home, &[parent]).unwrap();
        assert_eq!(links[0].state, SupervisionState::Active);
    }

    #[test]
    fn detached_keeps_supervisor_despite_stale_ask() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let supervisor: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-manager/inactive-tree-supervisor.json"
        ))
        .unwrap();
        let state: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-manager/inactive-tree-state.json"
        ))
        .unwrap();
        let parent = key(supervisor["sessionId"].as_str().unwrap());
        let id = state["threads"][0]["id"].as_str().unwrap();
        fs::create_dir_all(home.join(&parent.id)).unwrap();
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::write(home.join(&parent.id).join("state.json"), state.to_string()).unwrap();
        fs::write(
            home.join(format!("threads/{id}.json")),
            supervisor.to_string(),
        )
        .unwrap();
        let check = || {
            let links = supervisions(home, std::slice::from_ref(&parent)).unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].supervisor, parent);
            assert_eq!(links[0].state, SupervisionState::Detached);
        };
        check();
        fs::write(
            home.join(&parent.id).join(format!("{id}.ask.stale.json")),
            b"{}",
        )
        .unwrap();
        check();
    }

    #[test]
    fn stored_supervisor_ignores_live_registry_changes_and_exit() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("manager");
        let parent = attached_parent(&home, "claude-parent");
        let config = directory.path().join("claude/sessions");
        fs::create_dir_all(&config).unwrap();
        let mut child = Command::new("sleep").arg("60").spawn().unwrap();
        let mut supervisor: serde_json::Value =
            serde_json::from_str(r#"{"sessionId":"claude-parent","pid":70792}"#).unwrap();
        supervisor["pid"] = child.id().into();
        fs::write(home.join("threads/thread-12.json"), supervisor.to_string()).unwrap();
        let registry = config.join(format!("{}.json", child.id()));
        let mut entry: serde_json::Value =
            serde_json::from_str(r#"{"pid":70748,"sessionId":"claude-current"}"#).unwrap();
        entry["pid"] = child.id().into();
        let check = || {
            let links = supervisions(&home, std::slice::from_ref(&parent)).unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].supervisor, parent);
        };
        fs::write(&registry, entry.to_string()).unwrap();
        check();
        entry["sessionId"] = "claude-after-another-clear".into();
        fs::write(&registry, entry.to_string()).unwrap();
        check();
        fs::remove_file(registry).unwrap();
        check();
        child.kill().unwrap();
        child.wait().unwrap();
        check();
    }

    #[test]
    fn malformed_requested_state_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let parent = attached_parent(directory.path(), "claude-parent");
        fs::write(directory.path().join("threads/thread-12.json"), b"invalid").unwrap();
        assert_eq!(
            supervisions(directory.path(), &[parent])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn detach_uses_the_installed_user_plugin_and_provider_directories() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("claude");
        fs::create_dir_all(config.join("plugins")).unwrap();
        let record = config.join("plugins/installed_plugins.json");
        let mut installed: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-manager/installed-plugins.json"
        ))
        .unwrap();
        for path in ["plugin cache/codex/first", "plugin cache/codex/second"] {
            let install = directory.path().join(path);
            installed["plugins"]["codex@blockchainian"][0]["installPath"] =
                install.to_str().unwrap().into();
            fs::write(&record, installed.to_string()).unwrap();
            let state = directory.path().join("manager");
            let command = detach_command(&config, &state, "thread-12").unwrap();
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
                HashMap::from([("CODEX_MANAGER_STATE_DIR".as_ref(), Some(state.as_os_str())),])
            );
        }
        installed["plugins"].as_object_mut().unwrap().clear();
        fs::write(&record, installed.to_string()).unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        fs::write(&record, "invalid").unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
