//! Queries terminal capabilities and sends image commands, including tmux passthrough.

use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;
use std::time::Instant;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_REPLY_BYTES: usize = 4096;
const POLL_STEP: Duration = Duration::from_millis(10);

/// Capabilities reported by the terminal when the dashboard enters it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub background: Option<(u8, u8, u8)>,
    pub supports_images: bool,
    pub cell_pixels: Option<(u16, u16)>,
}

/// Queries `/dev/tty` in raw mode; DA1 ends the wait even when OSC 11 or kitty is unsupported.
///
/// The tty is read without blocking: macOS `poll()` reports `/dev/tty` ready even when nothing is
/// pending, so a blocking read would wait for the user's first keystroke and swallow it.
pub fn query(timeout: Duration) -> io::Result<Report> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/tty")?;
    let cell_pixels = read_cell_pixels(&tty)?;
    write_queries(&mut tty, in_tmux())?;
    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::new();
    let mut report = loop {
        if let Some(report) = parse_reply(&buffer) {
            break report;
        }
        if Instant::now() >= deadline || buffer.len() >= MAX_REPLY_BYTES {
            break parse_report(&buffer);
        }
        let mut chunk = [0_u8; 256];
        match tty.read(&mut chunk) {
            Ok(0) => break parse_report(&buffer),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL_STEP);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    };
    report.cell_pixels = cell_pixels;
    Ok(report)
}

fn write_queries(out: &mut impl Write, tmux: bool) -> io::Result<()> {
    // tmux answers OSC 11 itself but consumes the outer terminal's OSC 11 reply.
    out.write_all(b"\x1B]11;?\x1B\\")?;
    // Keep DA1 with kitty so tmux's faster local reply cannot end the query early.
    write_commands(
        out,
        b"\x1B_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1B\\\x1B[c",
        tmux,
    )
}

/// Sends graphics commands before the frame's placeholder cells, through tmux when needed.
pub fn send_image_commands(out: &mut impl Write, commands: &[u8]) -> io::Result<()> {
    write_commands(out, commands, in_tmux())
}

fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some_and(|value| !value.is_empty())
}

fn write_commands(out: &mut impl Write, commands: &[u8], tmux: bool) -> io::Result<()> {
    if commands.is_empty() {
        return Ok(());
    }
    if tmux {
        // Each graphics APC already contains a bounded upload chunk. Preserve those
        // boundaries so a batch cannot overflow tmux's DCS input buffer.
        for command in commands.split_inclusive(|byte| *byte == b'\\') {
            out.write_all(b"\x1bPtmux;")?;
            for part in command.split_inclusive(|byte| *byte == 0x1b) {
                out.write_all(part)?;
                if part.last() == Some(&0x1b) {
                    out.write_all(b"\x1b")?;
                }
            }
            out.write_all(b"\x1b\\")?;
        }
    } else {
        out.write_all(commands)?;
    }
    out.flush()
}

/// Reads cell pixels again after a window size change, without sending terminal queries.
pub fn cell_pixels() -> io::Result<Option<(u16, u16)>> {
    read_cell_pixels(&OpenOptions::new().read(true).open("/dev/tty")?)
}

fn read_cell_pixels(tty: &impl AsRawFd) -> io::Result<Option<(u16, u16)>> {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(pixels_per_cell(
        size.ws_xpixel,
        size.ws_ypixel,
        size.ws_col,
        size.ws_row,
    ))
}

fn pixels_per_cell(width: u16, height: u16, columns: u16, rows: u16) -> Option<(u16, u16)> {
    let width = width.checked_div(columns)?;
    let height = height.checked_div(rows)?;
    (width > 0 && height > 0).then_some((width, height))
}

pub fn is_light((r, g, b): (u8, u8, u8)) -> bool {
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    luma > 128.0
}

/// Waits for DA1 so the input reader does not receive the remaining query replies.
fn parse_reply(buffer: &[u8]) -> Option<Report> {
    has_device_attributes(buffer).then(|| parse_report(buffer))
}

fn parse_report(buffer: &[u8]) -> Report {
    let kitty_ok = b"\x1b_Gi=31;OK\x1b\\";
    Report {
        background: parse_osc_background(buffer),
        supports_images: buffer
            .windows(kitty_ok.len())
            .any(|window| window == kitty_ok),
        cell_pixels: None,
    }
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
    fn tmux_background_query_stays_local_and_graphics_waits_for_outer_da1() {
        let mut out = Vec::new();
        write_queries(&mut out, true).unwrap();
        assert_eq!(
            out,
            b"\x1b]11;?\x1b\\\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\\x1bPtmux;\x1b\x1b[c\x1b\\"
        );

        // Captured from iTerm2 through tmux with allow-passthrough on.
        let background = b"\x1b]11;rgb:b0b0/d0d0/e0e0\x1b\\";
        let kitty = b"\x1b_Gi=31;OK\x1b\\";
        let da1 = b"\x1b[?64;1;2;4;6;17;18;21;22;52c";
        assert_eq!(parse_reply(background), None);
        assert_eq!(parse_reply(&[&background[..], kitty].concat()), None);
        assert_eq!(
            parse_reply(&[&background[..], kitty, da1].concat()),
            Some(Report {
                background: Some((176, 208, 224)),
                supports_images: true,
                cell_pixels: None,
            })
        );
        assert_eq!(
            parse_reply(&[&kitty[..], da1].concat()),
            Some(Report {
                supports_images: true,
                ..Report::default()
            })
        );
    }

    #[test]
    fn queries_without_tmux_go_directly_to_the_terminal() {
        let mut out = Vec::new();
        write_queries(&mut out, false).unwrap();
        assert_eq!(
            out,
            b"\x1b]11;?\x1b\\\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c"
        );
    }

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
            Some(Report {
                background: Some((0x11, 0x22, 0x33)),
                ..Report::default()
            })
        );
        assert_eq!(parse_reply(da1), Some(Report::default()));
        assert_eq!(parse_reply(b"\x1B]11;rgb:1111/2222/3333\x07"), None);
        assert_eq!(parse_reply(b"\x1B[?62;22"), None);
        assert_eq!(parse_reply(b"\x1B[?1u"), None);
        assert_eq!(parse_reply(b"\x1B[?1u\x1B[?1;2c"), Some(Report::default()));
    }

    #[test]
    fn kitty_support_requires_the_query_id_and_complete_ok_reply() {
        for reply in [
            &b"\x1b_Gi=31;OK\x1b\\"[..],
            b"\x1b[?1u\x1b_Gi=31;OK\x1b\\\x1b]11;rgb:11/22/33\x07",
        ] {
            assert!(parse_report(reply).supports_images);
            assert_eq!(parse_reply(reply), None);
        }
        for reply in [
            &b"\x1b_Gi=30;OK\x1b\\"[..],
            b"\x1b_Gi=31;ENOENT\x1b\\",
            b"\x1b_Gi=31;OK",
        ] {
            assert!(!parse_report(reply).supports_images);
        }
    }

    #[test]
    fn parses_mixed_tmux_and_outer_terminal_replies_until_da1() {
        let reply = b"\x1b[?1u\x1b[?0n\x1b_Gi=31;OK\x1b\\\x1b]11;rgb:1111/2222/3333\x1b\\\x1b[?64;1;2;4;6;9;15;18;21;22c";
        let expected = Report {
            background: Some((0x11, 0x22, 0x33)),
            supports_images: true,
            cell_pixels: None,
        };
        for end in 0..reply.len() {
            assert_eq!(parse_reply(&reply[..end]), None);
        }
        assert_eq!(parse_reply(reply), Some(expected));
    }

    #[test]
    fn cell_pixels_need_nonzero_dimensions_and_cell_sizes() {
        assert_eq!(pixels_per_cell(800, 600, 100, 30), Some((8, 20)));
        for (width, height, columns, rows) in [
            (0, 600, 100, 30),
            (800, 0, 100, 30),
            (800, 600, 0, 30),
            (800, 600, 100, 0),
            (80, 600, 100, 30),
            (800, 20, 100, 30),
        ] {
            assert_eq!(pixels_per_cell(width, height, columns, rows), None);
        }
    }

    #[test]
    fn passthrough_doubles_escapes_and_keeps_commands_in_order() {
        let commands = b"\x1b_Gi=31;AAAA\x1b\\\x1b_Ga=p,i=31\x1b\\";
        let mut out = Vec::new();
        write_commands(&mut out, commands, true).unwrap();
        assert_eq!(
            out,
            b"\x1bPtmux;\x1b\x1b_Gi=31;AAAA\x1b\x1b\\\x1b\\\x1bPtmux;\x1b\x1b_Ga=p,i=31\x1b\x1b\\\x1b\\"
        );
        out.clear();
        write_commands(&mut out, commands, false).unwrap();
        assert_eq!(out, commands);
        out.clear();
        write_commands(&mut out, b"", true).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn large_image_upload_keeps_each_command_in_its_own_passthrough() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        // Preserve the real Read result shape, using public slide pixels instead of a terminal capture.
        let fixture = include_str!("../tests/fixtures/claude-read-image.jsonl");
        let record: serde_json::Value =
            serde_json::from_str(fixture.lines().nth(1).unwrap()).unwrap();
        let pointer = "/message/content/0/content/0/source/data";
        let bytes = STANDARD
            .decode(record.pointer(pointer).unwrap().as_str().unwrap())
            .unwrap();
        let slide = image::load_from_memory(&bytes).unwrap();
        let mut tall = slide
            .resize_exact(1353, 2000, image::imageops::FilterType::Triangle)
            .to_rgb8();
        // Low-bit texture makes the slide's upload large enough to exercise transport framing.
        let mut texture = 1_u32;
        for byte in tall.iter_mut() {
            texture ^= texture << 13;
            texture ^= texture >> 17;
            texture ^= texture << 5;
            *byte = (*byte & !3) | (texture as u8 & 3);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        )
        .write_image(tall.as_raw(), 1353, 2000, image::ExtendedColorType::Rgb8)
        .unwrap();
        let encoded = STANDARD.encode(png.into_inner());
        let chunks: Vec<_> = encoded.as_bytes().chunks(4096).collect();
        let mut commands = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let more = u8::from(index + 1 < chunks.len());
            let control = if index == 0 {
                format!("a=t,f=100,t=d,i=1,q=2,m={more}")
            } else {
                format!("m={more}")
            };
            commands.extend_from_slice(format!("\x1b_G{control};").as_bytes());
            commands.extend_from_slice(chunk);
            commands.extend_from_slice(b"\x1b\\");
        }
        commands.extend_from_slice(b"\x1b_Ga=p,U=1,i=1,p=1,c=194,r=96,q=2\x1b\\");
        assert!(
            commands.len() > 1_048_576,
            "fixture must exceed tmux's input buffer: {}",
            commands.len()
        );
        let mut out = Vec::new();
        write_commands(&mut out, &commands, true).unwrap();
        let text = String::from_utf8(out).unwrap();
        let mut recovered = Vec::new();
        for envelope in text.split("\x1bPtmux;").skip(1) {
            assert!(
                envelope.len() < 8192,
                "one envelope contains multiple upload chunks: {}",
                envelope.len()
            );
            let command = envelope
                .strip_suffix("\x1b\\")
                .unwrap()
                .replace("\x1b\x1b", "\x1b");
            recovered.extend_from_slice(command.as_bytes());
        }
        assert_eq!(recovered, commands);
    }

    #[test]
    fn luminance_splits_light_and_dark() {
        assert!(is_light((255, 255, 255)));
        assert!(is_light((238, 238, 238)));
        assert!(!is_light((0, 0, 0)));
        assert!(!is_light((30, 30, 46)));
    }
}
