use super::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;

#[test]
fn rename_uses_session_socket_without_modifying_claude_files() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut entry = registry::tests::fixture(dir.path());
    entry["messagingSocketPath"] = json!(socket);
    let id = entry["sessionId"].as_str().unwrap();
    let transcript = tests::transcript(dir.path(), id, 0);
    let registry = dir
        .path()
        .join(format!("sessions/{}.json", std::process::id()));
    let bytes = serde_json::to_vec(&entry).unwrap();
    fs::write(&registry, &bytes).unwrap();
    let original = fs::read(&transcript).unwrap();
    let mut provider = ClaudeProvider::new(dir.path().into());
    let session = provider.refresh(1).sessions.remove(0);
    let name = "中文 \"quote\" $HOME /exit";
    provider
        .send(&session, SessionInput::Rename(name.into()))
        .unwrap()()
    .unwrap();
    let (stream, _) = listener.accept().unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    assert!(line.ends_with('\n'));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"type":"control","action":"rename","name":name})
    );
    assert_eq!(fs::read(registry).unwrap(), bytes);
    assert_eq!(fs::read(transcript).unwrap(), original);
}
