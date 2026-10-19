pub(super) mod account;
#[cfg(test)]
mod tests;
pub(super) mod thread;
mod transport;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{io, path::Path};
use transport::Transport;

pub(super) struct Client {
    transport: Transport,
    next_id: u64,
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
        };
        let _: InitializeResponse = client.request(
            "initialize",
            InitializeParams {
                capabilities: Capabilities {
                    experimental_api: true,
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
                if response.get("id").is_none() {
                    continue;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unexpected Codex app-server request",
                ));
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
