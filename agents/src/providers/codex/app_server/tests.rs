use crate::providers::codex::*;
use serde_json::{Value, json};
use std::{
    os::unix::net::{UnixListener, UnixStream},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket};

fn read_rpc(socket: &mut WebSocket<UnixStream>) -> Value {
    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
}

fn send_rpc(socket: &mut WebSocket<UnixStream>, message: Value) {
    socket
        .send(Message::Text(message.to_string().into()))
        .unwrap();
}

fn with_rpc_server(
    server: impl FnOnce(WebSocket<UnixStream>) + Send + 'static,
) -> (tempfile::TempDir, std::thread::JoinHandle<()>) {
    let home = tempfile::tempdir().unwrap();
    let control = home.path().join("app-server-control");
    std::fs::create_dir(&control).unwrap();
    let listener = UnixListener::bind(control.join("app-server-control.sock")).unwrap();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        server(tungstenite::accept(stream).unwrap());
    });
    (home, handle)
}

fn initialize_rpc(socket: &mut WebSocket<UnixStream>) -> Value {
    let request = read_rpc(socket);
    assert_eq!(request["method"], "initialize");
    send_rpc(
        socket,
        json!({"id": request["id"], "result": {"userAgent": "codex"}}),
    );
    assert_eq!(read_rpc(socket), json!({"method": "initialized"}));
    request
}

#[test]
fn initialize_opts_out_of_delta_notifications() {
    let (home, server) = with_rpc_server(|mut socket| {
        let request = initialize_rpc(&mut socket);
        assert_eq!(
            request["params"]["capabilities"],
            json!({
                "experimentalApi": true,
                "optOutNotificationMethods": [
                    "item/agentMessage/delta",
                    "item/reasoning/summaryTextDelta",
                    "item/reasoning/summaryPartAdded",
                    "item/reasoning/textDelta"
                ]
            })
        );
    });
    let _client = super::Client::connect(home.path()).unwrap();
    server.join().unwrap();
}

#[test]
fn discard_until_drops_frames_and_pending_requests_until_deadline() {
    let (release, hold) = std::sync::mpsc::channel();
    let (home, server) = with_rpc_server(move |mut socket| {
        initialize_rpc(&mut socket);
        send_rpc(
            &mut socket,
            json!({"method": "thread/started", "params": {}}),
        );
        send_rpc(
            &mut socket,
            json!({"id": 42, "method": "unknown/serverRequest", "params": {}}),
        );
        // A subsequent request proves all preceding frames were consumed.
        let request = read_rpc(&mut socket);
        send_rpc(&mut socket, json!({"id": request["id"], "result": {}}));
        hold.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let mut client = super::Client::connect(home.path()).unwrap();
    client.pending.push(super::ServerRequest {
        id: json!(1),
        params: json!({"threadId": "thread", "itemId": "item"}),
    });
    let deadline = Instant::now() + Duration::from_millis(50);
    client.discard_until(deadline).unwrap();
    assert!(Instant::now() >= deadline);
    assert!(client.pending.is_empty());
    let _: Value = client.request("thread/loaded/list", json!({})).unwrap();
    client.discard_until(Instant::now()).unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn discard_until_reports_closed_connection() {
    let (home, server) = with_rpc_server(|mut socket| {
        initialize_rpc(&mut socket);
        socket.close(None).unwrap();
    });
    let mut client = super::Client::connect(home.path()).unwrap();
    assert!(
        client
            .discard_until(Instant::now() + Duration::from_secs(1))
            .is_err()
    );
    server.join().unwrap();
}

#[test]
fn unsubscribe_sends_thread_id() {
    let (home, server) = with_rpc_server(|mut socket| {
        initialize_rpc(&mut socket);
        let request = read_rpc(&mut socket);
        assert_eq!(
            request,
            json!({"id": 2, "method": "thread/unsubscribe", "params": {"threadId": "thread-123"}})
        );
        send_rpc(&mut socket, json!({"id": request["id"], "result": {}}));
    });
    let mut client = super::Client::connect(home.path()).unwrap();
    super::thread::unsubscribe(&mut client, "thread-123").unwrap();
    server.join().unwrap();
}

#[test]
fn actions_remove_thread_records_only_after_success() {
    const CHILD: &str = "AGENTS_THREAD_RECORD_ACTION_TEST";
    if std::env::var_os(CHILD).is_none() {
        let state = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "providers::codex::app_server::tests::actions_remove_thread_records_only_after_success", "--nocapture"])
            .env(CHILD, "1")
            .env("CODEX_MANAGER_STATE_DIR", state.path())
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let state = supervision::state_dir().unwrap();
    std::fs::create_dir_all(state.join("threads")).unwrap();
    for action in ["archive", "delete", "terminal"] {
        for succeeds in [false, true] {
            let record = state.join("threads/thread-0.json");
            std::fs::write(&record, "{}").unwrap();
            let (home, server) = with_rpc_server(move |mut socket| {
                initialize_rpc(&mut socket);
                if action == "terminal" {
                    let request = read_rpc(&mut socket);
                    assert_eq!(request["method"], "thread/turns/list");
                    send_rpc(
                        &mut socket,
                        json!({"id": request["id"], "result": {"data": []}}),
                    );
                }
                let request = read_rpc(&mut socket);
                assert_eq!(
                    request["method"],
                    if action == "delete" {
                        "thread/delete"
                    } else {
                        "thread/archive"
                    }
                );
                assert_eq!(request["params"]["threadId"], "thread-0");
                send_rpc(
                    &mut socket,
                    if succeeds {
                        json!({"id": request["id"], "result": {}})
                    } else {
                        json!({"id": request["id"], "error": {"message": "action failed"}})
                    },
                );
            });
            let (_fixture, source) = crate::providers::codex::tests::fixture();
            let mut session = source
                .snapshot(2)
                .unwrap()
                .sessions
                .into_iter()
                .find(|session| session.id.id == "thread-0")
                .unwrap();
            let provider = CodexProvider::new(home.path().into());
            let job = match action {
                "archive" => provider.archive(&session).unwrap(),
                "delete" => provider.delete(&session).unwrap(),
                "terminal" => {
                    let path = home.path().join("rollout.jsonl");
                    std::fs::write(
                        &path,
                        include_str!("../../../../tests/fixtures/codex-thread-clock.jsonl"),
                    )
                    .unwrap();
                    session.transcript = Some(path);
                    provider.terminal_cleanup(&session).unwrap().unwrap()
                }
                _ => unreachable!(),
            };
            let result = job();
            assert_eq!(
                result.is_ok(),
                succeeds,
                "{action}, succeeds={succeeds}: {result:?}"
            );
            server.join().unwrap();
            assert_eq!(record.exists(), !succeeds, "{action}, succeeds={succeeds}");
        }
    }
}
