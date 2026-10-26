use super::*;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    os::unix::net::{UnixListener, UnixStream},
    thread,
    time::Instant,
};
use tungstenite::{Message, WebSocket};

fn read(socket: &mut WebSocket<UnixStream>) -> Value {
    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
}

fn reply(socket: &mut WebSocket<UnixStream>, request: &Value, result: Value) {
    socket
        .send(Message::Text(
            json!({"id": request["id"], "result": result})
                .to_string()
                .into(),
        ))
        .unwrap();
}

fn accept(listener: &UnixListener) -> WebSocket<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "subscriber did not connect");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut socket = tungstenite::accept(stream).unwrap();
    let request = read(&mut socket);
    assert_eq!(request["method"], "initialize");
    reply(&mut socket, &request, json!({"userAgent": "codex"}));
    assert_eq!(read(&mut socket)["method"], "initialized");
    socket
}

fn fixture() -> (tempfile::TempDir, PathBuf, UnixListener) {
    let home = tempfile::tempdir().unwrap();
    Connection::open(home.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let state = home.path().join("manager");
    fs::create_dir_all(state.join("threads")).unwrap();
    let control = home.path().join("app-server-control");
    fs::create_dir(&control).unwrap();
    let listener = UnixListener::bind(control.join("app-server-control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    (home, state, listener)
}

fn resume(socket: &mut WebSocket<UnixStream>) -> String {
    let request = read(socket);
    assert_eq!(request["method"], "thread/resume");
    assert_eq!(request["params"]["excludeTurns"], true);
    let id = request["params"]["threadId"].as_str().unwrap().to_owned();
    reply(socket, &request, json!({"thread": {"id": id}}));
    id
}

#[test]
fn newly_created_record_is_resumed() {
    let (home, state, listener) = fixture();
    let records = state.join("threads");
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        // Add a record only after an earlier pass has resumed another thread.
        assert_eq!(resume(&mut socket), "thread-0");
        fs::write(records.join("thread-1.json"), "{}").unwrap();
        assert_eq!(resume(&mut socket), "thread-1");
    });
    fs::write(state.join("threads/thread-0.json"), "{}").unwrap();
    let _subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    server.join().unwrap();
}

#[test]
fn removed_record_is_unsubscribed() {
    let (home, state, listener) = fixture();
    let record = state.join("threads/thread-0.json");
    fs::write(&record, "{}").unwrap();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        assert_eq!(resume(&mut socket), "thread-0");
        fs::remove_file(record).unwrap();
        let request = read(&mut socket);
        assert_eq!(request["method"], "thread/unsubscribe");
        assert_eq!(request["params"]["threadId"], "thread-0");
        reply(&mut socket, &request, json!({}));
    });
    let _subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    server.join().unwrap();
}

#[test]
fn reconnect_resumes_every_record_again() {
    let (home, state, listener) = fixture();
    for id in ["thread-0", "thread-1"] {
        fs::write(state.join("threads").join(format!("{id}.json")), "{}").unwrap();
    }
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let mut socket = accept(&listener);
            let ids = HashSet::from([resume(&mut socket), resume(&mut socket)]);
            assert_eq!(ids, HashSet::from(["thread-0".into(), "thread-1".into()]));
            socket.close(None).unwrap();
        }
    });
    let _subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    server.join().unwrap();
}

#[test]
fn archived_and_missing_threads_lose_their_records() {
    let (home, state, listener) = fixture();
    let db = Connection::open(home.path().join("state_5.sqlite")).unwrap();
    db.execute("UPDATE threads SET archived=1 WHERE id='thread-0'", [])
        .unwrap();
    let records = state.join("threads");
    for id in ["thread-0", "missing", "thread-1"] {
        fs::write(records.join(format!("{id}.json")), "{}").unwrap();
    }
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        assert_eq!(resume(&mut socket), "thread-1");
        assert!(!records.join("thread-0.json").exists());
        assert!(!records.join("missing.json").exists());
        assert!(records.join("thread-1.json").exists());
    });
    let _subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    server.join().unwrap();
}

#[test]
fn failed_resume_retries_without_resuming_successful_threads() {
    let (home, state, listener) = fixture();
    for id in ["thread-0", "thread-1"] {
        fs::write(state.join("threads").join(format!("{id}.json")), "{}").unwrap();
    }
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        let failed = read(&mut socket);
        assert_eq!(failed["method"], "thread/resume");
        socket
            .send(Message::Text(
                json!({"id": failed["id"], "error": {"message": "retry"}})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let succeeded = resume(&mut socket);
        assert_ne!(failed["params"]["threadId"], succeeded);
        assert_eq!(resume(&mut socket), failed["params"]["threadId"]);
        // The successful thread must not be resumed again in the following pass.
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        assert!(socket.read().is_err());
    });
    let _subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    server.join().unwrap();
}

#[test]
fn drop_closes_the_subscription_connection_at_next_wake() {
    let (home, state, listener) = fixture();
    fs::write(state.join("threads/thread-0.json"), "{}").unwrap();
    let (ready, resumed) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener);
        assert_eq!(resume(&mut socket), "thread-0");
        ready.send(()).unwrap();
        let error = socket.read().unwrap_err();
        match error {
            tungstenite::Error::Protocol(
                tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
            ) => {}
            tungstenite::Error::Io(error) => assert!(matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            )),
            other => panic!("subscription did not close: {other}"),
        }
    });
    let subscriptions =
        UnsupervisedThreads::start(home.path().into(), state, Duration::from_millis(30)).unwrap();
    resumed.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(subscriptions);
    server.join().unwrap();
}
