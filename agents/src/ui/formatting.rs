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
