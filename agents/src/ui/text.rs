//! Message layout and one-line summaries shared by the panels.

use super::tool_detail::Part;
use crate::log::Kind;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::ops::Range;
use std::path::Path;

#[cfg(test)]
thread_local! {
    static SUMMARY_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Only the Markdown layout is retained; path discovery still runs on each frame.
const SUMMARY_CACHE_BYTES: usize = 4 * 1024 * 1024;
const SUMMARY_CACHE_ENTRIES: usize = 128;

#[derive(PartialEq, Eq)]
struct SummaryKey {
    text: String,
    cwd: std::path::PathBuf,
    home: Option<std::ffi::OsString>,
    paths: Vec<Range<usize>>,
    style: Style,
    accent: ratatui::style::Color,
    theme: u64,
}

struct SummaryEntry {
    key: SummaryKey,
    line: Line<'static>,
    bytes: usize,
}

thread_local! {
    static SUMMARIES: std::cell::RefCell<std::collections::VecDeque<SummaryEntry>> = const {
        std::cell::RefCell::new(std::collections::VecDeque::new())
    };
}

pub(super) fn lines(text: &str, cwd: &Path, width: usize, markdown: bool) -> Vec<Line<'static>> {
    Part::prose_parts(text.to_owned(), markdown)
        .into_iter()
        .flat_map(|part| part.lines_in(width, Style::default(), chrono::DateTime::UNIX_EPOCH, cwd))
        .collect()
}

/// Whether a summary uses the message body's Markdown interpretation.
pub(super) fn is_prose(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::User
            | Kind::Compaction
            | Kind::Assistant
            | Kind::Agent
            | Kind::QueuedUser
            | Kind::QueuedSystem
    )
}

/// A message or activity on one row, with optional provider-marked paths.
pub(crate) fn summary(
    text: &str,
    cwd: &Path,
    width: usize,
    kind: &Kind,
    paths: Option<&[Range<usize>]>,
) -> Line<'static> {
    let style = super::style::log_styles(kind).1;
    let normalized;
    let text = if !is_prose(kind) && paths.is_none() {
        normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        normalized.as_str()
    } else {
        text
    };
    let detected;
    let paths = match paths {
        Some(paths) => paths,
        None => {
            detected = super::paths::text_paths(text, cwd);
            &detected
        }
    };
    let text = if is_prose(kind) {
        super::line_truncation::abbreviate_paths_in_text(text, paths, width, cwd)
    } else {
        super::line_truncation::abbreviate_paths(text, paths, width, cwd)
    };
    let text = crate::providers::text::printable(&text);
    if !is_prose(kind) {
        return Line::styled(text.split_whitespace().collect::<Vec<_>>().join(" "), style);
    }
    let key = SummaryKey {
        paths: super::paths::text_paths(&text, cwd),
        text: text.clone(),
        cwd: cwd.to_owned(),
        home: std::env::var_os("HOME"),
        style,
        accent: crate::theme::inline_code_color(),
        theme: super::highlight::theme_generation(),
    };
    if let Some(line) = SUMMARIES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache.iter().position(|entry| entry.key == key)?;
        let entry = cache.remove(index).unwrap();
        let line = entry.line.clone();
        cache.push_back(entry);
        Some(line)
    }) {
        return line;
    }
    #[cfg(test)]
    SUMMARY_RENDERS.set(SUMMARY_RENDERS.get() + 1);
    let mut spans = Vec::new();
    for line in lines(&text, cwd, usize::MAX, true) {
        if line
            .to_string()
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '─' | '┼'))
        {
            continue;
        }
        let mut pending_space = !spans.is_empty();
        for span in line.spans {
            for fragment in span.content.split_inclusive(char::is_whitespace) {
                let word = fragment.trim_end_matches(char::is_whitespace);
                if !word.is_empty() {
                    if pending_space {
                        spans.push(Span::styled(" ", style));
                    }
                    spans.push(Span::styled(word.to_owned(), style.patch(span.style)));
                    pending_space = false;
                }
                pending_space |= word.len() != fragment.len();
            }
        }
    }
    let line = Line::from(spans);
    let bytes = std::mem::size_of::<SummaryEntry>()
        + key.text.capacity()
        + key.cwd.as_os_str().len()
        + key.home.as_ref().map_or(0, |home| home.len())
        + key.paths.capacity() * std::mem::size_of::<Range<usize>>()
        + line.spans.capacity() * std::mem::size_of::<Span<'static>>()
        + line
            .spans
            .iter()
            .map(|span| match &span.content {
                std::borrow::Cow::Owned(text) => text.capacity(),
                std::borrow::Cow::Borrowed(_) => 0,
            })
            .sum::<usize>();
    if bytes <= SUMMARY_CACHE_BYTES {
        SUMMARIES.with(|cache| {
            let mut cache = cache.borrow_mut();
            while cache.len() >= SUMMARY_CACHE_ENTRIES
                || cache.iter().map(|entry| entry.bytes).sum::<usize>() + bytes
                    > SUMMARY_CACHE_BYTES
            {
                cache.pop_front();
            }
            cache.push_back(SummaryEntry {
                key,
                line: line.clone(),
                bytes,
            });
        });
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_summary_reuses_markdown_layout() {
        let source = include_str!("../../tests/fixtures/claude-peer-message.txt");
        SUMMARY_RENDERS.set(0);
        let first = summary(source, Path::new(""), 120, &Kind::Assistant, None);
        assert!(first.to_string().starts_with("FYI: VS Code traps"));
        let second = summary(source, Path::new(""), 120, &Kind::Assistant, None);
        assert_eq!(first, second);
        assert_eq!(SUMMARY_RENDERS.get(), 1);
    }

    #[test]
    fn recorded_summary_tracks_content_styles_and_paths() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let source = include_str!("../../tests/fixtures/claude-peer-message.txt");
        let dir = tempfile::tempdir().unwrap();
        SUMMARIES.with(|cache| cache.borrow_mut().clear());
        SUMMARY_RENDERS.set(0);
        let first = summary(source, dir.path(), 120, &Kind::Assistant, None);
        let user = summary(source, dir.path(), 120, &Kind::User, None);
        assert_eq!(first.to_string(), user.to_string());
        assert_ne!(first, user);
        let changed = summary(source.trim_end(), dir.path(), 120, &Kind::Assistant, None);
        assert_eq!(first.to_string(), changed.to_string());
        let renders = SUMMARY_RENDERS.get();
        crate::theme::use_default_accents(true);
        let light = summary(source, dir.path(), 120, &Kind::Assistant, None);
        assert_eq!(first.to_string(), light.to_string());
        assert!(SUMMARY_RENDERS.get() > renders);
        crate::theme::use_default_accents(false);
        // The report's relative path can appear or disappear between frames.
        let renders = SUMMARY_RENDERS.get();
        std::fs::create_dir(dir.path().join("git")).unwrap();
        std::fs::write(dir.path().join("git/README.md"), source).unwrap();
        let after = summary(source, dir.path(), 120, &Kind::Assistant, None);
        assert!(SUMMARY_RENDERS.get() > renders);
        SUMMARIES.with(|cache| cache.borrow_mut().clear());
        assert_eq!(
            after,
            summary(source, dir.path(), 120, &Kind::Assistant, None)
        );
        std::fs::remove_file(dir.path().join("git/README.md")).unwrap();
        assert_eq!(
            first,
            summary(source, dir.path(), 120, &Kind::Assistant, None)
        );
        let path = source
            .split_whitespace()
            .find(|word| word.contains("extensions.json"))
            .unwrap();
        let wide = summary(path, dir.path(), 120, &Kind::Assistant, None);
        let narrow = summary(path, dir.path(), 8, &Kind::Assistant, None);
        assert_ne!(wide.to_string(), narrow.to_string());
    }

    #[test]
    fn recorded_summary_cache_has_bounded_storage() {
        SUMMARIES.with(|cache| cache.borrow_mut().clear());
        let source = include_str!("../../tests/fixtures/claude-peer-message.txt")
            .lines()
            .next()
            .unwrap();
        let first = format!("{source} 0");
        for index in 0..=SUMMARY_CACHE_ENTRIES {
            summary(
                &format!("{source} {index}"),
                Path::new(""),
                120,
                &Kind::Assistant,
                None,
            );
        }
        SUMMARIES.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.len(), SUMMARY_CACHE_ENTRIES);
            assert!(cache.iter().map(|entry| entry.bytes).sum::<usize>() <= SUMMARY_CACHE_BYTES);
            assert!(!cache.iter().any(|entry| entry.key.text == first));
        });
    }

    #[test]
    fn recorded_report_summary_keeps_all_paragraphs_on_one_line() {
        let source = include_str!("../../tests/fixtures/claude-peer-message.txt");
        let line = summary(source, Path::new(""), usize::MAX, &Kind::Assistant, None).to_string();
        assert!(line.starts_with("FYI: VS Code traps"));
        assert!(line.contains("State as I understand it:"));
        assert!(line.contains("Traps:"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn recorded_question_summary_keeps_inline_code_next_to_chinese() {
        let record: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/claude-questions-pending.jsonl"
        ))
        .unwrap();
        let text = record["message"]["content"][0]["input"]["questions"][0]["question"]
            .as_str()
            .unwrap();
        let line = summary(text, Path::new(""), 200, &Kind::Assistant, None).to_string();
        assert!(line.contains("是子命令？比如 rg -n 'background' src 里的 background"));
        assert!(line.contains("cat README.md 的是文件"));
        assert!(!line.contains('`'));
    }

    #[test]
    fn colored_document_keeps_structured_indentation() {
        let raw = include_str!("../../tests/fixtures/claude-attach/attach.raw");
        let start = raw.find("\x1b[2mAttaching…").unwrap();
        let styled = &raw[start..];
        let reset = styled.find("\x1b[0m").unwrap();
        let xml = include_str!("../../tests/fixtures/structured_text/real-theme.plist");
        let source = format!("{}{xml}{}", &styled[..4], &styled[reset..reset + 4]);
        let rendered = lines(&source, Path::new(""), 80, true);
        assert_eq!(rendered[0].to_string(), "<plist version=\"1.0\">");
        assert_eq!(rendered[1].to_string(), "  <dict>");
        assert!(
            rendered
                .iter()
                .all(|line| !line.to_string().contains('\x1b'))
        );
    }
}
