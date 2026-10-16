//! Finds the file paths in displayed text, for shortening and for links alike.
use std::ops::Range;
use std::path::Path;

/// The paths in `text`: absolute and home paths, and relative paths that exist under `cwd`.
pub(super) fn text_paths(text: &str, cwd: &Path) -> Vec<Range<usize>> {
    let mut paths = Vec::new();
    let mut start = 0;
    for part in text.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
        if let Some(range) = word_path(word)
            && (is_rooted(&word[range.clone()]) || cwd.join(&word[range.clone()]).exists())
        {
            paths.push(start + range.start..start + range.end);
        }
        start += part.len();
    }
    paths
}

/// The bytes of the path-shaped part of one word, before any existence check:
/// an absolute or home path after punctuation, or a relative path such as
/// `src/ui/log.rs`. URLs and words with letters before the path stay prose.
pub(super) fn word_path(word: &str) -> Option<Range<usize>> {
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
        path[1..].contains('/')
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
    let mut offset = 0;
    for part in text.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
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
        offset += part.len();
    }
    ranges
}

fn word_bounds(word: &str, start: usize) -> Range<usize> {
    let mut end = start + trim_closing(&word[start..]).len();
    // CJK sentence punctuation ends a path without an intervening space.
    if let Some(cut) = word[start..end].find(['，', '。', '；', '：', '！', '？']) {
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
    fn commit_candidates_follow_prose_boundaries() {
        for (text, expected) in [
            ("已提交 `cd410a5`，推送了", vec!["cd410a5"]),
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
}
