use super::*;
use crate::providers::{Supervision, SupervisionState};

fn setup(config: &Path, state: &Path) -> (SessionKey, PathBuf) {
    let id = "00000000-0000-0000-0000-000000000001";
    super::tests::transcript(config, id, 0);
    fs::create_dir_all(state.join(id)).unwrap();
    fs::create_dir_all(state.join("threads")).unwrap();
    let path = state.join(id).join("state.json");
    fs::write(
        &path,
        include_bytes!("../../../tests/fixtures/codex-manager/attached-state.json"),
    )
    .unwrap();
    let mut supervisor: serde_json::Value =
        serde_json::from_str(r#"{"sessionId":"claude-parent","pid":70792}"#).unwrap();
    supervisor["sessionId"] = id.into();
    fs::write(state.join("threads/thread-12.json"), supervisor.to_string()).unwrap();
    (
        SessionKey {
            provider: ProviderId::Claude,
            id: id.into(),
        },
        path,
    )
}

#[test]
fn codex_manager_refreshes_cached_supervisors_and_session_source() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let state = directory.path().join("manager");
    let (parent, _) = setup(&config, &state);
    let mut provider = ClaudeProvider::new(config).with_codex_manager(state.clone());
    let snapshot = provider.refresh(1);
    assert_eq!(snapshot.supervisions.len(), 1);
    assert_eq!(snapshot.supervisions[0].supervisor, parent);
    assert_eq!(snapshot.supervisions[0].state, SupervisionState::Active);
    let transcript = snapshot.sessions[0].transcript.as_ref().unwrap();
    fs::write(
        transcript,
        b"invalid transcript proving the row is cached\n",
    )
    .unwrap();
    let ask = state.join(&parent.id).join("thread-12.ask.pending.json");
    let pending: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/supervision-ask.json")).unwrap();
    fs::write(&ask, pending.to_string()).unwrap();
    let refreshed = provider.refresh_sessions(1, &[], &snapshot.sessions);
    assert_eq!(refreshed.sessions[0].title, snapshot.sessions[0].title);
    assert_eq!(
        refreshed.supervisions[0].state,
        SupervisionState::Asking(vec![pending["text"].as_str().unwrap().into()])
    );
    fs::remove_file(ask).unwrap();
    assert_eq!(
        provider
            .refresh_sessions(1, &[], &snapshot.sessions)
            .supervisions[0]
            .state,
        SupervisionState::Active
    );
    assert!(provider.watch_roots().unwrap().contains(&state));
    let mut source = provider.session_source();
    assert!(source.watch_roots().unwrap().contains(&state));
    assert_eq!(source.refresh(1).supervisions.len(), 1);
}

#[test]
fn codex_manager_is_optional_and_watches_existing_state_only() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let state = directory.path().join("manager");
    let (parent, _) = setup(&config, &state);
    let mut provider = ClaudeProvider::new(config);
    let supervision = Supervision {
        supervisor: parent,
        session: SessionKey {
            provider: ProviderId::Codex,
            id: "thread-12".into(),
        },
        state: SupervisionState::Active,
    };
    assert!(provider.refresh(1).supervisions.is_empty());
    assert!(!provider.watch_roots().unwrap().contains(&state));
    assert!(
        matches!(provider.detach(&supervision), Err(error) if error.kind() == io::ErrorKind::Unsupported)
    );
    let missing = directory.path().join("missing-manager");
    let provider = provider.with_codex_manager(missing.clone());
    assert!(!provider.watch_roots().unwrap().contains(&missing));
    let mut wrong = supervision;
    wrong.session.provider = ProviderId::Claude;
    assert!(
        matches!(provider.detach(&wrong), Err(error) if error.kind() == io::ErrorKind::Unsupported)
    );
}

#[test]
fn codex_manager_detach_job_rereads_state_before_running() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let state = directory.path().join("manager");
    let (_, state_path) = setup(&config, &state);
    let mut provider = ClaudeProvider::new(config).with_codex_manager(state);
    let supervision = provider.refresh(1).supervisions.remove(0);
    let job = provider.detach(&supervision).unwrap();
    let mut payload: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    payload["threads"][0]["detached"] = true.into();
    fs::write(state_path, payload.to_string()).unwrap();
    // No plugin installation exists; success means the job skipped the command.
    job().unwrap();
}
