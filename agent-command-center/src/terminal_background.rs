//! Asks the terminal for its default background (OSC 11) so the selection fill suits light and dark themes.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use std::time::Instant;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_REPLY_BYTES: usize = 4096;

/// Queries the terminal on `/dev/tty`; the terminal must be in raw mode so the reply is not echoed.
/// Returns `None` when the terminal does not answer within the timeout.
pub fn query(timeout: Duration) -> io::Result<Option<(u8, u8, u8)>> {
    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    tty.write_all(b"\x1B]11;?\x1B\\")?;
    tty.flush()?;
    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::new();
    loop {
        if let Some(color) = parse_osc_background(&buffer) {
            return Ok(Some(color));
        }
        let now = Instant::now();
        if now >= deadline || buffer.len() >= MAX_REPLY_BYTES {
            return Ok(None);
        }
        if !poll_readable(&tty, deadline - now)? {
            return Ok(None);
        }
        let mut chunk = [0_u8; 256];
        let read = tty.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn poll_readable(tty: &File, timeout: Duration) -> io::Result<bool> {
    let mut fds = libc::pollfd {
        fd: tty.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
    let ready = unsafe { libc::poll(&mut fds, 1, millis) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(true);
        }
        return Err(error);
    }
    Ok(ready > 0)
}

pub fn is_light((r, g, b): (u8, u8, u8)) -> bool {
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    luma > 128.0
}

/// Finds the `ESC ] 11 ; rgb:RRRR/GGGG/BBBB` reply, terminated by BEL or ST.
pub fn parse_osc_background(buffer: &[u8]) -> Option<(u8, u8, u8)> {
    let prefix = b"\x1B]11;";
    let start = buffer
        .windows(prefix.len())
        .position(|window| window == prefix)?;
    let rest = &buffer[start + prefix.len()..];
    let end = (0..rest.len()).find(|&index| {
        rest[index] == 0x07 || (rest[index] == 0x1B && rest.get(index + 1) == Some(&b'\\'))
    })?;
    parse_osc_rgb(std::str::from_utf8(&rest[..end]).ok()?)
}

fn parse_osc_rgb(payload: &str) -> Option<(u8, u8, u8)> {
    let (prefix, values) = payload.trim().split_once(':')?;
    if !prefix.eq_ignore_ascii_case("rgb") && !prefix.eq_ignore_ascii_case("rgba") {
        return None;
    }
    let mut parts = values.split('/');
    let r = parse_component(parts.next()?)?;
    let g = parse_component(parts.next()?)?;
    let b = parse_component(parts.next()?)?;
    if prefix.eq_ignore_ascii_case("rgba") {
        parse_component(parts.next()?)?;
    }
    parts.next().is_none().then_some((r, g, b))
}

/// Components carry 1 to 4 hex digits; scale them to 8 bits.
fn parse_component(component: &str) -> Option<u8> {
    if !(1..=4).contains(&component.len()) {
        return None;
    }
    let value = u32::from(u16::from_str_radix(component, 16).ok()?);
    let maximum = (1_u32 << (component.len() * 4)) - 1;
    Some((value * u32::from(u8::MAX) / maximum) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_st_and_bel_terminated_replies() {
        assert_eq!(
            parse_osc_background(b"\x1B]11;rgb:ffff/ffff/ffff\x1B\\"),
            Some((255, 255, 255))
        );
        assert_eq!(
            parse_osc_background(b"\x1B[?1u\x1B]11;rgb:11/22/33\x07"),
            Some((0x11, 0x22, 0x33))
        );
        assert_eq!(parse_osc_background(b"\x1B]11;rgb:ffff/ffff"), None);
        assert_eq!(parse_osc_background(b"\x1B]10;rgb:0/0/0\x07"), None);
    }

    #[test]
    fn luminance_splits_light_and_dark() {
        assert!(is_light((255, 255, 255)));
        assert!(is_light((238, 238, 238)));
        assert!(!is_light((0, 0, 0)));
        assert!(!is_light((30, 30, 46)));
    }
}
