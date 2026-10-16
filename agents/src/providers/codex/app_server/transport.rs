use std::{io, os::unix::net::UnixStream, path::Path, time::Duration};
use tungstenite::{Message, WebSocket};

pub(super) struct Transport(WebSocket<UnixStream>);

impl Transport {
    pub(super) fn connect(home: &Path) -> io::Result<Self> {
        let socket = home.join("app-server-control/app-server-control.sock");
        let stream = UnixStream::connect(socket).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) {
                io::Error::new(error.kind(), "Codex's app-server daemon isn't running")
            } else {
                error
            }
        })?;
        let timeout = Some(Duration::from_secs(10));
        stream.set_read_timeout(timeout)?;
        stream.set_write_timeout(timeout)?;
        let (websocket, _) =
            tungstenite::client("ws://localhost/rpc", stream).map_err(io::Error::other)?;
        Ok(Self(websocket))
    }

    pub(super) fn send(&mut self, text: String) -> io::Result<()> {
        self.0
            .send(Message::Text(text.into()))
            .map_err(io::Error::other)
    }

    pub(super) fn receive(&mut self) -> io::Result<String> {
        loop {
            match self.0.read().map_err(io::Error::other)? {
                Message::Text(text) => return Ok(text.to_string()),
                Message::Ping(_) => self.0.flush().map_err(io::Error::other)?,
                Message::Pong(_) => {}
                Message::Close(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Codex app-server closed the connection",
                    ));
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Expected a Codex app-server text frame",
                    ));
                }
            }
        }
    }
}
