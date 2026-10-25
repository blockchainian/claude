use super::*;
use crate::model::{Action, Process, Session};
use crate::providers::{SessionAction, Supervision, SupervisionState};
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeProvider {
    id: ProviderId,
    page: Vec<Session>,
    children: Vec<Session>,
    loaded: Vec<Session>,
    supervisions: Vec<Supervision>,
    child_supervisions: Vec<Supervision>,
    load_supervisions: Vec<Supervision>,
    calls: Arc<Mutex<Vec<Vec<SessionKey>>>>,
    actions: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeProvider {
    fn new(id: ProviderId) -> Self {
        Self {
            id,
            page: vec![],
            children: vec![],
            loaded: vec![],
            supervisions: vec![],
            child_supervisions: vec![],
            load_supervisions: vec![],
            calls: Arc::default(),
            actions: Arc::default(),
        }
    }
}

impl Provider for FakeProvider {
    fn session_source(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }
    fn id(&self) -> ProviderId {
        self.id
    }
    fn display_name(&self) -> &'static str {
        "Fake"
    }
    fn watch_roots(&self) -> io::Result<Vec<PathBuf>> {
        Ok(vec![])
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn refresh(&mut self, _: usize) -> Snapshot {
        Snapshot {
            sessions: self.page.clone(),
            has_more: false,
            supervisions: self.supervisions.clone(),
        }
    }
    fn children(&mut self, parents: &[SessionKey]) -> Snapshot {
        Snapshot {
            sessions: self
                .children
                .iter()
                .filter(|s| s.parent.as_ref().is_some_and(|p| parents.contains(p)))
                .cloned()
                .collect(),
            has_more: false,
            supervisions: self
                .child_supervisions
                .iter()
                .filter(|supervision| {
                    self.children.iter().any(|session| {
                        session.id == supervision.supervisor
                            && session
                                .parent
                                .as_ref()
                                .is_some_and(|parent| parents.contains(parent))
                    })
                })
                .cloned()
                .collect(),
        }
    }
    fn load(&mut self, ids: &[SessionKey]) -> Snapshot {
        self.calls.lock().unwrap().push(ids.to_vec());
        Snapshot {
            sessions: self
                .loaded
                .iter()
                .filter(|s| ids.contains(&s.id))
                .cloned()
                .collect(),
            has_more: false,
            supervisions: self.load_supervisions.clone(),
        }
    }
    fn enrich(&mut self, mut snapshot: Snapshot) -> Snapshot {
        for session in &mut snapshot.sessions {
            session.status = SessionStatus::Inactive;
        }
        snapshot
    }
    fn event_panel(&self, _: &Session, _: usize, _: bool) -> io::Result<Vec<crate::event::Event>> {
        Ok(vec![])
    }
    fn launch(&self, _: &Action, _: Option<&Session>) -> Option<Command> {
        None
    }
    fn delete(&self, _: &Session) -> io::Result<SessionAction> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn archive(&self, _: &Session) -> io::Result<SessionAction> {
        let actions = self.actions.clone();
        Ok(Box::new(move || {
            actions.lock().unwrap().push("archive");
            Ok(())
        }))
    }
    fn detach(&self, _: &Supervision) -> io::Result<SessionAction> {
        let actions = self.actions.clone();
        Ok(Box::new(move || {
            actions.lock().unwrap().push("detach");
            Ok(())
        }))
    }
}

// Parse a real upstream transcript before changing only the relationship under test.
fn session(provider: ProviderId, id: &str) -> Session {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("00000000-0000-0000-0000-000000000001.jsonl"),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut session = crate::providers::claude::ClaudeProvider::new(dir.path().into())
        .refresh(1)
        .sessions
        .remove(0);
    session.id = SessionKey {
        provider,
        id: id.into(),
    };
    session
}

fn providers() -> (FakeProvider, FakeProvider) {
    (
        FakeProvider::new(ProviderId::Claude),
        FakeProvider::new(ProviderId::Codex),
    )
}

fn supervision(supervisor: &Session, session: &Session, state: SupervisionState) -> Supervision {
    Supervision {
        supervisor: supervisor.id.clone(),
        session: session.id.clone(),
        state,
    }
}

fn pending_questions() -> Vec<String> {
    let ask: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/supervision-ask.json")).unwrap();
    vec![ask["text"].as_str().unwrap().into()]
}

fn live(session: &mut Session) {
    session.process = Some(Process {
        pid: std::process::id(),
        label: None,
        started_at: None,
    });
}

#[test]
fn loads_supervised_sessions_only_from_live_trees() {
    for is_live in [false, true] {
        let (mut owner, mut target) = providers();
        let mut root = session(owner.id, "root");
        if is_live {
            live(&mut root);
        }
        let thread = session(target.id, "thread");
        owner
            .supervisions
            .push(supervision(&root, &thread, SupervisionState::Active));
        owner.page.push(root.clone());
        if !is_live {
            let mut unrelated = session(owner.id, "unrelated-live");
            live(&mut unrelated);
            owner.page.push(unrelated);
        }
        target.loaded.push(thread.clone());
        let calls = target.calls.clone();
        let result = Hub::new(vec![Box::new(owner), Box::new(target)]).refresh();
        assert_eq!(
            *calls.lock().unwrap(),
            if is_live {
                vec![vec![thread.id.clone()]]
            } else {
                vec![]
            }
        );
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.sessions.iter().any(|s| s.id == thread.id), is_live);
        if is_live {
            assert_eq!(
                result
                    .sessions
                    .iter()
                    .find(|s| s.id == thread.id)
                    .unwrap()
                    .parent,
                Some(root.id)
            );
        }
    }
}

#[test]
fn page_sessions_are_not_loaded_again_and_asking_is_applied_after_enrich() {
    let (mut owner, mut target) = providers();
    let mut root = session(owner.id, "root");
    live(&mut root);
    let mut thread = session(target.id, "thread");
    thread.name = Some("Named thread".into());
    owner.supervisions.push(supervision(
        &root,
        &thread,
        SupervisionState::Asking(pending_questions()),
    ));
    owner.page.push(root.clone());
    target.page.push(thread.clone());
    let calls = target.calls.clone();
    let result = Hub::new(vec![Box::new(target), Box::new(owner)]).refresh();
    assert!(calls.lock().unwrap().is_empty());
    let row = result.sessions.iter().find(|s| s.id == thread.id).unwrap();
    assert_eq!(row.parent, Some(root.id));
    assert_eq!(row.title, "Named thread");
    assert_eq!(row.status, SessionStatus::NeedsInput);
    assert_eq!(row.pending_questions, pending_questions());
    assert_eq!(result.supervisions.len(), 1);
}

#[test]
fn collects_child_and_loaded_snapshot_supervisions_and_walks_loaded_descendants() {
    let (mut owner, mut target) = providers();
    let mut root = session(owner.id, "root");
    live(&mut root);
    let mut child = session(owner.id, "child");
    child.parent = Some(root.id.clone());
    let thread = session(target.id, "thread");
    let nested = session(owner.id, "nested");
    let mut descendant = session(target.id, "descendant");
    descendant.parent = Some(thread.id.clone());
    owner
        .child_supervisions
        .push(supervision(&child, &thread, SupervisionState::Active));
    target
        .load_supervisions
        .push(supervision(&thread, &nested, SupervisionState::Detached));
    owner.page.push(root);
    owner.children.push(child);
    owner.loaded.push(nested.clone());
    target.loaded.push(thread.clone());
    target.children.push(descendant.clone());
    let owner_calls = owner.calls.clone();
    let target_calls = target.calls.clone();
    let result = Hub::new(vec![Box::new(target), Box::new(owner)]).refresh();
    assert_eq!(*target_calls.lock().unwrap(), vec![vec![thread.id.clone()]]);
    assert_eq!(*owner_calls.lock().unwrap(), vec![vec![nested.id.clone()]]);
    assert_eq!(result.sessions.len(), 5);
    assert_eq!(
        result
            .sessions
            .iter()
            .find(|s| s.id == nested.id)
            .unwrap()
            .parent,
        Some(thread.id)
    );
    assert!(result.sessions.iter().any(|s| s.id == descendant.id));
}

#[test]
fn cached_rows_receive_current_supervisions_without_enrich() {
    let (mut owner, mut target) = providers();
    let root = session(owner.id, "root");
    let unrelated = session(owner.id, "unrelated");
    let mut thread = session(target.id, "thread");
    thread.name = Some("Cached name".into());
    thread.title = "Cached title".into();
    thread.status = SessionStatus::Ready;
    owner.page = vec![root.clone(), unrelated.clone()];
    target.page = vec![thread.clone()];
    let current = supervision(
        &root,
        &thread,
        SupervisionState::Asking(pending_questions()),
    );
    owner.supervisions.push(current.clone());
    let previous = Snapshot {
        sessions: vec![root.clone(), unrelated.clone(), thread.clone()],
        has_more: false,
        supervisions: vec![],
    };
    let affected = HashSet::from([unrelated.id]);
    let result = Hub::new(vec![Box::new(owner), Box::new(target)])
        .refresh_selected(Some((&affected, &previous)));
    let row = result.sessions.iter().find(|s| s.id == thread.id).unwrap();
    assert_eq!(row.parent, Some(root.id));
    assert_eq!(row.title, "Cached name");
    assert_eq!(row.status, SessionStatus::NeedsInput);
    assert_eq!(row.pending_questions, pending_questions());
    assert_eq!(result.supervisions, vec![current]);
}

#[test]
fn supervision_preserves_title_of_sessions_with_their_own_parent() {
    let (mut owner, mut target) = providers();
    let root = session(owner.id, "root");
    let mut thread = session(target.id, "thread");
    thread.parent = Some(session(target.id, "own-parent").id);
    thread.name = Some("Name".into());
    let title = thread.title.clone();
    owner
        .supervisions
        .push(supervision(&root, &thread, SupervisionState::Detached));
    owner.page.push(root.clone());
    target.page.push(thread.clone());
    let result = Hub::new(vec![Box::new(owner), Box::new(target)]).refresh();
    let row = result.sessions.iter().find(|s| s.id == thread.id).unwrap();
    assert_eq!(row.title, title);
    assert_eq!(row.parent, Some(root.id));
    assert_eq!(row.status, SessionStatus::Inactive);
}

#[test]
fn refresh_paths_selects_supervised_descendants_through_parent() {
    let (owner, target) = providers();
    let root = session(owner.id, "root");
    let mut thread = session(target.id, "thread");
    thread.parent = Some(root.id.clone());
    let mut descendant = session(target.id, "descendant");
    descendant.parent = Some(thread.id.clone());
    let expected = HashSet::from([root.id.clone(), thread.id.clone(), descendant.id.clone()]);
    assert_eq!(
        refresh::affected_sessions(
            &[PathBuf::from("state/root/state.json")],
            &[root, thread, descendant]
        ),
        Some(expected)
    );
}

#[test]
fn archive_routes_held_sessions_to_supervisor_and_detached_sessions_to_owner() {
    for state in [
        Some(SupervisionState::Active),
        Some(SupervisionState::Asking(pending_questions())),
        Some(SupervisionState::Detached),
        None,
    ] {
        let (owner, target) = providers();
        let root = session(owner.id, "root");
        let thread = session(target.id, "thread");
        let relation = state
            .clone()
            .map(|state| supervision(&root, &thread, state));
        let owner_actions = owner.actions.clone();
        let target_actions = target.actions.clone();
        let hub = Hub::new(vec![Box::new(target), Box::new(owner)]);
        let job = hub.archive(&thread, relation.as_ref()).unwrap();
        assert!(owner_actions.lock().unwrap().is_empty());
        assert!(target_actions.lock().unwrap().is_empty());
        job().unwrap();
        let held = matches!(
            state,
            Some(SupervisionState::Active | SupervisionState::Asking(_))
        );
        assert_eq!(
            *owner_actions.lock().unwrap(),
            if held { vec!["detach"] } else { vec![] }
        );
        assert_eq!(
            *target_actions.lock().unwrap(),
            if held { vec![] } else { vec!["archive"] }
        );
    }
}
