use std::{
    io::{self, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

pub(super) struct Transport(UnixStream);

impl Transport {
    pub(super) fn connect(socket: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) {
                io::Error::new(error.kind(), "Claude's session socket isn't running")
            } else {
                error
            }
        })?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(Self(stream))
    }

    pub(super) fn send(&mut self, mut text: String) -> io::Result<()> {
        text.push('\n');
        self.0.write_all(text.as_bytes())
    }
}
