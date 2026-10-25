//! Finds the file paths in displayed text, for shortening and for links alike.
use std::ops::Range;
use std::path::Path;

/// The paths in `text`: absolute and home paths, and relative paths that exist under `cwd`.
pub(super) fn text_paths(text: &str, cwd: &Path) -> Vec<Range<usize>> {
    let mut paths = Vec::new();
    for (start, word) in text_words(text) {
        if let Some(range) = word_path(word)
            && (is_rooted(&word[range.clone()]) || cwd.join(&word[range.clone()]).exists())
        {
            paths.push(start + range.start..start + range.end);
        }
    }
    paths
}

/// A path inside `cwd`, relative to that directory; `cwd` itself is its own name. Component boundaries matter;
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
        cwd.file_name()?.to_str()?.to_owned()
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

/// The bytes of the path-shaped part of one word, before any existence check:
/// an absolute or home path after punctuation, or a relative path such as
/// `src/ui/log.rs` or `screen.png`. URLs and words with letters before the path stay prose.
pub(super) fn word_path(word: &str) -> Option<Range<usize>> {
    // A rooted path starts at the first `/` or `~` after punctuation only, so
    // `a/b`, `./a/b` and `https://…` are not rooted while `报告：/tmp/…` is.
    let rooted = word.find(['/', '~']).filter(|&start| {
        !word[..start].contains(|c: char| c.is_ascii_alphanumeric() || matches!(c, ':' | '.'))
    });
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
        (path.contains('/') || Path::new(path).extension().is_some())
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

    #[test]
    fn existing_filenames_and_dot_paths_keep_relative_ranges() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("screens")).unwrap();
        for name in ["screen.png", "screens/screen.png"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        for label in ["screen.png", "./screen.png", "./screens/screen.png"] {
            let text = format!("看 `{label}`，");
            assert_eq!(found(&text, dir.path()), [label], "{label}");
        }
        assert!(found("missing.png ./missing.png ./missing/screen.png", dir.path()).is_empty());
    }

    #[test]
    fn dot_paths_link_and_abbreviate_as_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("screens")).unwrap();
        for name in ["screen.png", "screens/screen.png"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        for label in ["screen.png", "./screen.png", "./screens/screen.png"] {
            let links = super::super::links::text_links(label, dir.path(), &[]);
            assert_eq!(links.len(), 1, "{label}");
            assert_eq!(links[0].range, 0..label.len());
            assert_eq!(
                links[0].target,
                super::super::links::LinkTarget::Path(dir.path().join(label))
            );
        }
        let text = "./screens/screen.png";
        assert_eq!(
            super::super::line_truncation::fit_paths_to_wrap_width(
                text,
                &text_paths(text, dir.path()),
                12,
                dir.path()
            ),
            "…/screen.png"
        );
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
