use crate::providers::codex::*;

#[test]
#[ignore = "requires the running shared Codex daemon and local auth; creates and deletes a real throwaway thread"]
fn rename_real_codex_thread() {
    let provider = CodexProvider::from_config().unwrap();
    // Check before exec: never let exec start a missing daemon for this test.
    let socket = provider
        .home
        .join("app-server-control/app-server-control.sock");
    std::os::unix::net::UnixStream::connect(socket)
        .expect("Codex app-server daemon must already be running");
    let output = Command::new("codex")
        .args([
            "exec",
            "--ignore-user-config",
            "--skip-git-repo-check",
            "--json",
            "Reply OK only. Do not use tools.",
        ])
        .env("CODEX_HOME", &provider.home)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let event: serde_json::Value = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|v| v["type"] == "thread.started")
        .expect("exec must create a thread");
    let id = event["thread_id"].as_str().unwrap().to_owned();
    struct Cleanup(String, PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let status = Command::new("codex")
                .args(["delete", "--force", &self.0])
                .env("CODEX_HOME", &self.1)
                .stdin(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "throwaway cleanup failed");
        }
    }
    let _cleanup = Cleanup(id.clone(), provider.home.clone());
    assert!(output.status.success(), "codex exec failed");
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let session = db
        .query_row("SELECT * FROM threads WHERE id=?1", [&id], |row| {
            super::super::session(row, &HashMap::new())
        })
        .unwrap()
        .unwrap();
    assert!(provider.capabilities().rename);
    provider
        .rename(&session, "  agents-rpc-integration  ")
        .unwrap();
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let name: String = db
        .query_row("SELECT name FROM threads WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(name, "agents-rpc-integration");
    let index = std::fs::read_to_string(provider.home.join("session_index.jsonl")).unwrap();
    let line = index
        .lines()
        .rev()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|v| v["id"] == id)
        .unwrap();
    assert_eq!(line["thread_name"], name);
    let mut client = super::Client::connect(&provider.home).unwrap();
    super::thread::set_name(&mut client, &id, "agents-rpc-integration").unwrap();
    assert_eq!(
        super::thread::set_name(&mut client, &id, "   ")
            .unwrap_err()
            .to_string(),
        "thread name must not be empty"
    );
}
