use super::formatting::format_duration;
use super::*;
use crate::providers::{claude::ClaudeProvider, codex::CodexProvider};
use std::fs::{self, File};
use std::io::Write;
use std::process::{Child, Command};

// Captured from the two await commands and their managed Codex threads in the
// reported Claude session. Paths and user text are scrubbed; timestamps are intact.
const AWAITS: &str = include_str!("../../tests/fixtures/background/two-awaits.jsonl");
const ACTIVITY: &str = include_str!("../../tests/fixtures/background/child-activity.jsonl");

struct Writers(Vec<Child>);
impl Drop for Writers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn background_children_keep_start_order_through_provider_refreshes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let claude = root.join("claude");
    let project = claude.join("projects/project");
    fs::create_dir_all(&project).unwrap();
    let records: Vec<serde_json::Value> = AWAITS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let parent_id = records[0]["sessionId"].as_str().unwrap();
    fs::write(project.join(format!("{parent_id}.jsonl")), AWAITS).unwrap();

    let codex = root.join("codex");
    let locks = codex.join("thread-writer-locks");
    fs::create_dir_all(&locks).unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!(
        "../../tests/fixtures/background/await-children.sql"
    ))
    .unwrap();
    let mut statement = db
        .prepare("SELECT id, name, created_at_ms FROM threads ORDER BY created_at_ms")
        .unwrap();
    let threads: Vec<(String, String, i64)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let manager = root.join("manager");
    fs::create_dir_all(manager.join("threads")).unwrap();
    fs::create_dir_all(manager.join(parent_id)).unwrap();
    fs::write(manager.join(parent_id).join("state.json"), serde_json::json!({
        "threads": threads.iter().map(|(id,name,_)| serde_json::json!({"id":id,"name":name,"detached":false,"waiting":[]})).collect::<Vec<_>>()
    }).to_string()).unwrap();

    // Actual processes keep the writer descriptors and exclusive locks alive.
    let executable = root.join("codex-bg-test");
    fs::copy("/bin/sleep", &executable).unwrap();
    // Relocated platform binaries need a local signature to run on macOS.
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "cannot sign writer process: {}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let mut writers = Writers(Vec::new());
    for (id, _, _) in &threads {
        let path = codex.join(format!("{id}.jsonl"));
        fs::write(&path, "").unwrap();
        db.execute(
            "UPDATE threads SET rollout_path=? WHERE id=?",
            [path.to_str().unwrap(), id],
        )
        .unwrap();
        fs::write(
            manager.join("threads").join(format!("{id}.json")),
            serde_json::json!({"sessionId":parent_id,"pid":std::process::id()}).to_string(),
        )
        .unwrap();
        let writer = File::create(locks.join(format!("{id}.lock"))).unwrap();
        writer.lock().unwrap();
        writers.0.push(
            Command::new(&executable)
                .arg("60")
                .stdout(writer)
                .spawn()
                .unwrap(),
        );
    }
    let mut hub = crate::sessions::Hub::new(vec![
        Box::new(ClaudeProvider::new(claude)),
        Box::new(CodexProvider::new(codex.clone(), manager)),
    ]);
    let mut view = View {
        status_filter: TASK_FILTERS.len() - 1,
        ..View::default()
    };
    let mut orders = Vec::new();
    for (step, line) in ACTIVITY.lines().enumerate() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let id = record["payload"]["thread_id"].as_str().unwrap();
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(codex.join(format!("{id}.jsonl")))
                .unwrap(),
            "{line}"
        )
        .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap())
            .unwrap()
            .with_timezone(&chrono::Utc);
        let snapshot = hub.refresh();
        for (id, _, start) in &threads {
            let child = snapshot.sessions.iter().find(|s| &s.id.id == id).unwrap();
            assert_eq!(child.status, SessionStatus::Working);
            assert_eq!(
                child.started_at,
                chrono::DateTime::from_timestamp_millis(*start)
            );
        }
        view.set_sessions(snapshot.sessions, snapshot.has_more);
        view.selected = view.rows.iter().position(|s| s.id.id == parent_id).unwrap();
        let area = Rect::new(0, 0, 100, 40);
        let mut buffer = Buffer::empty(area);
        view.render_details(area, &mut buffer, now);
        let rows: Vec<String> = buffer
            .content
            .chunks(100)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect();
        let positions: Vec<_> = threads
            .iter()
            .map(|(_, name, start)| {
                // The latest activity above may quote a child's name; children follow the
                // Background label.
                let background = rows
                    .iter()
                    .position(|line| line.trim() == "Background")
                    .unwrap_or_else(|| panic!("missing Background: {}", rows.join("\n")));
                let position = background
                    + rows[background..]
                        .iter()
                        .position(|line| line.replace(' ', "").contains(&name.replace(' ', "")))
                        .unwrap_or_else(|| panic!("missing {name}: {}", rows.join("\n")));
                assert!(
                    rows[position].trim_end().ends_with(&format_duration(
                        (now - chrono::DateTime::from_timestamp_millis(*start).unwrap())
                            .num_seconds()
                    )),
                    "step {step}: {}",
                    rows[position]
                );
                position
            })
            .collect();
        eprintln!(
            "refresh {step}: starts={:?}, rows={positions:?}",
            threads.iter().map(|(_, _, at)| at).collect::<Vec<_>>()
        );
        orders.push(positions);
    }
    assert!(
        orders.iter().all(|positions| positions[0] < positions[1]),
        "Background rows swapped after activity refreshes: {orders:?}"
    );
}
