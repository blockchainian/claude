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
    fn paths_end_before_cjk_punctuation_and_markdown_markers() {
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
    }

    #[test]
    fn words_with_letters_or_a_scheme_before_the_slash_are_not_rooted_paths() {
        let cwd = Path::new("/");
        assert!(found("a:b/c http://x/y/z", cwd).is_empty());
        assert_eq!(word_path("tmp/x/y.md"), Some(0..10));
    }
}
