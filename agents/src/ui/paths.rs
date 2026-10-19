//! Finds the file paths in displayed text, for shortening and for links alike.
use std::{cell::RefCell, collections::VecDeque, ops::Range, path::Path, rc::Rc};

const PATH_CACHE_ENTRIES: usize = 128;
const PATH_CACHE_BYTES: usize = 4 * 1024 * 1024;
const PATH_ENTRY_BYTES: usize = std::mem::size_of::<PathEntry>() + 2 * std::mem::size_of::<usize>();

struct PathCandidate {
    range: Range<usize>,
    rooted: bool,
}

struct PathEntry {
    text: String,
    candidates: Rc<[PathCandidate]>,
    bytes: usize,
}

#[derive(Default)]
struct PathCache {
    entries: VecDeque<PathEntry>,
    bytes: usize,
}

thread_local! {
    static PATH_CANDIDATES: RefCell<PathCache> = RefCell::new(PathCache::default());
}

/// The paths in `text`: absolute and home paths, and relative paths that exist under `cwd`.
pub(super) fn text_paths(text: &str, cwd: &Path) -> Vec<Range<usize>> {
    let candidates = path_candidates(text);
    candidates
        .iter()
        .filter(|candidate| candidate.rooted || cwd.join(&text[candidate.range.clone()]).exists())
        .map(|candidate| candidate.range.clone())
        .collect()
}

/// Caches lexical candidates only; relative path existence is checked by each caller.
fn path_candidates(text: &str) -> Rc<[PathCandidate]> {
    if let Some(candidates) = PATH_CANDIDATES.with(|cache| {
        cache
            .borrow()
            .entries
            .iter()
            .find(|entry| entry.text == text)
            .map(|entry| Rc::clone(&entry.candidates))
    }) {
        return candidates;
    }
    let candidates: Rc<[PathCandidate]> = text_words(text)
        .filter_map(|(start, word)| {
            word_path(word).map(|range| PathCandidate {
                rooted: is_rooted(&word[range.clone()]),
                range: start + range.start..start + range.end,
            })
        })
        .collect();
    PATH_CANDIDATES.with(|cache| {
        cache
            .borrow_mut()
            .insert(text, Rc::clone(&candidates), PATH_CACHE_BYTES);
    });
    candidates
}

impl PathCache {
    fn insert(&mut self, text: &str, candidates: Rc<[PathCandidate]>, byte_limit: usize) {
        let bytes = PATH_ENTRY_BYTES + text.len() + std::mem::size_of_val(candidates.as_ref());
        if bytes > byte_limit {
            return;
        }
        while self.entries.len() >= PATH_CACHE_ENTRIES || self.bytes + bytes > byte_limit {
            let entry = self.entries.pop_front().expect("nonempty path cache");
            self.bytes -= entry.bytes;
        }
        self.entries.push_back(PathEntry {
            text: text.to_owned(),
            candidates,
            bytes,
        });
        self.bytes += bytes;
    }
}

/// A path inside `cwd`, relative to that directory. Component boundaries matter;
/// parent components stay absolute so a lexical prefix cannot hide an escape.
pub(super) fn relative_path(path: &str, cwd: &Path) -> Option<String> {
    let expanded;
    let absolute = if let Some(rest) = path.strip_prefix("~/") {
        expanded = std::path::PathBuf::from(std::env::var_os("HOME")?).join(rest);
        expanded.as_path()
    } else {
        Path::new(path)
    };
    if !absolute.is_absolute() || !cwd.is_absolute() {
        return None;
    }
    let relative = absolute.strip_prefix(cwd).ok()?;
    if relative
        .components()
        .any(|part| part == std::path::Component::ParentDir)
    {
        return None;
    }
    Some(if relative.as_os_str().is_empty() {
        ".".into()
    } else {
        relative.to_str()?.to_owned()
    })
}

/// CJK punctuation that ends a word without an intervening space.
const CJK_BREAKS: [char; 7] = ['，', '。', '、', '；', '：', '！', '？'];

/// The words that paths and commits are found in, with their byte offsets:
/// text split at whitespace and at CJK punctuation.
fn text_words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let breaks = |c: char| c.is_whitespace() || CJK_BREAKS.contains(&c);
    text.split_inclusive(breaks).scan(0, move |offset, part| {
        let start = *offset;
        *offset += part.len();
        Some((start, part.trim_end_matches(breaks)))
    })
}

#[cfg(test)]
thread_local! {
    static PATH_WORD_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The bytes of the path-shaped part of one word, before any existence check:
/// an absolute or home path after punctuation, or a relative path such as
/// `src/ui/log.rs`. URLs and words with letters before the path stay prose.
pub(super) fn word_path(word: &str) -> Option<Range<usize>> {
    #[cfg(test)]
    PATH_WORD_SCANS.with(|count| count.set(count.get() + 1));
    // A rooted path starts at the first `/` or `~` after punctuation only, so
    // `a/b` and `https://…` are not rooted while `报告：/tmp/…` is.
    let rooted = word
        .find(['/', '~'])
        .filter(|&start| !word[..start].contains(|c: char| c.is_ascii_alphanumeric() || c == ':'));
    let start = match rooted {
        Some(start) => start,
        None => {
            word.len()
                - word
                    .trim_start_matches(['`', '(', '"', '\'', '[', '*', '_', '<'])
                    .len()
        }
    };
    let range = word_bounds(word, start);
    let end = range.end;
    let path = &word[start..end];
    let shaped = if is_rooted(path) {
        // The first component after the root must be named, so `//` and `///` comments stay prose.
        path[1..].contains('/') && !path.trim_start_matches('~')[1..].starts_with('/')
    } else {
        path.contains('/')
            && !path.contains("://")
            && path
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.')
    };
    shaped.then_some(start..end)
}

/// Commit-shaped words, sharing the path finder's prose trimming. A range has two words.
pub(super) fn text_commits(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for (offset, word) in text_words(text) {
        if !word.contains(['/', '~']) && !word.contains("://") {
            let mut at = 0;
            for candidate in word.split("..") {
                let start = candidate.len()
                    - candidate
                        .trim_start_matches(['`', '(', '"', '\'', '[', '*', '_', '<'])
                        .len();
                let range = word_bounds(candidate, start);
                let sha = &candidate[range.clone()];
                if (7..=40).contains(&sha.len())
                    && sha
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    ranges.push(offset + at + range.start..offset + at + range.end);
                }
                at += candidate.len() + 2;
            }
        }
    }
    ranges
}

fn word_bounds(word: &str, start: usize) -> Range<usize> {
    let mut end = start + trim_closing(&word[start..]).len();
    // CJK sentence punctuation ends a path without an intervening space.
    if let Some(cut) = word[start..end].find(CJK_BREAKS) {
        end = start + trim_closing(&word[start..start + cut]).len();
    }
    for marker in ["**", "__", "~~", "*", "_"] {
        if word[..start].ends_with(marker) && word[start..end].ends_with(marker) {
            end -= marker.len();
            break;
        }
    }
    // A `:10` or `:10-40` line range follows the path rather than being part of it.
    if let Some(colon) = word[start..end].rfind(':')
        && line_range(&word[start + colon..end]).is_some_and(|(len, _)| colon + len == end - start)
    {
        end = start + colon;
    }
    start..end
}

/// The length and first line of the `:10`, `:10-` or `:10-40` line range that `text` starts with.
pub(super) fn line_range(text: &str) -> Option<(usize, u64)> {
    let digits =
        |text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let rest = text.strip_prefix(':')?;
    let first = digits(rest);
    let line = rest[..first].parse().ok()?;
    let mut len = 1 + first;
    if rest[first..].starts_with('-') {
        len += 1 + digits(&rest[first + 1..]);
    }
    Some((len, line))
}

/// Whether `path` is absolute or under the home directory, so it needs no `cwd`.
pub(super) fn is_rooted(path: &str) -> bool {
    path.starts_with('/') || path.starts_with("~/")
}

/// Drops the quotes, brackets and punctuation that close a path in prose.
fn trim_closing(path: &str) -> &str {
    path.trim_end_matches(['`', ')', ']', '>', '"', '\'', ',', '.', ';', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'a>(text: &'a str, cwd: &Path) -> Vec<&'a str> {
        text_paths(text, cwd)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    fn recorded_path_text() -> String {
        let payload: serde_json::Value = serde_json::from_str(
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        payload["message"]["content"][0]["input"]["command"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn recorded_path_candidates_are_reused() {
        let text = recorded_path_text();
        PATH_WORD_SCANS.with(|count| count.set(0));
        let first = text_paths(&text, Path::new("/"));
        assert!(!first.is_empty());
        let scans = PATH_WORD_SCANS.with(|count| count.get());
        assert!(scans > 0);
        assert_eq!(text_paths(&text, Path::new("/")), first);
        assert_eq!(PATH_WORD_SCANS.with(|count| count.get()), scans);
    }

    #[test]
    fn path_candidates_use_exact_content_and_bound_retained_memory() {
        PATH_CANDIDATES.with(|cache| *cache.borrow_mut() = PathCache::default());
        let text = recorded_path_text();
        let original = path_candidates(&text);
        assert!(Rc::ptr_eq(&original, &path_candidates(&text)));
        let changed = format!("{text} /cache/added.rs");
        let candidates = path_candidates(&changed);
        assert_eq!(
            &changed[candidates.last().unwrap().range.clone()],
            "/cache/added.rs"
        );
        assert!(!Rc::ptr_eq(&original, &candidates));
        for index in 0..PATH_CACHE_ENTRIES {
            path_candidates(&format!("{text} {index}"));
        }
        PATH_CANDIDATES.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.entries.len(), PATH_CACHE_ENTRIES);
            assert!(!cache.entries.iter().any(|entry| entry.text == text));
        });
        let entry_bytes = PATH_ENTRY_BYTES + text.len() + std::mem::size_of_val(original.as_ref());
        let byte_limit = entry_bytes * 2;
        assert!(byte_limit < PATH_CACHE_BYTES);
        let mut cache = PathCache::default();
        cache.insert(&text, Rc::clone(&original), byte_limit);
        cache.insert(&text, Rc::clone(&original), byte_limit);
        assert_eq!(cache.entries.len(), 2);
        cache.insert(&changed, Rc::clone(&candidates), byte_limit);
        assert!(cache.bytes <= byte_limit);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].text, changed);
        cache.insert(&text, Rc::clone(&original), entry_bytes - 1);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].text, changed);
    }

    #[test]
    fn cached_candidates_keep_relative_paths_live_for_each_directory() {
        let text = format!("{} src/ui/log.rs", recorded_path_text());
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let before = found(&text, first.path());
        assert!(!before.contains(&"src/ui/log.rs"));
        std::fs::create_dir_all(second.path().join("src/ui")).unwrap();
        let path = second.path().join("src/ui/log.rs");
        std::fs::write(&path, "").unwrap();
        assert!(found(&text, second.path()).contains(&"src/ui/log.rs"));
        assert_eq!(found(&text, first.path()), before);
        std::fs::create_dir_all(first.path().join("src/ui")).unwrap();
        let path_first = first.path().join("src/ui/log.rs");
        std::fs::write(&path_first, "").unwrap();
        assert!(found(&text, first.path()).contains(&"src/ui/log.rs"));
        std::fs::remove_file(path_first).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(found(&text, first.path()), before);
        assert_eq!(found(&text, second.path()), before);
        let changed = text.replace("src/ui/log.rs", "src/ui/missing.rs");
        assert!(!found(&changed, second.path()).contains(&"src/ui/log.rs"));
    }

    #[test]
    fn commit_candidates_follow_prose_boundaries() {
        for (text, expected) in [
            ("已提交 `cd410a5`，推送了", vec!["cd410a5"]),
            ("**提交**：`0e61b5b1`，已推到", vec!["0e61b5b1"]),
            ("`cd410a5`、`0e61b5b1`，见 ~/x", vec!["cd410a5", "0e61b5b1"]),
            ("commits 604ccc3..e8f6bc8", vec!["604ccc3", "e8f6bc8"]),
            (
                "**deadbeef** (1234567), `abcdefabcdef`",
                vec!["deadbeef", "1234567", "abcdefabcdef"],
            ),
            (
                "abcdef ABCDEF1 abc123 01234567890123456789012345678901234567890",
                vec![],
            ),
            (
                "/tmp/cd410a5 src/cd410a5 https://host/cd410a5 prefixcd410a5",
                vec![],
            ),
        ] {
            let found: Vec<_> = text_commits(text).into_iter().map(|r| &text[r]).collect();
            assert_eq!(found, expected, "{text}");
        }
    }

    #[test]
    fn finds_rooted_paths_and_existing_relative_paths_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/ui")).unwrap();
        std::fs::write(dir.path().join("src/ui/log.rs"), "").unwrap();
        assert_eq!(
            found(
                "see `src/ui/log.rs`, and/or src/missing.rs ~/notes/a.md /tmp/a/b.md https://x.io/a/b /report.md",
                dir.path()
            ),
            ["src/ui/log.rs", "~/notes/a.md", "/tmp/a/b.md"]
        );
        assert_eq!(
            found("**改在**：`src/ui/log.rs`，", dir.path()),
            ["src/ui/log.rs"]
        );
    }

    #[test]
    fn paths_end_before_cjk_punctuation_markdown_markers_and_line_ranges() {
        let cwd = Path::new("/");
        assert_eq!(
            found("报告：`/private/workspace/report.md`，已发送。", cwd),
            ["/private/workspace/report.md"]
        );
        assert_eq!(
            found("装进了`~/.cargo/bin`。正在跑的", cwd),
            ["~/.cargo/bin"]
        );
        assert_eq!(found("**/tmp/a/b.md**", cwd), ["/tmp/a/b.md"]);
        assert_eq!(
            found("`/tmp/a.md`、`~/b/c.md`", cwd),
            ["/tmp/a.md", "~/b/c.md"]
        );
        assert_eq!(found("(/tmp/a/b.md).", cwd), ["/tmp/a/b.md"]);
        assert_eq!(
            found("/tmp/a/b.rs:10-40 /tmp/a/c.rs:7 /tmp/a/d.rs:7-", cwd),
            ["/tmp/a/b.rs", "/tmp/a/c.rs", "/tmp/a/d.rs"]
        );
    }

    #[test]
    fn words_with_letters_or_a_scheme_before_the_slash_are_not_rooted_paths() {
        let cwd = Path::new("/");
        assert!(found("a:b/c http://x/y/z", cwd).is_empty());
        assert_eq!(word_path("tmp/x/y.md"), Some(0..10));
    }

    #[test]
    fn comment_slashes_are_not_rooted_paths() {
        let cwd = Path::new("/");
        assert!(found("src/model.rs:85:    /// Display parent // note", cwd).is_empty());
        assert!(found("//! Module docs ~//x", cwd).is_empty());
    }
}
