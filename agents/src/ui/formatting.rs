use super::*;
use chrono::{DateTime, Utc};
use std::io::{Read, Seek, SeekFrom};

impl View {
    /// A session's model as the screen names it: the name its provider recorded, else the
    /// provider's name for its id; `None` when the session reports no model.
    pub(super) fn session_model_text(&self, session: &Session) -> Option<String> {
        match (session.model_label.as_deref(), session.model.as_deref()) {
            (Some(label), _) => Some(label.to_owned()),
            (None, Some(model)) if !model.is_empty() => {
                Some(self.model_text(session.id.provider, model))
            }
            _ => None,
        }
    }

    /// A model id or alias as the screen names it: its provider's name for it, else itself.
    pub(super) fn model_text(&self, provider: ProviderId, model: &str) -> String {
        self.input_choices
            .get(&provider)
            .and_then(|choices| choices.label(model))
            .unwrap_or(model)
            .to_owned()
    }
}

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

pub(super) fn format_log_duration(duration: chrono::Duration) -> String {
    let duration = duration.max(chrono::Duration::zero());
    if duration < chrono::Duration::seconds(1) {
        let milliseconds = (duration.subsec_nanos() + 500_000) / 1_000_000;
        if milliseconds == 0 {
            String::new()
        } else {
            format!("{milliseconds}ms")
        }
    } else {
        format_duration(duration.num_seconds())
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
    fn log_durations_round_subseconds_to_whole_milliseconds() {
        for (nanos, expected) in [
            (-1, ""),
            (0, ""),
            (499_999, ""),
            (500_000, "1ms"),
            (339_499_999, "339ms"),
            (339_500_000, "340ms"),
            (999_499_999, "999ms"),
            (999_500_000, "1000ms"),
            (1_000_000_000, "1s"),
            (1_999_999_999, "1s"),
            (60_000_000_000, "1m 0s"),
            (3_600_000_000_000, "1h 0m"),
        ] {
            assert_eq!(
                format_log_duration(chrono::Duration::nanoseconds(nanos)),
                expected
            );
        }
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
