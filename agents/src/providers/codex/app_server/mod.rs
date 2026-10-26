pub(super) mod account;
#[cfg(test)]
mod tests;
pub(super) mod thread;
mod transport;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};
use transport::Transport;

pub(super) struct Client {
    transport: Transport,
    next_id: u64,
    pending: Vec<ServerRequest>,
}

pub(super) struct ServerRequest {
    pub(super) id: Value,
    pub(super) params: Value,
}

impl ServerRequest {
    fn from_message(message: &Value) -> io::Result<Option<Self>> {
        let Some(method) = message.get("method") else {
            return Ok(None);
        };
        let Some(id) = message.get("id") else {
            return Ok(None);
        };
        if method != "item/tool/requestUserInput" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unexpected Codex app-server request: {method}"),
            ));
        }
        let params = message.get("params").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Missing Codex question params")
        })?;
        for field in ["threadId", "itemId"] {
            if params.get(field).and_then(Value::as_str).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Missing Codex question {field}"),
                ));
            }
        }
        Ok(Some(Self {
            id: id.clone(),
            params: params.clone(),
        }))
    }

    pub(super) fn matches_question(&self, thread_id: &str, item_id: &str) -> bool {
        self.params["threadId"].as_str() == Some(thread_id)
            && self.params["itemId"].as_str() == Some(item_id)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    client_info: ClientInfo,
    capabilities: Capabilities,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    experimental_api: bool,
    opt_out_notification_methods: [&'static str; 4],
}

#[derive(Serialize)]
struct ClientInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Deserialize)]
struct InitializeResponse {
    #[serde(rename = "userAgent")]
    _user_agent: String,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

impl Client {
    pub(super) fn connect(home: &Path) -> io::Result<Self> {
        let mut client = Self {
            transport: Transport::connect(home)?,
            next_id: 1,
            pending: Vec::new(),
        };
        let _: InitializeResponse = client.request(
            "initialize",
            InitializeParams {
                capabilities: Capabilities {
                    experimental_api: true,
                    opt_out_notification_methods: [
                        "item/agentMessage/delta",
                        "item/reasoning/summaryTextDelta",
                        "item/reasoning/summaryPartAdded",
                        "item/reasoning/textDelta",
                    ],
                },
                client_info: ClientInfo {
                    name: "agents",
                    version: env!("CARGO_PKG_VERSION"),
                },
            },
        )?;
        client
            .transport
            .send(json!({"method": "initialized"}).to_string())?;
        Ok(client)
    }

    /// Reads and drops every frame until `deadline`, server requests included, and clears
    /// `pending`. Ok at the deadline; Err when the connection fails.
    pub(super) fn discard_until(&mut self, deadline: Instant) -> io::Result<()> {
        self.pending.clear();
        loop {
            match self.transport.receive_until(deadline) {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(super) fn wait_question(
        &mut self,
        thread_id: &str,
        item_id: &str,
        timeout: Duration,
    ) -> io::Result<Option<ServerRequest>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self
                .pending
                .iter()
                .position(|request| request.matches_question(thread_id, item_id))
            {
                return Ok(Some(self.pending.remove(index)));
            }
            let frame = match self.transport.receive_until(deadline) {
                Ok(frame) => frame,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            let message: Value = serde_json::from_str(&frame).map_err(io::Error::other)?;
            if let Some(request) = ServerRequest::from_message(&message)? {
                self.pending.push(request);
            }
        }
    }

    pub(super) fn respond<R: Serialize>(&mut self, id: &Value, result: R) -> io::Result<()> {
        let response = json!({"id": id, "result": result});
        self.transport
            .send(serde_json::to_string(&response).map_err(io::Error::other)?)
    }

    pub(super) fn request<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: P,
    ) -> io::Result<R> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("JSON-RPC request ids exhausted"))?;
        let params = serde_json::to_value(params).map_err(io::Error::other)?;
        let request = json!({"id":id,"method":method,"params":params});
        self.transport
            .send(serde_json::to_string(&request).map_err(io::Error::other)?)?;
        loop {
            let response: Value =
                serde_json::from_str(&self.transport.receive()?).map_err(io::Error::other)?;
            if response.get("method").is_some() {
                if let Some(request) = ServerRequest::from_message(&response)? {
                    self.pending.push(request);
                }
                continue;
            }
            let response_id = response.get("id").ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Missing JSON-RPC response id")
            })?;
            if response_id != &json!(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                let error: RpcError =
                    serde_json::from_value(error.clone()).map_err(io::Error::other)?;
                return Err(io::Error::other(error.message));
            }
            let result = response.get("result").ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Missing JSON-RPC result")
            })?;
            return serde_json::from_value(result.clone()).map_err(io::Error::other);
        }
    }
}

#[cfg(test)]
mod question_tests {
    use super::*;

    // Captured from Codex 0.163.0, run-fresh/protocol.jsonl, connection B.
    const REPLAY: &str = r#"{"method":"item/tool/requestUserInput","id":1,"params":{"threadId":"01a15740-14d2-7832-926a-4bc46e3f660b","turnId":"01a15740-152f-7543-bc95-b10007089bd5","itemId":"call_42e8d097bb0941b4962b8eaab5651f42","questions":[{"id":"color","header":"Color","question":"Pick a color?","isOther":true,"isSecret":false,"options":[{"label":"Red","description":"Choose Red."},{"label":"Blue","description":"Choose Blue."}]}],"isBlocking":false,"autoResolutionMs":null}}"#;

    #[test]
    fn question_replay_preserves_id_and_matches_rollout_call() {
        let frame: Value = serde_json::from_str(REPLAY).unwrap();
        let request = ServerRequest::from_message(&frame).unwrap().unwrap();
        assert_eq!(request.id, json!(1));
        assert_eq!(request.params, frame["params"]);
        assert!(request.matches_question(
            "01a15740-14d2-7832-926a-4bc46e3f660b",
            "call_42e8d097bb0941b4962b8eaab5651f42"
        ));
        assert!(
            !request.matches_question("another-thread", "call_42e8d097bb0941b4962b8eaab5651f42")
        );
        assert!(!request.matches_question("01a15740-14d2-7832-926a-4bc46e3f660b", "another-call"));
    }

    #[test]
    fn question_replay_requires_request_id() {
        let mut frame: Value = serde_json::from_str(REPLAY).unwrap();
        frame.as_object_mut().unwrap().remove("id");
        assert!(ServerRequest::from_message(&frame).unwrap().is_none());
    }

    #[test]
    fn other_server_requests_are_rejected() {
        let frame =
            json!({"id": 1, "method": "item/commandExecution/requestApproval", "params": {}});
        let error = ServerRequest::from_message(&frame).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
