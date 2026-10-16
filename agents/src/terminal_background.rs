//! Asks the terminal for its default background (OSC 11) so the selection fill suits light and dark themes.

use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;
use std::time::Instant;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_REPLY_BYTES: usize = 4096;
const POLL_STEP: Duration = Duration::from_millis(10);

/// Queries the terminal on `/dev/tty`; the terminal must be in raw mode so the reply is not echoed.
/// A primary device attributes request (DA1) follows the query: every terminal answers it, and
/// in order, so its reply ends the wait whether or not the terminal knows OSC 11. Returns `None`
/// when the terminal reports no background, or answers neither within the timeout.
///
/// The tty is read without blocking: macOS `poll()` reports `/dev/tty` ready even when nothing is
/// pending, so a blocking read would wait for the user's first keystroke and swallow it.
pub fn query(timeout: Duration) -> io::Result<Option<(u8, u8, u8)>> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/tty")?;
    tty.write_all(b"\x1B]11;?\x1B\\\x1B[c")?;
    tty.flush()?;
    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::new();
    loop {
        if let Some(color) = parse_reply(&buffer) {
            return Ok(color);
        }
        if Instant::now() >= deadline || buffer.len() >= MAX_REPLY_BYTES {
            return Ok(parse_osc_background(&buffer));
        }
        let mut chunk = [0_u8; 256];
        match tty.read(&mut chunk) {
            Ok(0) => return Ok(None),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_STEP);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

pub fn is_light((r, g, b): (u8, u8, u8)) -> bool {
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    luma > 128.0
}

/// Finds the `ESC ] 11 ; rgb:RRRR/GGGG/BBBB` reply, terminated by BEL or ST.
/// Returns the reported background once the DA1 reply has arrived, so no reply bytes are left
/// for the input reader; `None` while the reply is incomplete.
pub fn parse_reply(buffer: &[u8]) -> Option<Option<(u8, u8, u8)>> {
    has_device_attributes(buffer).then(|| parse_osc_background(buffer))
}

/// Finds a DA1 reply, `ESC [ ? <digits and semicolons> c`.
fn has_device_attributes(buffer: &[u8]) -> bool {
    let prefix = b"\x1B[?";
    buffer
        .windows(prefix.len())
        .enumerate()
        .filter(|(_, window)| *window == prefix)
        .any(|(start, _)| {
            let rest = &buffer[start + prefix.len()..];
            let end = rest
                .iter()
                .position(|byte| !(byte.is_ascii_digit() || *byte == b';'));
            end.is_some_and(|end| end > 0 && rest[end] == b'c')
        })
}

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
    fn device_attributes_reply_ends_the_wait() {
        let da1 = b"\x1B[?62;22;52c";
        assert_eq!(
            parse_reply(&[&b"\x1B]11;rgb:1111/2222/3333\x1B\\"[..], da1].concat()),
            Some(Some((0x11, 0x22, 0x33)))
        );
        assert_eq!(parse_reply(da1), Some(None));
        assert_eq!(parse_reply(b"\x1B]11;rgb:1111/2222/3333\x07"), None);
        assert_eq!(parse_reply(b"\x1B[?62;22"), None);
        assert_eq!(parse_reply(b"\x1B[?1u"), None);
        assert_eq!(parse_reply(b"\x1B[?1u\x1B[?1;2c"), Some(None));
    }

    #[test]
    fn luminance_splits_light_and_dark() {
        assert!(is_light((255, 255, 255)));
        assert!(is_light((238, 238, 238)));
        assert!(!is_light((0, 0, 0)));
        assert!(!is_light((30, 30, 46)));
    }
}
