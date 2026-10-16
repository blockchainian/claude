use super::*;
use chrono::{DateTime, Utc};
use std::io::{Read, Seek, SeekFrom};

pub(super) fn tail_lines(path: &Path, count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    let read_tail = || -> std::io::Result<Vec<String>> {
        let mut file = std::fs::File::open(path)?;
        let length = file.metadata()?.len();
        let mut window = length.min(4096);
        let mut bytes = Vec::new();
        loop {
            let start = length - window;
            file.seek(SeekFrom::Start(start))?;
            bytes.clear();
            (&mut file).take(window).read_to_end(&mut bytes)?;
            // One extra newline ensures the earliest returned line is complete,
            // including when the file ends with a newline.
            if start == 0 || bytes.iter().filter(|&&byte| byte == b'\n').count() > count {
                break;
            }
            window = window.saturating_mul(2).min(length);
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<_> = text.lines().rev().take(count).map(str::to_owned).collect();
        lines.reverse();
        Ok(lines)
    };
    read_tail().unwrap_or_default()
}

pub(super) fn format_directory_display(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = path.strip_prefix(Path::new(&home))
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    path.display().to_string()
}
pub(super) fn format_relative_time(reference: DateTime<Utc>, ts: Option<DateTime<Utc>>) -> String {
    let Some(ts) = ts else {
        return "-".into();
    };
    let seconds = (reference - ts).num_seconds().max(0);
    if seconds == 0 {
        "now".into()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}
pub(super) fn format_session_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= 86400 {
        format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600)
    } else {
        format_duration(seconds)
    }
}

pub(super) fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn output_tail_returns_only_the_requested_lines() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "one\ntwo\nthree\nfour\nfive").unwrap();
        assert_eq!(tail_lines(file.path(), 3), ["three", "four", "five"]);
        assert!(tail_lines(file.path(), 0).is_empty());
    }

    #[test]
    fn output_tail_handles_short_empty_and_missing_files() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        assert!(tail_lines(file.path(), 3).is_empty());
        write!(file, "only line").unwrap();
        assert_eq!(tail_lines(file.path(), 3), ["only line"]);
        let directory = tempfile::tempdir().unwrap();
        assert!(tail_lines(&directory.path().join("missing"), 3).is_empty());
    }

    #[test]
    fn output_tail_grows_for_an_unterminated_long_line() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let long_line = "界".repeat(8192);
        write!(file, "discard\nsecond\nthird\n{long_line}").unwrap();
        assert_eq!(tail_lines(file.path(), 3), ["second", "third", &long_line]);
    }

    #[test]
    fn output_tail_does_not_return_a_partial_line_at_the_window_boundary() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let long_line = "x".repeat(4096);
        write!(file, "discard\n{long_line}\nsecond\nthird\n").unwrap();
        assert_eq!(tail_lines(file.path(), 3), [&long_line, "second", "third"]);
    }

    #[test]
    fn durations_keep_hours_and_zero_subfields() {
        for (seconds, expected) in [
            (0, "0s"),
            (45, "45s"),
            (60, "1m 0s"),
            (600, "10m 0s"),
            (605, "10m 5s"),
            (3599, "59m 59s"),
            (3600, "1h 0m"),
            (4200, "1h 10m"),
            (93780, "26h 3m"),
        ] {
            assert_eq!(format_duration(seconds), expected);
        }
    }
}
