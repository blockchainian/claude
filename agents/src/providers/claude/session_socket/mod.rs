pub(super) mod session;
#[cfg(test)]
mod tests;
mod transport;

use serde::Serialize;
use std::{fs, io, path::Path};
use transport::Transport;

pub(super) struct Client {
    transport: Transport,
}

impl Client {
    pub(super) fn connect(config_dir: &Path, pid: u32, session_id: &str) -> io::Result<Self> {
        let entry: super::registry::Entry = serde_json::from_slice(&fs::read(
            config_dir.join("sessions").join(format!("{pid}.json")),
        )?)?;
        if entry.pid != Some(pid) || entry.session_id.as_deref() != Some(session_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude session registry changed",
            ));
        }
        let socket = entry
            .messaging_socket_path
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing or invalid Claude messaging socket path",
                )
            })?;
        Ok(Self {
            transport: Transport::connect(&socket)?,
        })
    }

    pub(super) fn send<P: Serialize>(&mut self, frame: P) -> io::Result<()> {
        self.transport
            .send(serde_json::to_string(&frame).map_err(io::Error::other)?)
    }
}
