use super::*;
use chrono::{DateTime, Utc};
use std::io::{Read, Seek, SeekFrom};

impl View {
    pub(super) fn draft_model<'a>(&self, draft: &'a super::drafts::Draft) -> Option<&'a str> {
        draft
            .setup
            .model
            .as_deref()
            .or(draft.defaults.model.as_deref())
    }

    pub(super) fn draft_default_effort<'a>(
        &'a self,
        draft: &'a super::drafts::Draft,
    ) -> Option<&'a str> {
        if draft.setup.model.is_none() || draft.setup.model == draft.defaults.model {
            draft.defaults.effort.as_deref()
        } else {
            self.input_choices
                .get(&draft.setup.provider)?
                .model(self.draft_model(draft)?)?
                .default_effort
                .as_deref()
        }
    }

    pub(super) fn draft_effort<'a>(&'a self, draft: &'a super::drafts::Draft) -> Option<&'a str> {
        draft
            .setup
            .effort
            .as_deref()
            .or_else(|| self.draft_default_effort(draft))
    }

    /// The session model, resolved through the same catalog as the picker.
    pub(super) fn session_model_text(&self, session: &Session) -> Option<String> {
        Some(self.model_text(
            session.id.provider,
            session.model.as_deref().unwrap_or_default(),
        ))
    }

    /// A model id or alias as the screen names it: its provider's name for it, else itself.
    pub(super) fn model_text(&self, provider: ProviderId, model: &str) -> String {
        if model.is_empty() {
            return "Unknown".into();
        }
        self.input_choices
            .get(&provider)
            .and_then(|choices| choices.label(model))
            .unwrap_or(model)
            .to_owned()
    }
}

pub(super) fn clock_text(at: DateTime<Utc>) -> String {
    at.with_timezone(&chrono::Local)
        .format(super::log::CLOCK_FORMAT)
        .to_string()
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

pub(super) fn format_directory_display(path: &Path, width: Option<usize>) -> String {
    let directory = if path.as_os_str().is_empty() {
        "No directory".to_owned()
    } else {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    let Some(width) = width else {
        return directory;
    };
    super::line_truncation::truncate_line_with_ellipsis_if_overflow(Line::from(directory), width)
        .to_string()
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
pub(super) fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds >= 86400 {
        format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600)
    } else if seconds < 60 {
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

    fn recorded_directory() -> std::path::PathBuf {
        include_str!("../../tests/fixtures/claude-start.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find_map(|record| record["cwd"].as_str().map(std::path::PathBuf::from))
            .unwrap()
    }

    #[test]
    fn directory_display_uses_only_the_recorded_cwd_basename() {
        let path = recorded_directory();
        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(format_directory_display(&path, None), name);
        assert_eq!(format_directory_display(&path, Some(80)), name);
        assert_eq!(format_directory_display(&path, Some(name.width())), name);
    }

    #[test]
    fn directory_display_truncates_the_basename_on_the_right() {
        let path = recorded_directory();
        assert_eq!(format_directory_display(&path, Some(5)), "clau…");
        assert_eq!(format_directory_display(&path, Some(2)), "c…");
        assert_eq!(format_directory_display(&path, Some(1)), "…");
        assert_eq!(format_directory_display(&path, Some(0)), "");
        assert_eq!(format_directory_display(Path::new("/"), None), "/");
        assert_eq!(
            format_directory_display(Path::new(""), None),
            "No directory"
        );
    }

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
            (93_780_000_000_000, "1d 2h"),
        ] {
            assert_eq!(
                format_log_duration(chrono::Duration::nanoseconds(nanos)),
                expected
            );
        }
    }

    #[test]
    fn durations_use_days_and_zero_subfields() {
        for (seconds, expected) in [
            (0, "0s"),
            (45, "45s"),
            (60, "1m 0s"),
            (600, "10m 0s"),
            (605, "10m 5s"),
            (3599, "59m 59s"),
            (3600, "1h 0m"),
            (4200, "1h 10m"),
            (93780, "1d 2h"),
            (-5, "0s"),
        ] {
            assert_eq!(format_duration(seconds), expected);
        }
    }
}
