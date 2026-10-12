use super::*;
use chrono::{DateTime, Utc};
use unicode_segmentation::UnicodeSegmentation;

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
pub(super) fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

pub(super) fn visible_suffix(input: &str, available: usize) -> &str {
    let mut start = input.len();
    let mut width = 0;
    for (offset, grapheme) in input.grapheme_indices(true).rev() {
        if width + grapheme.width() > available {
            break;
        }
        width += grapheme.width();
        start = offset;
    }
    &input[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

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
