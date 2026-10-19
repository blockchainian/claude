//! Resolves commit links on a worker, keeping git off the render loop.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

type Key = (PathBuf, String);
type Entries = Arc<Mutex<HashMap<Key, Entry>>>;
const RETRY_AFTER: Duration = Duration::from_secs(5);

struct Entry {
    target: Option<PathBuf>,
    checked: Option<Instant>,
}

struct Cache {
    entries: Entries,
    requests: mpsc::Sender<Key>,
    visible: AtomicBool,
}

static CACHE: OnceLock<Cache> = OnceLock::new();

fn cache() -> &'static Cache {
    CACHE.get_or_init(|| {
        let entries: Entries = Arc::default();
        let results = Arc::clone(&entries);
        let (requests, receiver) = mpsc::channel::<Key>();
        std::thread::Builder::new()
            .name("commit-links".into())
            .spawn(move || {
                for key in receiver {
                    let target = resolve(&key.0, &key.1);
                    results.lock().expect("commit cache poisoned").insert(
                        key,
                        Entry {
                            target,
                            checked: Some(Instant::now()),
                        },
                    );
                }
            })
            .expect("start commit link worker");
        Cache {
            entries,
            requests,
            visible: AtomicBool::new(false),
        }
    })
}

/// Positive results live for the process; misses retry after five seconds.
pub(super) fn target(cwd: &Path, sha: &str) -> Option<PathBuf> {
    let cache = cache();
    let key = (cwd.to_path_buf(), sha.to_owned());
    let mut entries = cache.entries.lock().expect("commit cache poisoned");
    if let Some(entry) = entries.get(&key) {
        if entry.target.is_some() {
            return entry.target.clone();
        }
        cache.visible.store(true, Ordering::Relaxed);
        if entry.checked.is_none_or(|at| at.elapsed() < RETRY_AFTER) {
            return None;
        }
    }
    cache.visible.store(true, Ordering::Relaxed);
    entries.insert(
        key.clone(),
        Entry {
            target: None,
            checked: None,
        },
    );
    cache
        .requests
        .send(key)
        .expect("commit link worker stopped");
    None
}

/// Wake periodically once candidates are visible, including while a miss can become a commit.
pub(crate) fn redraw_in() -> Option<Duration> {
    CACHE
        .get()
        .filter(|cache| cache.visible.swap(false, Ordering::Relaxed))
        .map(|_| Duration::from_millis(250))
}

fn resolve(cwd: &Path, sha: &str) -> Option<PathBuf> {
    let exists = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?
        .success();
    if !exists {
        return None;
    }
    let root = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !root.status.success() {
        return None;
    }
    let repo = root.stdout.strip_suffix(b"\n")?;
    Some(
        format!(
            "vscode://blockchainian.git/view-commit?repo={}&sha={}",
            super::vscode::encode(repo),
            super::vscode::encode(sha.as_bytes())
        )
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn commit_cache_rechecks_misses_and_rejects_non_commits() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str], input: &[u8]| {
            let mut child = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success(), "{output:?}");
            output.stdout
        };
        git(&["init", "-q"], b"");
        git(
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
            b"",
        );
        let mut payload = git(&["cat-file", "commit", "HEAD"], b"");
        payload.extend_from_slice(b"\nfuture commit\n");
        let sha = String::from_utf8(git(&["hash-object", "-t", "commit", "--stdin"], &payload))
            .unwrap()
            .trim()
            .to_owned();
        assert_eq!(target(dir.path(), &sha), None);
        let deadline = Instant::now() + Duration::from_secs(10);
        let checked = loop {
            if let Some(at) =
                cache().entries.lock().unwrap()[&(dir.path().into(), sha.clone())].checked
            {
                break at;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        };
        git(&["hash-object", "-w", "-t", "commit", "--stdin"], &payload);
        assert_eq!(
            target(dir.path(), &sha),
            None,
            "negative cache must suppress immediate retry"
        );
        assert_eq!(
            cache().entries.lock().unwrap()[&(dir.path().into(), sha.clone())].checked,
            Some(checked)
        );
        let url = loop {
            if let Some(url) = target(dir.path(), &sha) {
                break url;
            }
            assert!(Instant::now() < deadline, "new commit stayed unlinked");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(url.to_str().unwrap().ends_with(&format!("&sha={sha}")));
        assert_eq!(target(dir.path(), &sha), Some(url));
        assert_eq!(resolve(dir.path(), "deadbeef"), None);
        let blob = String::from_utf8(git(&["hash-object", "-w", "--stdin"], b"blob")).unwrap();
        assert_eq!(resolve(dir.path(), blob.trim()), None);
        let other = tempfile::tempdir().unwrap();
        assert_eq!(resolve(other.path(), &sha), None);
        assert_eq!(target(other.path(), &sha), None, "cwd is part of the key");
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        assert_eq!(
            resolve(&dir.path().join("nested"), &sha),
            resolve(dir.path(), &sha)
        );
    }
}
