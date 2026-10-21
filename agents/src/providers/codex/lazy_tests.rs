use super::*;
use crate::{model::Grouping, providers::claude::ClaudeProvider, sessions::Hub, ui::View};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{buffer::Buffer, layout::Rect};
use std::{
    fs,
    time::{Duration, SystemTime},
};

const LIVE_PARENT: &str = "00000000-0000-0000-0000-000000000001";
const STOPPED_PARENT: &str = "00000000-0000-0000-0000-000000000002";

struct Homes {
    root: tempfile::TempDir,
    started_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Homes {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("claude");
        let project = claude.join("projects/project");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(claude.join("sessions")).unwrap();
        let transcript = include_bytes!("../../../tests/fixtures/transcript.jsonl");
        for index in 1..=13 {
            let id = format!("00000000-0000-0000-0000-{index:012}");
            let path = project.join(format!("{id}.jsonl"));
            fs::write(&path, transcript).unwrap();
            fs::File::open(path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(
                    SystemTime::UNIX_EPOCH
                        + Duration::from_secs(1_791_911_000 + if index == 2 { 100 } else { index }),
                ))
                .unwrap();
        }
        let pid = std::process::id();
        let started_at = crate::processes::start_time(pid)
            .expect("libproc must read the actual test process start time");
        let updated_at = Utc::now() - chrono::Duration::hours(2) - chrono::Duration::minutes(1);
        let mut registry: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/registry.json")).unwrap();
        registry["sessionId"] = LIVE_PARENT.into();
        registry["pid"] = pid.into();
        registry["procStart"] = started_at.format("%a %b %e %H:%M:%S %Y").to_string().into();
        registry["startedAt"] = started_at.timestamp_millis().into();
        registry["updatedAt"] = updated_at.timestamp_millis().into();
        registry["name"] = "Live integration parent".into();
        registry["cwd"] = "/workspace/project".into();
        registry["status"] = "busy".into();
        fs::write(
            claude.join("sessions/live.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();

        let db = Connection::open(root.path().join("state_5.sqlite")).unwrap();
        db.execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1600)
            INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,model)
            SELECT 'history-'||x,rollout_path,created_at,1,source,model_provider,cwd,title,sandbox_policy,approval_mode,X'FF' FROM threads,n WHERE id='thread-0';
            INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode)
            SELECT 'native-grandchild',rollout_path,created_at,2,source,model_provider,cwd,'Native grandchild',sandbox_policy,approval_mode FROM threads WHERE id='thread-13';
            INSERT INTO thread_spawn_edges VALUES ('thread-13','native-grandchild','closed');
            INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode)
            SELECT 'detached-child',rollout_path,created_at,3,source,model_provider,cwd,'Detached child',sandbox_policy,approval_mode FROM threads WHERE id='thread-12';
            INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,updated_at_ms)
            SELECT 'recent-orphan',rollout_path,created_at,updated_at,source,model_provider,cwd,'Recent orphan',sandbox_policy,approval_mode,9999999999999 FROM threads WHERE id='thread-0';
            INSERT INTO thread_spawn_edges VALUES ('missing-native-parent','recent-orphan','closed');
            INSERT INTO thread_spawn_edges VALUES ('recent-orphan','history-2','closed');").unwrap();
        // Rollout paths are absolute and absent, so fixtures cannot read the worktree's files.
        db.execute(
            "UPDATE threads SET rollout_path=?",
            [root.path().join("missing-rollout.jsonl").to_str().unwrap()],
        )
        .unwrap();
        let manager = root.path().join("manager");
        fs::create_dir_all(manager.join("threads")).unwrap();
        let mut state: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/attached-state.json"
        ))
        .unwrap();
        let detached: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/detached-state.json"
        ))
        .unwrap();
        let mut child = detached["threads"][0].clone();
        child["id"] = "detached-child".into();
        state["threads"].as_array_mut().unwrap().push(child);
        Self::write_parent(&manager, LIVE_PARENT, &state);
        let mut stopped = state.clone();
        stopped["threads"] = serde_json::json!([state["threads"][0].clone()]);
        stopped["threads"][0]["id"] = "history-1".into();
        Self::write_parent(&manager, STOPPED_PARENT, &stopped);
        // A wholly unrelated damaged registry must never be opened by refresh.
        fs::write(
            manager.join("threads/unrelated.json"),
            b"invalid unrelated registry",
        )
        .unwrap();
        Self {
            root,
            started_at,
            updated_at,
        }
    }

    fn write_parent(home: &Path, parent: &str, state: &serde_json::Value) {
        fs::create_dir_all(home.join(parent)).unwrap();
        fs::write(
            home.join(parent).join("state.json"),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
        let mut supervisor: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/attached-supervisor.json"
        ))
        .unwrap();
        supervisor["sessionId"] = parent.into();
        for child in state["threads"].as_array().unwrap() {
            let id = child["id"].as_str().unwrap();
            fs::write(
                home.join("threads").join(format!("{id}.json")),
                serde_json::to_vec(&supervisor).unwrap(),
            )
            .unwrap();
        }
    }

    fn hub(&self, reverse: bool) -> Hub {
        let mut providers: Vec<Box<dyn Provider>> = vec![
            Box::new(ClaudeProvider::new(self.root.path().join("claude"))),
            Box::new(CodexProvider::new(
                self.root.path().into(),
                self.root.path().join("manager"),
            )),
        ];
        if reverse {
            providers.reverse();
        }
        Hub::new(providers)
    }
}

#[test]
fn live_claude_descendants_are_recursive_bounded_and_independent_of_provider_order() {
    let homes = Homes::new();
    let expected_codex: HashSet<_> = (0..9)
        .map(|n| format!("thread-{n}"))
        .chain(
            [
                "recent-orphan",
                "thread-12",
                "thread-13",
                "native-grandchild",
                "detached-child",
            ]
            .map(str::to_owned),
        )
        .collect();
    let mut snapshots = Vec::new();
    for reverse in [false, true] {
        let snapshot = homes.hub(reverse).refresh();
        assert!(snapshot.has_more);
        let codex: HashSet<_> = snapshot
            .sessions
            .iter()
            .filter(|s| s.id.provider == ProviderId::Codex)
            .map(|s| s.id.id.clone())
            .collect();
        assert_eq!(
            codex, expected_codex,
            "only the history budget and live descendants are decoded"
        );
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .filter(|s| s.id.provider == ProviderId::Claude && s.process.is_none())
                .count(),
            PAGE_SIZE
        );
        let parent = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == LIVE_PARENT)
            .unwrap();
        assert_eq!(parent.status, SessionStatus::Working);
        assert_eq!(parent.process.as_ref().unwrap().pid, std::process::id());
        assert_eq!(
            parent
                .process
                .as_ref()
                .unwrap()
                .started_at
                .unwrap()
                .timestamp_millis(),
            homes.started_at.timestamp_millis()
        );
        assert_eq!(
            parent.updated_at.timestamp_millis(),
            homes.updated_at.timestamp_millis()
        );
        for (id, parent_id) in [
            ("thread-12", LIVE_PARENT),
            ("detached-child", LIVE_PARENT),
            ("thread-13", "thread-12"),
            ("native-grandchild", "thread-13"),
        ] {
            let child = snapshot.sessions.iter().find(|s| s.id.id == id).unwrap();
            assert_eq!(child.parent.as_ref().unwrap().id, parent_id);
            assert_eq!(child.status, SessionStatus::Inactive);
        }
        assert!(
            snapshot
                .sessions
                .iter()
                .find(|s| s.id.id == "recent-orphan")
                .unwrap()
                .parent
                .is_none()
        );
        snapshots.push(snapshot.sessions);
    }
    assert_eq!(snapshots[0], snapshots[1]);
}

fn screen(view: &View) -> Vec<String> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 300, 55));
    view.render(buffer.area, &mut buffer);
    buffer
        .content
        .chunks(300)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

#[test]
fn inactive_ui_keeps_real_live_parent_with_finished_attached_and_detached_descendants() {
    let homes = Homes::new();
    let mut hub = homes.hub(false);
    let snapshot = hub.refresh();
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(hub.capabilities());
    view.set_sessions(snapshot.sessions, snapshot.has_more);
    let initial = screen(&view);
    let (row, column) = initial
        .iter()
        .enumerate()
        .find_map(|(row, text)| text.find("Inactive").map(|column| (row, column)))
        .unwrap();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        view.handle_mouse(MouseEvent {
            kind,
            column: column as u16,
            row: row as u16,
            modifiers: KeyModifiers::NONE,
        });
    }
    let lines = screen(&view);
    let output = lines.join("\n");
    let parent = lines
        .iter()
        .position(|line| line.contains("Live integration parent"))
        .expect(&output);
    assert!(lines[parent].contains("Working"), "{output}");
    assert!(
        lines[parent].contains("2h ago"),
        "the live parent's registry timestamp is shown: {output}"
    );
    for title in [
        "Managed worker",
        "Detached child",
        "Example task 13",
        "Native grandchild",
    ] {
        let row = lines
            .iter()
            .position(|line| line.contains(title))
            .unwrap_or_else(|| panic!("missing {title}: {output}"));
        assert!(row > parent, "{output}");
        assert!(
            lines[row].contains('├') || lines[row].contains('└'),
            "nested {title}: {output}"
        );
    }
    let orphan = lines
        .iter()
        .find(|line| line.contains("Recent orphan"))
        .expect(&output);
    assert!(
        !orphan.contains('├') && !orphan.contains('└'),
        "orphan is flat: {output}"
    );
}
