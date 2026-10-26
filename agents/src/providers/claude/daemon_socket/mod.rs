//! Queues human input through Claude's authenticated daemon control socket.

use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt, net::UnixStream},
    },
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_FRAME: usize = 64 * 1024;

pub(super) struct Client {
    config_dir: PathBuf,
}

impl Client {
    pub(super) fn new(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_owned(),
        }
    }

    pub(super) fn enqueue(&self, job_id: &str, text: &str) -> io::Result<()> {
        let deadline = Instant::now() + TIMEOUT;
        let prepare = || -> io::Result<(UnixStream, Vec<u8>, String)> {
            validate_input(job_id, text)?;
            let config_dir = absolute_config_dir(&self.config_dir)?;
            let socket = socket_path(&config_dir)?;
            let key = read_key(&config_dir)?;
            let request = reply_frame(job_id, text, &key)?;
            let mut ping = connect(&socket, deadline)?;
            write_frame(&mut ping, b"{\"proto\":1,\"op\":\"ping\"}\n", deadline)?;
            validate_ping(&read_frame(&mut ping, deadline)?)?;
            drop(ping);
            let stream = connect(&socket, deadline)?;
            remaining(deadline)?;
            Ok((stream, request, key))
        };
        let (mut stream, request, key) = prepare().map_err(|error| {
            io::Error::new(error.kind(), format!("Claude input not sent: {error}"))
        })?;
        // Once writing starts, a transport failure cannot establish non-delivery.
        let result = write_frame(&mut stream, &request, deadline)
            .and_then(|()| read_frame(&mut stream, deadline))
            .map_err(unknown)
            .and_then(|frame| validate_reply(&frame));
        result.map_err(|error| {
            io::Error::new(error.kind(), error.to_string().replace(&key, "<redacted>"))
        })
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn unknown(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!(
        "Delivery unconfirmed; check the session before resending. {error}"
    ))
}

fn absolute_config_dir(config_dir: &Path) -> io::Result<PathBuf> {
    if config_dir.as_os_str().is_empty() || config_dir.to_str().is_none() {
        return Err(invalid("Claude config directory must be nonempty UTF-8"));
    }
    let absolute = if config_dir.is_absolute() {
        config_dir.to_owned()
    } else {
        std::env::current_dir()?.join(config_dir)
    };
    if absolute.to_str().is_none() {
        return Err(invalid("Claude config directory must be UTF-8"));
    }
    let mut normalized = PathBuf::from("/");
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::CurDir => {}
            Component::Prefix(_) => return Err(invalid("Unsupported Claude config directory")),
        }
    }
    Ok(normalized)
}

fn socket_path(config_dir: &Path) -> io::Result<PathBuf> {
    let directory = absolute_config_dir(config_dir)?;
    let hash = Sha256::digest(directory.as_os_str().as_bytes());
    let namespace = format!(
        "{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3]
    );
    // Claude's macOS daemon uses /tmp regardless of TMPDIR.
    let uid = unsafe { libc::getuid() };
    Ok(PathBuf::from(format!(
        "/tmp/cc-daemon-{uid}/{namespace}/control.sock"
    )))
}

fn read_key(config_dir: &Path) -> io::Result<String> {
    // Nonblocking open allows rejecting a FIFO without waiting for a writer.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(config_dir.join("daemon/control.key"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(invalid(
            "Claude control key must be a regular file of at most 4096 bytes",
        ));
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid("Claude control key exceeds 4096 bytes"));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("Claude control key is not UTF-8"))?;
    let key = text.trim();
    if key.is_empty() {
        return Err(invalid("Claude control key is empty"));
    }
    Ok(key.to_owned())
}

fn validate_input(job_id: &str, text: &str) -> io::Result<()> {
    super::attach::validate_job_id(job_id)?;
    super::attach::validate_text(text)?;
    if text.is_empty() {
        return Err(invalid("Empty Claude input"));
    }
    Ok(())
}

fn reply_frame(job_id: &str, text: &str, key: &str) -> io::Result<Vec<u8>> {
    validate_input(job_id, text)?;
    let mut frame = serde_json::to_vec(&serde_json::json!({
        "proto": 1, "op": "reply", "short": job_id, "text": text,
        "auth": key, "nextTurn": true,
    }))?;
    frame.push(b'\n');
    Ok(frame)
}

fn parse_frame(frame: &[u8]) -> io::Result<serde_json::Value> {
    if frame.len() > MAX_FRAME || frame.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Incomplete or oversized daemon frame",
        ));
    }
    if frame[..frame.len() - 1].contains(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Multiple daemon frames",
        ));
    }
    serde_json::from_slice(frame).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn validate_ping(frame: &[u8]) -> io::Result<()> {
    let value = parse_frame(frame)?;
    if value["ok"] != true || value["op"] != "ping" || value["proto"].as_u64() != Some(1) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid Claude daemon ping handshake",
        ));
    }
    if value["version"] != "2.1.309" {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported Claude daemon version; expected 2.1.309",
        ));
    }
    Ok(())
}

fn validate_reply(frame: &[u8]) -> io::Result<()> {
    let value = parse_frame(frame).map_err(unknown)?;
    match value["ok"].as_bool() {
        Some(true) if value["op"] == "reply" => Ok(()),
        Some(false) if value.get("op").is_none() || value["op"] == "reply" => {
            let code = value["code"].as_str().filter(|code| !code.is_empty());
            let reason = value["error"].as_str().filter(|reason| !reason.is_empty());
            match (code, reason) {
                (Some(code), Some(reason)) => Err(io::Error::other(format!(
                    "Claude daemon rejected input ({code}): {reason}"
                ))),
                _ => Err(unknown("Malformed Claude daemon rejection")),
            }
        }
        _ => Err(unknown("Invalid Claude daemon reply acknowledgement")),
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Claude daemon deadline exceeded"))
}

fn wait(stream: &UnixStream, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    loop {
        let timeout = remaining(deadline)?
            .as_millis()
            .max(1)
            .min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result > 0 {
            remaining(deadline)?;
            return Ok(());
        }
        if result == 0 {
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn connect(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
    remaining(deadline)?;
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(invalid("Invalid Claude daemon socket path"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in address.sun_path.iter_mut().zip(bytes) {
        *target = *byte as libc::c_char;
    }
    let length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = UnixStream::from(owned);
    stream.set_nonblocking(true)?;
    remaining(deadline)?;
    let result =
        unsafe { libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), length) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS | libc::EALREADY | libc::EINTR)
        ) {
            return Err(error);
        }
        wait(&stream, libc::POLLOUT, deadline)?;
        if let Some(error) = stream.take_error()? {
            return Err(error);
        }
    }
    remaining(deadline)?;
    Ok(stream)
}

fn write_frame(stream: &mut UnixStream, mut frame: &[u8], deadline: Instant) -> io::Result<()> {
    while !frame.is_empty() {
        remaining(deadline)?;
        match stream.write(frame) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => frame = &frame[written..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait(stream, libc::POLLOUT, deadline)?
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    remaining(deadline)?;
    Ok(())
}

fn read_frame(stream: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        remaining(deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Daemon closed before newline",
                ));
            }
            Ok(count) => {
                let end = buffer[..count]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|index| index + 1);
                frame.extend_from_slice(&buffer[..end.unwrap_or(count)]);
                if frame.len() > MAX_FRAME {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Daemon frame exceeds 64 KiB",
                    ));
                }
                if end.is_some() {
                    remaining(deadline)?;
                    return Ok(frame);
                }
                if frame.len() == MAX_FRAME {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Daemon frame lacks newline within 64 KiB",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait(stream, libc::POLLIN, deadline)?
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
