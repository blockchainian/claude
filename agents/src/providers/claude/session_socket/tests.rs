use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
};

fn registry(dir: &Path, socket: &Path) -> std::path::PathBuf {
    let mut entry: Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/registry.json")).unwrap();
    entry["messagingSocketPath"] = json!(socket);
    fs::create_dir_all(dir.join("sessions")).unwrap();
    let path = dir.join("sessions/10896.json");
    fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    path
}
const ID: &str = "d6e5280c-6bcd-42c4-8bb9-6ff96d1b0744";

#[test]
fn sends_spec_frames_without_waiting_for_replies() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let path = registry(dir.path(), &socket);
    let original = fs::read(&path).unwrap();
    let mut client = Client::connect(dir.path(), 10896, ID).unwrap();
    let name = "中文 \"quote\" $HOME /exit";
    session::set_name(&mut client, name).unwrap();
    for priority in [
        session::Priority::Now,
        session::Priority::Next,
        session::Priority::Later,
    ] {
        session::prompt(&mut client, "hello\n中文", priority).unwrap();
    }
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    for expected in [
        json!({"type":"control","action":"rename","name":name}),
        json!({"type":"user","message":{"role":"user","content":"hello\n中文"},"priority":"now"}),
        json!({"type":"user","message":{"role":"user","content":"hello\n中文"},"priority":"next"}),
        json!({"type":"user","message":{"role":"user","content":"hello\n中文"},"priority":"later"}),
    ] {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'));
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), expected);
    }
    drop(client);
    let mut rest = String::new();
    assert_eq!(reader.read_line(&mut rest).unwrap(), 0);
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn rejects_missing_or_invalid_registry_and_changed_identity() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        Client::connect(dir.path(), 10896, ID).err().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    let socket = dir.path().join("s.sock");
    let path = registry(dir.path(), &socket);
    let entry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for field in ["messagingSocketPath", "pid", "sessionId"] {
        let mut invalid = entry.clone();
        invalid.as_object_mut().unwrap().remove(field);
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert_eq!(
            Client::connect(dir.path(), 10896, ID).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
    fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    assert_eq!(
        Client::connect(dir.path(), 10896, "another-session")
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        Client::connect(dir.path(), 10896, ID)
            .err()
            .unwrap()
            .to_string(),
        "Claude's session socket isn't running"
    );
    let mut invalid = entry.clone();
    invalid["messagingSocketPath"] = json!("");
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert_eq!(
        Client::connect(dir.path(), 10896, ID).err().unwrap().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn rejects_empty_names_and_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    registry(dir.path(), &socket);
    let mut client = Client::connect(dir.path(), 10896, ID).unwrap();
    for name in ["", "  "] {
        assert_eq!(
            session::set_name(&mut client, name).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        session::prompt(&mut client, "", session::Priority::Next)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}
