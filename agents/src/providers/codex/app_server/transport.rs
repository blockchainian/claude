use std::{
    io,
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket};

fn transport_error(error: tungstenite::Error) -> io::Error {
    match error {
        tungstenite::Error::Io(error) => error,
        other => io::Error::other(other),
    }
}

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
            .map_err(transport_error)
    }

    pub(super) fn receive(&mut self) -> io::Result<String> {
        self.receive_until(Instant::now() + Duration::from_secs(10))
    }

    pub(super) fn receive_until(&mut self, deadline: Instant) -> io::Result<String> {
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "Codex app-server read timed out")
                })?;
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Codex app-server read timed out",
                ));
            }
            self.0.get_ref().set_read_timeout(Some(remaining))?;
            match self.0.read().map_err(transport_error)? {
                Message::Text(text) => return Ok(text.to_string()),
                Message::Ping(_) => self.0.flush().map_err(transport_error)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_io_errors_preserve_timeout_kinds() {
        for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
            let error = transport_error(tungstenite::Error::Io(io::Error::from(kind)));
            assert_eq!(error.kind(), kind);
        }
    }
}
