//! Links that open repositories, files and pull requests in VS Code instead of their default app.
use std::{
    ffi::OsStr,
    io::Read,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The VS Code link for a clicked path: a repository or worktree root opens as a folder
/// and a text file opens in an editor. Other targets keep their default app.
pub(crate) fn open_target(target: &Path) -> Option<String> {
    let path = target.canonicalize().ok()?;
    if path.is_dir() {
        return (repository_root(&path)? == path).then(|| {
            format!(
                "vscode://blockchainian.git/view-folder?path={}",
                encode(path.as_os_str().as_bytes())
            )
        });
    }
    is_text(&path).then(|| {
        format!(
            "vscode://file{}",
            encode_except(path.as_os_str().as_bytes(), b"-._~/")
        )
    })
}

/// The link that opens a GitHub pull request URL in the window of `cwd`'s repository.
pub(super) fn pull_request_link(url: &str, cwd: &Path) -> Option<String> {
    let parts: Vec<_> = url
        .strip_prefix("https://github.com/")?
        .split('/')
        .collect();
    let [owner, repo, "pull", number] = parts[..] else {
        return None;
    };
    let shaped = !owner.is_empty()
        && !repo.is_empty()
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit());
    shaped.then(|| {
        format!(
            "vscode://blockchainian.git/view-pr?repo={}&url={}",
            encode(cwd.as_os_str().as_bytes()),
            encode(url.as_bytes())
        )
    })
}

fn repository_root(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let root = output.stdout.strip_suffix(b"\n")?;
    output
        .status
        .success()
        .then(|| PathBuf::from(OsStr::from_bytes(root)))
}

/// Whether a file reads as text: no NUL byte in its first 8 KB, as git decides.
fn is_text(path: &Path) -> bool {
    let mut head = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(8000).read_to_end(&mut head))
        .is_ok_and(|_| !head.contains(&0))
}

/// Percent-encodes `value` for a URL query.
pub(super) fn encode(value: &[u8]) -> String {
    encode_except(value, b"-._~")
}

fn encode_except(value: &[u8], kept: &[u8]) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for &byte in value {
        if byte.is_ascii_alphanumeric() || kept.contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("write URL");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn repository_and_worktree_roots_open_as_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("my repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        git(&repo, &["init", "-q"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
        let worktree = root.join("tree");
        git(
            &repo,
            &["worktree", "add", "-q", worktree.to_str().unwrap()],
        );
        let folder = |path: &Path| {
            format!(
                "vscode://blockchainian.git/view-folder?path={}",
                encode(path.to_str().unwrap().as_bytes())
            )
        };
        assert_eq!(open_target(&repo), Some(folder(&repo)));
        assert_eq!(open_target(&worktree), Some(folder(&worktree)));
        assert_eq!(
            open_target(&repo.join("src")),
            None,
            "a subdirectory stays in Finder"
        );
        assert_eq!(
            open_target(&root),
            None,
            "a plain directory stays in Finder"
        );
    }

    #[test]
    fn text_files_open_in_an_editor_and_binary_files_keep_their_app() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("a b#1.md");
        std::fs::write(&notes, "# notes\n").unwrap();
        let empty = dir.path().join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        let image = dir.path().join("shot.png");
        std::fs::write(&image, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(
            open_target(&notes),
            Some(format!(
                "vscode://file{}/a%20b%231.md",
                encode_except(
                    dir.path().canonicalize().unwrap().as_os_str().as_bytes(),
                    b"-._~/"
                )
            ))
        );
        assert!(open_target(&empty).is_some());
        assert_eq!(open_target(&image), None);
        assert_eq!(open_target(&dir.path().join("missing.md")), None);
        assert_eq!(open_target(Path::new("https://example.com/a/b")), None);
    }

    #[test]
    fn github_pull_requests_open_in_the_repository_window() {
        let cwd = Path::new("/work/my repo");
        let url = "https://github.com/owner/project/pull/12";
        assert_eq!(
            pull_request_link(url, cwd).as_deref(),
            Some(
                "vscode://blockchainian.git/view-pr?repo=%2Fwork%2Fmy%20repo\
                 &url=https%3A%2F%2Fgithub.com%2Fowner%2Fproject%2Fpull%2F12"
            )
        );
        for other in [
            "https://github.com/owner/project/issues/12",
            "https://github.com/owner/project/pull/12/files",
            "https://example.com/owner/project/pull/12",
        ] {
            assert_eq!(pull_request_link(other, cwd), None, "{other}");
        }
    }
}
